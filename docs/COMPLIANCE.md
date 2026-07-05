# Compliance — DPDP Act 2023 (India)

How this codebase maps to the Digital Personal Data Protection Act, 2023 and the
DPDP Rules, 2025 (phased enforcement through May 2027). This is an engineering
map, not legal advice — have counsel review before scale.

## What personal data exists here

| Data | Where | Purpose | Basis |
|---|---|---|---|
| Name, phone, address, optional email | `orders` | Prepare + deliver the order, send order updates | Consent at checkout / before payment |
| Message on cake | `order_items.customization` | Written on the product | Same consent |
| WhatsApp number + conversation state | `wa_sessions` | Operate the ordering conversation | Customer-initiated chat |

Deliberately absent: no customer accounts or passwords, no payment credentials
(Razorpay-hosted), no analytics/trackers, no marketing lists, no profiling,
no children-directed features, no cross-purpose reuse.

## Obligation → implementation

- **Notice + consent (§5–6):** `/privacy` states what/why/who/how-long in plain
  language. Web checkout requires an unticked-by-default consent checkbox
  (enforced server-side, `routes/checkout.rs::validate`); the WhatsApp bot links
  the notice in the payment message. Consent is purpose-specific — delivery and
  order updates only; there is no marketing to bundle.
- **Reasonable security safeguards (§8(5)):** TLS everywhere (Vercel/Supabase),
  encrypted at rest (Supabase), HMAC-verified webhooks, argon2id admin auth,
  CSRF, rate limiting, parameterized SQL, CSP. Full model in
  [ARCHITECTURE.md](ARCHITECTURE.md#security-model).
- **Data principal rights (§11–14), 7-day response:** access/correction/erasure
  requests arrive via the grievance channel (WhatsApp/shop, published on
  `/privacy`). Erasure once retention law allows:
  ```sql
  update orders set customer_name='erased', phone='erased', email=null,
         address='erased', notes=null where order_number = $1;
  delete from wa_sessions where phone = $2;
  ```
- **Retention/minimisation:** orders kept for the statutory tax period, then
  erasable as above; `wa_sessions` is transient conversation state; the cart
  cookie holds product ids only.
- **Breach notification (§8(6), up to ₹200 crore for failure):** on suspicion —
  rotate all secrets (`SESSION_SECRET`, DB password, API keys), inspect Supabase
  auth/query logs and Vercel function logs, notify the Data Protection Board and
  affected users. Keep a written incident log.
- **Children (§9):** no child-directed processing, no behavioural ads, nothing
  to gate.
- **Significant Data Fiduciary duties (§10):** not applicable at this scale
  (no DPO/audit mandate unless the government notifies the business as an SDF).

## Grievance officer

Publish a named person + contact on `/privacy` before go-live (currently the
shop's WhatsApp). The 2025 Rules expect a working, answered channel.
