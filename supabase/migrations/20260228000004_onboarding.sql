-- Migration 004: App onboarding + fuel ledger
-- Depends on: 001_base_tables, 002_rbac_rls

begin;

-- ─── 1. App service config (bidirectional handshake: App → HQ credentials) ──

create table if not exists app_service_config (
  app_id            text not null references apps(app_id) on delete cascade,
  tenant_id         text not null references tenants(tenant_id) on delete cascade,
  service_url       text,
  api_key_encrypted text,
  capabilities      jsonb not null default '[]',
  status            text not null default 'pending'
                    check (status in ('pending', 'active', 'suspended', 'revoked')),
  onboarded_at      timestamptz,
  onboarded_by      text references users(user_id),
  created_at        timestamptz not null default now(),
  updated_at        timestamptz not null default now(),
  primary key (app_id, tenant_id)
);

alter table app_service_config enable row level security;

drop policy if exists app_service_config_select_admin on app_service_config;
create policy app_service_config_select_admin on app_service_config
  for select using (
    app.is_app_admin(tenant_id, app_id)
    or app.is_tenant_admin(tenant_id)
  );

drop policy if exists app_service_config_write_app_admin on app_service_config;
create policy app_service_config_write_app_admin on app_service_config
  for all using (app.is_app_admin(tenant_id, app_id))
  with check (app.is_app_admin(tenant_id, app_id));

-- ─── 2. Fuel events ledger (normalized usage, append-only) ──────────────────

create table if not exists fuel_events (
  event_id        text primary key default gen_random_uuid()::text,
  idempotency_key text not null unique,
  tenant_id       text not null references tenants(tenant_id),
  app_id          text not null references apps(app_id),
  user_id         text not null references users(user_id),
  units           numeric not null,
  unit_type       text not null,
  occurred_at     timestamptz not null default now(),
  source          text not null,
  metadata        jsonb default '{}',
  created_at      timestamptz not null default now()
);

create index if not exists idx_fuel_events_tenant_app on fuel_events (tenant_id, app_id, occurred_at desc);
create index if not exists idx_fuel_events_user on fuel_events (user_id, occurred_at desc);
create index if not exists idx_fuel_events_idempotency on fuel_events (idempotency_key);

-- Append-only: block UPDATE and DELETE
create or replace function app.prevent_fuel_modification()
returns trigger language plpgsql as $$
begin
  raise exception 'fuel events are immutable (append-only ledger)';
end;
$$;

drop trigger if exists no_modify_fuel on fuel_events;
create trigger no_modify_fuel
  before update or delete on fuel_events
  for each row execute function app.prevent_fuel_modification();

alter table fuel_events enable row level security;

-- Apps can insert fuel events for their own app_id
drop policy if exists fuel_events_insert_app on fuel_events;
create policy fuel_events_insert_app on fuel_events
  for insert with check (app.is_app_member(tenant_id, app_id));

-- App admins can read their own app's fuel events
drop policy if exists fuel_events_select_app_admin on fuel_events;
create policy fuel_events_select_app_admin on fuel_events
  for select using (
    app.is_app_admin(tenant_id, app_id)
    or app.is_tenant_admin(tenant_id)
  );

-- Primary append-only defense: revoke mutating permissions at the grant level
revoke update, delete on fuel_events from authenticated, anon, public;

commit;
