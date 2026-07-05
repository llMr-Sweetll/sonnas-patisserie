# Deployment — Vercel + Supabase

## 1. Supabase

1. Create a project (region `ap-south-1` is closest to Hubli).
2. Create a dedicated login role for the app (SQL editor; prefer setting the
   password as a SCRAM verifier so plaintext never leaves your machine):
   `create role sonnas_app with login password '…' connection limit 20;`
   `grant usage, create on schema public to sonnas_app;`
3. `DATABASE_URL` uses the **session pooler** (port **5432**) —
   `postgres://sonnas_app.<ref>:<password>@aws-1-<region>.pooler.supabase.com:5432/postgres?sslmode=require`.
   Transaction mode (6543) breaks sqlx's prepared statements; the direct
   `db.<ref>.supabase.co` host is IPv6-only on the free tier.
4. Storage → create a **public bucket named `products`** (admin image uploads go here).
5. Project settings → API → copy `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY`.

Migrations, seed data, and RLS (enabled with zero policies, so the auto-generated
PostgREST API exposes nothing) run automatically the first time the app connects.

## 2. Razorpay

1. Create an account, use **test mode** keys first → `RAZORPAY_KEY_ID`,
   `RAZORPAY_KEY_SECRET`.
2. After the first deploy: Dashboard → Webhooks → add
   `https://<your-domain>/webhooks/razorpay`, subscribe to **payment.captured**
   and **payment_link.paid**, set a secret → `RAZORPAY_WEBHOOK_SECRET`.

## 3. Meta WhatsApp Cloud API

1. Meta for Developers → create an app → add the **WhatsApp** product.
   The free tier includes a test number; later, register the shop's real number.
2. Copy the permanent access token → `WHATSAPP_TOKEN`, the phone number id →
   `WHATSAPP_PHONE_NUMBER_ID`, and the app secret → `WHATSAPP_APP_SECRET`.
3. Webhooks → subscribe to `messages` with callback
   `https://<your-domain>/webhooks/whatsapp` and a verify token you invent →
   `WHATSAPP_VERIFY_TOKEN`.
4. `OWNER_WHATSAPP_NUMBER` — the owner's number (E.164 without `+`, e.g. `9191132…`)
   for order alerts and the storefront's "Order on WhatsApp" button.

**24-hour window caveat.** Free-form messages only reach a customer within 24h of
their last message. Bot customers always qualify. For *web* orders from customers
who never messaged the number, production-grade confirmations need **approved
message templates** (Business Manager → create `order_confirmation` /
`order_status_update` utility templates, then extend `src/whatsapp.rs` to send by
template name). Owner alerts work as long as the owner has chatted with the bot
number once. This is a Meta policy limit, not a code limit.

## 4. Claude (bot free text)

`ANTHROPIC_API_KEY` from console.anthropic.com. Without it the bot still works —
it falls back to the tappable menus and skips natural-language understanding.

## 5. App secrets

```sh
openssl rand -hex 32                                   # → SESSION_SECRET
cargo run --bin hash-password -- 'strong-admin-pass'   # → ADMIN_PASSWORD_HASH
```

## 6. Vercel

```sh
npm i -g vercel@latest
vercel link
# add every env var from .env.example (single-quote ADMIN_PASSWORD_HASH):
vercel env add DATABASE_URL production
# … repeat for the rest; also set BASE_URL=https://<your-domain>
vercel deploy --prod
```

The app uses Vercel's **official Rust runtime** (public beta, Fluid compute):
`vercel_runtime = "2.4"` with the `axum` feature — `api/main.rs` is the only
function, `vercel.json` rewrites every path to it, and static assets are
compiled into the binary, so no static-dir configuration is needed. Do not use
the deprecated `vercel-rust` community runtime.

**Currently live:** https://sonnas-patisserie-seven.vercel.app (project
`sonnas-patisserie`, Supabase project `qxvwqpyflwrisgncugka` in `ap-south-1`).

## 7. Post-deploy checklist

- [ ] `https://<domain>/` renders the storefront with seeded products
- [ ] Razorpay webhook registered and its **test** payment flips an order to `paid`
- [ ] Meta webhook verified (green check in the app dashboard)
- [ ] Send "hi" to the WhatsApp number → welcome + menu buttons arrive
- [ ] Full bot order with a test payment link end-to-end
- [ ] `/admin` login works; change an order status → customer gets the update
- [ ] Switch Razorpay to live keys when ready

## Local development

See the README quick start. Webhooks need a public URL — use
`vercel dev` or a tunnel (`cloudflared tunnel --url http://localhost:3000`)
and point Razorpay/Meta test webhooks at it.
