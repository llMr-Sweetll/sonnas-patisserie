-- Growth features: admin-managed images, customer memory, promotions, birthday reminders.

-- Admin photo uploads live in Postgres (no external Storage dependency).
create table product_images (
    id bigint generated always as identity primary key,
    bytes bytea not null,
    content_type text not null default 'image/jpeg',
    created_at timestamptz not null default now()
);

-- Customer memory: one row per phone, for repeat-recognition + birthday greetings.
create table customers (
    phone text primary key,
    name text,
    email text,
    birthday date,
    marketing_opt_in boolean not null default false,
    orders_count int not null default 0,
    total_spent_inr bigint not null default 0,
    first_seen timestamptz not null default now(),
    last_order_at timestamptz,
    notes text
);

create index customers_birthday_idx on customers (birthday);

-- Homepage promotions / feature banners, managed in admin.
create table promotions (
    id bigint generated always as identity primary key,
    title text not null,
    subtitle text not null default '',
    cta_label text not null default 'Order now',
    cta_href text not null default '/',
    image_url text not null default '',
    active boolean not null default true,
    sort_order int not null default 0,
    created_at timestamptz not null default now()
);

-- Idempotency ledger for the daily birthday cron.
create table birthday_log (
    customer_phone text not null,
    sent_on date not null,
    primary key (customer_phone, sent_on)
);

alter table product_images enable row level security;
alter table customers enable row level security;
alter table promotions enable row level security;
alter table birthday_log enable row level security;

-- Backfill customers from existing paid orders so analytics/repeat-rate start populated.
insert into customers (phone, name, email, orders_count, total_spent_inr, first_seen, last_order_at)
select
    o.phone,
    (array_agg(o.customer_name order by o.created_at desc))[1],
    (array_agg(o.email order by o.created_at desc))[1],
    count(*),
    coalesce(sum(o.total_inr), 0),
    min(o.created_at),
    max(o.created_at)
from orders o
where o.status <> 'pending' and o.status <> 'cancelled'
group by o.phone
on conflict (phone) do nothing;

-- Seed one promotion so the homepage promo slot is populated out of the box.
insert into promotions (title, subtitle, cta_label, cta_href, sort_order)
select 'Celebration cakes, made to order',
       'Name on the cake, eggless option, delivered across Hubli.',
       'Start your order', '/category/signature-cakes', 1
where not exists (select 1 from promotions);
