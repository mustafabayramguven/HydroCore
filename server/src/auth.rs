//! Optional authentication.
//!
//! * `POST` (the app's sync): `Authorization: Bearer <HYDROCORE_API_TOKEN>`, i.e. the
//!   "API token" setting in the app.
//! * `GET` (dashboard + read API): HTTP Basic auth, so a browser can show a login prompt.
//!
//! Unset values mean "open". `/healthz` is always open.

use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use std::sync::Arc;
use subtle::ConstantTimeEq;

fn ct_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn unauthorized(challenge: &'static str, body: &'static str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [
            (header::WWW_AUTHENTICATE, challenge),
            (header::CONTENT_TYPE, "application/json"),
        ],
        body,
    )
        .into_response()
}

fn bearer_ok(req: &Request, token: &str) -> bool {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .is_some_and(|(scheme, value)| scheme.eq_ignore_ascii_case("bearer") && ct_eq(value.trim(), token))
}

fn basic_ok(req: &Request, user: &str, password: &str) -> bool {
    let Some((scheme, value)) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
    else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value.trim()) else {
        return false;
    };
    let Ok(decoded) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((u, p)) = decoded.split_once(':') else {
        return false;
    };
    // Evaluate both comparisons (no short-circuit) to keep timing independent of which failed.
    let user_ok = ct_eq(u, user);
    let pass_ok = ct_eq(p, password);
    user_ok & pass_ok
}

pub async fn middleware(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let cfg = &state.config;
    let path = req.uri().path();

    if path != "/healthz" {
        if req.method() == Method::POST {
            if let Some(token) = &cfg.api_token {
                if !bearer_ok(&req, token) {
                    return unauthorized(
                        "Bearer realm=\"HydroCore\"",
                        r#"{"ok":false,"error":"missing or invalid bearer token"}"#,
                    );
                }
            }
        } else if let Some(password) = &cfg.dashboard_password {
            if !basic_ok(&req, &cfg.dashboard_user, password) {
                return unauthorized(
                    "Basic realm=\"HydroCore\", charset=\"UTF-8\"",
                    r#"{"ok":false,"error":"authentication required"}"#,
                );
            }
        }
    }

    let mut response = next.run(req).await;
    let h = response.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'",
        ),
    );
    response
}
