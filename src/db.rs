use chrono::NaiveDate;
use rand::Rng;
use sqlx::{PgPool, Postgres, Transaction};

use crate::models::{CartLine, Category, Order, OrderItem, Product};

pub async fn list_categories(pool: &PgPool) -> sqlx::Result<Vec<Category>> {
    sqlx::query_as("select * from categories order by sort_order, id")
        .fetch_all(pool)
        .await
}

pub async fn category_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<Option<Category>> {
    sqlx::query_as("select * from categories where slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await
}

pub async fn featured_products(pool: &PgPool) -> sqlx::Result<Vec<Product>> {
    sqlx::query_as("select * from products where is_featured and is_available order by id")
        .fetch_all(pool)
        .await
}

pub async fn products_in_category(pool: &PgPool, category_id: i64) -> sqlx::Result<Vec<Product>> {
    sqlx::query_as("select * from products where category_id = $1 and is_available order by id")
        .bind(category_id)
        .fetch_all(pool)
        .await
}

pub async fn product_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<Option<Product>> {
    sqlx::query_as("select * from products where slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await
}

pub async fn product_by_id(pool: &PgPool, id: i64) -> sqlx::Result<Option<Product>> {
    sqlx::query_as("select * from products where id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn products_by_ids(pool: &PgPool, ids: &[i64]) -> sqlx::Result<Vec<Product>> {
    sqlx::query_as("select * from products where id = any($1)")
        .bind(ids)
        .fetch_all(pool)
        .await
}

pub async fn search_products(pool: &PgPool, query: &str) -> sqlx::Result<Vec<Product>> {
    sqlx::query_as(
        "select * from products where is_available \
         and (name ilike '%' || $1 || '%' or description ilike '%' || $1 || '%') \
         order by is_featured desc, id limit 10",
    )
    .bind(query)
    .fetch_all(pool)
    .await
}

pub async fn all_products(pool: &PgPool) -> sqlx::Result<Vec<Product>> {
    sqlx::query_as("select * from products order by category_id, id")
        .fetch_all(pool)
        .await
}

fn generate_order_number() -> String {
    // Unguessable enough to act as the order-lookup token (~41 bits).
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let tail: String = (0..8)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("SP-{tail}")
}

pub struct NewOrder {
    pub customer_name: String,
    pub phone: String,
    pub email: Option<String>,
    pub address: String,
    pub delivery_date: NaiveDate,
    pub delivery_slot: String,
    pub notes: Option<String>,
    pub source: String,
}

/// Creates the order with items in one transaction. Prices are always read
/// from the products table here — cart input is untrusted.
pub async fn create_order(pool: &PgPool, new: NewOrder, cart: &[CartLine]) -> sqlx::Result<Order> {
    let ids: Vec<i64> = cart.iter().map(|l| l.product_id).collect();
    let products = products_by_ids(pool, &ids).await?;
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    let mut subtotal: i64 = 0;
    let mut lines: Vec<(&Product, &CartLine)> = Vec::new();
    for line in cart {
        let product = products
            .iter()
            .find(|p| p.id == line.product_id && p.is_available)
            .ok_or(sqlx::Error::RowNotFound)?;
        let qty = line.qty.clamp(1, 20) as i64;
        subtotal += product.price_inr * qty;
        lines.push((product, line));
    }

    let order: Order = sqlx::query_as(
        "insert into orders (order_number, customer_name, phone, email, address, \
         delivery_date, delivery_slot, notes, subtotal_inr, total_inr, source) \
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9, $10) returning *",
    )
    .bind(generate_order_number())
    .bind(&new.customer_name)
    .bind(&new.phone)
    .bind(&new.email)
    .bind(&new.address)
    .bind(new.delivery_date)
    .bind(&new.delivery_slot)
    .bind(&new.notes)
    .bind(subtotal)
    .bind(&new.source)
    .fetch_one(&mut *tx)
    .await?;

    for (product, line) in lines {
        sqlx::query(
            "insert into order_items (order_id, product_id, product_name, unit_price_inr, qty, eggless, customization) \
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(order.id)
        .bind(product.id)
        .bind(&product.name)
        .bind(product.price_inr)
        .bind(line.qty.clamp(1, 20))
        .bind(line.eggless)
        .bind(&line.customization)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(order)
}

pub async fn order_by_number(pool: &PgPool, number: &str) -> sqlx::Result<Option<Order>> {
    sqlx::query_as("select * from orders where order_number = $1")
        .bind(number)
        .fetch_optional(pool)
        .await
}

pub async fn order_by_id(pool: &PgPool, id: i64) -> sqlx::Result<Option<Order>> {
    sqlx::query_as("select * from orders where id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn order_items(pool: &PgPool, order_id: i64) -> sqlx::Result<Vec<OrderItem>> {
    sqlx::query_as("select * from order_items where order_id = $1 order by id")
        .bind(order_id)
        .fetch_all(pool)
        .await
}

pub async fn set_razorpay_order(pool: &PgPool, id: i64, rzp_order_id: &str) -> sqlx::Result<()> {
    sqlx::query("update orders set razorpay_order_id = $2, updated_at = now() where id = $1")
        .bind(id)
        .bind(rzp_order_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_payment_link(pool: &PgPool, id: i64, link_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "update orders set razorpay_payment_link_id = $2, updated_at = now() where id = $1",
    )
    .bind(id)
    .bind(link_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Marks an order paid (idempotent). Returns the order only on the first transition,
/// so notification sends exactly once even if the webhook retries.
pub async fn mark_paid(
    pool: &PgPool,
    order_number: &str,
    payment_id: &str,
) -> sqlx::Result<Option<Order>> {
    sqlx::query_as(
        "update orders set status = 'paid', razorpay_payment_id = $2, updated_at = now() \
         where order_number = $1 and status = 'pending' returning *",
    )
    .bind(order_number)
    .bind(payment_id)
    .fetch_optional(pool)
    .await
}

pub async fn update_status(pool: &PgPool, id: i64, status: &str) -> sqlx::Result<Option<Order>> {
    sqlx::query_as("update orders set status = $2, updated_at = now() where id = $1 returning *")
        .bind(id)
        .bind(status)
        .fetch_optional(pool)
        .await
}

pub async fn recent_orders(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<Order>> {
    sqlx::query_as("select * from orders order by created_at desc limit $1")
        .bind(limit)
        .fetch_all(pool)
        .await
}

// --- Admin product management ---

pub struct ProductInput {
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

pub async fn insert_product(pool: &PgPool, p: &ProductInput) -> sqlx::Result<Product> {
    sqlx::query_as(
        "insert into products (name, slug, description, price_inr, image_url, category_id, \
         is_eggless_available, is_available, is_featured) \
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning *",
    )
    .bind(&p.name)
    .bind(&p.slug)
    .bind(&p.description)
    .bind(p.price_inr)
    .bind(&p.image_url)
    .bind(p.category_id)
    .bind(p.is_eggless_available)
    .bind(p.is_available)
    .bind(p.is_featured)
    .fetch_one(pool)
    .await
}

pub async fn update_product(pool: &PgPool, id: i64, p: &ProductInput) -> sqlx::Result<()> {
    sqlx::query(
        "update products set name = $2, slug = $3, description = $4, price_inr = $5, \
         image_url = $6, category_id = $7, is_eggless_available = $8, is_available = $9, \
         is_featured = $10 where id = $1",
    )
    .bind(id)
    .bind(&p.name)
    .bind(&p.slug)
    .bind(&p.description)
    .bind(p.price_inr)
    .bind(&p.image_url)
    .bind(p.category_id)
    .bind(p.is_eggless_available)
    .bind(p.is_available)
    .bind(p.is_featured)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Analytics ---

#[derive(Debug, sqlx::FromRow)]
pub struct DailyStat {
    pub day: NaiveDate,
    pub revenue_inr: i64,
    pub orders: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TopProduct {
    pub product_name: String,
    pub qty: i64,
    pub revenue_inr: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// Paid-or-later orders per day over the last 30 days (gaps filled with zeros).
pub async fn daily_stats(pool: &PgPool) -> sqlx::Result<Vec<DailyStat>> {
    sqlx::query_as(
        "select d::date as day, \
                coalesce(sum(o.total_inr), 0)::bigint as revenue_inr, \
                count(o.id)::bigint as orders \
         from generate_series(current_date - 29, current_date, interval '1 day') d \
         left join orders o on o.created_at::date = d::date and o.status <> 'pending' and o.status <> 'cancelled' \
         group by d order by d",
    )
    .fetch_all(pool)
    .await
}

pub async fn top_products(pool: &PgPool) -> sqlx::Result<Vec<TopProduct>> {
    sqlx::query_as(
        "select i.product_name, sum(i.qty)::bigint as qty, \
                sum(i.qty * i.unit_price_inr)::bigint as revenue_inr \
         from order_items i join orders o on o.id = i.order_id \
         where o.status <> 'pending' and o.status <> 'cancelled' \
           and o.created_at > now() - interval '30 days' \
         group by i.product_name order by revenue_inr desc limit 5",
    )
    .fetch_all(pool)
    .await
}

pub async fn status_counts(pool: &PgPool) -> sqlx::Result<Vec<StatusCount>> {
    sqlx::query_as(
        "select status, count(*)::bigint as count from orders \
         where created_at > now() - interval '30 days' group by status order by count desc",
    )
    .fetch_all(pool)
    .await
}

// --- WhatsApp bot sessions ---

#[derive(Debug, sqlx::FromRow)]
pub struct WaSession {
    pub phone: String,
    pub state: String,
    pub cart: serde_json::Value,
    pub context: serde_json::Value,
}

pub async fn wa_session(pool: &PgPool, phone: &str) -> sqlx::Result<WaSession> {
    sqlx::query_as(
        "insert into wa_sessions (phone) values ($1) \
         on conflict (phone) do update set phone = excluded.phone \
         returning phone, state, cart, context",
    )
    .bind(phone)
    .fetch_one(pool)
    .await
}

pub async fn wa_session_save(
    pool: &PgPool,
    phone: &str,
    state: &str,
    cart: &serde_json::Value,
    context: &serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "update wa_sessions set state = $2, cart = $3, context = $4, updated_at = now() \
         where phone = $1",
    )
    .bind(phone)
    .bind(state)
    .bind(cart)
    .bind(context)
    .execute(pool)
    .await?;
    Ok(())
}
