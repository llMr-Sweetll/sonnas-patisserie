# Architecture

One Rust binary, one Axum router, three front doors: the web storefront, the
WhatsApp webhook, and the admin panel. Everything talks to one Postgres database.

```
                        ┌────────────────────────────────────────┐
  Browser ──────────────►  Axum router (src/lib.rs)              │
                        │   ├─ routes/store     catalog, cart,   │
  WhatsApp ─► Meta ─────►   │                   order tracking   │
  (customer)  Cloud API │   ├─ routes/checkout  order + Razorpay │
                        │   ├─ routes/webhook   razorpay, whatsapp│
  Razorpay ─────────────►   ├─ routes/admin     dashboard, CRUD  │
  (webhooks)            │   └─ bot/             state machine +  │
                        │                       Claude tools     │
                        └───────────────┬────────────────────────┘
                                        ▼
                          Postgres (Supabase) via SQLx
```

The same router runs two ways:

- `src/main.rs` — local dev server (`cargo run`), serves `public/` itself.
- `api/main.rs` — Vercel Rust function; `vercel.json` rewrites every path to it
  (static `public/` files are served by Vercel's CDN first).

## Data model (migrations/0001_schema.sql)

- `categories` → `products` (price in whole ₹, single image URL, eggless/featured/available flags)
- `orders` → `order_items` (snapshot of name + unit price at purchase; `customization`
  holds the message-on-cake). `orders.status`:
  `pending → paid → confirmed → out_for_delivery → delivered`, or `cancelled`.
  `source` is `web` or `whatsapp`.
- `wa_sessions` — one row per customer phone: bot `state`, `cart` (JSON), `context`
  (name/address/date collected during checkout).

Migrations are embedded via `include_str!` and run idempotently at startup by a
small runner in `src/lib.rs` (a `_migrations` table records applied files; the
insert serialises concurrent cold starts). Deliberately not `sqlx::migrate!` —
the macros subtree pulls in sqlx-mysql and with it the unfixed RUSTSEC-2023-0071
`rsa` advisory.

## Key flows

**Web checkout.** Cart lives in a cookie holding only product ids/qty/flags —
never prices. `POST /checkout` re-reads prices from the DB, creates the order
(status `pending`) plus a Razorpay order, and renders the payment page.
Two confirmation paths, both idempotent via
`UPDATE … WHERE status='pending' RETURNING *`:

1. Razorpay webhook `payment.captured` (source of truth in production).
2. Browser callback `POST /checkout/verify` (signature-checked; makes local dev
   and instant redirects work).

The first path to land flips the order to `paid` and fires WhatsApp notifications;
the other becomes a no-op.

**WhatsApp bot.** `POST /webhooks/whatsapp` (HMAC-verified) dispatches each
message. Tapped buttons/list rows carry action ids (`cat:3`, `add:6`,
`date:2026-07-10`) so browsing is stateless; text-entry steps
(`customize → ask_name → ask_address → ask_date → ask_slot`) advance
`wa_sessions.state`. Free text in the idle state goes to Claude with five tools
(`search_products`, `get_cart`, `add_to_cart`, `checkout`, `get_order_status`);
tools are the only side effects, so the model can never invent prices or items.
Checkout creates a normal order (`source='whatsapp'`) and sends a Razorpay
payment link; the `payment_link.paid` webhook completes it like any other order.

**Admin.** Single admin, argon2id password hash from env, HMAC-signed expiring
session cookie, all `/admin/*` routes behind a middleware guard. Status changes
send the customer a WhatsApp update. The dashboard is plain SQL aggregates
rendered as server-side SVG — no JS charting.

## Security model

- **Webhooks**: constant-time HMAC-SHA256 verification (Razorpay signature,
  Meta `X-Hub-Signature-256`) before any parsing; paid amount cross-checked
  against the order total; `mark_paid` idempotent against replays.
- **Sessions/CSRF**: HMAC-signed admin cookie with expiry; double-submit CSRF
  tokens on admin and checkout POSTs; cookies `HttpOnly`, `SameSite=Lax`,
  `Secure` outside localhost. Login rate-limited (5 tries / 15 min per IP).
- **Injection/XSS**: SQLx parameterized queries throughout; Askama escapes all
  template output; CSP allows scripts only from self + Razorpay checkout.
- **Untrusted input**: cart cookies and bot carts carry ids only — prices always
  come from the DB at order time; quantities clamped 1–20; delivery dates
  validated (≤30 days out, never Tuesday); order lookup uses unguessable order
  numbers (~41 bits), not sequential ids.
- **Secrets**: env-only, `.env` gitignored, server refuses to start with a weak
  `SESSION_SECRET`.

Known ceilings (deliberate, marked with `ponytail:` comments in code):
login rate-limit table is in-memory per instance; a fresh instance forgets
failure counts. Move it to Postgres if login abuse ever matters.
