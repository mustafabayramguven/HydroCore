# HydroCore companion server

Self-hosted receiver and dashboard for the HydroCore Android app. One static Rust binary
(axum + SQLite compiled in); the web UI is embedded in it and makes no external requests.

## Run (only Docker needed)

```sh
cp .env.example .env        # set HYDROCORE_API_TOKEN and HYDROCORE_DASHBOARD_PASSWORD
docker compose up -d --build
```

Only `Dockerfile`, `docker-compose.yml` (and optionally `.env`) are needed on the host: the
builder stage clones this repository from GitHub (`HYDROCORE_REPO` / `HYDROCORE_REF`).
The `server/` directory must therefore be pushed to that repository first.
To pick up new commits: `docker compose build --no-cache && docker compose up -d`.

The database lives in the `hydrocore-data` named volume (`/data/hydrocore.db`).

## Connect the app

In HydroCore settings: **Companion Server URL** `http://<host>:8080` (or `http://<host>:8080/api/sync` — base URLs are normalized automatically), **Bearer Token** = `HYDROCORE_API_TOKEN`
(leave empty if you did not set one). Open `http://<host>:8080` for the dashboard.

## Configuration (environment)

| Variable | Default | Meaning |
|---|---|---|
| `HYDROCORE_API_TOKEN` | *(empty: open)* | Required as `Authorization: Bearer` on `POST` |
| `HYDROCORE_DASHBOARD_USER` / `_PASSWORD` | `hydro` / *(empty: open)* | HTTP Basic login for dashboard and `GET` API |
| `HYDROCORE_BIND` | `0.0.0.0:8080` | Listen address |
| `HYDROCORE_DB` | `/data/hydrocore.db` | SQLite file |
| `RUST_LOG` | `info` | Log level |

Use HTTPS (e.g. a reverse proxy) if the server is reachable from the internet: tokens and
Basic credentials are otherwise sent in clear text.

## API

* `POST /api/sync` (or `POST /`): the app's payload (active day plus `history` archive), see the root README. Each day is stored as a
  full snapshot per `(user_id, date)`, so re-syncing is idempotent, full history archives are ingested in one batch transaction, and the app's *Reset*
  propagates. A snapshot older than the stored one is ignored (`"status":"ignored_stale"`).
* `GET /api/users`, `/api/days?user=&from=&to=`, `/api/days/{date}?user=`,
  `/api/summary?user=&today=`, `/api/export.csv?user=`, `/healthz`.

## Develop

```sh
cd server
cargo test
HYDROCORE_DB=./dev.db HYDROCORE_BIND=127.0.0.1:8080 cargo run
```
