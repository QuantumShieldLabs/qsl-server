// NA-0678 (D614 F5): the store's schema-version marker must track reality.
//
// The defect this closes, measured during the D614 census rather than inferred:
// the marker was written with `INSERT OR IGNORE`, a no-op on an existing key, so
// a forward migration never advanced it. A SCHEMA_VERSION=2 binary opened a v1
// store, created its new table, and left `meta.schema_version = '1'`. The
// fail-closed downgrade guard D-0011 designed ("a store written by a NEWER
// binary must refuse to open") could therefore never fire after the first schema
// change -- and NA-0678 is the first schema change since D-0011.
//
// These tests are the guard's positive AND negative control: one proves the
// marker advances, the other proves the refusal still happens. A test that only
// checked the happy path would have passed against the broken code.

use qsl_server::{AppState, InviteLimits, Limits, ResourceControls, ServerInfoCfg, StoreConfig};
use rusqlite::Connection;

fn temp_db(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "na0678-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir.join("relay.db").to_string_lossy().into_owned()
}

fn open_store(path: &str) -> Result<AppState, String> {
    AppState::new_full(
        Limits::default(),
        ResourceControls::default(),
        None,
        StoreConfig {
            path: path.to_string(),
            ..StoreConfig::default()
        },
        ServerInfoCfg::default(),
        InviteLimits::default(),
    )
}

fn stored_version(path: &str) -> String {
    let c = Connection::open(path).unwrap_or_else(|e| panic!("{e}"));
    c.query_row(
        "SELECT value FROM meta WHERE key='schema_version'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_fresh_store_records_the_current_version() {
    let path = temp_db("fresh");
    open_store(&path).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(stored_version(&path), "2");
}

#[test]
fn a_forward_migrated_store_advances_its_marker() {
    // Build a store that looks like one this binary's predecessor created: the
    // pre-NA-0678 tables, and a marker reading "1".
    let path = temp_db("migrate");
    {
        let c = Connection::open(&path).unwrap_or_else(|e| panic!("{e}"));
        c.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE routes (
                 route_key TEXT PRIMARY KEY, log_id TEXT NOT NULL,
                 created_at INTEGER NOT NULL, last_touched INTEGER NOT NULL);
             CREATE TABLE messages (
                 seq INTEGER PRIMARY KEY AUTOINCREMENT, msg_id TEXT NOT NULL,
                 route_key TEXT NOT NULL REFERENCES routes(route_key) ON DELETE CASCADE,
                 body BLOB NOT NULL, enqueued_at INTEGER NOT NULL, leased_until INTEGER);
             INSERT INTO meta(key, value) VALUES('schema_version', '1');",
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }
    assert_eq!(
        stored_version(&path),
        "1",
        "precondition: the store starts at 1"
    );

    open_store(&path).unwrap_or_else(|e| panic!("{e}"));

    // THE FIX. Before NA-0678 this assertion failed while everything else about
    // the migration succeeded -- the new table appeared and the marker did not
    // move, which is exactly what made the defect invisible.
    assert_eq!(
        stored_version(&path),
        "2",
        "a forward migration must advance the marker, or the downgrade guard is inert"
    );

    // And the migration really did happen.
    let c = Connection::open(&path).unwrap_or_else(|e| panic!("{e}"));
    let has_invites: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='invites')",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(has_invites, "the invites table must exist after migration");
}

#[test]
fn a_store_from_a_newer_binary_is_refused() {
    // The negative control: the guard must still FIRE. Without this, the test
    // above could pass against an implementation that simply stopped checking.
    let path = temp_db("newer");
    {
        let c = Connection::open(&path).unwrap_or_else(|e| panic!("{e}"));
        c.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta(key, value) VALUES('schema_version', '99');",
        )
        .unwrap_or_else(|e| panic!("{e}"));
    }
    // `AppState` is deliberately not `Debug` (it holds secrets), so match rather
    // than `expect_err` -- adding a derive to satisfy a test would be the tail
    // wagging the dog.
    match open_store(&path) {
        Err(e) => assert_eq!(e, "ERR_STORE_VERSION"),
        Ok(_) => panic!("a store written by a newer binary must be refused"),
    }
}
