-- Defense in depth on Supabase: the app talks straight to Postgres as the table
-- owner (which bypasses non-forced RLS), while PostgREST roles (anon/authenticated)
-- hit RLS with zero policies — i.e. no access via the auto-generated REST API.
alter table categories enable row level security;
alter table products enable row level security;
alter table orders enable row level security;
alter table order_items enable row level security;
alter table wa_sessions enable row level security;
