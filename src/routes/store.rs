use askama::Template;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum_extra::extract::cookie::CookieJar;

use crate::cart::read_cart;
use crate::models::{CartLine, Category, Order, OrderItem, Product, status_label};
use crate::{AppResult, AppState, db};

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub categories: Vec<Category>,
    pub featured: Vec<Product>,
    pub bestsellers: Vec<Product>,
    pub promo: Option<crate::models::Promotion>,
    pub hero_product: Option<Product>,
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
#[template(path = "privacy.html")]
pub struct PrivacyTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "terms.html")]
pub struct TermsTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "refunds.html")]
pub struct RefundsTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "shipping.html")]
pub struct ShippingTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "contact.html")]
pub struct ContactTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "pricing.html")]
pub struct PricingTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

#[derive(Template)]
#[template(path = "404.html")]
pub struct NotFoundTemplate {
    pub categories: Vec<Category>,
    pub cart_count: usize,
}

crate::impl_template_response!(
    IndexTemplate,
    CategoryTemplate,
    ProductTemplate,
    CartTemplate,
    OrderTemplate,
    PrivacyTemplate,
    TermsTemplate,
    RefundsTemplate,
    ShippingTemplate,
    ContactTemplate,
    PricingTemplate,
    NotFoundTemplate,
);

pub async fn not_found(state: &AppState, jar: &CookieJar) -> AppResult<axum::response::Response> {
    let categories = db::list_categories(&state.db).await?;
    let t = NotFoundTemplate {
        categories,
        cart_count: read_cart(jar).len(),
    };
    Ok((StatusCode::NOT_FOUND, t).into_response())
}

async fn home(State(state): State<AppState>, jar: CookieJar) -> AppResult<IndexTemplate> {
    let featured = db::featured_products(&state.db).await?;
    // A featured item with a real photo anchors the hero's right panel.
    let hero_product = featured.iter().find(|p| !p.image_url.is_empty()).cloned();
    Ok(IndexTemplate {
        categories: db::list_categories(&state.db).await?,
        bestsellers: db::bestsellers(&state.db, 4).await?,
        promo: db::active_promotions(&state.db).await?.into_iter().next(),
        hero_product,
        featured,
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

pub fn whatsapp_cart_link(owner_whatsapp_number: &str, lines: &[CartView]) -> String {
    let mut text = String::from("Hi Sonna's Patisserie! I'd like to order:\n");
    for v in lines {
        text.push_str(&format!("• {} × {}", v.product.name, v.line.qty));
        if v.line.eggless {
            text.push_str(" (eggless)");
        }
        text.push('\n');
    }
    format!(
        "https://wa.me/{}?text={}",
        owner_whatsapp_number,
        urlencoding::encode(&text)
    )
}

async fn cart_page(State(state): State<AppState>, jar: CookieJar) -> AppResult<CartTemplate> {
    let cart = read_cart(&jar);
    let (lines, subtotal) = build_cart_view(&state, &cart).await?;
    let wa_link = whatsapp_cart_link(&state.cfg.owner_whatsapp_number, &lines);
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

async fn privacy(State(state): State<AppState>, jar: CookieJar) -> AppResult<PrivacyTemplate> {
    Ok(PrivacyTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn terms(State(state): State<AppState>, jar: CookieJar) -> AppResult<TermsTemplate> {
    Ok(TermsTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn refunds(State(state): State<AppState>, jar: CookieJar) -> AppResult<RefundsTemplate> {
    Ok(RefundsTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn shipping(State(state): State<AppState>, jar: CookieJar) -> AppResult<ShippingTemplate> {
    Ok(ShippingTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn contact(State(state): State<AppState>, jar: CookieJar) -> AppResult<ContactTemplate> {
    Ok(ContactTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

async fn pricing(State(state): State<AppState>, jar: CookieJar) -> AppResult<PricingTemplate> {
    Ok(PricingTemplate {
        categories: db::list_categories(&state.db).await?,
        cart_count: read_cart(&jar).len(),
    })
}

/// Liveness/readiness probe: 200 only when the database answers.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("select 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, "ok"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "db unreachable"),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(home))
        .route("/category/{slug}", get(category))
        .route("/product/{slug}", get(product))
        .route("/cart", get(cart_page))
        .route("/order/{number}", get(order_page))
        .route("/privacy", get(privacy))
        .route("/terms", get(terms))
        .route("/refunds", get(refunds))
        .route("/shipping", get(shipping))
        .route("/contact", get(contact))
        .route("/pricing", get(pricing))
        .route("/health", get(health))
}
