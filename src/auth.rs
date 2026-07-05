//! Admin session + CSRF, both HMAC-signed with SESSION_SECRET.

use argon2::password_hash::PasswordHash;
use argon2::{Argon2, PasswordVerifier};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const ADMIN_COOKIE: &str = "sp_admin";
const CSRF_COOKIE: &str = "sp_csrf";
const SESSION_TTL_SECS: u64 = 12 * 60 * 60;

fn sign(secret: &[u8], data: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn verify_sig(secret: &[u8], data: &str, sig: &str) -> bool {
    sign(secret, data).as_bytes().ct_eq(sig.as_bytes()).into()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn admin_session_cookie(secret: &[u8]) -> Cookie<'static> {
    let exp = now_unix() + SESSION_TTL_SECS;
    let payload = format!("admin.{exp}");
    let value = format!("{payload}.{}", sign(secret, &payload));
    build_cookie(ADMIN_COOKIE, value, true)
}

pub fn admin_logout_cookie() -> Cookie<'static> {
    let mut c = build_cookie(ADMIN_COOKIE, String::new(), true);
    c.make_removal();
    c
}

pub fn is_admin(jar: &CookieJar, secret: &[u8]) -> bool {
    let Some(cookie) = jar.get(ADMIN_COOKIE) else {
        return false;
    };
    let value = cookie.value();
    let Some((payload, sig)) = value.rsplit_once('.') else {
        return false;
    };
    if !verify_sig(secret, payload, sig) {
        return false;
    }
    match payload
        .strip_prefix("admin.")
        .and_then(|e| e.parse::<u64>().ok())
    {
        Some(exp) => exp > now_unix(),
        None => false,
    }
}

/// Double-submit CSRF: random token in a cookie, echoed in a hidden form field.
pub fn ensure_csrf(jar: CookieJar) -> (CookieJar, String) {
    if let Some(c) = jar.get(CSRF_COOKIE) {
        let token = c.value().to_string();
        if token.len() == 64 {
            return (jar, token);
        }
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let jar = jar.add(build_cookie(CSRF_COOKIE, token.clone(), true));
    (jar, token)
}

pub fn csrf_ok(jar: &CookieJar, submitted: &str) -> bool {
    match jar.get(CSRF_COOKIE) {
        Some(c) => !submitted.is_empty() && c.value().as_bytes().ct_eq(submitted.as_bytes()).into(),
        None => false,
    }
}

fn build_cookie(name: &'static str, value: String, http_only: bool) -> Cookie<'static> {
    let mut c = Cookie::new(name, value);
    c.set_path("/");
    c.set_http_only(http_only);
    c.set_same_site(SameSite::Lax);
    // ponytail: Secure is set unless localhost — detected once at startup via BASE_URL
    if !std::env::var("BASE_URL")
        .unwrap_or_default()
        .starts_with("http://localhost")
    {
        c.set_secure(true);
    }
    c
}

// ponytail: in-memory per-instance rate limit; move to the DB if login abuse ever matters
static LOGIN_ATTEMPTS: Mutex<Option<HashMap<String, (u32, Instant)>>> = Mutex::new(None);
const MAX_ATTEMPTS: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(15 * 60);

pub fn login_allowed(ip: &str) -> bool {
    let mut guard = LOGIN_ATTEMPTS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(ip) {
        Some((count, since)) if *count >= MAX_ATTEMPTS => {
            if since.elapsed() > LOCKOUT {
                map.remove(ip);
                true
            } else {
                false
            }
        }
        _ => true,
    }
}

pub fn record_login_failure(ip: &str) {
    let mut guard = LOGIN_ATTEMPTS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let entry = map.entry(ip.to_string()).or_insert((0, Instant::now()));
    entry.0 += 1;
    entry.1 = Instant::now();
}

pub fn clear_login_failures(ip: &str) {
    let mut guard = LOGIN_ATTEMPTS.lock().unwrap();
    if let Some(map) = guard.as_mut() {
        map.remove(ip);
    }
}
