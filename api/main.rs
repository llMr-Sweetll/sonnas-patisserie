//! Vercel entrypoint (official Rust runtime, Fluid compute): every request is
//! rewritten here (see vercel.json) and served by the same Axum router the
//! local server uses. The DB pool is created once per instance at cold start.

use tower::ServiceBuilder;
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let db = sonnas_patisserie::connect_db().await;
    let router = sonnas_patisserie::build_router(sonnas_patisserie::build_state(db));
    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(router);
    vercel_runtime::run(app).await
}
