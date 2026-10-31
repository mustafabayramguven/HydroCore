# HydroCore

Open-source Android hydration tracker. All hydration logic runs in a **Rust core**; Kotlin/Jetpack Compose is a thin Material You UI. Data stays on the device and is sent to **your own HTTP server only when you tap "Manual Sync"**.

No background work of any kind: no WorkManager, no foreground service, no JobScheduler, no polling.

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

`POST <Server URL>` with `Content-Type: application/json` and, if a token is set, `Authorization: Bearer <token>`:

```json
{
  "user_id": "alice",
  "date": "2026-10-31",
  "intake_ml": 750,
  "daily_goal_ml": 2500,
  "timestamp_utc": "2026-10-31T17:00:00Z",
  "entries": [{ "timestamp_utc": "2026-10-31T10:00:00Z", "amount_ml": 750 }]
}
```

Any `2xx` response counts as success. Redirects are not followed.

## Notes

* The API token is stored in plain text in the app-private `state.json`; app backup is disabled in the manifest.
* If the device clock moves backwards (e.g. travelling west), today's data is kept rather than wiped; the rollover only happens when the date moves forward.
* Plain `http://` is allowed because self-hosted servers on a LAN are a primary use case; prefer `https://` otherwise.

## Layout

```
.github/workflows/build.yml   CI: test, build, verify, upload APK
rust/                         Rust core (lib.rs + tests, jni_bridge.rs, build.rs)
app/                          Android app (Kotlin, Compose, unit tests)
```
