//! SQLite storage. One row per (user, day) holding the latest snapshot the client
//! sent, plus that day's individual drinks.
//!
//! The client always sends the *complete* snapshot of its current day, so a sync is an
//! idempotent replace: re-sending is harmless and the "Reset" button in the app
//! propagates on the next sync.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS days (
    user_id        TEXT    NOT NULL,
    date           TEXT    NOT NULL,           -- client-local calendar day, YYYY-MM-DD
    intake_ml      INTEGER NOT NULL,
    daily_goal_ml  INTEGER NOT NULL,
    client_ts      TEXT    NOT NULL,           -- timestamp_utc of the newest accepted sync
    updated_at     TEXT    NOT NULL,           -- when the server stored it (UTC)
    PRIMARY KEY (user_id, date)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS entries (
    id             INTEGER PRIMARY KEY,
    user_id        TEXT    NOT NULL,
    date           TEXT    NOT NULL,
    timestamp_utc  TEXT    NOT NULL,
    amount_ml      INTEGER NOT NULL,
    FOREIGN KEY (user_id, date) REFERENCES days (user_id, date) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS entries_by_day ON entries (user_id, date, timestamp_utc);
"#;

#[derive(Debug, Clone)]
pub struct IncomingDay {
    pub user_id: String,
    pub date: String,
    pub intake_ml: u32,
    pub daily_goal_ml: u32,
    pub client_ts: String,
    pub entries: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpsertOutcome {
    Stored,
    /// A newer snapshot for this day is already stored; the incoming one was ignored.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchUpsertOutcome {
    pub active_outcome: UpsertOutcome,
    pub stored_days: usize,
    pub stale_days: usize,
}

#[derive(Debug, Clone)]
pub struct DayRow {
    pub date: String,
    pub intake_ml: i64,
    pub daily_goal_ml: i64,
    pub entry_count: i64,
    pub last_sync_utc: String,
}

#[derive(Debug, Clone)]
pub struct EntryRow {
    pub timestamp_utc: String,
    pub amount_ml: i64,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub user_id: String,
    pub days: i64,
    pub last_sync_utc: String,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            // Best effort; a real problem will surface when opening the file below.
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let conn = Connection::open(path)?;
    init(&conn)?;
    Ok(conn)
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    // WAL keeps readers (dashboard) from blocking the writer (sync) and is crash safe.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)
}

fn upsert_day_tx(
    tx: &rusqlite::Transaction<'_>,
    day: &IncomingDay,
    now_utc: &str,
) -> rusqlite::Result<UpsertOutcome> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT client_ts FROM days WHERE user_id = ?1 AND date = ?2",
            params![day.user_id, day.date],
            |r| r.get(0),
        )
        .optional()?;
    // Normalised `...Z` timestamps compare correctly as strings.
    if existing.is_some_and(|ts| ts > day.client_ts) {
        return Ok(UpsertOutcome::Stale);
    }
    tx.execute(
        "INSERT INTO days (user_id, date, intake_ml, daily_goal_ml, client_ts, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (user_id, date) DO UPDATE SET
             intake_ml = excluded.intake_ml,
             daily_goal_ml = excluded.daily_goal_ml,
             client_ts = excluded.client_ts,
             updated_at = excluded.updated_at",
        params![
            day.user_id,
            day.date,
            day.intake_ml,
            day.daily_goal_ml,
            day.client_ts,
            now_utc
        ],
    )?;
    tx.execute(
        "DELETE FROM entries WHERE user_id = ?1 AND date = ?2",
        params![day.user_id, day.date],
    )?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO entries (user_id, date, timestamp_utc, amount_ml) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (ts, amount) in &day.entries {
            insert.execute(params![day.user_id, day.date, ts, amount])?;
        }
    }
    Ok(UpsertOutcome::Stored)
}

pub fn upsert_day(
    conn: &mut Connection,
    day: &IncomingDay,
    now_utc: &str,
) -> rusqlite::Result<UpsertOutcome> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let outcome = upsert_day_tx(&tx, day, now_utc)?;
    tx.commit()?;
    Ok(outcome)
}

pub fn upsert_batch(
    conn: &mut Connection,
    active: &IncomingDay,
    history: &[IncomingDay],
    now_utc: &str,
) -> rusqlite::Result<BatchUpsertOutcome> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut stored_days = 0usize;
    let mut stale_days = 0usize;

    for h in history {
        if h.date == active.date {
            continue;
        }
        match upsert_day_tx(&tx, h, now_utc)? {
            UpsertOutcome::Stored => stored_days += 1,
            UpsertOutcome::Stale => stale_days += 1,
        }
    }

    // If this sync includes archived history (e.g. a midnight rollover sync) and the new
    // active day has 0 ml with no entries and is not yet in the database, skip inserting a
    // 0 ml placeholder for the brand-new day while still allowing a Reset on an existing day.
    let active_exists: bool = tx
        .query_row(
            "SELECT 1 FROM days WHERE user_id = ?1 AND date = ?2",
            params![active.user_id, active.date],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);

    let active_outcome = if !history.is_empty()
        && active.intake_ml == 0
        && active.entries.is_empty()
        && !active_exists
    {
        UpsertOutcome::Stored
    } else {
        let out = upsert_day_tx(&tx, active, now_utc)?;
        match out {
            UpsertOutcome::Stored => stored_days += 1,
            UpsertOutcome::Stale => stale_days += 1,
        }
        out
    };

    tx.commit()?;
    Ok(BatchUpsertOutcome {
        active_outcome,
        stored_days,
        stale_days,
    })
}

pub fn list_users(conn: &Connection) -> rusqlite::Result<Vec<UserRow>> {
    let mut stmt = conn.prepare(
        "SELECT user_id, COUNT(*), MAX(updated_at) FROM days
         GROUP BY user_id ORDER BY MAX(updated_at) DESC, user_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(UserRow {
            user_id: r.get(0)?,
            days: r.get(1)?,
            last_sync_utc: r.get(2)?,
        })
    })?;
    rows.collect()
}

/// Days for `user`, oldest first, optionally limited to `from..=to` (inclusive).
pub fn list_days(
    conn: &Connection,
    user: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> rusqlite::Result<Vec<DayRow>> {
    let mut stmt = conn.prepare(
        "SELECT d.date, d.intake_ml, d.daily_goal_ml,
                (SELECT COUNT(*) FROM entries e WHERE e.user_id = d.user_id AND e.date = d.date),
                d.updated_at
         FROM days d
         WHERE d.user_id = ?1 AND (?2 IS NULL OR d.date >= ?2) AND (?3 IS NULL OR d.date <= ?3)
         ORDER BY d.date",
    )?;
    let rows = stmt.query_map(params![user, from, to], |r| {
        Ok(DayRow {
            date: r.get(0)?,
            intake_ml: r.get(1)?,
            daily_goal_ml: r.get(2)?,
            entry_count: r.get(3)?,
            last_sync_utc: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn get_day(
    conn: &Connection,
    user: &str,
    date: &str,
) -> rusqlite::Result<Option<(DayRow, Vec<EntryRow>)>> {
    let mut days = list_days(conn, user, Some(date), Some(date))?;
    let Some(day) = days.pop() else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT timestamp_utc, amount_ml FROM entries
         WHERE user_id = ?1 AND date = ?2 ORDER BY timestamp_utc, id",
    )?;
    let entries = stmt
        .query_map(params![user, date], |r| {
            Ok(EntryRow {
                timestamp_utc: r.get(0)?,
                amount_ml: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some((day, entries)))
}

/// Every drink of `user`, oldest first: `(date, timestamp_utc, amount_ml, daily_goal_ml)`.
pub fn export_entries(
    conn: &Connection,
    user: &str,
) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT e.date, e.timestamp_utc, e.amount_ml, d.daily_goal_ml
         FROM entries e JOIN days d ON d.user_id = e.user_id AND d.date = e.date
         WHERE e.user_id = ?1 ORDER BY e.date, e.timestamp_utc, e.id",
    )?;
    let rows = stmt.query_map(params![user], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?;
    rows.collect()
}

/// Cheap handle around a single connection. SQLite work is blocking, so every call runs
/// on tokio's blocking pool. Volumes here are tiny (a few rows per day), so one
/// connection behind a mutex is plenty.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn new(conn: Connection) -> Self {
        Db {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub async fn run<T, F>(&self, f: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut guard).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("database task failed: {e}"))?
    }
}
