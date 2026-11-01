# HydroCore

Open-source Android hydration tracker (**v0.3**). All hydration logic runs in a **Rust core**; Kotlin/Jetpack Compose is a thin Material You UI. Data stays on the device and syncs to **your own self-hosted companion server** automatically at midnight (`00:00` local time) or whenever you tap **Manual Sync**.

## What's new in 0.3

* **Companion Server & URL Normalization (`POST /api/sync`).** Enter either a base URL (`https://hydro.example.com`, `http://192.168.1.10:8080`) or the full `/api/sync` endpoint in Settings; the core automatically normalizes it to `/api/sync`, sends `Authorization: Bearer <token>`, and follows HTTP/HTTPS `301`/`302`/`307`/`308` redirects while preserving the `POST` body and headers.
* **Full History & Unsynced Days Sync.** Every sync includes the local `history` archive alongside the active day, so past days (or days logged while offline) are never lost.
* **Automatic Midnight Sync & Offline Resilience.** A nightly `WorkManager` job (`MidnightSyncWorker`) runs at `00:00` local time (complemented by rollover sync on app launch/resume), finalizes and archives the completed day's snapshot into `state.json` before resetting the new day's counter, and syncs it to the companion server. If the device has no internet connection at midnight, a network-constrained retry job (`NetworkType.CONNECTED`) queues automatically and syncs as soon as connectivity returns.
* **Manual Sync in Settings & Today Screen.** Trigger an immediate full sync and test your server connection directly inside the Settings dialog or from the Today screen.

## What's new in 0.2

**History ("watch back").** Tap the calendar icon on the main screen to see a 7-day chart, summary stats (7-day average, 30-day average, >7-day average, and days the goal was met), and past days categorized into **Recent (last 7 days)** and **Extended archive (> 7 days ago)**. Tap a day to expand its individual drinks. Back (including the predictive back gesture) returns to today.

**Plain-text history export (`Export .txt`).** Tap **Export .txt** on the History screen to compile all archived records (up to 365 days, separating records > 7 days old from the recent 7-day window, plus a complete chronological log and every individual drink entry without truncation) into a structured plain-text report. It writes `hydration_history.txt` atomically in app storage and opens a preview dialog where you can save a `.txt` file (`hydrocore-history-YYYY-MM-DD.txt`) anywhere on your phone or copy the plain text to the clipboard.

**Midnight save.** Every drink is written to `state.json` the moment you add it. When the date changes at 00:00, the core archives the finished day (total, goal and entries) into the history and starts a fresh day. Because the app has no background component, the archive happens:
* immediately at 00:00 if the app is open on screen (a foreground-only timer, cancelled when the app stops), or
* on the next launch/resume otherwise.

Either way the day is stored under its own date, so nothing is lost or mis-dated. Days with no logged drinks are not archived. The last 365 days are kept. Existing 0.1 data is migrated automatically. The sync payload is unchanged, so your server needs no changes.

## Get the APK (no local tools needed)

1. Push this repository to GitHub.
2. Open **Actions → Build HydroCore** (it runs on every push, or use **Run workflow**).
3. When it finishes, download the **HydroCore-debug.apk** artifact and install it.

The workflow installs Rust, the Android targets, `cargo-ndk`, the SDK/NDK and Gradle itself, runs `cargo test` and the Gradle unit tests, builds the native libraries with 16 KB page alignment (verified with `llvm-readelf`), assembles the APK and uploads it.

## Architecture

```
Compose UI ─ HydrationViewModel ─ HydrationEngine (interface)
                                        │
                              NativeHydrationEngine ── JSON ──► NativeTransport (JNI)
                                                                        │
                                                         rust/src/jni_bridge.rs
                                                                        │
                                          rust/src/lib.rs: Runtime ► Engine ► state.json
                                                                   └► ureq + rustls (POST)
```

* **Rust core** (`rust/`): state, automatic daily rollover, atomic `state.json` persistence (temp file, fsync, rename), goal maths, HTTP sync via `ureq` + `rustls` (no OpenSSL).
* **JNI bridge**: one function, `nativeDispatch(String): String?`, JSON in / JSON out. Panic-safe (`catch_unwind`) and thread-safe (mutex; never held during network I/O).
* **Clock**: Kotlin passes the local date and the UTC timestamp with each request, so the core is deterministic and testable.

## Sync contract

`POST <Server URL>/api/sync` with `Content-Type: application/json` and, if a token is set, `Authorization: Bearer <token>`:

```json
{
  "user_id": "alice",
  "date": "2026-10-31",
  "intake_ml": 750,
  "daily_goal_ml": 2500,
  "timestamp_utc": "2026-10-31T17:00:00Z",
  "entries": [{ "timestamp_utc": "2026-10-31T10:00:00Z", "amount_ml": 750 }],
  "history": [
    {
      "date": "2026-10-30",
      "intake_ml": 2000,
      "daily_goal_ml": 2500,
      "timestamp_utc": "2026-10-31T17:00:00Z",
      "entries": [{ "timestamp_utc": "2026-10-30T09:00:00Z", "amount_ml": 2000 }]
    }
  ]
}
```

Any `2xx` response counts as success. HTTP/HTTPS `301`/`302`/`307`/`308` redirects are followed automatically (up to 5 hops).

## Notes

* The API token is stored in plain text in the app-private `state.json`; app backup is disabled in the manifest.
* If the device clock moves backwards (e.g. travelling west), today's data is kept rather than wiped; the rollover only happens when the date moves forward.
* Plain `http://` is allowed because self-hosted servers on a LAN are a primary use case; prefer `https://` otherwise.

## Companion server

`server/` contains a self-hosted Rust server (SQLite + web dashboard) that receives the sync above. Build and run it with Docker only: see [server/README.md](server/README.md).

## Layout

```
.github/workflows/build.yml   CI: test, build, verify, upload APK
rust/                         Rust core (lib.rs + tests, jni_bridge.rs, build.rs)
app/                          Android app (Kotlin, Compose, unit tests)
server/                       Companion server (Rust, axum, SQLite, Dockerfile, compose)
```
