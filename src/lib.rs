pub mod auth;
pub mod bot;
pub mod cart;
pub mod db;
pub mod models;
pub mod razorpay;
pub mod routes;
pub mod whatsapp;

use axum::{
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct Config {
    pub session_secret: Vec<u8>,
    pub admin_password_hash: String,
    pub razorpay_key_id: String,
    pub razorpay_key_secret: String,
    pub razorpay_webhook_secret: String,
    pub whatsapp_token: String,
    pub whatsapp_phone_number_id: String,
    pub whatsapp_verify_token: String,
    pub whatsapp_app_secret: String,
    pub owner_whatsapp_number: String,
    pub anthropic_api_key: String,
    pub supabase_url: String,
    pub supabase_service_role_key: String,
    pub base_url: String,
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

impl Config {
    pub fn from_env() -> Self {
        let session_secret = env("SESSION_SECRET");
        if session_secret.len() < 32 {
            // Refuse to run with a weak/missing signing key rather than issue forgeable cookies.
            panic!("SESSION_SECRET must be set to at least 32 characters");
        }
        Self {
            session_secret: session_secret.into_bytes(),
            admin_password_hash: env("ADMIN_PASSWORD_HASH"),
            razorpay_key_id: env("RAZORPAY_KEY_ID"),
            razorpay_key_secret: env("RAZORPAY_KEY_SECRET"),
            razorpay_webhook_secret: env("RAZORPAY_WEBHOOK_SECRET"),
            whatsapp_token: env("WHATSAPP_TOKEN"),
            whatsapp_phone_number_id: env("WHATSAPP_PHONE_NUMBER_ID"),
            whatsapp_verify_token: env("WHATSAPP_VERIFY_TOKEN"),
            whatsapp_app_secret: env("WHATSAPP_APP_SECRET"),
            owner_whatsapp_number: env("OWNER_WHATSAPP_NUMBER"),
            anthropic_api_key: env("ANTHROPIC_API_KEY"),
            supabase_url: env("SUPABASE_URL"),
            supabase_service_role_key: env("SUPABASE_SERVICE_ROLE_KEY"),
            base_url: {
                let b = env("BASE_URL");
                if b.is_empty() {
                    "http://localhost:3000".into()
                } else {
                    b
                }
            },
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    pub http: reqwest::Client,
}

pub async fn connect_db() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("failed to connect to Postgres");
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("migrations failed");
    pool
}

pub fn build_state(db: PgPool) -> AppState {
    AppState {
        db,
        cfg: Arc::new(Config::from_env()),
        http: reqwest::Client::new(),
    }
}

/// Application error: logs the cause, returns an opaque 500 to the client.
pub struct AppError(pub String);

impl<E: std::fmt::Display> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "request failed");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Something went wrong. Please try again.",
        )
            .into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

async fn security_headers(req: axum::extract::Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' https://checkout.razorpay.com; \
             frame-src https://api.razorpay.com https://checkout.razorpay.com; \
             img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; \
             connect-src 'self' https://api.razorpay.com https://lumberjack.razorpay.com",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    res
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::store::router())
        .merge(routes::checkout::router())
        .merge(routes::webhook::router())
        .merge(routes::admin::router(state.clone()))
        .merge(cart::router())
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
