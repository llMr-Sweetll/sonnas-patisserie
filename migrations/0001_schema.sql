create table categories (
    id bigint generated always as identity primary key,
    name text not null,
    slug text not null unique,
    kind text not null default 'collection' check (kind in ('occasion', 'flavour', 'collection')),
    sort_order int not null default 0
);

create table products (
    id bigint generated always as identity primary key,
    name text not null,
    slug text not null unique,
    description text not null default '',
    price_inr bigint not null check (price_inr > 0),
    image_url text not null default '',
    category_id bigint not null references categories (id),
    is_eggless_available boolean not null default true,
    is_available boolean not null default true,
    is_featured boolean not null default false,
    created_at timestamptz not null default now()
);

create index products_category_idx on products (category_id);

create table orders (
    id bigint generated always as identity primary key,
    order_number text not null unique,
    customer_name text not null,
    phone text not null,
    email text,
    address text not null,
    delivery_date date not null,
    delivery_slot text not null,
    notes text,
    subtotal_inr bigint not null check (subtotal_inr >= 0),
    total_inr bigint not null check (total_inr >= 0),
    status text not null default 'pending' check (
        status in ('pending', 'paid', 'confirmed', 'out_for_delivery', 'delivered', 'cancelled')
    ),
    source text not null default 'web' check (source in ('web', 'whatsapp')),
    razorpay_order_id text,
    razorpay_payment_id text,
    razorpay_payment_link_id text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index orders_status_idx on orders (status);
create index orders_created_idx on orders (created_at);

create table order_items (
    id bigint generated always as identity primary key,
    order_id bigint not null references orders (id) on delete cascade,
    product_id bigint references products (id),
    product_name text not null,
    unit_price_inr bigint not null check (unit_price_inr >= 0),
    qty int not null check (qty > 0 and qty <= 20),
    eggless boolean not null default false,
    customization text
);

create index order_items_order_idx on order_items (order_id);

-- WhatsApp ordering bot: one conversation per phone number
create table wa_sessions (
    phone text primary key,
    state text not null default 'start',
    cart jsonb not null default '[]',
    context jsonb not null default '{}',
    updated_at timestamptz not null default now()
);
