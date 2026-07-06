//! Scheduled tasks, triggered by Vercel Cron (see vercel.json). Guarded by a
//! shared secret so only the scheduler can invoke them.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use subtle::ConstantTimeEq;

use crate::{AppState, db, whatsapp};

fn authorized(headers: &HeaderMap, secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    let expected = format!("Bearer {secret}");
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|got| got.as_bytes().ct_eq(expected.as_bytes()).into())
        .unwrap_or(false)
}

/// Daily job: greet customers whose birthday is today (once per year), idempotent.
async fn daily(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &state.cfg.cron_secret) {
        return (StatusCode::UNAUTHORIZED, "unauthorized".to_string());
    }
    let due = match db::birthdays_due_today(&state.db).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "birthday query failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error".to_string());
        }
    };
    let mut sent = 0;
    for c in &due {
        whatsapp::send_birthday_greeting(&state, &c.phone, c.name.as_deref()).await;
        if let Err(e) = db::log_birthday_sent(&state.db, &c.phone).await {
            tracing::error!(error = %e, phone = %c.phone, "failed to log birthday send");
        } else {
            sent += 1;
        }
    }
    tracing::info!(sent, "birthday greetings processed");
    (StatusCode::OK, format!("birthday greetings sent: {sent}"))
}

pub fn router() -> Router<AppState> {
    // Support GET too, since some schedulers issue GET.
    Router::new().route("/tasks/daily", post(daily).get(daily))
}
