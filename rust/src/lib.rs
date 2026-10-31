//! HydroCore engine.
//!
//! Everything that matters lives here: state management, automatic daily rollover,
//! atomic persistence (`state.json`), goal progression and the manual HTTP sync.
//! Kotlin is a thin UI that talks to this crate through one JSON-in/JSON-out entry
//! point ([`handle_request`]) exposed over JNI in [`jni_bridge`].
//!
//! Design notes
//! * The core is clock-agnostic: Kotlin passes the device-local `today` (`YYYY-MM-DD`)
//!   and `now_utc` (RFC 3339) with every request, which keeps the core deterministic
//!   and trivially testable.
//! * Mutations are "copy, modify, persist, then swap", so a failed disk write never
//!   leaves memory and disk out of sync.
//! * The global lock is *not* held during network I/O.

pub mod jni_bridge;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

pub const DEFAULT_GOAL_ML: u32 = 2_500;
pub const MIN_GOAL_ML: u32 = 500;
pub const MAX_GOAL_ML: u32 = 10_000;
pub const MAX_SINGLE_ADD_ML: u32 = 5_000;
pub const MAX_DAILY_INTAKE_ML: u32 = 20_000;
/// Finished days kept for the history view; the oldest are dropped beyond this.
pub const MAX_HISTORY_DAYS: usize = 365;

const STATE_FILE: &str = "state.json";
const STATE_TMP_FILE: &str = "state.json.tmp";
const STATE_CORRUPT_FILE: &str = "state.json.corrupt";
const HISTORY_TXT_FILE: &str = "hydration_history.txt";
const HISTORY_TXT_TMP_FILE: &str = "hydration_history.txt.tmp";

// ---------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    NotInitialized,
    Invalid(String),
    Storage(String),
    Network(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NotInitialized => write!(f, "engine is not initialized"),
            EngineError::Invalid(m) => write!(f, "{m}"),
            EngineError::Storage(m) => write!(f, "storage error: {m}"),
            EngineError::Network(m) => write!(f, "network error: {m}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(e: serde_json::Error) -> Self {
        EngineError::Storage(e.to_string())
    }
}

type Result<T> = std::result::Result<T, EngineError>;

// ---------------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub user_id: String,
    pub server_url: String,
    pub api_token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub timestamp_utc: String,
    pub amount_ml: u32,
}

/// A finished day, archived at rollover (midnight). Intake is derived from `entries`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayRecord {
    pub date: String,
    pub daily_goal_ml: u32,
    pub entries: Vec<Entry>,
}

impl DayRecord {
    pub fn intake_ml(&self) -> u32 {
        self.entries
            .iter()
            .fold(0u32, |acc, e| acc.saturating_add(e.amount_ml))
    }
}

/// The persisted state. Intake is always derived from `entries`, so it cannot drift.
/// `history` is oldest-first on disk; files written by v0.1 simply have none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub date: String,
    pub daily_goal_ml: u32,
    pub entries: Vec<Entry>,
    pub settings: Settings,
    pub last_sync_utc: Option<String>,
    pub history: Vec<DayRecord>,
}

impl Default for State {
    fn default() -> Self {
        State {
            date: String::new(),
            daily_goal_ml: DEFAULT_GOAL_ML,
            entries: Vec::new(),
            settings: Settings::default(),
            last_sync_utc: None,
            history: Vec::new(),
        }
    }
}

impl State {
    pub fn intake_ml(&self) -> u32 {
        self.entries
            .iter()
            .fold(0u32, |acc, e| acc.saturating_add(e.amount_ml))
    }
}

/// A finished day as shown in the history view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryDay {
    pub date: String,
    pub intake_ml: u32,
    pub daily_goal_ml: u32,
    pub goal_reached: bool,
    pub entries: Vec<Entry>,
}

/// What the UI sees: the persisted state plus derived goal-progression values.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateView {
    pub date: String,
    pub intake_ml: u32,
    pub daily_goal_ml: u32,
    /// `0.0..=1.0`, capped once the goal is reached.
    pub progress: f32,
    pub remaining_ml: u32,
    pub goal_reached: bool,
    pub entries: Vec<Entry>,
    pub settings: Settings,
    pub last_sync_utc: Option<String>,
    /// Finished days, newest first.
    pub history: Vec<HistoryDay>,
}

/// A fully prepared sync request, built while holding the lock and sent without it.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncRequest {
    pub url: String,
    pub bearer_token: Option<String>,
    pub body: String,
    pub entry_count: usize,
}

#[derive(Serialize)]
struct SyncPayload<'a> {
    user_id: &'a str,
    date: &'a str,
    intake_ml: u32,
    daily_goal_ml: u32,
    timestamp_utc: &'a str,
    entries: &'a [Entry],
}

// ---------------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------------

/// Accepts `YYYY-MM-DD` with plausible month/day values.
pub fn is_valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits_ok = b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !digits_ok {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// Converts `YYYY-MM-DD` into days since 1970-01-01 (Howard Hinnant's civil algorithm).
pub fn parse_epoch_days(s: &str) -> Option<i64> {
    if !is_valid_date(s) {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Accepts only `http://` / `https://` URLs with a non-empty, whitespace-free remainder.
pub fn is_valid_server_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    match rest {
        Some(r) => !r.is_empty() && !r.starts_with('/') && !r.chars().any(char::is_whitespace),
        None => false,
    }
}

// ---------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------

pub struct Engine {
    dir: PathBuf,
    state: State,
}

impl Engine {
    /// Opens (or creates) the engine in `dir` and applies the daily rollover for `today`.
    pub fn open(dir: impl Into<PathBuf>, today: &str) -> Result<Engine> {
        if !is_valid_date(today) {
            return Err(EngineError::Invalid(format!("invalid date: {today:?}")));
        }
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let state = load_state(&dir);
        let mut engine = Engine { dir, state };
        engine.rollover(today)?;
        Ok(engine)
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn view(&self) -> StateView {
        let intake = self.state.intake_ml();
        let goal = self.state.daily_goal_ml.max(1);
        StateView {
            date: self.state.date.clone(),
            intake_ml: intake,
            daily_goal_ml: self.state.daily_goal_ml,
            progress: (intake as f32 / goal as f32).clamp(0.0, 1.0),
            remaining_ml: self.state.daily_goal_ml.saturating_sub(intake),
            goal_reached: intake >= self.state.daily_goal_ml,
            entries: self.state.entries.clone(),
            settings: self.state.settings.clone(),
            last_sync_utc: self.state.last_sync_utc.clone(),
            history: self
                .state
                .history
                .iter()
                .rev()
                .map(|d| {
                    let intake = d.intake_ml();
                    HistoryDay {
                        date: d.date.clone(),
                        intake_ml: intake,
                        daily_goal_ml: d.daily_goal_ml,
                        goal_reached: intake >= d.daily_goal_ml,
                        entries: d.entries.clone(),
                    }
                })
                .collect(),
        }
    }

    /// Starts a new day when `today` is later than the stored date, archiving the
    /// finished day (if anything was logged) into the history. Returns whether a
    /// rollover happened. A clock that moves *backwards* (e.g. travelling west) never
    /// wipes data. ISO dates compare correctly as strings.
    ///
    /// The core never runs on its own: this is triggered by the first request after
    /// midnight (app open/resume, or the foreground midnight tick in the UI).
    pub fn rollover(&mut self, today: &str) -> Result<bool> {
        if !is_valid_date(today) {
            return Err(EngineError::Invalid(format!("invalid date: {today:?}")));
        }
        if !self.state.date.is_empty() && today <= self.state.date.as_str() {
            return Ok(false);
        }
        let mut next = self.state.clone();
        if !next.date.is_empty() && !next.entries.is_empty() {
            let finished = DayRecord {
                date: next.date.clone(),
                daily_goal_ml: next.daily_goal_ml,
                entries: std::mem::take(&mut next.entries),
            };
            next.history.retain(|d| d.date != finished.date);
            next.history.push(finished);
            next.history.sort_by(|a, b| a.date.cmp(&b.date));
            let excess = next.history.len().saturating_sub(MAX_HISTORY_DAYS);
            next.history.drain(..excess);
        }
        next.date = today.to_string();
        next.entries.clear();
        self.commit(next)?;
        Ok(true)
    }

    pub fn add_water(&mut self, amount_ml: u32, now_utc: &str) -> Result<()> {
        if amount_ml == 0 || amount_ml > MAX_SINGLE_ADD_ML {
            return Err(EngineError::Invalid(format!(
                "amount must be between 1 and {MAX_SINGLE_ADD_ML} ml"
            )));
        }
        if self.state.intake_ml().saturating_add(amount_ml) > MAX_DAILY_INTAKE_ML {
            return Err(EngineError::Invalid(format!(
                "daily intake cannot exceed {MAX_DAILY_INTAKE_ML} ml"
            )));
        }
        let mut next = self.state.clone();
        next.entries.push(Entry {
            timestamp_utc: now_utc.to_string(),
            amount_ml,
        });
        self.commit(next)
    }

    /// Clears today's intake (entries); settings and goal are untouched.
    pub fn reset(&mut self) -> Result<()> {
        let mut next = self.state.clone();
        next.entries.clear();
        self.commit(next)
    }

    pub fn save_settings(&mut self, settings: Settings, daily_goal_ml: u32) -> Result<()> {
        if !(MIN_GOAL_ML..=MAX_GOAL_ML).contains(&daily_goal_ml) {
            return Err(EngineError::Invalid(format!(
                "daily goal must be between {MIN_GOAL_ML} and {MAX_GOAL_ML} ml"
            )));
        }
        let server_url = settings.server_url.trim().to_string();
        if !server_url.is_empty() && !is_valid_server_url(&server_url) {
            return Err(EngineError::Invalid(
                "server URL must start with http:// or https://".into(),
            ));
        }
        let mut next = self.state.clone();
        next.settings = Settings {
            user_id: settings.user_id.trim().to_string(),
            server_url,
            api_token: settings.api_token.trim().to_string(),
        };
        next.daily_goal_ml = daily_goal_ml;
        self.commit(next)
    }

    /// Builds the sync request (JSON body, URL, optional bearer token).
    pub fn build_sync_request(&self, now_utc: &str) -> Result<SyncRequest> {
        let s = &self.state.settings;
        if s.user_id.trim().is_empty() {
            return Err(EngineError::Invalid("set a User ID in Settings first".into()));
        }
        if !is_valid_server_url(s.server_url.trim()) {
            return Err(EngineError::Invalid(
                "set a valid Server URL (http:// or https://) in Settings first".into(),
            ));
        }
        let payload = SyncPayload {
            user_id: &s.user_id,
            date: &self.state.date,
            intake_ml: self.state.intake_ml(),
            daily_goal_ml: self.state.daily_goal_ml,
            timestamp_utc: now_utc,
            entries: &self.state.entries,
        };
        let token = s.api_token.trim();
        Ok(SyncRequest {
            url: s.server_url.trim().to_string(),
            bearer_token: (!token.is_empty()).then(|| token.to_string()),
            body: serde_json::to_string(&payload)?,
            entry_count: self.state.entries.len(),
        })
    }

    pub fn record_sync_success(&mut self, now_utc: &str) -> Result<()> {
        let mut next = self.state.clone();
        next.last_sync_utc = Some(now_utc.to_string());
        self.commit(next)
    }

    /// Formats the full historical archive (including records > 7 days old and today's
    /// live intake) into structured plain-text blocks without truncation.
    pub fn format_history_report(&self, now_utc: &str) -> String {
        use std::fmt::Write as _;

        let today_days = parse_epoch_days(&self.state.date).unwrap_or(0);
        // Newest-first for detailed sections.
        let mut newest_first: Vec<(&DayRecord, i64)> = self
            .state
            .history
            .iter()
            .rev()
            .map(|d| {
                let age = parse_epoch_days(&d.date)
                    .map(|days| (today_days - days).max(1))
                    .unwrap_or(1);
                (d, age)
            })
            .collect();
        newest_first.sort_by(|a, b| b.0.date.cmp(&a.0.date));

        let extended: Vec<&(&DayRecord, i64)> =
            newest_first.iter().filter(|(_, age)| *age > 7).collect();
        let recent_7: Vec<&(&DayRecord, i64)> =
            newest_first.iter().filter(|(_, age)| *age <= 7).collect();
        let recent_30: Vec<&(&DayRecord, i64)> =
            newest_first.iter().filter(|(_, age)| *age <= 30).collect();

        let total_days = newest_first.len();
        let total_volume: u64 = newest_first
            .iter()
            .map(|(d, _)| d.intake_ml() as u64)
            .sum();
        let goal_met_days = newest_first
            .iter()
            .filter(|(d, _)| d.intake_ml() >= d.daily_goal_ml)
            .count();

        let avg = |items: &[&(&DayRecord, i64)]| -> Option<u64> {
            if items.is_empty() {
                None
            } else {
                let sum: u64 = items.iter().map(|(d, _)| d.intake_ml() as u64).sum();
                Some(sum / items.len() as u64)
            }
        };
        let avg_all = if total_days == 0 {
            None
        } else {
            Some(total_volume / total_days as u64)
        };
        let avg_ext = avg(&extended);
        let avg_30 = avg(&recent_30);
        let avg_7 = avg(&recent_7);

        let user = if self.state.settings.user_id.is_empty() {
            "(not set)"
        } else {
            self.state.settings.user_id.as_str()
        };

        let mut out = String::new();
        let _ = writeln!(out, "# HYDROCORE HISTORICAL HYDRATION REPORT");
        let _ = writeln!(out, "- Generated (UTC): {now_utc}");
        let _ = writeln!(out, "- Active Date: {}", self.state.date);
        let _ = writeln!(out, "- User ID: {user}");
        let _ = writeln!(out, "- Active Daily Goal: {} ml", self.state.daily_goal_ml);
        let _ = writeln!(out);

        let _ = writeln!(out, "## SUMMARY STATISTICS");
        let _ = writeln!(out, "- Total Archived Days: {total_days}");
        let _ = writeln!(out, "- Extended Archive (> 7 Days Ago): {} days", extended.len());
        let _ = writeln!(out, "- Recent Window (Last 7 Days): {} days", recent_7.len());
        let _ = writeln!(out, "- Total Archived Volume: {total_volume} ml");
        let _ = writeln!(
            out,
            "- All-Time Daily Average: {}",
            avg_all.map(|v| format!("{v} ml")).unwrap_or_else(|| "N/A".into())
        );
        let _ = writeln!(
            out,
            "- Extended (> 7 Days) Daily Average: {}",
            avg_ext.map(|v| format!("{v} ml")).unwrap_or_else(|| "N/A".into())
        );
        let _ = writeln!(
            out,
            "- Last 30 Days Daily Average: {}",
            avg_30.map(|v| format!("{v} ml")).unwrap_or_else(|| "N/A".into())
        );
        let _ = writeln!(
            out,
            "- Last 7 Days Daily Average: {}",
            avg_7.map(|v| format!("{v} ml")).unwrap_or_else(|| "N/A".into())
        );
        let _ = writeln!(out, "- Goal Met: {goal_met_days} / {total_days} days");
        let _ = writeln!(out);

        let today_intake = self.state.intake_ml();
        let today_goal = self.state.daily_goal_ml.max(1);
        let today_pct = (today_intake as u64 * 100) / today_goal as u64;
        let _ = writeln!(out, "## CURRENT DAY (LIVE / UNARCHIVED)");
        let _ = writeln!(out, "- Date: {}", self.state.date);
        let _ = writeln!(
            out,
            "- Intake: {today_intake} / {} ml ({today_pct}%)",
            self.state.daily_goal_ml
        );
        if today_intake >= self.state.daily_goal_ml {
            let _ = writeln!(out, "- Status: GOAL REACHED");
        } else {
            let _ = writeln!(
                out,
                "- Status: {} ml remaining",
                self.state.daily_goal_ml - today_intake
            );
        }
        let _ = writeln!(out, "- Entries ({}):", self.state.entries.len());
        if self.state.entries.is_empty() {
            let _ = writeln!(out, "  * (no entries logged today)");
        } else {
            for entry in &self.state.entries {
                let _ = writeln!(out, "  * {} : +{} ml", entry.timestamp_utc, entry.amount_ml);
            }
        }
        let _ = writeln!(out);

        let write_day_section = |out: &mut String, title: &str, items: &[&(&DayRecord, i64)]| {
            let _ = writeln!(out, "## {title}");
            if items.is_empty() {
                let _ = writeln!(out, "- (no records in this timespan)");
                let _ = writeln!(out);
                return;
            }
            for (day, age) in items {
                let intake = day.intake_ml();
                let goal = day.daily_goal_ml.max(1);
                let pct = (intake as u64 * 100) / goal as u64;
                let _ = writeln!(out, "### {} ({} days ago)", day.date, age);
                let _ = writeln!(out, "- Total Intake: {intake} ml");
                let _ = writeln!(out, "- Daily Goal: {} ml", day.daily_goal_ml);
                if intake >= day.daily_goal_ml {
                    let _ = writeln!(out, "- Goal Status: GOAL REACHED ({pct}%)");
                } else {
                    let _ = writeln!(
                        out,
                        "- Goal Status: SHORT BY {} ml ({pct}%)",
                        day.daily_goal_ml - intake
                    );
                }
                let _ = writeln!(out, "- Logged Entries ({}):", day.entries.len());
                for entry in &day.entries {
                    let _ =
                        writeln!(out, "  * {} : +{} ml", entry.timestamp_utc, entry.amount_ml);
                }
                let _ = writeln!(out);
            }
        };

        write_day_section(
            &mut out,
            "EXTENDED HISTORICAL ARCHIVE (> 7 DAYS AGO)",
            &extended,
        );
        write_day_section(&mut out, "RECENT HISTORICAL ARCHIVE (LAST 7 DAYS)", &recent_7);

        let _ = writeln!(out, "## COMPLETE CHRONOLOGICAL LOG (OLDEST TO NEWEST)");
        if self.state.history.is_empty() {
            let _ = writeln!(out, "- (no archived days)");
        } else {
            for day in &self.state.history {
                let intake = day.intake_ml();
                let status = if intake >= day.daily_goal_ml {
                    "GOAL REACHED"
                } else {
                    "BELOW GOAL"
                };
                let _ = writeln!(
                    out,
                    "- {} : {} / {} ml [{}] ({} entries)",
                    day.date,
                    intake,
                    day.daily_goal_ml,
                    status,
                    day.entries.len()
                );
            }
        }
        out
    }

    /// Writes `hydration_history.txt` atomically in the engine directory and returns its text.
    pub fn export_history_file(&self, now_utc: &str) -> Result<String> {
        let report = self.format_history_report(now_utc);
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(HISTORY_TXT_TMP_FILE);
        let dst = self.dir.join(HISTORY_TXT_FILE);
        {
            let mut file = File::create(&tmp)?;
            file.write_all(report.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &dst)?;
        Ok(report)
    }

    /// Persist first, then swap into memory.
    fn commit(&mut self, next: State) -> Result<()> {
        persist_state(&self.dir, &next)?;
        self.state = next;
        Ok(())
    }
}

fn load_state(dir: &Path) -> State {
    let path = dir.join(STATE_FILE);
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(_) => return State::default(),
    };
    match serde_json::from_slice::<State>(&raw) {
        Ok(mut state) => {
            state.daily_goal_ml = state.daily_goal_ml.clamp(MIN_GOAL_ML, MAX_GOAL_ML);
            state
        }
        Err(_) => {
            // Keep the unreadable file for forensics and start fresh instead of crashing.
            let _ = fs::rename(&path, dir.join(STATE_CORRUPT_FILE));
            State::default()
        }
    }
}

/// Atomic write: temp file in the same directory, fsync, rename over the target.
fn persist_state(dir: &Path, state: &State) -> Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(STATE_TMP_FILE);
    let dst = dir.join(STATE_FILE);
    let json = serde_json::to_vec_pretty(state)?;
    {
        let mut file = File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &dst)?;
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all(); // best effort: make the rename durable
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Networking
// ---------------------------------------------------------------------------------

pub trait Transport {
    /// POSTs `body` as JSON and returns the HTTP status code.
    fn post_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<u16>;
}

/// Blocking pure-Rust (rustls) HTTP client.
pub struct UreqTransport;

impl Transport for UreqTransport {
    fn post_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<u16> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .redirects(0)
            .build();
        let mut request = agent
            .post(url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .set(
                "User-Agent",
                concat!("HydroCore/", env!("CARGO_PKG_VERSION")),
            );
        if let Some(token) = bearer {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        match request.send_string(body) {
            Ok(response) => Ok(response.status()),
            Err(ureq::Error::Status(code, _)) => Ok(code),
            Err(ureq::Error::Transport(t)) => Err(EngineError::Network(t.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------------
// Request dispatch (JSON in, JSON out)
// ---------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Request {
    today: String,
    now_utc: String,
    #[serde(flatten)]
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    Init {
        data_dir: String,
    },
    GetState,
    Add {
        amount_ml: u32,
    },
    Reset,
    SaveSettings {
        user_id: String,
        server_url: String,
        api_token: String,
        daily_goal_ml: u32,
    },
    Sync,
    Export,
}

#[derive(Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<StateView>,
}

impl Response {
    fn success(state: StateView, message: Option<String>) -> Self {
        Response {
            ok: true,
            error: None,
            message,
            report: None,
            state: Some(state),
        }
    }

    fn failure(error: impl ToString, state: Option<StateView>) -> Self {
        Response {
            ok: false,
            error: Some(error.to_string()),
            message: None,
            report: None,
            state,
        }
    }
}

/// Minimal error envelope, usable even when nothing else works.
pub fn error_envelope(message: &str) -> String {
    serde_json::to_string(&Response::failure(message, None))
        .unwrap_or_else(|_| r#"{"ok":false,"error":"internal error"}"#.to_string())
}

/// Owns the engine behind a mutex. The process uses one global instance; tests build
/// their own so they never interfere with each other.
pub struct Runtime {
    engine: Mutex<Option<Engine>>,
}

impl Runtime {
    pub const fn new() -> Self {
        Runtime {
            engine: Mutex::new(None),
        }
    }

    /// Never panics: internal panics are converted into an error envelope.
    pub fn handle(&self, request: &str, transport: &dyn Transport) -> String {
        let response = catch_unwind(AssertUnwindSafe(|| self.handle_inner(request, transport)))
            .unwrap_or_else(|_| Response::failure("internal error (panic)", None));
        serde_json::to_string(&response)
            .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization failure"}"#.to_string())
    }

    /// A panic while holding the lock only poisons it; the state itself is consistent
    /// (copy, persist, swap), so recovering the guard is safe.
    fn lock(&self) -> MutexGuard<'_, Option<Engine>> {
        self.engine.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn handle_inner(&self, raw: &str, transport: &dyn Transport) -> Response {
        let request: Request = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => return Response::failure(format!("bad request: {e}"), None),
        };
        if !is_valid_date(&request.today) {
            return Response::failure(format!("invalid date: {:?}", request.today), None);
        }
        if request.now_utc.len() < 10 || request.now_utc.len() > 40 {
            return Response::failure("invalid timestamp", None);
        }
        let today = request.today.as_str();
        let now = request.now_utc.as_str();

        match request.command {
            Command::Init { data_dir } => match Engine::open(data_dir, today) {
                Ok(engine) => {
                    let view = engine.view();
                    *self.lock() = Some(engine);
                    Response::success(view, None)
                }
                Err(e) => Response::failure(e, None),
            },
            Command::GetState => self.with_engine(today, |_| Ok(None)),
            Command::Add { amount_ml } => {
                self.with_engine(today, |e| e.add_water(amount_ml, now).map(|_| None))
            }
            Command::Reset => self.with_engine(today, |e| e.reset().map(|_| None)),
            Command::SaveSettings {
                user_id,
                server_url,
                api_token,
                daily_goal_ml,
            } => self.with_engine(today, |e| {
                e.save_settings(
                    Settings {
                        user_id,
                        server_url,
                        api_token,
                    },
                    daily_goal_ml,
                )
                .map(|_| Some("Settings saved".to_string()))
            }),
            Command::Sync => self.sync(today, now, transport),
            Command::Export => {
                let mut guard = self.lock();
                let Some(engine) = guard.as_mut() else {
                    return Response::failure(EngineError::NotInitialized, None);
                };
                let outcome = engine
                    .rollover(today)
                    .and_then(|_| engine.export_history_file(now));
                let view = engine.view();
                match outcome {
                    Ok(report) => Response {
                        ok: true,
                        error: None,
                        message: Some("History exported to hydration_history.txt".to_string()),
                        report: Some(report),
                        state: Some(view),
                    },
                    Err(e) => Response::failure(e, Some(view)),
                }
            }
        }
    }

    /// Runs `f` against the engine (after rollover) and always returns the fresh view.
    fn with_engine<F>(&self, today: &str, f: F) -> Response
    where
        F: FnOnce(&mut Engine) -> Result<Option<String>>,
    {
        let mut guard = self.lock();
        let Some(engine) = guard.as_mut() else {
            return Response::failure(EngineError::NotInitialized, None);
        };
        let outcome = engine.rollover(today).and_then(|_| f(engine));
        let view = engine.view();
        match outcome {
            Ok(message) => Response::success(view, message),
            Err(e) => Response::failure(e, Some(view)),
        }
    }

    /// Three phases so the lock is never held across network I/O:
    /// prepare (locked) -> POST (unlocked) -> record result (locked).
    fn sync(&self, today: &str, now: &str, transport: &dyn Transport) -> Response {
        let prepared = {
            let mut guard = self.lock();
            let Some(engine) = guard.as_mut() else {
                return Response::failure(EngineError::NotInitialized, None);
            };
            let prepared = engine
                .rollover(today)
                .and_then(|_| engine.build_sync_request(now));
            match prepared {
                Ok(p) => p,
                Err(e) => return Response::failure(e, Some(engine.view())),
            }
        };

        let status = transport.post_json(
            &prepared.url,
            prepared.bearer_token.as_deref(),
            &prepared.body,
        );

        let mut guard = self.lock();
        let Some(engine) = guard.as_mut() else {
            return Response::failure(EngineError::NotInitialized, None);
        };
        match status {
            Ok(code) if (200..300).contains(&code) => match engine.record_sync_success(now) {
                Ok(()) => Response::success(
                    engine.view(),
                    Some(format!("Synced {} entries", prepared.entry_count)),
                ),
                Err(e) => Response::failure(e, Some(engine.view())),
            },
            Ok(code) => Response::failure(
                format!("server responded with HTTP {code}"),
                Some(engine.view()),
            ),
            Err(e) => Response::failure(e, Some(engine.view())),
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

static RUNTIME: Runtime = Runtime::new();

/// Process-wide entry point used by the JNI bridge.
pub fn handle_request(request: &str) -> String {
    RUNTIME.handle(request, &UreqTransport)
}

// ---------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;
    use tempfile::TempDir;

    const D1: &str = "2026-10-31";
    const D2: &str = "2026-11-01";
    const T1: &str = "2026-10-31T10:00:00Z";

    fn open(dir: &TempDir, today: &str) -> Engine {
        Engine::open(dir.path(), today).unwrap()
    }

    fn settings(url: &str) -> Settings {
        Settings {
            user_id: "alice".into(),
            server_url: url.into(),
            api_token: "secret".into(),
        }
    }

    // --- engine -----------------------------------------------------------------

    #[test]
    fn add_accumulates_and_persists_across_reopen() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.add_water(200, T1).unwrap();
        e.add_water(500, T1).unwrap();
        assert_eq!(e.view().intake_ml, 700);
        drop(e);
        let e = open(&dir, D1);
        assert_eq!(e.view().intake_ml, 700);
        assert_eq!(e.state().entries.len(), 2);
    }

    #[test]
    fn rejects_invalid_amounts() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        assert!(e.add_water(0, T1).is_err());
        assert!(e.add_water(MAX_SINGLE_ADD_ML + 1, T1).is_err());
        for _ in 0..4 {
            e.add_water(MAX_SINGLE_ADD_ML, T1).unwrap();
        }
        assert!(e.add_water(1, T1).is_err(), "daily cap must hold");
        assert_eq!(e.view().intake_ml, MAX_DAILY_INTAKE_ML);
    }

    #[test]
    fn rollover_resets_on_new_day_and_keeps_settings() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(settings("https://example.org/api"), 3000).unwrap();
        e.add_water(250, T1).unwrap();
        assert!(e.rollover(D2).unwrap());
        let v = e.view();
        assert_eq!(v.date, D2);
        assert_eq!(v.intake_ml, 0);
        assert_eq!(v.daily_goal_ml, 3000);
        assert_eq!(v.settings.user_id, "alice");
    }

    #[test]
    fn rollover_applies_on_open_and_ignores_clock_going_backwards() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D2);
        e.add_water(300, T1).unwrap();
        drop(e);
        let mut e = open(&dir, D1); // earlier date: no data loss
        assert_eq!(e.view().intake_ml, 300);
        assert!(!e.rollover(D1).unwrap());
        assert_eq!(e.view().date, D2);
        drop(e);
        let e = open(&dir, "2026-11-02"); // later date: new day
        assert_eq!(e.view().intake_ml, 0);
    }

    #[test]
    fn reset_clears_intake_only() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(settings("http://10.0.0.2:8080/sync"), 2000).unwrap();
        e.add_water(500, T1).unwrap();
        e.reset().unwrap();
        let v = e.view();
        assert_eq!(v.intake_ml, 0);
        assert!(v.entries.is_empty());
        assert_eq!(v.daily_goal_ml, 2000);
    }

    #[test]
    fn progress_is_capped_and_goal_flag_works() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(Settings::default(), 1000).unwrap();
        e.add_water(250, T1).unwrap();
        let v = e.view();
        assert!((v.progress - 0.25).abs() < f32::EPSILON);
        assert_eq!(v.remaining_ml, 750);
        assert!(!v.goal_reached);
        e.add_water(500, T1).unwrap();
        e.add_water(500, T1).unwrap();
        let v = e.view();
        assert_eq!(v.progress, 1.0);
        assert_eq!(v.remaining_ml, 0);
        assert!(v.goal_reached);
    }

    #[test]
    fn settings_validation() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        assert!(e.save_settings(Settings::default(), MIN_GOAL_ML - 1).is_err());
        assert!(e.save_settings(Settings::default(), MAX_GOAL_ML + 1).is_err());
        assert!(e.save_settings(settings("ftp://host"), 2000).is_err());
        assert!(e.save_settings(settings("https://"), 2000).is_err());
        assert!(e.save_settings(settings("http://a b"), 2000).is_err());
        assert!(e.save_settings(settings(""), 2000).is_ok(), "empty URL is allowed");
        let ok = Settings {
            user_id: "  bob ".into(),
            server_url: " https://h.example/x ".into(),
            api_token: " tok ".into(),
        };
        e.save_settings(ok, 2200).unwrap();
        let s = &e.state().settings;
        assert_eq!(
            (s.user_id.as_str(), s.server_url.as_str(), s.api_token.as_str()),
            ("bob", "https://h.example/x", "tok")
        );
    }

    #[test]
    fn corrupt_state_file_is_recovered() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(STATE_FILE), b"{ definitely not json").unwrap();
        let e = open(&dir, D1);
        assert_eq!(e.view().intake_ml, 0);
        assert!(dir.path().join(STATE_CORRUPT_FILE).exists());
    }

    #[test]
    fn persistence_is_atomic_and_leaves_no_temp_file() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.add_water(200, T1).unwrap();
        assert!(dir.path().join(STATE_FILE).exists());
        assert!(!dir.path().join(STATE_TMP_FILE).exists());
        let parsed: State =
            serde_json::from_slice(&fs::read(dir.path().join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn failed_persist_does_not_change_memory() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.add_water(200, T1).unwrap();
        // Make the destination a directory so the final rename must fail.
        fs::remove_file(dir.path().join(STATE_FILE)).unwrap();
        fs::create_dir(dir.path().join(STATE_FILE)).unwrap();
        assert!(e.add_water(300, T1).is_err());
        assert_eq!(e.view().intake_ml, 200);
    }

    #[test]
    fn date_and_url_validators() {
        assert!(is_valid_date("2026-02-28"));
        assert!(!is_valid_date("2026-2-28"));
        assert!(!is_valid_date("2026-13-01"));
        assert!(!is_valid_date("2026-00-10"));
        assert!(!is_valid_date("20261031xx"));
        assert!(!is_valid_date("abcd-ef-gh"));
        assert!(is_valid_server_url("https://a.b"));
        assert!(is_valid_server_url("http://192.168.1.5:8080/x"));
        assert!(!is_valid_server_url("a.b"));
        assert!(!is_valid_server_url("https:///x"));
    }

    #[test]
    fn sync_request_contains_required_fields() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(settings("https://h.example/sync"), 2400).unwrap();
        e.add_water(250, T1).unwrap();
        let req = e.build_sync_request("2026-10-31T11:00:00Z").unwrap();
        assert_eq!(req.url, "https://h.example/sync");
        assert_eq!(req.bearer_token.as_deref(), Some("secret"));
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(v["user_id"], "alice");
        assert_eq!(v["date"], D1);
        assert_eq!(v["intake_ml"], 250);
        assert_eq!(v["daily_goal_ml"], 2400);
        assert_eq!(v["timestamp_utc"], "2026-10-31T11:00:00Z");
        assert_eq!(v["entries"][0]["amount_ml"], 250);
        assert_eq!(v["entries"][0]["timestamp_utc"], T1);
    }

    #[test]
    fn sync_requires_configuration_and_token_is_optional() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        assert!(e.build_sync_request(T1).is_err());
        let mut s = settings("http://h.example");
        s.api_token.clear();
        e.save_settings(s, 2000).unwrap();
        assert_eq!(e.build_sync_request(T1).unwrap().bearer_token, None);
    }

    // --- history (v0.2) -----------------------------------------------------------

    /// Strictly increasing valid dates, so the rollover always moves forward.
    fn nth_date(i: usize) -> String {
        let year = 2000 + i / 336;
        let idx = i % 336;
        format!("{year}-{:02}-{:02}", idx / 28 + 1, idx % 28 + 1)
    }

    #[test]
    fn midnight_rollover_archives_the_finished_day() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(Settings::default(), 2000).unwrap();
        e.add_water(250, T1).unwrap();
        e.add_water(500, T1).unwrap();
        assert!(e.view().history.is_empty(), "today is not history yet");

        assert!(e.rollover(D2).unwrap());
        let v = e.view();
        assert_eq!(v.intake_ml, 0);
        assert_eq!(v.history.len(), 1);
        let day = &v.history[0];
        assert_eq!(day.date, D1);
        assert_eq!(day.intake_ml, 750);
        assert_eq!(day.daily_goal_ml, 2000);
        assert!(!day.goal_reached);
        assert_eq!(day.entries.len(), 2);
    }

    #[test]
    fn archived_day_reports_goal_reached() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.save_settings(Settings::default(), 1000).unwrap();
        e.add_water(500, T1).unwrap();
        e.add_water(500, T1).unwrap();
        e.rollover(D2).unwrap();
        assert!(e.view().history[0].goal_reached);
    }

    #[test]
    fn days_without_intake_are_not_archived() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.rollover(D2).unwrap();
        assert!(e.view().history.is_empty());
    }

    #[test]
    fn history_is_newest_first_and_survives_reopen() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, "2026-10-29");
        e.add_water(100, T1).unwrap();
        e.rollover("2026-10-30").unwrap();
        e.add_water(200, T1).unwrap();
        e.rollover(D1).unwrap();
        e.add_water(300, T1).unwrap();
        drop(e);

        let e = open(&dir, D1);
        let v = e.view();
        assert_eq!(v.intake_ml, 300);
        let dates: Vec<_> = v.history.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(dates, ["2026-10-30", "2026-10-29"]);
        assert_eq!(v.history[0].intake_ml, 200);
    }

    #[test]
    fn reset_and_settings_do_not_touch_history() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D1);
        e.add_water(400, T1).unwrap();
        e.rollover(D2).unwrap();
        e.add_water(100, T1).unwrap();
        e.reset().unwrap();
        e.save_settings(settings("https://h.example"), 3000).unwrap();
        assert_eq!(e.view().history.len(), 1);
        assert_eq!(e.view().history[0].intake_ml, 400);
    }

    #[test]
    fn clock_going_backwards_never_archives_or_duplicates() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, D2);
        e.add_water(300, T1).unwrap();
        assert!(!e.rollover(D1).unwrap());
        assert!(e.view().history.is_empty());
        assert_eq!(e.view().intake_ml, 300);
    }

    #[test]
    fn history_is_capped_to_the_most_recent_days() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, &nth_date(0));
        let total = MAX_HISTORY_DAYS + 20;
        for i in 0..total {
            e.add_water(100, T1).unwrap();
            e.rollover(&nth_date(i + 1)).unwrap();
        }
        let v = e.view();
        assert_eq!(v.history.len(), MAX_HISTORY_DAYS);
        assert_eq!(v.history[0].date, nth_date(total - 1), "newest kept");
        assert_eq!(
            v.history.last().unwrap().date,
            nth_date(total - MAX_HISTORY_DAYS),
            "oldest beyond the cap dropped"
        );
    }

    #[test]
    fn state_file_from_v0_1_without_history_still_loads() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(STATE_FILE),
            r#"{"date":"2026-10-31","daily_goal_ml":2500,
                "entries":[{"timestamp_utc":"2026-10-31T10:00:00Z","amount_ml":250}],
                "settings":{"user_id":"a","server_url":"","api_token":""},
                "last_sync_utc":null}"#,
        )
        .unwrap();
        let e = open(&dir, D1);
        assert_eq!(e.view().intake_ml, 250);
        assert!(e.view().history.is_empty());
        assert!(!dir.path().join(STATE_CORRUPT_FILE).exists());
    }

    #[test]
    fn dispatch_exposes_history_after_midnight() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let t = MockTransport::new(Ok(200));
        init(&rt, &t, &dir);
        call(&rt, &t, serde_json::json!({"cmd":"add","amount_ml":400}));
        let next_day = serde_json::json!({"cmd":"get_state","today":D2,"now_utc":"2026-11-01T00:00:01Z"});
        let r: serde_json::Value = serde_json::from_str(&rt.handle(&next_day.to_string(), &t)).unwrap();
        assert_eq!(r["state"]["intake_ml"], 0);
        assert_eq!(r["state"]["history"][0]["date"], D1);
        assert_eq!(r["state"]["history"][0]["intake_ml"], 400);
        assert_eq!(r["state"]["history"][0]["goal_reached"], false);
    }

    #[test]
    fn export_compiles_extended_history_beyond_seven_days_and_writes_txt_file() {
        let dir = TempDir::new().unwrap();
        let mut e = open(&dir, "2026-10-15");
        e.save_settings(settings("https://h.example/sync"), 2000).unwrap();
        // Day 1: 16 days before 2026-10-31 (> 7 days)
        e.add_water(500, "2026-10-15T09:00:00Z").unwrap();
        e.add_water(1600, "2026-10-15T18:00:00Z").unwrap();
        // Day 2: 10 days before 2026-10-31 (> 7 days)
        e.rollover("2026-10-21").unwrap();
        e.add_water(1200, "2026-10-21T12:00:00Z").unwrap();
        // Day 3: 2 days before 2026-10-31 (within last 7 days)
        e.rollover("2026-10-29").unwrap();
        e.add_water(2000, "2026-10-29T15:00:00Z").unwrap();
        // Active day: 2026-10-31
        e.rollover(D1).unwrap();
        e.add_water(300, T1).unwrap();

        let report = e.export_history_file("2026-10-31T22:00:00Z").unwrap();
        let saved = fs::read_to_string(dir.path().join(HISTORY_TXT_FILE)).unwrap();
        assert_eq!(report, saved);
        assert!(!dir.path().join(HISTORY_TXT_TMP_FILE).exists());

        assert!(report.contains("# HYDROCORE HISTORICAL HYDRATION REPORT"));
        assert!(report.contains("- Total Archived Days: 3"));
        assert!(report.contains("- Extended Archive (> 7 Days Ago): 2 days"));
        assert!(report.contains("- Recent Window (Last 7 Days): 1 days"));
        assert!(report.contains("## EXTENDED HISTORICAL ARCHIVE (> 7 DAYS AGO)"));
        assert!(report.contains("### 2026-10-21 (10 days ago)"));
        assert!(report.contains("### 2026-10-15 (16 days ago)"));
        assert!(report.contains("2026-10-15T09:00:00Z : +500 ml"));
        assert!(report.contains("2026-10-15T18:00:00Z : +1600 ml"));
        assert!(report.contains("## RECENT HISTORICAL ARCHIVE (LAST 7 DAYS)"));
        assert!(report.contains("### 2026-10-29 (2 days ago)"));
        assert!(report.contains("## COMPLETE CHRONOLOGICAL LOG (OLDEST TO NEWEST)"));
    }

    #[test]
    fn dispatch_export_returns_report_and_saves_file() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let t = MockTransport::new(Ok(200));
        init(&rt, &t, &dir);
        call(&rt, &t, serde_json::json!({"cmd":"add","amount_ml":500}));
        let r = call(&rt, &t, serde_json::json!({"cmd":"export"}));
        assert_eq!(r["ok"], true);
        assert!(r["report"].as_str().unwrap().contains("# HYDROCORE HISTORICAL HYDRATION REPORT"));
        assert!(dir.path().join(HISTORY_TXT_FILE).exists());
    }

    // --- runtime / dispatch -----------------------------------------------------

    struct MockTransport {
        reply: Result<u16>,
        seen: RefCell<Vec<(String, Option<String>, String)>>,
    }

    impl MockTransport {
        fn new(reply: Result<u16>) -> Self {
            MockTransport {
                reply,
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl Transport for MockTransport {
        fn post_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<u16> {
            self.seen
                .borrow_mut()
                .push((url.into(), bearer.map(Into::into), body.into()));
            self.reply.clone()
        }
    }

    struct PanicTransport;
    impl Transport for PanicTransport {
        fn post_json(&self, _: &str, _: Option<&str>, _: &str) -> Result<u16> {
            panic!("boom");
        }
    }

    fn call(rt: &Runtime, t: &dyn Transport, body: serde_json::Value) -> serde_json::Value {
        let mut body = body;
        body["today"] = D1.into();
        body["now_utc"] = T1.into();
        serde_json::from_str(&rt.handle(&body.to_string(), t)).unwrap()
    }

    fn init(rt: &Runtime, t: &dyn Transport, dir: &TempDir) -> serde_json::Value {
        call(
            rt,
            t,
            serde_json::json!({"cmd":"init","data_dir": dir.path().to_str().unwrap()}),
        )
    }

    #[test]
    fn dispatch_full_flow() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let t = MockTransport::new(Ok(200));

        let r = init(&rt, &t, &dir);
        assert_eq!(r["ok"], true);
        assert_eq!(r["state"]["daily_goal_ml"], DEFAULT_GOAL_ML);

        let r = call(&rt, &t, serde_json::json!({"cmd":"add","amount_ml":250}));
        assert_eq!(r["ok"], true);
        assert_eq!(r["state"]["intake_ml"], 250);

        let r = call(&rt, &t, serde_json::json!({"cmd":"get_state"}));
        assert_eq!(r["state"]["intake_ml"], 250);

        let r = call(&rt, &t, serde_json::json!({
            "cmd":"save_settings","user_id":"alice","server_url":"https://h.example/s",
            "api_token":"tok","daily_goal_ml":3000
        }));
        assert_eq!(r["ok"], true);
        assert_eq!(r["state"]["daily_goal_ml"], 3000);

        let r = call(&rt, &t, serde_json::json!({"cmd":"sync"}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["message"], "Synced 1 entries");
        assert_eq!(r["state"]["last_sync_utc"], T1);
        let seen = t.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "https://h.example/s");
        assert_eq!(seen[0].1.as_deref(), Some("tok"));

        let r = call(&rt, &t, serde_json::json!({"cmd":"reset"}));
        assert_eq!(r["state"]["intake_ml"], 0);
    }

    #[test]
    fn dispatch_sync_failures_keep_state_and_report_errors() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let ok = MockTransport::new(Ok(200));
        init(&rt, &ok, &dir);
        call(&rt, &ok, serde_json::json!({"cmd":"add","amount_ml":200}));

        // Not configured yet.
        let r = call(&rt, &ok, serde_json::json!({"cmd":"sync"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["state"]["intake_ml"], 200);
        assert!(ok.seen.borrow().is_empty(), "must not hit the network");

        call(&rt, &ok, serde_json::json!({
            "cmd":"save_settings","user_id":"a","server_url":"http://h.example","api_token":"","daily_goal_ml":2000
        }));

        let server_error = MockTransport::new(Ok(500));
        let r = call(&rt, &server_error, serde_json::json!({"cmd":"sync"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "server responded with HTTP 500");
        assert!(r["state"]["last_sync_utc"].is_null());

        let down = MockTransport::new(Err(EngineError::Network("connection refused".into())));
        let r = call(&rt, &down, serde_json::json!({"cmd":"sync"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "network error: connection refused");
    }

    #[test]
    fn dispatch_rejects_bad_input_without_panicking() {
        let rt = Runtime::new();
        let t = MockTransport::new(Ok(200));
        let parse = |s: &str| -> serde_json::Value { serde_json::from_str(&rt.handle(s, &t)).unwrap() };

        assert_eq!(parse("not json")["ok"], false);
        assert_eq!(parse("{}")["ok"], false);
        assert_eq!(parse(r#"{"cmd":"nope","today":"2026-10-31","now_utc":"2026-10-31T10:00:00Z"}"#)["ok"], false);
        assert_eq!(parse(r#"{"cmd":"get_state","today":"31/10/2026","now_utc":"2026-10-31T10:00:00Z"}"#)["ok"], false);
        let r = parse(r#"{"cmd":"get_state","today":"2026-10-31","now_utc":"2026-10-31T10:00:00Z"}"#);
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "engine is not initialized");
    }

    #[test]
    fn dispatch_survives_panics_and_stays_usable() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let ok = MockTransport::new(Ok(200));
        init(&rt, &ok, &dir);
        call(&rt, &ok, serde_json::json!({
            "cmd":"save_settings","user_id":"a","server_url":"http://h.example","api_token":"","daily_goal_ml":2000
        }));
        let r = call(&rt, &PanicTransport, serde_json::json!({"cmd":"sync"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"], "internal error (panic)");
        let r = call(&rt, &ok, serde_json::json!({"cmd":"add","amount_ml":200}));
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn dispatch_applies_rollover_on_every_call() {
        let dir = TempDir::new().unwrap();
        let rt = Runtime::new();
        let t = MockTransport::new(Ok(200));
        init(&rt, &t, &dir);
        call(&rt, &t, serde_json::json!({"cmd":"add","amount_ml":400}));
        let next_day = serde_json::json!({"cmd":"get_state","today":D2,"now_utc":"2026-11-01T08:00:00Z"});
        let r: serde_json::Value = serde_json::from_str(&rt.handle(&next_day.to_string(), &t)).unwrap();
        assert_eq!(r["state"]["date"], D2);
        assert_eq!(r["state"]["intake_ml"], 0);
    }

    // --- real HTTP (ureq + local socket) -----------------------------------------

    fn request_complete(buf: &[u8]) -> bool {
        let text = String::from_utf8_lossy(buf);
        match text.find("\r\n\r\n") {
            Some(idx) => {
                let len = text[..idx]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                text.len() >= idx + 4 + len
            }
            None => false,
        }
    }

    fn spawn_server(status: u16) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if request_complete(&buf) {
                    break;
                }
            }
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });
        (format!("http://{addr}/sync"), handle)
    }

    #[test]
    fn ureq_transport_posts_json_with_bearer_auth() {
        let (url, server) = spawn_server(200);
        let status = UreqTransport
            .post_json(&url, Some("tok123"), r#"{"user_id":"alice"}"#)
            .unwrap();
        assert_eq!(status, 200);
        let raw = server.join().unwrap();
        let lower = raw.to_ascii_lowercase();
        assert!(raw.starts_with("POST /sync HTTP/1.1"), "{raw}");
        assert!(lower.contains("authorization: bearer tok123"), "{raw}");
        assert!(lower.contains("content-type: application/json"), "{raw}");
        assert!(raw.ends_with(r#"{"user_id":"alice"}"#), "{raw}");
    }

    #[test]
    fn ureq_transport_reports_http_errors_and_connection_failures() {
        let (url, server) = spawn_server(503);
        assert_eq!(UreqTransport.post_json(&url, None, "{}").unwrap(), 503);
        let raw = server.join().unwrap();
        assert!(!raw.to_ascii_lowercase().contains("authorization:"), "{raw}");

        // Bind then drop to get a port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = UreqTransport
            .post_json(&format!("http://127.0.0.1:{port}/"), None, "{}")
            .unwrap_err();
        assert!(matches!(err, EngineError::Network(_)), "{err:?}");
    }
}
