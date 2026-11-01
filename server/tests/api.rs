use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use hydrocore_server::{build_app, config::Config, db, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn app_with(token: Option<&str>, password: Option<&str>) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        db_path: dir.path().join("t.db"),
        api_token: token.map(String::from),
        dashboard_user: "hydro".into(),
        dashboard_password: password.map(String::from),
    };
    let conn = db::open(&config.db_path).unwrap();
    let state = Arc::new(AppState { db: db::Db::new(conn), config });
    (build_app(state), dir)
}

async fn call(app: &Router, req: Request<Body>) -> (StatusCode, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn post(path: &str, body: &Value, auth: Option<&str>) -> Request<Body> {
    let mut b = Request::post(path).header(header::CONTENT_TYPE, "application/json");
    if let Some(a) = auth {
        b = b.header(header::AUTHORIZATION, a);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

async fn get_json(app: &Router, path: &str) -> Value {
    let (st, body) = call(app, get(path)).await;
    assert_eq!(st, StatusCode::OK, "{path}: {body}");
    serde_json::from_str(&body).unwrap()
}

/// The exact shape documented in the HydroCore README.
fn payload(date: &str, ts: &str, entries: &[(&str, u32)]) -> Value {
    json!({
        "user_id": "alice", "date": date,
        "intake_ml": entries.iter().map(|e| e.1).sum::<u32>(),
        "daily_goal_ml": 2500, "timestamp_utc": ts,
        "entries": entries.iter().map(|(t, a)| json!({"timestamp_utc": t, "amount_ml": a})).collect::<Vec<_>>(),
    })
}

#[tokio::test]
async fn readme_payload_is_accepted_and_readable() {
    let (app, _d) = app_with(None, None);
    let body = json!({
        "user_id": "alice", "date": "2026-10-31", "intake_ml": 750, "daily_goal_ml": 2500,
        "timestamp_utc": "2026-10-31T17:00:00Z",
        "entries": [{ "timestamp_utc": "2026-10-31T10:00:00Z", "amount_ml": 750 }]
    });
    let (st, resp) = call(&app, post("/api/sync", &body, None)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert_eq!(serde_json::from_str::<Value>(&resp).unwrap()["status"], "stored");

    let day = get_json(&app, "/api/days/2026-10-31?user=alice").await;
    assert_eq!(day["intake_ml"], 750);
    assert_eq!(day["goal_reached"], false);
    assert_eq!(day["entries"][0]["amount_ml"], 750);

    let users = get_json(&app, "/api/users").await;
    assert_eq!(users["users"][0]["user_id"], "alice");
    // POST / works too, for people who only enter the host in the app.
    let (st, _) = call(&app, post("/", &body, None)).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn resync_replaces_the_day_and_reset_propagates() {
    let (app, _d) = app_with(None, None);
    let d = "2026-10-31";
    call(&app, post("/api/sync", &payload(d, "2026-10-31T10:00:00Z", &[("2026-10-31T09:00:00Z", 300)]), None)).await;
    let two = [("2026-10-31T09:00:00Z", 300), ("2026-10-31T11:00:00Z", 500)];
    call(&app, post("/api/sync", &payload(d, "2026-10-31T12:00:00Z", &two), None)).await;
    // Same snapshot again: idempotent, no duplicate entries.
    call(&app, post("/api/sync", &payload(d, "2026-10-31T12:00:00Z", &two), None)).await;
    let day = get_json(&app, "/api/days/2026-10-31?user=alice").await;
    assert_eq!(day["intake_ml"], 800);
    assert_eq!(day["entries"].as_array().unwrap().len(), 2);

    // The app's "Reset" empties the day; the next sync must clear it here too.
    call(&app, post("/api/sync", &payload(d, "2026-10-31T13:00:00Z", &[]), None)).await;
    let day = get_json(&app, "/api/days/2026-10-31?user=alice").await;
    assert_eq!(day["intake_ml"], 0);
    assert_eq!(day["entries"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn out_of_order_sync_does_not_overwrite_newer_data() {
    let (app, _d) = app_with(None, None);
    let new = [("2026-10-31T09:00:00Z", 900)];
    call(&app, post("/api/sync", &payload("2026-10-31", "2026-10-31T20:00:00Z", &new), None)).await;
    let (st, resp) = call(
        &app,
        post("/api/sync", &payload("2026-10-31", "2026-10-31T08:00:00Z", &[("2026-10-31T07:00:00Z", 100)]), None),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(serde_json::from_str::<Value>(&resp).unwrap()["status"], "ignored_stale");
    assert_eq!(get_json(&app, "/api/days/2026-10-31?user=alice").await["intake_ml"], 900);
}

#[tokio::test]
async fn rejects_invalid_payloads() {
    let (app, _d) = app_with(None, None);
    let ok = payload("2026-10-31", "2026-10-31T10:00:00Z", &[("2026-10-31T09:00:00Z", 300)]);
    for (field, value) in [
        ("user_id", json!("  ")),
        ("date", json!("2026-02-30")),
        ("date", json!("2026-1-1")),
        ("daily_goal_ml", json!(0)),
        ("timestamp_utc", json!("yesterday")),
        ("entries", json!([{ "timestamp_utc": "2026-10-31T09:00:00Z", "amount_ml": 0 }])),
    ] {
        let mut bad = ok.clone();
        bad[field] = value.clone();
        let (st, _) = call(&app, post("/api/sync", &bad, None)).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{field}={value}");
    }
    let (st, _) = call(&app, Request::post("/api/sync").body(Body::from("not json")).unwrap()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(get_json(&app, "/api/users").await["users"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn bearer_token_protects_sync_and_basic_auth_protects_dashboard() {
    let (app, _d) = app_with(Some("tok"), Some("pw"));
    let body = payload("2026-10-31", "2026-10-31T10:00:00Z", &[("2026-10-31T09:00:00Z", 300)]);

    let (st, _) = call(&app, post("/api/sync", &body, None)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&app, post("/api/sync", &body, Some("Bearer nope"))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&app, post("/api/sync", &body, Some("Bearer tok"))).await;
    assert_eq!(st, StatusCode::OK);

    let res = app.clone().oneshot(get("/api/users")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(res.headers()[header::WWW_AUTHENTICATE].to_str().unwrap().starts_with("Basic"));
    // "hydro:pw" in base64
    let req = Request::get("/api/users").header(header::AUTHORIZATION, "Basic aHlkcm86cHc=").body(Body::empty()).unwrap();
    assert_eq!(call(&app, req).await.0, StatusCode::OK);
    let req = Request::get("/").header(header::AUTHORIZATION, "Basic aHlkcm86eHg=").body(Body::empty()).unwrap();
    assert_eq!(call(&app, req).await.0, StatusCode::UNAUTHORIZED);
    // Health check stays open for Docker.
    assert_eq!(call(&app, get("/healthz")).await.0, StatusCode::OK);
}

#[tokio::test]
async fn dashboard_assets_are_embedded() {
    let (app, _d) = app_with(None, None);
    for (path, needle) in [("/", "<title>HydroCore</title>"), ("/app.js", "HydroCore dashboard"), ("/style.css", "--accent")] {
        let (st, body) = call(&app, get(path)).await;
        assert_eq!(st, StatusCode::OK, "{path}");
        assert!(body.contains(needle), "{path}");
    }
}

#[tokio::test]
async fn summary_days_range_and_csv() {
    let (app, _d) = app_with(None, None);
    for (date, amount) in [("2026-10-29", 2600), ("2026-10-30", 2500), ("2026-10-31", 1000)] {
        let ts = format!("{date}T10:00:00Z");
        call(&app, post("/api/sync", &payload(date, &ts, &[(&ts, amount)]), None)).await;
    }
    let s = get_json(&app, "/api/summary?user=alice&today=2026-10-31").await;
    assert_eq!(s["summary"]["current_streak_days"], 2);
    assert_eq!(s["summary"]["today"]["intake_ml"], 1000);
    assert_eq!(s["summary"]["avg_last7_ml"], 2550);

    let d = get_json(&app, "/api/days?user=alice&from=2026-10-30&to=2026-10-31").await;
    assert_eq!(d["days"].as_array().unwrap().len(), 2);
    // `user` is optional: it defaults to the most recently synced one.
    assert_eq!(get_json(&app, "/api/days").await["user_id"], "alice");

    let (st, csv) = call(&app, get("/api/export.csv?user=alice")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(csv.starts_with("date,timestamp_utc,amount_ml,daily_goal_ml\n2026-10-29,2026-10-29T10:00:00Z,2600,2500\n"), "{csv}");
    assert_eq!(call(&app, get("/api/days/2026-01-01?user=alice")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, get("/api/days?from=bad")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn data_persists_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.db");
    let mk = || {
        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            db_path: path.clone(),
            api_token: None,
            dashboard_user: "hydro".into(),
            dashboard_password: None,
        };
        let conn = db::open(&config.db_path).unwrap();
        build_app(Arc::new(AppState { db: db::Db::new(conn), config }))
    };
    let app = mk();
    call(&app, post("/api/sync", &payload("2026-10-31", "2026-10-31T10:00:00Z", &[("2026-10-31T09:00:00Z", 300)]), None)).await;
    drop(app);
    let app = mk();
    assert_eq!(get_json(&app, "/api/days/2026-10-31?user=alice").await["intake_ml"], 300);
}

#[tokio::test]
async fn batch_history_sync_and_midnight_rollover_preserve_past_days() {
    let (app, _d) = app_with(None, None);
    // Simulate a midnight rollover sync at 2026-11-01T00:00:01Z:
    // - 2026-10-30 and 2026-10-31 are in `history` with their full completed snapshots
    // - 2026-11-01 is the new active day with 0 ml and no entries yet
    let body = json!({
        "user_id": "alice",
        "date": "2026-11-01",
        "intake_ml": 0,
        "daily_goal_ml": 2500,
        "timestamp_utc": "2026-11-01T00:00:01Z",
        "entries": [],
        "history": [
            {
                "date": "2026-10-30",
                "intake_ml": 2000,
                "daily_goal_ml": 2500,
                "timestamp_utc": "2026-11-01T00:00:01Z",
                "entries": [
                    { "timestamp_utc": "2026-10-30T09:00:00Z", "amount_ml": 1000 },
                    { "timestamp_utc": "2026-10-30T18:00:00Z", "amount_ml": 1000 }
                ]
            },
            {
                "date": "2026-10-31",
                "intake_ml": 2750,
                "daily_goal_ml": 2500,
                "timestamp_utc": "2026-11-01T00:00:01Z",
                "entries": [
                    { "timestamp_utc": "2026-10-31T10:00:00Z", "amount_ml": 1250 },
                    { "timestamp_utc": "2026-10-31T19:00:00Z", "amount_ml": 1500 }
                ]
            }
        ]
    });
    let (st, resp) = call(&app, post("/api/sync", &body, None)).await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["synced_days"], 2);

    let d30 = get_json(&app, "/api/days/2026-10-30?user=alice").await;
    assert_eq!(d30["intake_ml"], 2000);
    assert_eq!(d30["entries"].as_array().unwrap().len(), 2);

    let d31 = get_json(&app, "/api/days/2026-10-31?user=alice").await;
    assert_eq!(d31["intake_ml"], 2750);
    assert_eq!(d31["goal_reached"], true);
    assert_eq!(d31["entries"].as_array().unwrap().len(), 2);

    // And the unstarted new day (2026-11-01 with 0 entries) did not create a bogus 0 ml record.
    let days = get_json(&app, "/api/days?user=alice").await;
    assert_eq!(days["days"].as_array().unwrap().len(), 2);
}
