//! Meta WhatsApp Cloud API client: text, interactive menus, order notifications.
//!
//! Free-form messages reach customers inside the 24-hour service window (always
//! true for bot conversations). Web-order notifications outside a window need
//! pre-approved templates — see docs/DEPLOYMENT.md.

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::AppState;
use crate::models::{Order, OrderItem};

const GRAPH: &str = "https://graph.facebook.com/v21.0";

async fn send(state: &AppState, payload: Value) {
    if state.cfg.whatsapp_token.is_empty() {
        tracing::warn!("WHATSAPP_TOKEN not set; skipping send");
        return;
    }
    let url = format!("{GRAPH}/{}/messages", state.cfg.whatsapp_phone_number_id);
    match state
        .http
        .post(&url)
        .bearer_auth(&state.cfg.whatsapp_token)
        .json(&payload)
        .send()
        .await
    {
        Ok(res) if !res.status().is_success() => {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            tracing::error!(%status, %body, "whatsapp send failed");
        }
        Err(e) => tracing::error!(error = %e, "whatsapp send error"),
        _ => {}
    }
}

pub async fn send_text(state: &AppState, to: &str, body: &str) {
    send(
        state,
        json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": body, "preview_url": true },
        }),
    )
    .await;
}

/// Up to 3 reply buttons: (id, title). Titles max 20 chars per API rules.
pub async fn send_buttons(state: &AppState, to: &str, body: &str, buttons: &[(&str, &str)]) {
    let buttons: Vec<Value> = buttons
        .iter()
        .take(3)
        .map(|(id, title)| json!({ "type": "reply", "reply": { "id": id, "title": title } }))
        .collect();
    send(
        state,
        json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "interactive",
            "interactive": {
                "type": "button",
                "body": { "text": body },
                "action": { "buttons": buttons },
            },
        }),
    )
    .await;
}

pub struct ListRow {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// An interactive list message (max 10 rows per section per API rules).
pub async fn send_list(
    state: &AppState,
    to: &str,
    body: &str,
    button: &str,
    section_title: &str,
    rows: &[ListRow],
) {
    let rows: Vec<Value> = rows
        .iter()
        .take(10)
        .map(|r| {
            json!({
                "id": r.id,
                "title": truncate(&r.title, 24),
                "description": truncate(&r.description, 72),
            })
        })
        .collect();
    send(
        state,
        json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "interactive",
            "interactive": {
                "type": "list",
                "body": { "text": body },
                "action": {
                    "button": button,
                    "sections": [{ "title": truncate(section_title, 24), "rows": rows }],
                },
            },
        }),
    )
    .await;
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

/// Verifies Meta's `X-Hub-Signature-256: sha256=<hex>` webhook header.
pub fn verify_signature(app_secret: &str, body: &[u8], header: &str) -> bool {
    let Some(sig) = header.strip_prefix("sha256=") else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(app_secret.as_bytes()).expect("hmac key");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
        .as_bytes()
        .ct_eq(sig.as_bytes())
        .into()
}

fn format_items(items: &[OrderItem]) -> String {
    items
        .iter()
        .map(|i| {
            let mut line = format!(
                "• {} × {} — ₹{}",
                i.product_name,
                i.qty,
                i.unit_price_inr * i.qty as i64
            );
            if i.eggless {
                line.push_str(" (eggless)");
            }
            if let Some(c) = &i.customization {
                line.push_str(&format!("\n  ✍️ \"{c}\""));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// New paid order → alert the owner, confirm to the customer.
pub async fn notify_order_paid(state: &AppState, order: &Order, items: &[OrderItem]) {
    let items_text = format_items(items);
    let owner_msg = format!(
        "🧁 *New order {}*\n\n{}\n\nTotal: *₹{}*\nDeliver: {} ({})\n\n{}\n{}\n📍 {}",
        order.order_number,
        items_text,
        order.total_inr,
        order.delivery_date.format("%a, %d %b"),
        order.delivery_slot,
        order.customer_name,
        order.phone,
        order.address,
    );
    send_text(state, &state.cfg.owner_whatsapp_number, &owner_msg).await;

    let customer_msg = format!(
        "Thank you {}! 🎂 Your order *{}* is confirmed.\n\n{}\n\nTotal paid: *₹{}*\nDelivery: {} ({})\n\nTrack: {}/order/{}",
        order.customer_name,
        order.order_number,
        items_text,
        order.total_inr,
        order.delivery_date.format("%a, %d %b"),
        order.delivery_slot,
        state.cfg.base_url,
        order.order_number,
    );
    send_text(state, &order.phone, &customer_msg).await;
}

/// Sends a birthday greeting to a customer (called by the daily cron).
/// Note: outside the 24-hour service window this requires a Meta-approved
/// template — see docs/WHATSAPP_TEMPLATES.md. Here we send a friendly text.
pub async fn send_birthday_greeting(state: &AppState, phone: &str, name: Option<&str>) {
    let who = name.map(|n| format!(" {n}")).unwrap_or_default();
    let msg = format!(
        "Happy birthday{who}! 🎂 From all of us at Sonna's Patisserie — here's to a sweet year. \
         Treat yourself today: reply here or visit {}, and mention BIRTHDAY for a little something on us. 💛",
        state.cfg.base_url
    );
    send_text(state, phone, &msg).await;
}

/// Admin changed the order status → tell the customer.
pub async fn notify_status_change(state: &AppState, order: &Order) {
    let line = match order.status.as_str() {
        "confirmed" => "is confirmed and our bakers are on it! 👩‍🍳",
        "out_for_delivery" => "is out for delivery! 🛵",
        "delivered" => "has been delivered. Enjoy! 🎉",
        "cancelled" => "has been cancelled. Please contact us for a refund.",
        _ => return,
    };
    let msg = format!(
        "Update on your Sonna's Patisserie order *{}*: it {line}",
        order.order_number
    );
    send_text(state, &order.phone, &msg).await;
}
