use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use axum_extra::extract::cookie::CookieJar;

use crate::cart::read_cart;
use crate::models::{status_label, CartLine, Category, Order, OrderItem, Product};
use crate::{db, AppResult, AppState};

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub categories: Vec<Category>,
    pub featured: Vec<Product>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "category.html")]
pub struct CategoryTemplate {
    pub categories: Vec<Category>,
    pub category: Category,
    pub products: Vec<Product>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "product.html")]
pub struct ProductTemplate {
    pub categories: Vec<Category>,
    pub product: Product,
    pub cart_count: usize,
}

pub struct CartView {
    pub index: usize,
    pub product: Product,
    pub line: CartLine,
    pub line_total: i64,
}

#[derive(Template)]
#[template(path = "cart.html")]
pub struct CartTemplate {
    pub categories: Vec<Category>,
    pub lines: Vec<CartView>,
    pub subtotal: i64,
    pub wa_link: String,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "order.html")]
pub struct OrderTemplate {
    pub categories: Vec<Category>,
    pub order: Order,
    pub items: Vec<OrderItem>,
    pub status_text: &'static str,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "404.html")]
pub struct NotFoundTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

pub async fn not_found(state: &AppState, jar: &CookieJar) -> AppResult<axum::response::Response> {
    let categories = db::list_categories(&state.db).await?;
    let t = NotFoundTemplate {
        categories,
        cart_count: read_cart(jar).len(),
    };
    Ok((StatusCode::NOT_FOUND, t).into_response())
}

async fn home(State(state): State<AppState>, jar: CookieJar) -> AppResult<IndexTemplate> {
    Ok(IndexTemplate {
        categories: db::list_categories(&state.db).await?,
        featured: db::featured_products(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn category(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> AppResult<axum::response::Response> {
    let Some(category) = db::category_by_slug(&state.db, &slug).await? else {
        return not_found(&state, &jar).await;
    };
    let products = db::products_in_category(&state.db, category.id).await?;
    let t = CategoryTemplate {
        categories: db::list_categories(&state.db).await?,
        category,
        products,
        cart_count: read_cart(&jar).len(),
    };
    Ok(t.into_response())
}

async fn product(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> AppResult<axum::response::Response> {
    let Some(product) = db::product_by_slug(&state.db, &slug).await? else {
        return not_found(&state, &jar).await;
    };
    let t = ProductTemplate {
        categories: db::list_categories(&state.db).await?,
        product,
        cart_count: read_cart(&jar).len(),
    };
    Ok(t.into_response())
}

pub async fn build_cart_view(
    state: &AppState,
    cart: &[CartLine],
) -> AppResult<(Vec<CartView>, i64)> {
    let ids: Vec<i64> = cart.iter().map(|l| l.product_id).collect();
    let products = db::products_by_ids(&state.db, &ids).await?;
    let mut views = Vec::new();
    let mut subtotal = 0i64;
    for (index, line) in cart.iter().enumerate() {
        if let Some(p) = products
            .iter()
            .find(|p| p.id == line.product_id && p.is_available)
        {
            let line_total = p.price_inr * line.qty as i64;
            subtotal += line_total;
            views.push(CartView {
                index,
                product: p.clone(),
                line: line.clone(),
                line_total,
            });
        }
    }
    Ok((views, subtotal))
}

async fn cart_page(State(state): State<AppState>, jar: CookieJar) -> AppResult<CartTemplate> {
    let cart = read_cart(&jar);
    let (lines, subtotal) = build_cart_view(&state, &cart).await?;
    // "Order on WhatsApp" deep link with the cart pre-filled.
    let mut text = String::from("Hi Sonna's Patisserie! I'd like to order:\n");
    for v in &lines {
        text.push_str(&format!("• {} × {}", v.product.name, v.line.qty));
        if v.line.eggless {
            text.push_str(" (eggless)");
        }
        text.push('\n');
    }
    let wa_link = format!(
        "https://wa.me/{}?text={}",
        state.cfg.owner_whatsapp_number,
        urlencoding::encode(&text)
    );
    Ok(CartTemplate {
        categories: db::list_categories(&state.db).await?,
        lines,
        subtotal,
        wa_link,
        cart_count: cart.len(),
    })
}

async fn order_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(number): Path<String>,
) -> AppResult<axum::response::Response> {
    let Some(order) = db::order_by_number(&state.db, &number).await? else {
        return not_found(&state, &jar).await;
    };
    let items = db::order_items(&state.db, order.id).await?;
    let t = OrderTemplate {
        categories: db::list_categories(&state.db).await?,
        status_text: status_label(&order.status),
        order,
        items,
        cart_count: read_cart(&jar).len(),
    };
    Ok(t.into_response())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(home))
        .route("/category/:slug", get(category))
        .route("/product/:slug", get(product))
        .route("/cart", get(cart_page))
        .route("/order/:number", get(order_page))
}
