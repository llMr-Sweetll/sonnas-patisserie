use chrono::NaiveDate;
use sonnas_patisserie::models::is_deliverable;
use sonnas_patisserie::razorpay::{verify_checkout_signature, verify_webhook_signature};
use sonnas_patisserie::routes::checkout::normalize_phone;
use sonnas_patisserie::whatsapp::verify_signature;

fn hmac_hex(secret: &str, data: &[u8]) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

#[test]
fn tuesdays_are_not_deliverable() {
    // 2026-07-07 is a Tuesday, 2026-07-08 a Wednesday.
    assert!(!is_deliverable(NaiveDate::from_ymd_opt(2026, 7, 7).unwrap()));
    assert!(is_deliverable(NaiveDate::from_ymd_opt(2026, 7, 8).unwrap()));
}

#[test]
fn phone_normalization_assumes_india() {
    assert_eq!(normalize_phone("98765 43210"), "919876543210");
    assert_eq!(normalize_phone("+91 98765-43210"), "919876543210");
}

#[test]
fn razorpay_webhook_signature_roundtrip() {
    let body = br#"{"event":"payment.captured"}"#;
    let sig = hmac_hex("secret", body);
    assert!(verify_webhook_signature("secret", body, &sig));
    assert!(!verify_webhook_signature("secret", body, "deadbeef"));
    assert!(!verify_webhook_signature("wrong", body, &sig));
}

#[test]
fn razorpay_checkout_signature_roundtrip() {
    let sig = hmac_hex("keysecret", b"order_abc|pay_xyz");
    assert!(verify_checkout_signature("keysecret", "order_abc", "pay_xyz", &sig));
    assert!(!verify_checkout_signature("keysecret", "order_abc", "pay_other", &sig));
}

#[test]
fn whatsapp_signature_requires_prefix_and_match() {
    let body = br#"{"entry":[]}"#;
    let sig = format!("sha256={}", hmac_hex("appsecret", body));
    assert!(verify_signature("appsecret", body, &sig));
    assert!(!verify_signature("appsecret", body, "sha256=00"));
    assert!(!verify_signature("appsecret", body, "md5=whatever"));
}
