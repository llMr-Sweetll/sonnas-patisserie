//! Vercel entrypoint: every request is rewritten here (see vercel.json) and
//! dispatched into the same Axum router the local server uses.

use axum::body::Body as AxumBody;
use axum::Router;
use http_body_util::BodyExt;
use tokio::sync::OnceCell;
use tower::ServiceExt;
use vercel_runtime::{run, Body, Error, Request, Response};

static ROUTER: OnceCell<Router> = OnceCell::const_new();

async fn router() -> Router {
    let db = sonnas_patisserie::connect_db().await;
    sonnas_patisserie::build_router(sonnas_patisserie::build_state(db))
}

async fn handler(req: Request) -> Result<Response<Body>, Error> {
    let app = ROUTER.get_or_init(router).await.clone();

    let (parts, body) = req.into_parts();
    let bytes = match body {
        Body::Empty => Vec::new(),
        Body::Text(t) => t.into_bytes(),
        Body::Binary(b) => b,
    };
    let axum_req = axum::http::Request::from_parts(parts, AxumBody::from(bytes));

    let res = app.oneshot(axum_req).await?;
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await?.to_bytes();
    Ok(Response::from_parts(parts, Body::Binary(bytes.to_vec())))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(handler).await
}
