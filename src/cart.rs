//! Cookie cart. The cookie stores only ids/qty/flags — never prices; prices are
//! re-read from the DB when rendering and at checkout.

use axum::extract::State;
use axum::response::Redirect;
use axum::routing::post;
use axum::{Form, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use crate::AppState;
use crate::models::CartLine;

const CART_COOKIE: &str = "sp_cart";
const MAX_LINES: usize = 20;

pub fn read_cart(jar: &CookieJar) -> Vec<CartLine> {
    jar.get(CART_COOKIE)
        .and_then(|c| serde_json::from_str::<Vec<CartLine>>(c.value()).ok())
        .unwrap_or_default()
}

pub fn write_cart(jar: CookieJar, cart: &[CartLine]) -> CookieJar {
    let mut c = Cookie::new(CART_COOKIE, serde_json::to_string(cart).unwrap_or_default());
    c.set_path("/");
    c.set_same_site(SameSite::Lax);
    c.set_http_only(true);
    jar.add(c)
}

pub fn clear_cart(jar: CookieJar) -> CookieJar {
    let mut c = Cookie::new(CART_COOKIE, "");
    c.set_path("/");
    c.make_removal();
    jar.add(c)
}

#[derive(Deserialize)]
pub struct AddForm {
    pub product_id: i64,
    #[serde(default = "one")]
    pub qty: i32,
    #[serde(default)]
    pub eggless: Option<String>, // checkbox: "on" when checked
    #[serde(default)]
    pub customization: String,
}

fn one() -> i32 {
    1
}

async fn add(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AddForm>,
) -> (CookieJar, Redirect) {
    // Only real, available products enter the cart.
    let valid = crate::db::product_by_id(&state.db, form.product_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|p| p.is_available);
    if !valid {
        return (jar, Redirect::to("/cart"));
    }
    let mut cart = read_cart(&jar);
    if cart.len() >= MAX_LINES {
        return (jar, Redirect::to("/cart"));
    }
    let customization = form.customization.trim();
    cart.push(CartLine {
        product_id: form.product_id,
        qty: form.qty.clamp(1, 20),
        eggless: form.eggless.is_some(),
        customization: (!customization.is_empty())
            .then(|| customization.chars().take(200).collect()),
    });
    (write_cart(jar, &cart), Redirect::to("/cart"))
}

#[derive(Deserialize)]
pub struct LineForm {
    pub index: usize,
    #[serde(default = "one")]
    pub qty: i32,
}

async fn update(jar: CookieJar, Form(form): Form<LineForm>) -> (CookieJar, Redirect) {
    let mut cart = read_cart(&jar);
    if let Some(line) = cart.get_mut(form.index) {
        line.qty = form.qty.clamp(1, 20);
    }
    (write_cart(jar, &cart), Redirect::to("/cart"))
}

async fn remove(jar: CookieJar, Form(form): Form<LineForm>) -> (CookieJar, Redirect) {
    let mut cart = read_cart(&jar);
    if form.index < cart.len() {
        cart.remove(form.index);
    }
    (write_cart(jar, &cart), Redirect::to("/cart"))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cart/add", post(add))
        .route("/cart/update", post(update))
        .route("/cart/remove", post(remove))
}
