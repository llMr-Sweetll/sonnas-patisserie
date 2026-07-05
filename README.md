# Sonna's Patisserie

Online store, WhatsApp ordering bot, and admin backend for **Sonna's Patisserie**
(Hubli, Karnataka) — a 100% vegetarian artisanal cake & dessert bakery.
Written entirely in **Rust**.

## What it does

- **Storefront** — server-rendered catalog (signature cakes, cheesecakes, mousses,
  brownies & travel cakes), cookie cart with per-item eggless toggle and
  "message on the cake" customization, checkout with delivery date/slot selection
  (closed Tuesdays, enforced), Razorpay payment.
- **WhatsApp ordering bot** — customers chat with the shop's WhatsApp number and
  order end-to-end with zero human intervention: tappable menus (Meta interactive
  messages) for browsing, Claude (Haiku) for free-text like
  *"1kg eggless chocolate cake for Sunday, write Happy Birthday Ananya on it"*,
  and a Razorpay payment link to pay in-chat.
- **WhatsApp automation** — owner gets an alert for every paid order; customers get
  confirmations and status updates (confirmed → out for delivery → delivered).
- **Admin backend** — password-protected panel with an analytics dashboard
  (revenue by day, top products, status breakdown, AOV), order management with
  one-click status changes that notify the customer, and product CRUD with image
  upload to Supabase Storage.

## Stack

| Layer | Choice |
|---|---|
| Web framework | Axum 0.7 (one router, served locally and on Vercel) |
| Templates | Askama (compile-time, auto-escaping) |
| Database | Postgres (Supabase) via SQLx, embedded migrations |
| Payments | Razorpay REST (orders, payment links, webhooks) |
| Messaging | Meta WhatsApp Cloud API |
| AI | Claude `claude-haiku-4-5` (bot free-text understanding, tool calling) |
| Hosting | Vercel Rust runtime (`api/main.rs`) or any host that runs a binary |

## Quick start (local)

```sh
# 1. Postgres (any instance works; Supabase in production)
createdb sonnas

# 2. Configure
cp .env.example .env
#    - DATABASE_URL=postgres://you@localhost:5432/sonnas
#    - SESSION_SECRET=$(openssl rand -hex 32)
#    - ADMIN_PASSWORD_HASH: see below (single-quote it in .env — it contains $)

# 3. Admin password
cargo run --bin hash-password -- 'choose-a-password'

# 4. Run — migrations and seed data apply automatically
cargo run
```

Visit `http://localhost:3000` (store) and `http://localhost:3000/admin` (admin).
Payments, WhatsApp, and the AI bot each activate when their env keys are set —
everything else works without them.

## Repository map

```
src/
  main.rs        local dev server        api/main.rs   Vercel entrypoint
  lib.rs         config, router, errors  migrations/   schema + seed SQL
  routes/        store, checkout, webhooks, admin
  bot/           WhatsApp state machine + Claude tool-calling layer
  cart.rs        cookie cart             auth.rs       argon2 + signed sessions + CSRF
  razorpay.rs    payments REST           whatsapp.rs   Cloud API client
templates/       Askama HTML (store + admin)
public/          stylesheet + payment page JS
docs/            ARCHITECTURE.md · DEPLOYMENT.md
```

## Observability & debugging

- **Local**: structured logs via `tracing` — `RUST_LOG=debug cargo run` (defaults to
  `info` + `debug` for app modules). SQL, webhook, WhatsApp and Claude failures all
  log with context; webhook handlers log signature/amount rejections.
- **Production**: the same log stream lands in **Vercel function logs**
  (`vercel logs`); DB-side, use Supabase's query performance and auth logs.
- **`GET /health`** — 200 only when the database answers; wire it to an uptime
  monitor.
- **Simulating the outside world**: signed webhook payloads for Razorpay and
  WhatsApp can be crafted with `openssl dgst -sha256 -hmac <secret>` — examples in
  docs/DEPLOYMENT.md; `cargo test` covers the signature verifiers themselves.

## Privacy & compliance (DPDP Act 2023)

Customer data is minimal by design (name/phone/address per order, no accounts, no
trackers, payments never touch the server). `/privacy` and `/terms` are served by
the app; checkout requires explicit consent; erasure and breach procedures are in
[docs/COMPLIANCE.md](docs/COMPLIANCE.md).

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how the pieces fit, key flows, security model
- [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) — Vercel + Supabase + Razorpay + Meta setup, step by step
- [docs/COMPLIANCE.md](docs/COMPLIANCE.md) — DPDP Act 2023 mapping, data rights, breach playbook

## License

[AGPL-3.0](LICENSE) — free to use, study, and modify; anyone who deploys a
modified version (including as a hosted service) must publish their changes.
