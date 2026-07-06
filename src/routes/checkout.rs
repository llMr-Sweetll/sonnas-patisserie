use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Local, NaiveDate};
use serde::Deserialize;

use crate::auth::{csrf_ok, ensure_csrf};
use crate::cart::{clear_cart, read_cart};
use crate::models::{CartLine, Category, DELIVERY_SLOTS, Order, is_deliverable};
use crate::routes::store::{CartView, build_cart_view, whatsapp_cart_link};
use crate::{AppResult, AppState, db, razorpay, whatsapp};

#[derive(Template)]
#[template(path = "checkout.html")]
pub struct CheckoutTemplate {
    pub categories: Vec<Category>,
    pub lines: Vec<CartView>,
    pub subtotal: i64,
    pub wa_link: String,
    pub payment_ready: bool,
    pub csrf: String,
    pub min_date: String,
    pub slots: &'static [&'static str],
    pub cart_count: usize,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "payment.html")]
pub struct PaymentTemplate {
    pub categories: Vec<Category>,
    pub order: Order,
    pub key_id: String,
    pub rzp_order_id: String,
    pub amount_paise: i64,
    pub csrf: String,
    pub cart_count: usize,
}

crate::impl_template_response!(CheckoutTemplate, PaymentTemplate);

async fn checkout_template(
    state: &AppState,
    jar: CookieJar,
    cart: &[CartLine],
    error: Option<String>,
) -> AppResult<(CookieJar, CheckoutTemplate)> {
    let (lines, subtotal) = build_cart_view(state, cart).await?;
    let wa_link = whatsapp_cart_link(&state.cfg.owner_whatsapp_number, &lines);
    let (jar, csrf) = ensure_csrf(jar);
    let t = CheckoutTemplate {
        categories: db::list_categories(&state.db).await?,
        lines,
        subtotal,
        wa_link,
        payment_ready: state.cfg.razorpay_checkout_ready(),
        csrf,
        min_date: Local::now().date_naive().to_string(),
        slots: DELIVERY_SLOTS,
        cart_count: cart.len(),
        error,
    };
    Ok((jar, t))
}

async fn checkout_form(
    State(state): State<AppState>,
    jar: CookieJar,
) -> AppResult<impl IntoResponse> {
    let cart = read_cart(&jar);
    if cart.is_empty() {
        return Ok((jar, Redirect::to("/cart")).into_response());
    }
    let (jar, t) = checkout_template(&state, jar, &cart, None).await?;
    Ok((jar, t).into_response())
}

#[derive(Deserialize)]
pub struct CheckoutForm {
    pub csrf: String,
    pub customer_name: String,
    pub phone: String,
    pub email: String,
    pub address: String,
    pub delivery_date: NaiveDate,
    pub delivery_slot: String,
    pub notes: String,
    #[serde(default)]
    pub consent: Option<String>, // DPDP notice+consent checkbox: "on" when ticked
}

fn validate(form: &CheckoutForm) -> Result<(), &'static str> {
    if form.consent.is_none() {
        return Err("Please agree to the privacy notice so we can process your delivery details.");
    }
    let name = form.customer_name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err("Please enter your name.");
    }
    let phone: String = form.phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if phone.len() < 10 || phone.len() > 12 {
        return Err("Please enter a valid Indian phone number.");
    }
    let email = form.email.trim();
    if !email.is_empty() && (!email.contains('@') || !email.contains('.')) {
        return Err("Please enter a valid email (or leave it blank).");
    }
    if form.address.trim().len() < 10 {
        return Err("Please enter a full delivery address.");
    }
    let today = Local::now().date_naive();
    if form.delivery_date < today || form.delivery_date > today + chrono::Duration::days(30) {
        return Err("Delivery date must be within the next 30 days.");
    }
    if !is_deliverable(form.delivery_date) {
        return Err("We're closed on Tuesdays — please pick another day.");
    }
    if !DELIVERY_SLOTS.contains(&form.delivery_slot.as_str()) {
        return Err("Please pick a delivery slot.");
    }
    Ok(())
}

/// Normalises a phone to E.164-without-plus, assuming India when 10 digits.
pub fn normalize_phone(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 10 {
        format!("91{digits}")
    } else {
        digits
    }
}

async fn checkout_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CheckoutForm>,
) -> AppResult<axum::response::Response> {
    let cart = read_cart(&jar);
    if cart.is_empty() {
        return Ok(Redirect::to("/cart").into_response());
    }
    if !csrf_ok(&jar, &form.csrf) {
        return Ok((axum::http::StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    if let Err(msg) = validate(&form) {
        let (jar, t) = checkout_template(&state, jar, &cart, Some(msg.to_string())).await?;
        return Ok((jar, t).into_response());
    }
    if !state.cfg.razorpay_checkout_ready() {
        let (jar, t) = checkout_template(
            &state,
            jar,
            &cart,
            Some("Online payment is being connected. Please order on WhatsApp for now.".into()),
        )
        .await?;
        return Ok((jar, t).into_response());
    }

    let order = db::create_order(
        &state.db,
        db::NewOrder {
            customer_name: form.customer_name.trim().to_string(),
            phone: normalize_phone(&form.phone),
            email: (!form.email.trim().is_empty()).then(|| form.email.trim().to_string()),
            address: form.address.trim().to_string(),
            delivery_date: form.delivery_date,
            delivery_slot: form.delivery_slot.clone(),
            notes: (!form.notes.trim().is_empty())
                .then(|| form.notes.trim().chars().take(500).collect()),
            source: "web".into(),
        },
        &cart,
    )
    .await?;

    let rzp_order_id = razorpay::create_order(&state, order.total_inr, &order.order_number)
        .await
        .map_err(crate::AppError)?;
    db::set_razorpay_order(&state.db, order.id, &rzp_order_id).await?;

    let (jar, csrf) = ensure_csrf(jar);
    let t = PaymentTemplate {
        categories: db::list_categories(&state.db).await?,
        amount_paise: order.total_inr * 100,
        key_id: state.cfg.razorpay_key_id.clone(),
        rzp_order_id,
        order,
        csrf,
        cart_count: cart.len(),
    };
    Ok((jar, t).into_response())
}

#[derive(Deserialize)]
pub struct VerifyForm {
    pub csrf: String,
    pub order_number: String,
    pub razorpay_order_id: String,
    pub razorpay_payment_id: String,
    pub razorpay_signature: String,
}

/// Browser-side payment confirmation. The Razorpay webhook remains the source of
/// truth; this exists so local dev and fast redirects work without a public URL.
async fn checkout_verify(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<VerifyForm>,
) -> AppResult<axum::response::Response> {
    if !csrf_ok(&jar, &form.csrf) {
        return Ok((axum::http::StatusCode::FORBIDDEN, "Invalid request token").into_response());
    }
    let genuine = razorpay::verify_checkout_signature(
        &state.cfg.razorpay_key_secret,
        &form.razorpay_order_id,
        &form.razorpay_payment_id,
        &form.razorpay_signature,
    );
    // The signature only proves payment happened; the order must also match.
    let matches_order = db::order_by_number(&state.db, &form.order_number)
        .await?
        .is_some_and(|o| o.razorpay_order_id.as_deref() == Some(form.razorpay_order_id.as_str()));
    if genuine && matches_order {
        if let Some(order) =
            db::mark_paid(&state.db, &form.order_number, &form.razorpay_payment_id).await?
        {
            let _ = db::upsert_customer_order(
                &state.db,
                &order.phone,
                &order.customer_name,
                order.email.as_deref(),
                order.total_inr,
            )
            .await;
            let items = db::order_items(&state.db, order.id).await?;
            whatsapp::notify_order_paid(&state, &order, &items).await;
        }
        let jar = clear_cart(jar);
        return Ok((jar, Redirect::to(&format!("/order/{}", form.order_number))).into_response());
    }
    Ok((
        axum::http::StatusCode::BAD_REQUEST,
        "Payment verification failed",
    )
        .into_response())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/checkout", get(checkout_form).post(checkout_submit))
        .route("/checkout/verify", post(checkout_verify))
}
