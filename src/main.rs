use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use sonnas_patisserie::{build_router, build_state, connect_db};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sonnas_patisserie=debug".into()),
        )
        .init();

    let db = connect_db().await;
    let state = build_state(db);
    let app = build_router(state)
        .nest_service("/public", ServeDir::new("public"))
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind failed");
    tracing::info!("Sonna's Patisserie listening on http://localhost:{port}");
    axum::serve(listener, app).await.expect("server failed");
}
