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

// ponytail: hand-rolled migration runner — sqlx::migrate! drags in the whole
// macros/mysql subtree (incl. RUSTSEC-2023-0071's rsa). Add new files here in order.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_schema", include_str!("../migrations/0001_schema.sql")),
    ("0002_seed", include_str!("../migrations/0002_seed.sql")),
    ("0003_rls", include_str!("../migrations/0003_rls.sql")),
];

async fn run_migrations(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::raw_sql(
        "create table if not exists _migrations \
         (name text primary key, applied_at timestamptz not null default now())",
    )
    .execute(pool)
    .await?;
    for (name, sql) in MIGRATIONS {
        let mut tx = pool.begin().await?;
        // The insert serialises concurrent starters: only the winner runs the file.
        let inserted =
            sqlx::query("insert into _migrations (name) values ($1) on conflict do nothing")
                .bind(name)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if inserted == 1 {
            sqlx::raw_sql(sql).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

pub async fn connect_db() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let opts = PgPoolOptions::new()
        .max_connections(5)
        .min_connections(0)
        .acquire_timeout(std::time::Duration::from_secs(15))
        // The Supabase pooler reaps idle sessions; validate before reuse so a
        // long-idle Fluid instance doesn't hand out dead connections.
        .test_before_acquire(true)
        .idle_timeout(std::time::Duration::from_secs(10 * 60))
        .max_lifetime(std::time::Duration::from_secs(25 * 60));

    // Cold starts must survive transient pooler hiccups — retry before panicking.
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let connected = match opts.clone().connect(&url).await {
            Ok(pool) => match run_migrations(&pool).await {
                Ok(()) => Ok(pool),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        match connected {
            Ok(pool) => return pool,
            Err(e) if attempt < 3 => {
                tracing::warn!(error = %e, attempt, "db init failed; retrying");
                tokio::time::sleep(std::time::Duration::from_secs(attempt as u64)).await;
            }
            Err(e) => panic!("database initialisation failed after {attempt} attempts: {e}"),
        }
    }
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

/// Implements axum's IntoResponse for Askama templates (askama_axum has no
/// axum-0.8 release, and this is all it did anyway).
#[macro_export]
macro_rules! impl_template_response {
    ($($t:ty),+ $(,)?) => {$(
        impl axum::response::IntoResponse for $t {
            fn into_response(self) -> axum::response::Response {
                match askama::Template::render(&self) {
                    Ok(html) => axum::response::Html(html).into_response(),
                    Err(e) => $crate::AppError(e.to_string()).into_response(),
                }
            }
        }
    )+};
}

// Static assets are compiled into the binary so the same build serves them on
// any host — no dependence on Vercel static-dir conventions.
async fn style_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../public/style.css"),
    )
}

async fn payment_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../public/payment.js"),
    )
}

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
        .route("/public/style.css", axum::routing::get(style_css))
        .route("/public/payment.js", axum::routing::get(payment_js))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
