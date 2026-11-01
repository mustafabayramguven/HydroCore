//! HTTP handlers: the sync endpoint used by the app and the read API used by the dashboard.

use crate::db::{self, DayRow, IncomingDay, UpsertOutcome};
use crate::stats;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

// Deliberately more lenient than the app's own limits so that newer app versions keep
// syncing; these only guard against garbage and abuse.
const MAX_USER_ID_CHARS: usize = 64;
const MAX_ENTRIES: usize = 5_000;
const MAX_AMOUNT_ML: u32 = 100_000;
const MAX_GOAL_ML: u32 = 100_000;

type Shared = State<Arc<AppState>>;

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        ApiError(StatusCode::UNPROCESSABLE_ENTITY, msg.into())
    }
    fn internal(msg: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {msg}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "ok": false, "error": self.1 }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn parse_date(s: &str) -> Option<NaiveDate> {
    // Length check rejects non-padded forms such as `2026-1-1`.
    (s.len() == 10).then(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).flatten()
}

fn normalize_ts(s: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------------------------
// POST /api/sync   (also accepted on POST /)
// ---------------------------------------------------------------------------------

#[derive(Deserialize)]
struct SyncPayload {
    user_id: String,
    date: String,
    #[serde(default)]
    intake_ml: Option<u32>,
    daily_goal_ml: u32,
    timestamp_utc: String,
    #[serde(default)]
    entries: Vec<EntryIn>,
}

#[derive(Deserialize)]
struct EntryIn {
    timestamp_utc: String,
    amount_ml: u32,
}

fn validate(p: SyncPayload) -> ApiResult<IncomingDay> {
    let user_id = p.user_id.trim().to_string();
    if user_id.is_empty() || user_id.chars().count() > MAX_USER_ID_CHARS || user_id.chars().any(char::is_control) {
        return Err(ApiError::bad(format!(
            "user_id must be 1-{MAX_USER_ID_CHARS} characters without control characters"
        )));
    }
    if parse_date(&p.date).is_none() {
        return Err(ApiError::bad("date must be a valid YYYY-MM-DD"));
    }
    if p.daily_goal_ml == 0 || p.daily_goal_ml > MAX_GOAL_ML {
        return Err(ApiError::bad(format!("daily_goal_ml must be between 1 and {MAX_GOAL_ML}")));
    }
    let client_ts = normalize_ts(&p.timestamp_utc)
        .ok_or_else(|| ApiError::bad("timestamp_utc must be an RFC 3339 timestamp"))?;
    if p.entries.len() > MAX_ENTRIES {
        return Err(ApiError::bad(format!("at most {MAX_ENTRIES} entries per day")));
    }
    let mut entries = Vec::with_capacity(p.entries.len());
    let mut sum: u64 = 0;
    for e in &p.entries {
        if e.amount_ml == 0 || e.amount_ml > MAX_AMOUNT_ML {
            return Err(ApiError::bad(format!("entry amount_ml must be between 1 and {MAX_AMOUNT_ML}")));
        }
        let ts = normalize_ts(&e.timestamp_utc)
            .ok_or_else(|| ApiError::bad("entry timestamp_utc must be an RFC 3339 timestamp"))?;
        sum += u64::from(e.amount_ml);
        entries.push((ts, e.amount_ml));
    }
    // The app derives intake from its entries; do the same so the two can never disagree.
    let intake = if entries.is_empty() { u64::from(p.intake_ml.unwrap_or(0)) } else { sum };
    let intake_ml = u32::try_from(intake).map_err(|_| ApiError::bad("intake_ml is too large"))?;
    Ok(IncomingDay {
        user_id,
        date: p.date,
        intake_ml,
        daily_goal_ml: p.daily_goal_ml,
        client_ts,
        entries,
    })
}

pub async fn sync(State(state): Shared, body: Bytes) -> ApiResult<Json<Value>> {
    let payload: SyncPayload =
        serde_json::from_slice(&body).map_err(|e| ApiError::bad(format!("invalid JSON body: {e}")))?;
    let day = validate(payload)?;
    let summary = json!({
        "user_id": day.user_id, "date": day.date,
        "intake_ml": day.intake_ml, "entries": day.entries.len(),
    });
    let log = (day.user_id.clone(), day.date.clone(), day.intake_ml, day.entries.len());
    let outcome = state
        .db
        .run(move |conn| db::upsert_day(conn, &day, &now_utc()))
        .await
        .map_err(ApiError::internal)?;
    let (stored, status) = match outcome {
        UpsertOutcome::Stored => (true, "stored"),
        UpsertOutcome::Stale => (false, "ignored_stale"),
    };
    tracing::info!("sync user={:?} date={} intake={}ml entries={} -> {status}", log.0, log.1, log.2, log.3);
    Ok(Json(json!({ "ok": true, "status": status, "stored": stored, "day": summary })))
}

// ---------------------------------------------------------------------------------
// Read API
// ---------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DaysQuery {
    user: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Deserialize)]
pub struct UserQuery {
    user: Option<String>,
    today: Option<String>,
}

fn day_json(d: &DayRow) -> Value {
    json!({
        "date": d.date,
        "intake_ml": d.intake_ml,
        "daily_goal_ml": d.daily_goal_ml,
        "goal_reached": d.intake_ml >= d.daily_goal_ml,
        "entry_count": d.entry_count,
        "last_sync_utc": d.last_sync_utc,
    })
}

/// Uses the requested user, or the most recently synced one when none is given.
async fn resolve_user(state: &AppState, requested: Option<String>) -> ApiResult<Option<String>> {
    if let Some(u) = requested.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        return Ok(Some(u));
    }
    let users = state.db.run(|c| db::list_users(c)).await.map_err(ApiError::internal)?;
    Ok(users.into_iter().next().map(|u| u.user_id))
}

fn check_opt_date(name: &str, v: &Option<String>) -> ApiResult<()> {
    match v {
        Some(s) if parse_date(s).is_none() => Err(ApiError::bad(format!("{name} must be a valid YYYY-MM-DD"))),
        _ => Ok(()),
    }
}

pub async fn users(State(state): Shared) -> ApiResult<Json<Value>> {
    let rows = state.db.run(|c| db::list_users(c)).await.map_err(ApiError::internal)?;
    let users: Vec<Value> = rows
        .iter()
        .map(|u| json!({ "user_id": u.user_id, "days": u.days, "last_sync_utc": u.last_sync_utc }))
        .collect();
    Ok(Json(json!({ "users": users })))
}

pub async fn days(State(state): Shared, Query(q): Query<DaysQuery>) -> ApiResult<Json<Value>> {
    check_opt_date("from", &q.from)?;
    check_opt_date("to", &q.to)?;
    let Some(user) = resolve_user(&state, q.user).await? else {
        return Ok(Json(json!({ "user_id": null, "days": [] })));
    };
    let u = user.clone();
    let rows = state
        .db
        .run(move |c| db::list_days(c, &u, q.from.as_deref(), q.to.as_deref()))
        .await
        .map_err(ApiError::internal)?;
    let days: Vec<Value> = rows.iter().map(day_json).collect();
    Ok(Json(json!({ "user_id": user, "days": days })))
}

pub async fn day(
    State(state): Shared,
    Path(date): Path<String>,
    Query(q): Query<UserQuery>,
) -> ApiResult<Response> {
    if parse_date(&date).is_none() {
        return Err(ApiError::bad("date must be a valid YYYY-MM-DD"));
    }
    let Some(user) = resolve_user(&state, q.user).await? else {
        return Ok(not_found());
    };
    let (u, d) = (user.clone(), date.clone());
    let found = state.db.run(move |c| db::get_day(c, &u, &d)).await.map_err(ApiError::internal)?;
    Ok(match found {
        None => not_found(),
        Some((row, entries)) => {
            let mut v = day_json(&row);
            v["user_id"] = json!(user);
            v["entries"] = entries
                .iter()
                .map(|e| json!({ "timestamp_utc": e.timestamp_utc, "amount_ml": e.amount_ml }))
                .collect();
            Json(v).into_response()
        }
    })
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "ok": false, "error": "no data for that day" }))).into_response()
}

pub async fn summary(State(state): Shared, Query(q): Query<UserQuery>) -> ApiResult<Json<Value>> {
    check_opt_date("today", &q.today)?;
    let Some(user) = resolve_user(&state, q.user).await? else {
        return Ok(Json(json!({ "user_id": null, "summary": null })));
    };
    let u = user.clone();
    let rows = state.db.run(move |c| db::list_days(c, &u, None, None)).await.map_err(ApiError::internal)?;
    // The server has no idea what "today" is for the user (time zones), so the dashboard
    // passes its local date. Fall back to the newest stored day.
    let today = q
        .today
        .as_deref()
        .and_then(parse_date)
        .or_else(|| rows.last().and_then(|d| parse_date(&d.date)))
        .unwrap_or_else(|| Utc::now().date_naive());
    Ok(Json(json!({ "user_id": user, "today": today.to_string(), "summary": stats::summarize(&rows, today) })))
}

pub async fn export_csv(State(state): Shared, Query(q): Query<UserQuery>) -> ApiResult<Response> {
    let Some(user) = resolve_user(&state, q.user).await? else {
        return Ok(not_found());
    };
    let u = user.clone();
    let rows = state.db.run(move |c| db::export_entries(c, &u)).await.map_err(ApiError::internal)?;
    let mut out = String::from("date,timestamp_utc,amount_ml,daily_goal_ml\n");
    for (date, ts, amount, goal) in rows {
        out.push_str(&format!("{date},{ts},{amount},{goal}\n"));
    }
    let safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let disposition = format!("attachment; filename=\"hydrocore-{safe}.csv\"");
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        out,
    )
        .into_response())
}

pub async fn healthz(State(state): Shared) -> Response {
    match state.db.run(|c| c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))).await {
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response(),
    }
}
