use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub kind: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub price_inr: i64,
    pub image_url: String,
    pub category_id: i64,
    pub is_eggless_available: bool,
    pub is_available: bool,
    pub is_featured: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct Order {
    pub id: i64,
    pub order_number: String,
    pub customer_name: String,
    pub phone: String,
    pub email: Option<String>,
    pub address: String,
    pub delivery_date: NaiveDate,
    pub delivery_slot: String,
    pub notes: Option<String>,
    pub subtotal_inr: i64,
    pub total_inr: i64,
    pub status: String,
    pub source: String,
    pub razorpay_order_id: Option<String>,
    pub razorpay_payment_id: Option<String>,
    pub razorpay_payment_link_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OrderItem {
    pub id: i64,
    pub order_id: i64,
    pub product_id: Option<i64>,
    pub product_name: String,
    pub unit_price_inr: i64,
    pub qty: i32,
    pub eggless: bool,
    pub customization: Option<String>,
}

/// One line in a cart — shared by the web cookie cart and the WhatsApp bot cart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartLine {
    pub product_id: i64,
    pub qty: i32,
    #[serde(default)]
    pub eggless: bool,
    #[serde(default)]
    pub customization: Option<String>,
}

impl Product {
    /// Monogram for the placeholder "ganache tile" shown until real photos exist.
    pub fn initial(&self) -> String {
        self.name.chars().next().unwrap_or('S').to_string()
    }

    /// Deterministic tile variant so placeholder gradients vary across a grid.
    pub fn tile_variant(&self) -> i64 {
        self.id % 4
    }

    /// First sentence of the description, for compact product cards.
    pub fn short_desc(&self) -> String {
        match self.description.split_once(". ") {
            Some((first, _)) => format!("{first}."),
            None => self.description.clone(),
        }
    }
}

impl Order {
    pub fn has_status(&self, status: &str) -> bool {
        self.status == status
    }
}

impl OrderItem {
    pub fn line_total(&self) -> i64 {
        self.unit_price_inr * self.qty as i64
    }
}

pub const ORDER_STATUSES: &[&str] = &[
    "pending",
    "paid",
    "confirmed",
    "out_for_delivery",
    "delivered",
    "cancelled",
];

pub const DELIVERY_SLOTS: &[&str] = &["2pm – 5pm", "5pm – 8pm", "8pm – 10pm"];

/// The shop is closed on Tuesdays — no deliveries that day.
pub fn is_deliverable(date: NaiveDate) -> bool {
    date.weekday() != chrono::Weekday::Tue
}

pub fn status_label(status: &str) -> &'static str {
    match status {
        "pending" => "Pending payment",
        "paid" => "Paid",
        "confirmed" => "Confirmed",
        "out_for_delivery" => "Out for delivery",
        "delivered" => "Delivered",
        "cancelled" => "Cancelled",
        _ => "Unknown",
    }
}
