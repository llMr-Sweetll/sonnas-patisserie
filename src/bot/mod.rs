//! WhatsApp ordering bot. Menu-driven happy path (interactive lists/buttons);
//! free text falls through to the Claude layer in `claude.rs`.
//!
//! Conversation state lives in `wa_sessions` keyed by phone. Interactive reply
//! ids encode actions ("cat:3", "add:5", "date:2026-07-10") so most taps are
//! stateless; text-input steps use `state` (customize → ask_name → ask_address
//! → ask_date/slot).

pub mod claude;
pub mod faq;

use chrono::{Datelike, Duration, Local, NaiveDate};
use serde_json::{Value, json};

use crate::models::{CartLine, DELIVERY_SLOTS, is_deliverable};
use crate::whatsapp::{ListRow, send_buttons, send_list, send_text};
use crate::{AppResult, AppState, db, razorpay};

const SHOP_GREETING: &str = "Welcome to *Sonna's Patisserie* 🧁\nArtisanal, 100% vegetarian cakes & desserts, baked fresh in Hubli and delivered to your door (2pm–10pm, closed Tuesdays).";

pub async fn handle_message(state: &AppState, message: &Value) -> AppResult<()> {
    let phone = message["from"].as_str().unwrap_or_default().to_string();
    if phone.is_empty() {
        return Ok(());
    }
    let session = db::wa_session(&state.db, &phone).await?;
    let mut cart: Vec<CartLine> = serde_json::from_value(session.cart.clone()).unwrap_or_default();
    let mut ctx = session.context.clone();
    let mut next_state = session.state.clone();

    let action = message["interactive"]["button_reply"]["id"]
        .as_str()
        .or_else(|| message["interactive"]["list_reply"]["id"].as_str())
        .map(str::to_string);
    let text = message["text"]["body"].as_str().map(str::to_string);

    if let Some(action) = action {
        next_state =
            handle_action(state, &phone, &action, &mut cart, &mut ctx, &session.state).await?;
    } else if let Some(text) = text {
        next_state = handle_text(state, &phone, &text, &mut cart, &mut ctx, &session.state).await?;
    } else {
        // Media/location/etc — steer back to the menu.
        send_buttons(
            state,
            &phone,
            "I can help you browse and order 🍰",
            &[("menu", "Browse menu"), ("cart", "My cart")],
        )
        .await;
    }

    db::wa_session_save(&state.db, &phone, &next_state, &json!(cart), &ctx).await?;
    Ok(())
}

async fn handle_action(
    state: &AppState,
    phone: &str,
    action: &str,
    cart: &mut Vec<CartLine>,
    ctx: &mut Value,
    current_state: &str,
) -> AppResult<String> {
    match action {
        "menu" => {
            send_category_list(state, phone).await?;
            Ok("start".into())
        }
        "cart" => {
            send_cart_summary(state, phone, cart).await?;
            Ok("start".into())
        }
        "clear" => {
            cart.clear();
            send_buttons(state, phone, "Cart cleared 🧹", &[("menu", "Browse menu")]).await;
            Ok("start".into())
        }
        "checkout" => start_checkout(state, phone, cart, ctx).await,
        "skip" if current_state == "customize" => {
            after_add(state, phone, cart).await;
            Ok("start".into())
        }
        _ if action.starts_with("cat:") => {
            let id: i64 = action[4..].parse().unwrap_or(0);
            send_product_list(state, phone, id).await?;
            Ok("start".into())
        }
        _ if action.starts_with("prod:") => {
            let id: i64 = action[5..].parse().unwrap_or(0);
            send_product_card(state, phone, id).await?;
            Ok("start".into())
        }
        _ if action.starts_with("add:") || action.starts_with("addegg:") => {
            let eggless = action.starts_with("addegg:");
            let id: i64 = action
                .split(':')
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let Some(product) = db::product_by_id(&state.db, id).await? else {
                send_text(state, phone, "That item isn't available right now, sorry!").await;
                return Ok("start".into());
            };
            cart.push(CartLine {
                product_id: id,
                qty: 1,
                eggless,
                customization: None,
            });
            send_buttons(
                state,
                phone,
                &format!(
                    "*{}* added to your cart 🛒\n\nWould you like a message written on it (e.g. \"Happy Birthday Ananya\")? Reply with the message, or skip.",
                    product.name
                ),
                &[("skip", "Skip")],
            )
            .await;
            Ok("customize".into())
        }
        _ if action.starts_with("date:") => {
            let date = action[5..].to_string();
            ctx["delivery_date"] = json!(date);
            let ids: Vec<String> = (0..DELIVERY_SLOTS.len())
                .map(|i| format!("slot:{i}"))
                .collect();
            let slots: Vec<(&str, &str)> = ids
                .iter()
                .zip(DELIVERY_SLOTS)
                .map(|(id, s)| (id.as_str(), *s))
                .collect();
            send_buttons(
                state,
                phone,
                "And which delivery window suits you? ⏰",
                &slots,
            )
            .await;
            Ok("ask_slot".into())
        }
        _ if action.starts_with("slot:") => {
            let idx: usize = action[5..].parse().unwrap_or(0);
            let slot = DELIVERY_SLOTS.get(idx).unwrap_or(&DELIVERY_SLOTS[0]);
            finalize_order(state, phone, cart, ctx, slot).await
        }
        _ => {
            send_welcome(state, phone).await;
            Ok("start".into())
        }
    }
}

async fn handle_text(
    state: &AppState,
    phone: &str,
    text: &str,
    cart: &mut Vec<CartLine>,
    ctx: &mut Value,
    current_state: &str,
) -> AppResult<String> {
    let trimmed = text.trim();
    match current_state {
        "customize" => {
            let lower = trimmed.to_lowercase();
            if !matches!(lower.as_str(), "no" | "skip" | "nope" | "none") {
                if let Some(last) = cart.last_mut() {
                    last.customization = Some(trimmed.chars().take(200).collect());
                }
                send_text(state, phone, "Noted! ✍️").await;
            }
            after_add(state, phone, cart).await;
            Ok("start".into())
        }
        "ask_name" => {
            if trimmed.is_empty() || trimmed.len() > 100 {
                send_text(state, phone, "Please tell me the name for the delivery.").await;
                return Ok("ask_name".into());
            }
            ctx["customer_name"] = json!(trimmed);
            send_text(
                state,
                phone,
                "Thanks! And the full delivery address in Hubli? 📍",
            )
            .await;
            Ok("ask_address".into())
        }
        "ask_address" => {
            if trimmed.len() < 10 {
                send_text(state, phone, "That address looks too short — please send the full address with landmark/area.").await;
                return Ok("ask_address".into());
            }
            ctx["address"] = json!(trimmed.chars().take(500).collect::<String>());
            send_date_buttons(state, phone).await;
            Ok("ask_date".into())
        }
        "ask_date" => {
            send_date_buttons(state, phone).await;
            Ok("ask_date".into())
        }
        _ => {
            let lower = trimmed.to_lowercase();
            if matches!(
                lower.as_str(),
                "hi" | "hello" | "hey" | "menu" | "start" | "namaste"
            ) {
                send_welcome(state, phone).await;
                return Ok("start".into());
            }
            // If they volunteered a birthday, remember it (opt-in via volunteering).
            if let Some(bd) = faq::detect_birthday(trimmed) {
                let name = ctx["customer_name"].as_str();
                db::set_customer_birthday(&state.db, phone, name, bd).await?;
                send_text(
                    state,
                    phone,
                    &format!(
                        "Noted — I'll remember your birthday is *{}* 🎂 We'll have something sweet for you when it comes around!",
                        bd.format("%d %B")
                    ),
                )
                .await;
                return Ok("start".into());
            }
            // Deterministic FAQ answers work with zero AI keys.
            if let Some(answer) = faq::match_faq(trimmed) {
                send_buttons(
                    state,
                    phone,
                    &answer,
                    &[("menu", "Browse menu 🍰"), ("cart", "My cart 🛒")],
                )
                .await;
                return Ok("start".into());
            }
            // Anything else → Claude with catalog/cart tools (falls back to the
            // guided menu when no ANTHROPIC_API_KEY is configured).
            claude::handle_free_text(state, phone, trimmed, cart, ctx).await
        }
    }
}

pub async fn send_welcome(state: &AppState, phone: &str) {
    send_buttons(
        state,
        phone,
        SHOP_GREETING,
        &[("menu", "Browse menu 🍰"), ("cart", "My cart 🛒")],
    )
    .await;
}

async fn send_category_list(state: &AppState, phone: &str) -> AppResult<()> {
    let categories = db::list_categories(&state.db).await?;
    let rows: Vec<ListRow> = categories
        .iter()
        .map(|c| ListRow {
            id: format!("cat:{}", c.id),
            title: c.name.clone(),
            description: String::new(),
        })
        .collect();
    send_list(
        state,
        phone,
        "What are you craving today?",
        "Categories",
        "Our menu",
        &rows,
    )
    .await;
    Ok(())
}

async fn send_product_list(state: &AppState, phone: &str, category_id: i64) -> AppResult<()> {
    let products = db::products_in_category(&state.db, category_id).await?;
    if products.is_empty() {
        send_buttons(
            state,
            phone,
            "Nothing in that category right now.",
            &[("menu", "Back to menu")],
        )
        .await;
        return Ok(());
    }
    let rows: Vec<ListRow> = products
        .iter()
        .map(|p| ListRow {
            id: format!("prod:{}", p.id),
            title: p.name.clone(),
            description: format!("₹{}", p.price_inr),
        })
        .collect();
    send_list(
        state,
        phone,
        "Tap an item for details 👇",
        "View items",
        "Available today",
        &rows,
    )
    .await;
    Ok(())
}

async fn send_product_card(state: &AppState, phone: &str, id: i64) -> AppResult<()> {
    let Some(p) = db::product_by_id(&state.db, id).await? else {
        send_text(state, phone, "That item isn't available right now, sorry!").await;
        return Ok(());
    };
    let body = format!("*{}* — ₹{}\n\n{}", p.name, p.price_inr, p.description);
    let add = format!("add:{}", p.id);
    let addegg = format!("addegg:{}", p.id);
    let mut buttons: Vec<(&str, &str)> = vec![(add.as_str(), "Add to cart 🛒")];
    if p.is_eggless_available {
        buttons.push((addegg.as_str(), "Add eggless 🥚🚫"));
    }
    buttons.push(("menu", "Back"));
    send_buttons(state, phone, &body, &buttons).await;
    Ok(())
}

async fn after_add(state: &AppState, phone: &str, cart: &[CartLine]) {
    send_buttons(
        state,
        phone,
        &format!("You have *{}* item(s) in your cart.", cart.len()),
        &[
            ("menu", "Add more 🍰"),
            ("checkout", "Checkout ✅"),
            ("cart", "View cart 🛒"),
        ],
    )
    .await;
}

pub async fn cart_summary_text(state: &AppState, cart: &[CartLine]) -> AppResult<(String, i64)> {
    let ids: Vec<i64> = cart.iter().map(|l| l.product_id).collect();
    let products = db::products_by_ids(&state.db, &ids).await?;
    let mut total = 0i64;
    let mut out = String::new();
    for line in cart {
        if let Some(p) = products.iter().find(|p| p.id == line.product_id) {
            let line_total = p.price_inr * line.qty as i64;
            total += line_total;
            out.push_str(&format!("• {} × {} — ₹{line_total}", p.name, line.qty));
            if line.eggless {
                out.push_str(" (eggless)");
            }
            if let Some(c) = &line.customization {
                out.push_str(&format!("\n  ✍️ \"{c}\""));
            }
            out.push('\n');
        }
    }
    Ok((out, total))
}

async fn send_cart_summary(state: &AppState, phone: &str, cart: &[CartLine]) -> AppResult<()> {
    if cart.is_empty() {
        send_buttons(
            state,
            phone,
            "Your cart is empty.",
            &[("menu", "Browse menu 🍰")],
        )
        .await;
        return Ok(());
    }
    let (summary, total) = cart_summary_text(state, cart).await?;
    send_buttons(
        state,
        phone,
        &format!("🛒 *Your cart*\n\n{summary}\nTotal: *₹{total}*"),
        &[
            ("checkout", "Checkout ✅"),
            ("menu", "Add more"),
            ("clear", "Clear cart"),
        ],
    )
    .await;
    Ok(())
}

pub async fn start_checkout(
    state: &AppState,
    phone: &str,
    cart: &[CartLine],
    _ctx: &mut Value,
) -> AppResult<String> {
    if cart.is_empty() {
        send_buttons(
            state,
            phone,
            "Your cart is empty — let's fix that!",
            &[("menu", "Browse menu 🍰")],
        )
        .await;
        return Ok("start".into());
    }
    send_text(
        state,
        phone,
        "Lovely! What name should the delivery be under?",
    )
    .await;
    Ok("ask_name".into())
}

async fn send_date_buttons(state: &AppState, phone: &str) {
    let today = Local::now().date_naive();
    let dates: Vec<NaiveDate> = (0..5)
        .map(|d| today + Duration::days(d))
        .filter(|d| is_deliverable(*d))
        .take(3)
        .collect();
    let labels: Vec<(String, String)> = dates
        .iter()
        .map(|d| {
            let label = if *d == today {
                "Today".to_string()
            } else if *d == today + Duration::days(1) {
                "Tomorrow".to_string()
            } else {
                format!("{} {}", d.weekday(), d.format("%d %b"))
            };
            (format!("date:{d}"), label)
        })
        .collect();
    let buttons: Vec<(&str, &str)> = labels
        .iter()
        .map(|(id, l)| (id.as_str(), l.as_str()))
        .collect();
    send_buttons(
        state,
        phone,
        "When should we deliver? 📅\n(We're closed Tuesdays)",
        &buttons,
    )
    .await;
}

async fn finalize_order(
    state: &AppState,
    phone: &str,
    cart: &mut Vec<CartLine>,
    ctx: &mut Value,
    slot: &str,
) -> AppResult<String> {
    let name = ctx["customer_name"]
        .as_str()
        .unwrap_or("WhatsApp customer")
        .to_string();
    let address = ctx["address"].as_str().unwrap_or_default().to_string();
    let date: NaiveDate = ctx["delivery_date"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| Local::now().date_naive());
    if address.len() < 10 || cart.is_empty() {
        send_welcome(state, phone).await;
        return Ok("start".into());
    }

    let order = db::create_order(
        &state.db,
        db::NewOrder {
            customer_name: name.clone(),
            phone: phone.to_string(),
            email: None,
            address,
            delivery_date: date,
            delivery_slot: slot.to_string(),
            notes: None,
            source: "whatsapp".into(),
        },
        cart,
    )
    .await?;

    match razorpay::create_payment_link(state, order.total_inr, &order.order_number, &name, phone)
        .await
    {
        Ok((link_id, url)) => {
            db::set_payment_link(&state.db, order.id, &link_id).await?;
            send_text(
                state,
                phone,
                &format!(
                    "Your order *{}* is ready — total *₹{}* 🎂\n\nPay securely here to confirm:\n{url}\n\nBy paying you agree we use your name, number and address to deliver this order ({}/privacy).\n\nWe'll send a confirmation the moment payment lands!",
                    order.order_number, order.total_inr, state.cfg.base_url
                ),
            )
            .await;
            cart.clear();
            *ctx = json!({ "customer_name": name });
            Ok("start".into())
        }
        Err(e) => {
            tracing::error!(error = %e, "payment link failed");
            send_text(state, phone, "We couldn't create your payment link just now — please try again in a few minutes.").await;
            Ok("start".into())
        }
    }
}
