use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RETENTION_TTL_SECS_DEFAULT: usize = 604_800; // 7 days
pub const MAX_RETENTION_TTL_SECS_CEILING: usize = 2_592_000; // 30 days
pub const PULL_LEASE_SECS_DEFAULT: usize = 60;
pub const MAX_PULL_LEASE_SECS_CEILING: usize = 3_600;

// NA-0678 (D614 F5): bumped to 2 for the `invites` table. ⚠ The bump is only
// meaningful because the migration now ADVANCES the stored value -- see
// `write_schema_version`. Before this lane the version was written with
// `INSERT OR IGNORE`, a no-op on an existing key, so a forward-migrated store
// kept reporting the version it was created at and the downgrade guard below
// silently stopped tracking reality after the first schema change. Measured,
// not inferred: a SCHEMA_VERSION=2 binary opened a v1 store, created its new
// table, and left `meta.schema_version = '1'`.
const SCHEMA_VERSION: i64 = 2;

// Bounds a single ack request's IN-list; well under SQLite's variable limit.
pub const MAX_ACK_IDS: usize = 4_096;

// NA-0678 invite-slot states. Consumed and revoked slots are TOMBSTONED until
// expiry rather than deleted: the failure taxonomy requires
// `invite-already-used` and `invite-not-found` to stay DISTINCT causes, and a
// deleted slot reports "never existed" when the truth is "someone got here
// first" -- which is precisely the interception signal the design exists to
// surface. The bundle and signature blobs are cleared at consumption, so a
// tombstone carries no identity material.
pub(crate) const INVITE_ACTIVE: i64 = 0;
pub(crate) const INVITE_CONSUMED: i64 = 1;
pub(crate) const INVITE_REVOKED: i64 = 2;

/// Durable-store configuration. `path` accepts a filesystem path or the
/// literal `:memory:` for explicitly ephemeral stores (tests, dev runs).
#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub path: String,
    pub retention_ttl_secs: usize,
    pub pull_lease_secs: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: ":memory:".to_string(),
            retention_ttl_secs: RETENTION_TTL_SECS_DEFAULT,
            pull_lease_secs: PULL_LEASE_SECS_DEFAULT,
        }
    }
}

pub fn retention_ttl_or_error(value: usize) -> Result<usize, String> {
    if value == 0 {
        return Err("ERR_INVALID_CONFIG_RETENTION_TTL_SECS".to_string());
    }
    Ok(value.min(MAX_RETENTION_TTL_SECS_CEILING))
}

pub fn pull_lease_or_error(value: usize) -> Result<usize, String> {
    if value == 0 {
        return Err("ERR_INVALID_CONFIG_PULL_LEASE_SECS".to_string());
    }
    Ok(value.min(MAX_PULL_LEASE_SECS_CEILING))
}

#[derive(Debug)]
pub(crate) struct StoredMsg {
    pub msg_id: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct SweepStats {
    pub expired_messages: usize,
    // (redacted channel log id, expired message count) per affected route
    pub expired_routes: Vec<(String, usize)>,
    // route keys whose route row was removed (for rate-bucket pruning)
    pub removed_route_keys: Vec<String>,
    // NA-0678: invite slots (live and tombstoned) removed at their own expiry
    pub expired_invites: usize,
}

#[derive(Debug)]
pub(crate) enum EnqueueOutcome {
    Accepted,
    Overloaded { depth: usize },
    RouteCap { live_routes: usize },
    SlotRejected(SlotReject),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PullMode {
    // delete-on-deliver, the pre-durability wire contract
    Legacy,
    // mark in-flight with a visibility deadline; deletion happens on ack
    Lease,
}

#[derive(Debug)]
pub(crate) struct PullOutcome {
    pub items: Vec<StoredMsg>,
    pub route_drained: bool,
    pub sweep: SweepStats,
}

#[derive(Debug)]
pub(crate) struct AckOutcome {
    pub acked: usize,
    pub route_drained: bool,
    pub sweep: SweepStats,
}

#[derive(Debug)]
pub(crate) struct RouteStatus {
    pub route_exists: bool,
    pub depth: usize,
    pub live_routes: usize,
    pub sweep: SweepStats,
}

#[derive(Debug)]
pub(crate) enum InviteCreateOutcome {
    Created,
    Duplicate,
    CapFull { live_slots: usize },
}

#[derive(Debug)]
pub(crate) enum InviteRedeemOutcome {
    Redeemed {
        bundle: Vec<u8>,
        invite_sig: Vec<u8>,
    },
    NotFound,
    Revoked,
    Expired,
    AlreadyUsed,
    CapInvalid,
}

#[derive(Debug)]
pub(crate) enum InviteRevokeOutcome {
    Revoked,
    AlreadyRevoked,
    NotFound,
    TokenInvalid,
}

/// The columns a redemption reads. A named struct rather than a five-tuple so
/// the destructure below says what each field is.
struct InviteRow {
    state: i64,
    expiry: i64,
    cap_hash: String,
    bundle: Vec<u8>,
    invite_sig: Vec<u8>,
}

/// Why a push into a KNOWN invite slot was refused. Pushes to routes that are
/// not slots never reach this type -- that is the whole of the compatibility
/// guarantee (D614 C3): the existing relay contract is untouched for every
/// route the invite system did not create.
#[derive(Debug)]
pub(crate) enum SlotReject {
    Expired,
    Revoked,
    TicketInvalid,
}

pub(crate) fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub(crate) struct Store {
    conn: Arc<Mutex<Connection>>,
    retention_ttl_secs: i64,
    pull_lease_secs: i64,
}

fn map_err(e: rusqlite::Error) -> String {
    format!("ERR_STORE {e}")
}

/// Write the schema marker with an UPSERT rather than `INSERT OR IGNORE`, so a
/// forward migration actually advances it (NA-0678, D614 F5).
fn write_schema_version(conn: &Connection, version: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![version.to_string()],
    )
    .map_err(map_err)?;
    Ok(())
}

impl Store {
    pub(crate) fn open(cfg: &StoreConfig) -> Result<Self, String> {
        let retention_ttl_secs = retention_ttl_or_error(cfg.retention_ttl_secs)? as i64;
        let pull_lease_secs = pull_lease_or_error(cfg.pull_lease_secs)? as i64;
        let conn =
            Connection::open(&cfg.path).map_err(|_| "ERR_INVALID_CONFIG_STORE_PATH".to_string())?;
        // FULL: a 200 on push means the message is fsynced, not merely buffered.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(map_err)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(map_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(map_err)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS routes (
                 route_key    TEXT PRIMARY KEY,
                 log_id       TEXT NOT NULL,
                 created_at   INTEGER NOT NULL,
                 last_touched INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 seq          INTEGER PRIMARY KEY AUTOINCREMENT,
                 msg_id       TEXT NOT NULL,
                 route_key    TEXT NOT NULL REFERENCES routes(route_key) ON DELETE CASCADE,
                 body         BLOB NOT NULL,
                 enqueued_at  INTEGER NOT NULL,
                 leased_until INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_messages_route_seq ON messages(route_key, seq);
             CREATE INDEX IF NOT EXISTS idx_messages_enqueued ON messages(enqueued_at);
             CREATE TABLE IF NOT EXISTS invites (
                 slot_key     TEXT PRIMARY KEY,
                 log_id       TEXT NOT NULL,
                 cap_hash     TEXT NOT NULL,
                 revoke_hash  TEXT NOT NULL,
                 bundle       BLOB NOT NULL,
                 invite_sig   BLOB NOT NULL,
                 expiry       INTEGER NOT NULL,
                 created_at   INTEGER NOT NULL,
                 state        INTEGER NOT NULL,
                 consumed_at  INTEGER,
                 ticket_hash  TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_invites_expiry ON invites(expiry);",
        )
        .map_err(map_err)?;
        // Read BEFORE writing: `INSERT OR IGNORE` cannot distinguish "new store"
        // from "existing store at an older version", which is exactly how the
        // pre-NA-0678 guard went inert.
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_err)?;
        let stored: i64 = match stored {
            None => {
                write_schema_version(&conn, SCHEMA_VERSION)?;
                SCHEMA_VERSION
            }
            Some(raw) => raw.parse().map_err(|_| "ERR_STORE_VERSION".to_string())?,
        };
        // Fail closed on a store written by a NEWER binary: its schema may carry
        // columns and invariants this build does not know about.
        if stored > SCHEMA_VERSION {
            return Err("ERR_STORE_VERSION".to_string());
        }
        // Forward migration: the CREATE TABLE IF NOT EXISTS statements above have
        // already brought an older store up to date, so record that it happened.
        // Without this the marker stays at the version the file was CREATED at
        // and the guard above can never fire for a rollback.
        if stored < SCHEMA_VERSION {
            write_schema_version(&conn, SCHEMA_VERSION)?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            retention_ttl_secs,
            pull_lease_secs,
        })
    }

    pub(crate) fn retention_ttl_secs(&self) -> i64 {
        self.retention_ttl_secs
    }

    fn sweep_expired(conn: &Connection, ttl: i64, now: i64) -> Result<SweepStats, String> {
        let mut stats = SweepStats::default();
        {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT r.log_id, COUNT(m.seq) FROM messages m
                     JOIN routes r ON r.route_key = m.route_key
                     WHERE m.enqueued_at + ?1 <= ?2
                     GROUP BY m.route_key",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(params![ttl, now], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(map_err)?;
            for row in rows {
                let (log_id, n) = row.map_err(map_err)?;
                stats.expired_routes.push((log_id, n as usize));
            }
        }
        stats.expired_messages = conn
            .execute(
                "DELETE FROM messages WHERE enqueued_at + ?1 <= ?2",
                params![ttl, now],
            )
            .map_err(map_err)?;
        if stats.expired_messages > 0 {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT route_key FROM routes r
                     WHERE NOT EXISTS (SELECT 1 FROM messages m WHERE m.route_key = r.route_key)",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(map_err)?;
            for row in rows {
                stats.removed_route_keys.push(row.map_err(map_err)?);
            }
            conn.execute(
                "DELETE FROM routes WHERE NOT EXISTS
                     (SELECT 1 FROM messages WHERE messages.route_key = routes.route_key)",
                [],
            )
            .map_err(map_err)?;
        }
        // Invite slots expire on their OWN clock, not the retention TTL: an
        // invite's lifetime is set by its creator (design §8 Q1, 72 h default).
        // Tombstones live until that moment and are swept with the slot, which
        // is what bounds the tombstone's cost.
        stats.expired_invites = conn
            .execute("DELETE FROM invites WHERE expiry <= ?1", params![now])
            .map_err(map_err)?;
        Ok(stats)
    }

    pub(crate) fn retention_sweep(&self, now: i64) -> Result<SweepStats, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let stats = Self::sweep_expired(&tx, self.retention_ttl_secs, now)?;
        tx.commit().map_err(map_err)?;
        Ok(stats)
    }

    pub(crate) fn route_status(&self, route_key: &str, now: i64) -> Result<RouteStatus, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let sweep = Self::sweep_expired(&tx, self.retention_ttl_secs, now)?;
        let route_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM routes WHERE route_key = ?1)",
                params![route_key],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        let depth: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE route_key = ?1",
                params![route_key],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        let live_routes: i64 = tx
            .query_row("SELECT COUNT(*) FROM routes", [], |row| row.get(0))
            .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(RouteStatus {
            route_exists,
            depth: depth as usize,
            live_routes: live_routes as usize,
            sweep,
        })
    }

    /// Publish an invite slot. `cap_hash` and `revoke_hash` arrive ALREADY
    /// hashed by the caller -- the relay never holds either secret in plaintext
    /// (D614 F1: the client mints the capability; there is no mint endpoint).
    /// `bundle` and `invite_sig` are stored verbatim and are never parsed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn invite_create(
        &self,
        slot_key: &str,
        log_id: &str,
        cap_hash: &str,
        revoke_hash: &str,
        bundle: &[u8],
        invite_sig: &[u8],
        expiry: i64,
        now: i64,
        max_slots: usize,
    ) -> Result<InviteCreateOutcome, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        Self::sweep_expired(&tx, self.retention_ttl_secs, now)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM invites WHERE slot_key = ?1)",
                params![slot_key],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        if exists {
            return Ok(InviteCreateOutcome::Duplicate);
        }
        let live_slots: i64 = tx
            .query_row("SELECT COUNT(*) FROM invites", [], |row| row.get(0))
            .map_err(map_err)?;
        if live_slots as usize >= max_slots {
            // Reject, NEVER evict. An eviction path would let an attacker
            // delete other people's invites -- a worse failure than the denial
            // it would relieve (D614 F6).
            return Ok(InviteCreateOutcome::CapFull {
                live_slots: live_slots as usize,
            });
        }
        tx.execute(
            "INSERT INTO invites(slot_key, log_id, cap_hash, revoke_hash, bundle,
                                 invite_sig, expiry, created_at, state, consumed_at, ticket_hash)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL)",
            params![
                slot_key,
                log_id,
                cap_hash,
                revoke_hash,
                bundle,
                invite_sig,
                expiry,
                now,
                INVITE_ACTIVE
            ],
        )
        .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(InviteCreateOutcome::Created)
    }

    /// Redeem a slot: verify the capability and consume it ATOMICALLY, then
    /// issue the one-shot handshake ticket (D614 F3).
    ///
    /// `verify_cap` receives the STORED hash and performs the constant-time
    /// comparison in the caller -- the crypto stays in one place (`ct_eq_secret`)
    /// while the check and the consume stay inside one transaction, so a lost
    /// race cannot yield two winners. The single mutex-wrapped connection makes
    /// that exact rather than merely probable.
    ///
    /// Cause order is deliberate: not-found → revoked → expired → already-used →
    /// cap-invalid. Reaching this route at all requires knowing `invite_id`, a
    /// 128-bit secret that travels only inside the invite code -- so a caller
    /// who can address the slot already holds the capability, and reporting the
    /// slot's real state to them is information they were given, not a leak. The
    /// legitimate holder gets the most useful cause; the design's taxonomy
    /// requires exactly that.
    pub(crate) fn invite_redeem<F>(
        &self,
        slot_key: &str,
        now: i64,
        ticket_hash: &str,
        verify_cap: F,
    ) -> Result<InviteRedeemOutcome, String>
    where
        F: Fn(&str) -> bool,
    {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let row: Option<InviteRow> = tx
            .query_row(
                "SELECT state, expiry, cap_hash, bundle, invite_sig
                 FROM invites WHERE slot_key = ?1",
                params![slot_key],
                |r| {
                    Ok(InviteRow {
                        state: r.get(0)?,
                        expiry: r.get(1)?,
                        cap_hash: r.get(2)?,
                        bundle: r.get(3)?,
                        invite_sig: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)?;
        let Some(InviteRow {
            state,
            expiry,
            cap_hash,
            bundle,
            invite_sig,
        }) = row
        else {
            return Ok(InviteRedeemOutcome::NotFound);
        };
        if state == INVITE_REVOKED {
            return Ok(InviteRedeemOutcome::Revoked);
        }
        if expiry <= now {
            return Ok(InviteRedeemOutcome::Expired);
        }
        if state == INVITE_CONSUMED {
            return Ok(InviteRedeemOutcome::AlreadyUsed);
        }
        if !verify_cap(&cap_hash) {
            return Ok(InviteRedeemOutcome::CapInvalid);
        }
        // Compare-and-set: the WHERE clause re-asserts ACTIVE, so a concurrent
        // winner leaves this UPDATE matching zero rows.
        let updated = tx
            .execute(
                "UPDATE invites
                    SET state = ?2, consumed_at = ?3, ticket_hash = ?4,
                        bundle = x'', invite_sig = x''
                  WHERE slot_key = ?1 AND state = ?5",
                params![slot_key, INVITE_CONSUMED, now, ticket_hash, INVITE_ACTIVE],
            )
            .map_err(map_err)?;
        if updated == 0 {
            return Ok(InviteRedeemOutcome::AlreadyUsed);
        }
        tx.commit().map_err(map_err)?;
        Ok(InviteRedeemOutcome::Redeemed { bundle, invite_sig })
    }

    /// Kill a slot. Idempotent, and authorized by the `revoke_token` issued once
    /// at creation (D614 F2) -- without it, an open relay would let anyone who
    /// has merely SEEN an invite code destroy it.
    pub(crate) fn invite_revoke<F>(
        &self,
        slot_key: &str,
        now: i64,
        verify_revoke: F,
    ) -> Result<InviteRevokeOutcome, String>
    where
        F: Fn(&str) -> bool,
    {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let row: Option<(i64, String)> = tx
            .query_row(
                "SELECT state, revoke_hash FROM invites WHERE slot_key = ?1",
                params![slot_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(map_err)?;
        let Some((state, revoke_hash)) = row else {
            return Ok(InviteRevokeOutcome::NotFound);
        };
        // The credential is checked BEFORE any state is reported, because unlike
        // redemption a revoke needs a secret the invite code does not carry.
        if !verify_revoke(&revoke_hash) {
            return Ok(InviteRevokeOutcome::TokenInvalid);
        }
        if state == INVITE_REVOKED {
            return Ok(InviteRevokeOutcome::AlreadyRevoked);
        }
        tx.execute(
            "UPDATE invites
                SET state = ?2, bundle = x'', invite_sig = x'', ticket_hash = NULL
              WHERE slot_key = ?1",
            params![slot_key, INVITE_REVOKED],
        )
        .map_err(map_err)?;
        let _ = now;
        tx.commit().map_err(map_err)?;
        Ok(InviteRevokeOutcome::Revoked)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enqueue(
        &self,
        route_key: &str,
        log_id: &str,
        msg_id: &str,
        body: &[u8],
        now: i64,
        max_queue_depth: usize,
        max_route_count: usize,
        ticket: Option<&dyn Fn(&str) -> bool>,
    ) -> Result<EnqueueOutcome, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        // NA-0678 slot admission (D614 C3/§2c). ONE indexed lookup that MISSES
        // for every route the invite system did not create -- which is every
        // route that exists today. A miss takes the `None` arm and the rest of
        // this function is byte-for-byte the pre-lane behaviour, which is the
        // compatibility guarantee the whole epic's ordering rests on.
        //
        // Admission lives HERE, inside the same transaction as the message
        // insert, so the one-shot ticket is genuinely one-shot: checking it in
        // an earlier transaction would leave a race between two concurrent
        // pushes presenting the same ticket.
        let slot: Option<(i64, i64, Option<String>)> = tx
            .query_row(
                "SELECT state, expiry, ticket_hash FROM invites WHERE slot_key = ?1",
                params![route_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(map_err)?;
        if let Some((state, expiry, ticket_hash)) = slot {
            if state == INVITE_REVOKED {
                return Ok(EnqueueOutcome::SlotRejected(SlotReject::Revoked));
            }
            if expiry <= now {
                return Ok(EnqueueOutcome::SlotRejected(SlotReject::Expired));
            }
            // A live ticket exists only between redemption and the handshake it
            // authorizes. No ticket, no match, or no presented ticket -> refuse.
            let admitted = match (ticket_hash.as_deref(), ticket) {
                (Some(stored), Some(verify)) => verify(stored),
                _ => false,
            };
            if !admitted {
                return Ok(EnqueueOutcome::SlotRejected(SlotReject::TicketInvalid));
            }
            // Burn it: one handshake per redemption.
            tx.execute(
                "UPDATE invites SET ticket_hash = NULL WHERE slot_key = ?1",
                params![route_key],
            )
            .map_err(map_err)?;
        }
        let route_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM routes WHERE route_key = ?1)",
                params![route_key],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        if route_exists {
            let depth: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE route_key = ?1",
                    params![route_key],
                    |row| row.get(0),
                )
                .map_err(map_err)?;
            if depth as usize >= max_queue_depth {
                return Ok(EnqueueOutcome::Overloaded {
                    depth: depth as usize,
                });
            }
        } else {
            let live_routes: i64 = tx
                .query_row("SELECT COUNT(*) FROM routes", [], |row| row.get(0))
                .map_err(map_err)?;
            if live_routes as usize >= max_route_count {
                return Ok(EnqueueOutcome::RouteCap {
                    live_routes: live_routes as usize,
                });
            }
            tx.execute(
                "INSERT INTO routes(route_key, log_id, created_at, last_touched)
                 VALUES(?1, ?2, ?3, ?3)",
                params![route_key, log_id, now],
            )
            .map_err(map_err)?;
        }
        tx.execute(
            "INSERT INTO messages(msg_id, route_key, body, enqueued_at)
             VALUES(?1, ?2, ?3, ?4)",
            params![msg_id, route_key, body, now],
        )
        .map_err(map_err)?;
        tx.execute(
            "UPDATE routes SET last_touched = ?2 WHERE route_key = ?1",
            params![route_key, now],
        )
        .map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(EnqueueOutcome::Accepted)
    }

    pub(crate) fn pull(
        &self,
        route_key: &str,
        max: usize,
        now: i64,
        mode: PullMode,
    ) -> Result<PullOutcome, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let sweep = Self::sweep_expired(&tx, self.retention_ttl_secs, now)?;
        let mut items = Vec::new();
        let mut seqs: Vec<i64> = Vec::new();
        {
            let mut stmt = tx
                .prepare_cached(
                    "SELECT seq, msg_id, body FROM messages
                     WHERE route_key = ?1 AND (leased_until IS NULL OR leased_until <= ?2)
                     ORDER BY seq LIMIT ?3",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(params![route_key, now, max as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                })
                .map_err(map_err)?;
            for row in rows {
                let (seq, msg_id, body) = row.map_err(map_err)?;
                seqs.push(seq);
                items.push(StoredMsg { msg_id, body });
            }
        }
        if !seqs.is_empty() {
            let placeholders = vec!["?"; seqs.len()].join(",");
            match mode {
                PullMode::Legacy => {
                    let sql = format!("DELETE FROM messages WHERE seq IN ({placeholders})");
                    tx.execute(&sql, params_from_iter(seqs.iter()))
                        .map_err(map_err)?;
                }
                PullMode::Lease => {
                    let deadline = now + self.pull_lease_secs;
                    let sql = format!(
                        "UPDATE messages SET leased_until = {deadline} WHERE seq IN ({placeholders})"
                    );
                    tx.execute(&sql, params_from_iter(seqs.iter()))
                        .map_err(map_err)?;
                }
            }
        }
        let mut route_drained = false;
        if matches!(mode, PullMode::Legacy) {
            let remaining: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE route_key = ?1",
                    params![route_key],
                    |row| row.get(0),
                )
                .map_err(map_err)?;
            if remaining == 0 {
                let removed = tx
                    .execute(
                        "DELETE FROM routes WHERE route_key = ?1",
                        params![route_key],
                    )
                    .map_err(map_err)?;
                route_drained = removed > 0;
            } else {
                tx.execute(
                    "UPDATE routes SET last_touched = ?2 WHERE route_key = ?1",
                    params![route_key, now],
                )
                .map_err(map_err)?;
            }
        } else if !seqs.is_empty() {
            tx.execute(
                "UPDATE routes SET last_touched = ?2 WHERE route_key = ?1",
                params![route_key, now],
            )
            .map_err(map_err)?;
        }
        tx.commit().map_err(map_err)?;
        Ok(PullOutcome {
            items,
            route_drained,
            sweep,
        })
    }

    pub(crate) fn ack(
        &self,
        route_key: &str,
        ids: &[String],
        now: i64,
    ) -> Result<AckOutcome, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
        let sweep = Self::sweep_expired(&tx, self.retention_ttl_secs, now)?;
        // Only leased (in-flight or lease-expired-but-undelivered-again) copies
        // are deletable: an unleased duplicate copy was never delivered and
        // must survive (NA-0275 duplicate-id contract).
        let placeholders = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "DELETE FROM messages WHERE route_key = ?1 AND leased_until IS NOT NULL
             AND msg_id IN ({placeholders})"
        );
        let mut bind: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
        bind.push(&route_key);
        for id in ids {
            bind.push(id);
        }
        let acked = tx.execute(&sql, bind.as_slice()).map_err(map_err)?;
        let mut route_drained = false;
        if acked > 0 {
            let remaining: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE route_key = ?1",
                    params![route_key],
                    |row| row.get(0),
                )
                .map_err(map_err)?;
            if remaining == 0 {
                let removed = tx
                    .execute(
                        "DELETE FROM routes WHERE route_key = ?1",
                        params![route_key],
                    )
                    .map_err(map_err)?;
                route_drained = removed > 0;
            }
        }
        tx.commit().map_err(map_err)?;
        Ok(AckOutcome {
            acked,
            route_drained,
            sweep,
        })
    }
}
