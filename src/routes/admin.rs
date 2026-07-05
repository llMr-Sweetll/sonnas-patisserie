use askama::Template;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use crate::auth::{
    admin_logout_cookie, admin_session_cookie, clear_login_failures, csrf_ok, ensure_csrf,
    is_admin, login_allowed, record_login_failure, verify_password,
};
use crate::db::{self, DailyStat, ProductInput, StatusCount, TopProduct};
use crate::models::{Category, Order, OrderItem, Product, ORDER_STATUSES};
use crate::{whatsapp, AppError, AppResult, AppState};

// --- Templates ---

#[derive(Template)]
#[template(path = "admin/login.html")]
pub struct LoginTemplate {
    pub csrf: String,
    pub error: Option<String>,
}

pub struct Bar {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub label: String,
    pub value: String,
}

pub struct HBar {
    pub label: String,
    pub pct: i64,
    pub value: String,
}

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate {
    pub revenue_30d: i64,
    pub orders_30d: i64,
    pub aov: i64,
    pub pending_count: i64,
    pub total_customers: i64,
    pub repeat_customers: i64,
    pub repeat_rate: i64,
    pub revenue_bars: Vec<Bar>,
    pub new_customer_bars: Vec<Bar>,
    pub top_products: Vec<TopProduct>,
    pub top_max: i64,
    pub category_bars: Vec<HBar>,
    pub source_split: Vec<HBar>,
    pub slot_split: Vec<HBar>,
    pub status_counts: Vec<StatusCount>,
    pub recent: Vec<Order>,
}

#[derive(Template)]
#[template(path = "admin/customers.html")]
pub struct CustomersTemplate {
    pub customers: Vec<crate::models::Customer>,
}

#[derive(Template)]
#[template(path = "admin/promotions.html")]
pub struct PromotionsTemplate {
    pub promotions: Vec<crate::models::Promotion>,
}

#[derive(Template)]
#[template(path = "admin/promotion_form.html")]
pub struct PromotionFormTemplate {
    pub promotion: Option<crate::models::Promotion>,
    pub csrf: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "admin/orders.html")]
pub struct OrdersTemplate {
    pub orders: Vec<Order>,
}

#[derive(Template)]
#[template(path = "admin/order_detail.html")]
pub struct OrderDetailTemplate {
    pub order: Order,
    pub items: Vec<OrderItem>,
    pub statuses: &'static [&'static str],
    pub csrf: String,
}

#[derive(Template)]
#[template(path = "admin/products.html")]
pub struct ProductsTemplate {
    pub products: Vec<Product>,
    pub categories: Vec<Category>,
}

#[derive(Template)]
#[template(path = "admin/product_form.html")]
pub struct ProductFormTemplate {
    pub product: Option<Product>,
    pub categories: Vec<Category>,
    pub csrf: String,
    pub error: Option<String>,
}

crate::impl_template_response!(
    LoginTemplate,
    DashboardTemplate,
    OrdersTemplate,
    OrderDetailTemplate,
    ProductsTemplate,
    ProductFormTemplate,
    CustomersTemplate,
    PromotionsTemplate,
    PromotionFormTemplate,
);

// --- Auth ---

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("local")
        .trim()
        .to_string()
}

async fn login_form(jar: CookieJar) -> impl IntoResponse {
    let (jar, csrf) = ensure_csrf(jar);
    (jar, LoginTemplate { csrf, error: None })
}

#[derive(Deserialize)]
struct LoginForm {
    csrf: String,
    password: String,
}

async fn login_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = client_ip(&headers);
    if !login_allowed(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Try again in 15 minutes.",
        )
            .into_response();
    }
    if !csrf_ok(&jar, &form.csrf) {
        return (StatusCode::FORBIDDEN, "Invalid request token").into_response();
    }
    if state.cfg.admin_password_hash.is_empty()
        || !verify_password(&state.cfg.admin_password_hash, &form.password)
    {
        record_login_failure(&ip);
        let (jar, csrf) = ensure_csrf(jar);
        return (
            jar,
            LoginTemplate {
                csrf,
                error: Some("Incorrect password.".into()),
            },
        )
            .into_response();
    }
    clear_login_failures(&ip);
    let jar = jar.add(admin_session_cookie(&state.cfg.session_secret));
    (jar, Redirect::to("/admin")).into_response()
}

async fn logout(jar: CookieJar) -> impl IntoResponse {
    (jar.add(admin_logout_cookie()), Redirect::to("/admin/login"))
}

async fn require_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    if is_admin(&jar, &state.cfg.session_secret) {
        next.run(req).await
    } else {
        Redirect::to("/admin/login").into_response()
    }
}

// --- Dashboard ---

const CHART_W: f64 = 640.0;
const CHART_H: f64 = 160.0;

fn revenue_bars(stats: &[DailyStat]) -> Vec<Bar> {
    let max = stats
        .iter()
        .map(|s| s.revenue_inr)
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let n = stats.len().max(1) as f64;
    let gap = 3.0;
    let bw = (CHART_W - gap * (n - 1.0)) / n;
    stats
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let h = if s.revenue_inr == 0 {
                1.5
            } else {
                (s.revenue_inr as f64 / max) * (CHART_H - 4.0)
            };
            Bar {
                x: i as f64 * (bw + gap),
                y: CHART_H - h,
                w: bw,
                h,
                label: s.day.format("%d %b").to_string(),
                value: format!("₹{} · {} orders", s.revenue_inr, s.orders),
            }
        })
        .collect()
}

fn count_bars(rows: &[db::DayCount]) -> Vec<Bar> {
    let max = rows.iter().map(|r| r.count).max().unwrap_or(0).max(1) as f64;
    let n = rows.len().max(1) as f64;
    let gap = 3.0;
    let bw = (CHART_W - gap * (n - 1.0)) / n;
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let h = if r.count == 0 {
                1.5
            } else {
                (r.count as f64 / max) * (CHART_H - 4.0)
            };
            Bar {
                x: i as f64 * (bw + gap),
                y: CHART_H - h,
                w: bw,
                h,
                label: r.day.format("%d %b").to_string(),
                value: format!("{} new", r.count),
            }
        })
        .collect()
}

/// Turns labeled values into horizontal-bar rows (percent of the max), with a
/// custom value formatter (e.g. rupees vs. counts).
fn hbars(rows: &[db::LabeledValue], fmt: impl Fn(i64) -> String) -> Vec<HBar> {
    let max = rows.iter().map(|r| r.value).max().unwrap_or(0).max(1);
    rows.iter()
        .filter(|r| r.value > 0)
        .map(|r| HBar {
            label: r.label.clone(),
            pct: (r.value * 100 / max).clamp(2, 100),
            value: fmt(r.value),
        })
        .collect()
}

async fn dashboard(State(state): State<AppState>) -> AppResult<DashboardTemplate> {
    let stats = db::daily_stats(&state.db).await?;
    let revenue_30d: i64 = stats.iter().map(|s| s.revenue_inr).sum();
    let orders_30d: i64 = stats.iter().map(|s| s.orders).sum();
    let status_counts = db::status_counts(&state.db).await?;
    let top_products = db::top_products(&state.db).await?;
    let new_customers = db::new_customers_by_day(&state.db).await?;
    let (total_customers, repeat_customers) = db::customer_stats(&state.db).await?;
    let category = db::revenue_by_category(&state.db).await?;
    let source = db::orders_by_source(&state.db).await?;
    let slots = db::popular_slots(&state.db).await?;
    Ok(DashboardTemplate {
        revenue_30d,
        orders_30d,
        aov: if orders_30d > 0 { revenue_30d / orders_30d } else { 0 },
        pending_count: status_counts
            .iter()
            .filter(|s| s.status == "paid" || s.status == "confirmed")
            .map(|s| s.count)
            .sum(),
        total_customers,
        repeat_customers,
        repeat_rate: if total_customers > 0 {
            repeat_customers * 100 / total_customers
        } else {
            0
        },
        revenue_bars: revenue_bars(&stats),
        new_customer_bars: count_bars(&new_customers),
        top_max: top_products.iter().map(|t| t.revenue_inr).max().unwrap_or(1).max(1),
        top_products,
        category_bars: hbars(&category, |v| format!("₹{v}")),
        source_split: hbars(&source, |v| v.to_string()),
        slot_split: hbars(&slots, |v| v.to_string()),
        status_counts,
        recent: db::recent_orders(&state.db, 8).await?,
    })
}

// --- Customers ---

async fn customers(State(state): State<AppState>) -> AppResult<CustomersTemplate> {
    Ok(CustomersTemplate {
        customers: db::list_customers(&state.db, 500).await?,
    })
}

// --- Promotions ---

async fn promotions(State(state): State<AppState>) -> AppResult<PromotionsTemplate> {
    Ok(PromotionsTemplate {
        promotions: db::all_promotions(&state.db).await?,
    })
}

async fn promotion_new_form(jar: CookieJar) -> impl IntoResponse {
    let (jar, csrf) = ensure_csrf(jar);
    (jar, PromotionFormTemplate { promotion: None, csrf, error: None })
}

async fn promotion_edit_form(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let Some(promotion) = db::promotion_by_id(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "No such promotion").into_response());
    };
    let (jar, csrf) = ensure_csrf(jar);
    Ok((jar, PromotionFormTemplate { promotion: Some(promotion), csrf, error: None }).into_response())
}

#[derive(Deserialize)]
struct PromotionForm {
    csrf: String,
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    cta_label: String,
    #[serde(default)]
    cta_href: String,
    #[serde(default)]
    image_url: String,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    sort_order: i32,
}

impl PromotionForm {
    fn to_input(&self) -> db::PromotionInput {
        db::PromotionInput {
            title: self.title.trim().to_string(),
            subtitle: self.subtitle.trim().to_string(),
            cta_label: if self.cta_label.trim().is_empty() {
                "Order now".into()
            } else {
                self.cta_label.trim().to_string()
            },
            cta_href: if self.cta_href.trim().is_empty() {
                "/".into()
            } else {
                self.cta_href.trim().to_string()
            },
            image_url: self.image_url.trim().to_string(),
            active: self.active.is_some(),
            sort_order: self.sort_order,
        }
    }
}

async fn promotion_create(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<PromotionForm>,
) -> AppResult<Response> {
    if !csrf_ok(&jar, &form.csrf) {
        return Ok((StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    if form.title.trim().is_empty() {
        let (jar, csrf) = ensure_csrf(jar);
        return Ok((jar, PromotionFormTemplate { promotion: None, csrf, error: Some("Title is required.".into()) }).into_response());
    }
    db::insert_promotion(&state.db, &form.to_input()).await?;
    Ok(Redirect::to("/admin/promotions").into_response())
}

async fn promotion_update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<PromotionForm>,
) -> AppResult<Response> {
    if !csrf_ok(&jar, &form.csrf) {
        return Ok((StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    db::update_promotion(&state.db, id, &form.to_input()).await?;
    Ok(Redirect::to("/admin/promotions").into_response())
}

// --- Orders ---

async fn orders(State(state): State<AppState>) -> AppResult<OrdersTemplate> {
    Ok(OrdersTemplate {
        orders: db::recent_orders(&state.db, 200).await?,
    })
}

async fn order_detail(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let Some(order) = db::order_by_id(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "No such order").into_response());
    };
    let items = db::order_items(&state.db, order.id).await?;
    let (jar, csrf) = ensure_csrf(jar);
    Ok((
        jar,
        OrderDetailTemplate {
            order,
            items,
            statuses: ORDER_STATUSES,
            csrf,
        },
    )
        .into_response())
}

#[derive(Deserialize)]
struct StatusForm {
    csrf: String,
    status: String,
}

async fn update_status(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<StatusForm>,
) -> AppResult<Response> {
    if !csrf_ok(&jar, &form.csrf) {
        return Ok((StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    if !ORDER_STATUSES.contains(&form.status.as_str()) {
        return Ok((StatusCode::BAD_REQUEST, "Bad status").into_response());
    }
    if let Some(order) = db::update_status(&state.db, id, &form.status).await? {
        whatsapp::notify_status_change(&state, &order).await;
    }
    Ok(Redirect::to(&format!("/admin/orders/{id}")).into_response())
}

// --- Products ---

async fn products(State(state): State<AppState>) -> AppResult<ProductsTemplate> {
    Ok(ProductsTemplate {
        products: db::all_products(&state.db).await?,
        categories: db::list_categories(&state.db).await?,
    })
}

async fn product_new_form(State(state): State<AppState>, jar: CookieJar) -> AppResult<Response> {
    let (jar, csrf) = ensure_csrf(jar);
    let t = ProductFormTemplate {
        product: None,
        categories: db::list_categories(&state.db).await?,
        csrf,
        error: None,
    };
    Ok((jar, t).into_response())
}

async fn product_edit_form(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let Some(product) = db::product_by_id(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "No such product").into_response());
    };
    let (jar, csrf) = ensure_csrf(jar);
    let t = ProductFormTemplate {
        product: Some(product),
        categories: db::list_categories(&state.db).await?,
        csrf,
        error: None,
    };
    Ok((jar, t).into_response())
}

struct ParsedProductForm {
    csrf: String,
    input: ProductInput,
}

async fn parse_product_form(
    state: &AppState,
    mut multipart: Multipart,
    existing_image: String,
) -> Result<ParsedProductForm, AppError> {
    let mut csrf = String::new();
    let mut input = ProductInput {
        name: String::new(),
        slug: String::new(),
        description: String::new(),
        price_inr: 0,
        image_url: existing_image,
        category_id: 0,
        is_eggless_available: false,
        is_available: false,
        is_featured: false,
    };
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError(e.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "image" => {
                let filename = field.file_name().unwrap_or_default().to_lowercase();
                let content_type = match filename.rsplit_once('.').map(|(_, e)| e) {
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("png") => "image/png",
                    Some("webp") => "image/webp",
                    _ => continue, // no/unsupported file — keep existing image
                };
                let data = field.bytes().await.map_err(|e| AppError(e.to_string()))?;
                if data.is_empty() {
                    continue;
                }
                if data.len() > 5 * 1024 * 1024 {
                    return Err(AppError("image larger than 5 MB".into()));
                }
                let id = crate::db::insert_image(&state.db, &data, content_type).await?;
                input.image_url = format!("/img/db/{id}");
            }
            _ => {
                let value = field.text().await.map_err(|e| AppError(e.to_string()))?;
                match name.as_str() {
                    "csrf" => csrf = value,
                    "name" => input.name = value.trim().to_string(),
                    "slug" => input.slug = slugify(&value),
                    "description" => input.description = value.trim().to_string(),
                    "price_inr" => input.price_inr = value.trim().parse().unwrap_or(0),
                    "category_id" => input.category_id = value.trim().parse().unwrap_or(0),
                    "is_eggless_available" => input.is_eggless_available = true,
                    "is_available" => input.is_available = true,
                    "is_featured" => input.is_featured = true,
                    _ => {}
                }
            }
        }
    }
    Ok(ParsedProductForm { csrf, input })
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn validate_product(input: &ProductInput) -> Result<(), &'static str> {
    if input.name.is_empty() || input.slug.is_empty() {
        return Err("Name and slug are required.");
    }
    if input.price_inr <= 0 || input.price_inr > 1_000_000 {
        return Err("Price must be a positive rupee amount.");
    }
    if input.category_id <= 0 {
        return Err("Pick a category.");
    }
    Ok(())
}

async fn product_create(
    State(state): State<AppState>,
    jar: CookieJar,
    multipart: Multipart,
) -> AppResult<Response> {
    let parsed = parse_product_form(&state, multipart, String::new()).await?;
    if !csrf_ok(&jar, &parsed.csrf) {
        return Ok((StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    if let Err(msg) = validate_product(&parsed.input) {
        let (jar, csrf) = ensure_csrf(jar);
        let t = ProductFormTemplate {
            product: None,
            categories: db::list_categories(&state.db).await?,
            csrf,
            error: Some(msg.into()),
        };
        return Ok((jar, t).into_response());
    }
    db::insert_product(&state.db, &parsed.input).await?;
    Ok(Redirect::to("/admin/products").into_response())
}

async fn product_update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    multipart: Multipart,
) -> AppResult<Response> {
    let Some(existing) = db::product_by_id(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "No such product").into_response());
    };
    let parsed = parse_product_form(&state, multipart, existing.image_url.clone()).await?;
    if !csrf_ok(&jar, &parsed.csrf) {
        return Ok((StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    if let Err(msg) = validate_product(&parsed.input) {
        let (jar, csrf) = ensure_csrf(jar);
        let t = ProductFormTemplate {
            product: Some(existing),
            categories: db::list_categories(&state.db).await?,
            csrf,
            error: Some(msg.into()),
        };
        return Ok((jar, t).into_response());
    }
    db::update_product(&state.db, id, &parsed.input).await?;
    Ok(Redirect::to("/admin/products").into_response())
}

pub fn router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/admin", get(dashboard))
        .route("/admin/orders", get(orders))
        .route("/admin/orders/{id}", get(order_detail))
        .route("/admin/orders/{id}/status", post(update_status))
        .route("/admin/products", get(products).post(product_create))
        .route("/admin/products/new", get(product_new_form))
        .route(
            "/admin/products/{id}",
            get(product_edit_form).post(product_update),
        )
        .route("/admin/customers", get(customers))
        .route("/admin/promotions", get(promotions).post(promotion_create))
        .route("/admin/promotions/new", get(promotion_new_form))
        .route(
            "/admin/promotions/{id}",
            get(promotion_edit_form).post(promotion_update),
        )
        .route("/admin/logout", post(logout))
        .route_layer(middleware::from_fn_with_state(state, require_admin));
    Router::new()
        .route("/admin/login", get(login_form).post(login_submit))
        .merge(protected)
}
