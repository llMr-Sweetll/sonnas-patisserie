use chrono::NaiveDate;
use rand::Rng;
use sqlx::{PgPool, Postgres, Transaction};

use crate::models::{CartLine, Category, Customer, Order, OrderItem, Product, Promotion};

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

// --- Product images (stored in Postgres) ---

pub async fn insert_image(pool: &PgPool, bytes: &[u8], content_type: &str) -> sqlx::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "insert into product_images (bytes, content_type) values ($1, $2) returning id",
    )
    .bind(bytes)
    .bind(content_type)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn get_image(pool: &PgPool, id: i64) -> sqlx::Result<Option<(Vec<u8>, String)>> {
    let row: Option<(Vec<u8>, String)> =
        sqlx::query_as("select bytes, content_type from product_images where id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

// --- Customers ---

/// Records/updates a customer on a paid order (called from the webhook path).
pub async fn upsert_customer_order(
    pool: &PgPool,
    phone: &str,
    name: &str,
    email: Option<&str>,
    spent_inr: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "insert into customers (phone, name, email, orders_count, total_spent_inr, last_order_at) \
         values ($1, $2, $3, 1, $4, now()) \
         on conflict (phone) do update set \
            name = coalesce(excluded.name, customers.name), \
            email = coalesce(excluded.email, customers.email), \
            orders_count = customers.orders_count + 1, \
            total_spent_inr = customers.total_spent_inr + excluded.total_spent_inr, \
            last_order_at = now()",
    )
    .bind(phone)
    .bind(name)
    .bind(email)
    .bind(spent_inr)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records a birthday captured by the WhatsApp bot (opt-in).
pub async fn set_customer_birthday(
    pool: &PgPool,
    phone: &str,
    name: Option<&str>,
    birthday: chrono::NaiveDate,
) -> sqlx::Result<()> {
    sqlx::query(
        "insert into customers (phone, name, birthday, marketing_opt_in) values ($1, $2, $3, true) \
         on conflict (phone) do update set \
            birthday = excluded.birthday, \
            name = coalesce(customers.name, excluded.name), \
            marketing_opt_in = true",
    )
    .bind(phone)
    .bind(name)
    .bind(birthday)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_customers(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<Customer>> {
    sqlx::query_as("select * from customers order by last_order_at desc nulls last, first_seen desc limit $1")
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Customers whose birthday is today and who haven't been greeted this year.
pub async fn birthdays_due_today(pool: &PgPool) -> sqlx::Result<Vec<Customer>> {
    sqlx::query_as(
        "select c.* from customers c \
         where c.birthday is not null and c.marketing_opt_in \
           and extract(month from c.birthday) = extract(month from current_date) \
           and extract(day from c.birthday) = extract(day from current_date) \
           and not exists ( \
             select 1 from birthday_log b \
             where b.customer_phone = c.phone \
               and extract(year from b.sent_on) = extract(year from current_date))",
    )
    .fetch_all(pool)
    .await
}

pub async fn log_birthday_sent(pool: &PgPool, phone: &str) -> sqlx::Result<()> {
    sqlx::query(
        "insert into birthday_log (customer_phone, sent_on) values ($1, current_date) \
         on conflict do nothing",
    )
    .bind(phone)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Promotions ---

pub async fn active_promotions(pool: &PgPool) -> sqlx::Result<Vec<Promotion>> {
    sqlx::query_as("select * from promotions where active order by sort_order, id")
        .fetch_all(pool)
        .await
}

pub async fn all_promotions(pool: &PgPool) -> sqlx::Result<Vec<Promotion>> {
    sqlx::query_as("select * from promotions order by sort_order, id")
        .fetch_all(pool)
        .await
}

pub async fn promotion_by_id(pool: &PgPool, id: i64) -> sqlx::Result<Option<Promotion>> {
    sqlx::query_as("select * from promotions where id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub struct PromotionInput {
    pub title: String,
    pub subtitle: String,
    pub cta_label: String,
    pub cta_href: String,
    pub image_url: String,
    pub active: bool,
    pub sort_order: i32,
}

pub async fn insert_promotion(pool: &PgPool, p: &PromotionInput) -> sqlx::Result<()> {
    sqlx::query(
        "insert into promotions (title, subtitle, cta_label, cta_href, image_url, active, sort_order) \
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&p.title).bind(&p.subtitle).bind(&p.cta_label).bind(&p.cta_href)
    .bind(&p.image_url).bind(p.active).bind(p.sort_order)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_promotion(pool: &PgPool, id: i64, p: &PromotionInput) -> sqlx::Result<()> {
    sqlx::query(
        "update promotions set title=$2, subtitle=$3, cta_label=$4, cta_href=$5, \
         image_url=$6, active=$7, sort_order=$8 where id=$1",
    )
    .bind(id)
    .bind(&p.title).bind(&p.subtitle).bind(&p.cta_label).bind(&p.cta_href)
    .bind(&p.image_url).bind(p.active).bind(p.sort_order)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Bestsellers ("Loved this week") ---

/// Top products by quantity sold in the last 14 days (paid orders).
/// Falls back to featured products when there's no recent sales history.
pub async fn bestsellers(pool: &PgPool, limit: i64) -> sqlx::Result<Vec<Product>> {
    let top: Vec<Product> = sqlx::query_as(
        "select p.* from products p \
         join order_items i on i.product_id = p.id \
         join orders o on o.id = i.order_id \
         where p.is_available and o.status <> 'pending' and o.status <> 'cancelled' \
           and o.created_at > now() - interval '14 days' \
         group by p.id order by sum(i.qty) desc limit $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    if top.is_empty() {
        return featured_products(pool).await;
    }
    Ok(top)
}

// --- Extended analytics ---

#[derive(Debug, sqlx::FromRow)]
pub struct DayCount {
    pub day: chrono::NaiveDate,
    pub count: i64,
}

pub async fn new_customers_by_day(pool: &PgPool) -> sqlx::Result<Vec<DayCount>> {
    sqlx::query_as(
        "select d::date as day, count(c.phone)::bigint as count \
         from generate_series(current_date - 29, current_date, interval '1 day') d \
         left join customers c on c.first_seen::date = d::date \
         group by d order by d",
    )
    .fetch_all(pool)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct LabeledValue {
    pub label: String,
    pub value: i64,
}

pub async fn revenue_by_category(pool: &PgPool) -> sqlx::Result<Vec<LabeledValue>> {
    sqlx::query_as(
        "select cat.name as label, coalesce(sum(i.qty * i.unit_price_inr),0)::bigint as value \
         from categories cat \
         left join products p on p.category_id = cat.id \
         left join order_items i on i.product_id = p.id \
         left join orders o on o.id = i.order_id and o.status <> 'pending' and o.status <> 'cancelled' \
           and o.created_at > now() - interval '30 days' \
         group by cat.name order by value desc",
    )
    .fetch_all(pool)
    .await
}

pub async fn orders_by_source(pool: &PgPool) -> sqlx::Result<Vec<LabeledValue>> {
    sqlx::query_as(
        "select source as label, count(*)::bigint as value from orders \
         where created_at > now() - interval '30 days' group by source order by value desc",
    )
    .fetch_all(pool)
    .await
}

pub async fn popular_slots(pool: &PgPool) -> sqlx::Result<Vec<LabeledValue>> {
    sqlx::query_as(
        "select delivery_slot as label, count(*)::bigint as value from orders \
         where status <> 'pending' and status <> 'cancelled' \
           and created_at > now() - interval '30 days' \
         group by delivery_slot order by value desc",
    )
    .fetch_all(pool)
    .await
}

/// (total customers, repeat customers with >1 order) over all time.
pub async fn customer_stats(pool: &PgPool) -> sqlx::Result<(i64, i64)> {
    let row: (i64, i64) = sqlx::query_as(
        "select count(*)::bigint, count(*) filter (where orders_count > 1)::bigint from customers",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}
