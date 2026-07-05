use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;

use crate::{bot, db, razorpay, whatsapp, AppState};

/// Razorpay webhook: `payment.captured` (web checkout) and
/// `payment_link.paid` (WhatsApp orders). Idempotent via db::mark_paid.
async fn razorpay_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("x-razorpay-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !razorpay::verify_webhook_signature(&state.cfg.razorpay_webhook_secret, &body, signature) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event): Result<Value, _> = serde_json::from_slice(&body) else {
        return StatusCode::BAD_REQUEST;
    };

    let (order_number, payment_id, paid_amount_paise) = match event["event"].as_str() {
        Some("payment.captured") => {
            let payment = &event["payload"]["payment"]["entity"];
            // receipt travels back via the order; we stored order_number as receipt,
            // and razorpay_order_id on the order row — look up by the latter.
            let rzp_order_id = payment["order_id"].as_str().unwrap_or_default();
            let Ok(Some(order)) = order_by_rzp_order(&state, rzp_order_id).await else {
                return StatusCode::OK; // not ours; ack so Razorpay stops retrying
            };
            (
                order.order_number,
                payment["id"].as_str().unwrap_or_default().to_string(),
                payment["amount"].as_i64().unwrap_or(0),
            )
        }
        Some("payment_link.paid") => {
            let link = &event["payload"]["payment_link"]["entity"];
            (
                link["reference_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                event["payload"]["payment"]["entity"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                link["amount"].as_i64().unwrap_or(0),
            )
        }
        _ => return StatusCode::OK,
    };

    // Cross-check the paid amount against our order before honouring it.
    let Ok(Some(existing)) = db::order_by_number(&state.db, &order_number).await else {
        return StatusCode::OK;
    };
    if paid_amount_paise != existing.total_inr * 100 {
        tracing::error!(order = %order_number, paid_amount_paise, "amount mismatch on webhook");
        return StatusCode::OK;
    }

    if let Ok(Some(order)) = db::mark_paid(&state.db, &order_number, &payment_id).await {
        let _ = db::upsert_customer_order(
            &state.db,
            &order.phone,
            &order.customer_name,
            order.email.as_deref(),
            order.total_inr,
        )
        .await;
        if let Ok(items) = db::order_items(&state.db, order.id).await {
            whatsapp::notify_order_paid(&state, &order, &items).await;
        }
    }
    StatusCode::OK
}

async fn order_by_rzp_order(
    state: &AppState,
    rzp_order_id: &str,
) -> sqlx::Result<Option<crate::models::Order>> {
    sqlx::query_as("select * from orders where razorpay_order_id = $1")
        .bind(rzp_order_id)
        .fetch_optional(&state.db)
        .await
}

#[derive(serde::Deserialize)]
struct VerifyParams {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

/// Meta webhook handshake.
async fn whatsapp_verify(
    State(state): State<AppState>,
    Query(params): Query<VerifyParams>,
) -> impl IntoResponse {
    if params.mode.as_deref() == Some("subscribe")
        && params.verify_token.as_deref() == Some(state.cfg.whatsapp_verify_token.as_str())
        && !state.cfg.whatsapp_verify_token.is_empty()
    {
        (StatusCode::OK, params.challenge.unwrap_or_default())
    } else {
        (StatusCode::FORBIDDEN, String::new())
    }
}

/// Inbound WhatsApp messages → ordering bot. Always 200 (Meta retries otherwise).
async fn whatsapp_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !whatsapp::verify_signature(&state.cfg.whatsapp_app_secret, &body, signature) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event): Result<Value, _> = serde_json::from_slice(&body) else {
        return StatusCode::OK;
    };
    // Statuses (sent/delivered/read) arrive on the same webhook — only handle messages.
    for entry in event["entry"].as_array().unwrap_or(&vec![]) {
        for change in entry["changes"].as_array().unwrap_or(&vec![]) {
            for message in change["value"]["messages"].as_array().unwrap_or(&vec![]) {
                if let Err(e) = bot::handle_message(&state, message).await {
                    tracing::error!(error = %e.0, "bot failed to handle message");
                }
            }
        }
    }
    StatusCode::OK
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/webhooks/razorpay", post(razorpay_webhook))
        .route(
            "/webhooks/whatsapp",
            get(whatsapp_verify).post(whatsapp_webhook),
        )
}
