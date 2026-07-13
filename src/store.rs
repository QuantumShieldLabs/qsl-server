use rusqlite::{params, params_from_iter, Connection};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RETENTION_TTL_SECS_DEFAULT: usize = 604_800; // 7 days
pub const MAX_RETENTION_TTL_SECS_CEILING: usize = 2_592_000; // 30 days
pub const PULL_LEASE_SECS_DEFAULT: usize = 60;
pub const MAX_PULL_LEASE_SECS_CEILING: usize = 3_600;

const SCHEMA_VERSION: i64 = 1;

// Bounds a single ack request's IN-list; well under SQLite's variable limit.
pub const MAX_ACK_IDS: usize = 4_096;

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
}

#[derive(Debug)]
pub(crate) enum EnqueueOutcome {
    Accepted,
    Overloaded { depth: usize },
    RouteCap { live_routes: usize },
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
             CREATE INDEX IF NOT EXISTS idx_messages_enqueued ON messages(enqueued_at);",
        )
        .map_err(map_err)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )
        .map_err(map_err)?;
        let stored: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(map_err)?;
        let stored: i64 = stored
            .parse()
            .map_err(|_| "ERR_STORE_VERSION".to_string())?;
        if stored > SCHEMA_VERSION {
            return Err("ERR_STORE_VERSION".to_string());
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
    ) -> Result<EnqueueOutcome, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "ERR_LOCK_POISON".to_string())?;
        let tx = guard.transaction().map_err(map_err)?;
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
