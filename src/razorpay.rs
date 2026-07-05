//! Razorpay REST client: orders, payment links, signature verification.

use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::AppState;

const API: &str = "https://api.razorpay.com/v1";

fn inr_to_paise(inr: i64) -> i64 {
    inr * 100
}

/// Creates a Razorpay order for the web checkout flow. Returns the Razorpay order id.
pub async fn create_order(
    state: &AppState,
    amount_inr: i64,
    receipt: &str,
) -> Result<String, String> {
    let res: Value = state
        .http
        .post(format!("{API}/orders"))
        .basic_auth(
            &state.cfg.razorpay_key_id,
            Some(&state.cfg.razorpay_key_secret),
        )
        .json(&json!({
            "amount": inr_to_paise(amount_inr),
            "currency": "INR",
            "receipt": receipt,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    res["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("razorpay order creation failed: {res}"))
}

/// Creates a payment link for WhatsApp orders. Returns (link_id, short_url).
pub async fn create_payment_link(
    state: &AppState,
    amount_inr: i64,
    order_number: &str,
    customer_name: &str,
    phone: &str,
) -> Result<(String, String), String> {
    let res: Value = state
        .http
        .post(format!("{API}/payment_links"))
        .basic_auth(
            &state.cfg.razorpay_key_id,
            Some(&state.cfg.razorpay_key_secret),
        )
        .json(&json!({
            "amount": inr_to_paise(amount_inr),
            "currency": "INR",
            "reference_id": order_number,
            "description": format!("Sonna's Patisserie order {order_number}"),
            "customer": { "name": customer_name, "contact": format!("+{phone}") },
            "notify": { "sms": false, "email": false },
            "callback_url": format!("{}/order/{order_number}", state.cfg.base_url),
            "callback_method": "get",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    match (res["id"].as_str(), res["short_url"].as_str()) {
        (Some(id), Some(url)) => Ok((id.to_string(), url.to_string())),
        _ => Err(format!("razorpay payment link failed: {res}")),
    }
}

fn hmac_hex(secret: &str, data: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time verification of the `X-Razorpay-Signature` webhook header.
pub fn verify_webhook_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    hmac_hex(secret, body)
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .into()
}

/// Verifies the signature Razorpay Checkout returns to the browser
/// (HMAC of "order_id|payment_id" with the key secret).
pub fn verify_checkout_signature(
    key_secret: &str,
    rzp_order_id: &str,
    payment_id: &str,
    signature: &str,
) -> bool {
    let data = format!("{rzp_order_id}|{payment_id}");
    hmac_hex(key_secret, data.as_bytes())
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .into()
}
