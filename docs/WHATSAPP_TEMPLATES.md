# WhatsApp Message Templates (for Meta approval)

Free-form WhatsApp messages only reach a customer **within 24 hours of their last
message**. Anything outside that window — a status update to a website customer, or
a birthday greeting — must use a **pre-approved message template**. Create these in
**Meta Business Manager → WhatsApp Manager → Message Templates**, then the app sends
them by name. Until they're approved, these sends only work inside the 24h window
(fine for bot conversations, not for cold sends).

Category is **Utility** for order messages, **Marketing** for birthdays.
`{{1}}`, `{{2}}` … are Meta's variable placeholders.

---

## 1. order_confirmation (Utility)
> Thank you {{1}}! Your Sonna's Patisserie order {{2}} is confirmed.
> Total paid: ₹{{3}}. Delivery: {{4}}.
> Track it here: {{5}}

Variables: name, order number, total, delivery date + slot, tracking URL.

## 2. order_status_update (Utility)
> Update on your Sonna's Patisserie order {{1}}: it is now {{2}}.
> Questions? Just reply here.

Variables: order number, status phrase ("out for delivery", "delivered", …).

## 3. owner_new_order (Utility, sent to the owner)
> New order {{1}} — ₹{{2}}. Deliver {{3}}. {{4}} ({{5}}). {{6}}

Variables: order number, total, delivery date/slot, customer name, phone, address.

## 4. birthday_greeting (Marketing)
> Happy birthday {{1}}! 🎂 From all of us at Sonna's Patisserie — here's to a sweet
> year. Treat yourself today and mention BIRTHDAY for a little something on us: {{2}}

Variables: name, site URL. Requires the customer's prior marketing opt-in (captured
when they share their birthday with the bot).

---

## Wiring templates in code
`src/whatsapp.rs` currently sends these as plain text (works inside the 24h window,
and for local/testing). To send approved templates, swap the `send_text(...)` calls
in `notify_order_paid`, `notify_status_change`, and `send_birthday_greeting` for a
`send_template(name, [vars])` helper that POSTs a `type: "template"` message to the
Graph API. The helper shape:

```jsonc
{
  "messaging_product": "whatsapp",
  "to": "<phone>",
  "type": "template",
  "template": {
    "name": "order_status_update",
    "language": { "code": "en" },
    "components": [{ "type": "body", "parameters": [
      { "type": "text", "text": "SP-ABC123" },
      { "type": "text", "text": "out for delivery" }
    ]}]
  }
}
```

## Activation checklist (owner)
1. Verify the business in Meta Business Manager (legal name + address must match the
   website and registration docs).
2. Add a WhatsApp number, get the **display name** approved.
3. Create the four templates above; wait for approval (usually 3–7 days).
4. Put `WHATSAPP_TOKEN`, `WHATSAPP_PHONE_NUMBER_ID`, `WHATSAPP_APP_SECRET`,
   `WHATSAPP_VERIFY_TOKEN` into Vercel env; point the webhook at
   `https://<domain>/webhooks/whatsapp`.
