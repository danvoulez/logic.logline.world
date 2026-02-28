-- Migration 005: CLI passkey credentials table + RLS
-- Users can register and manage their own passkeys

begin;

-- Drop and recreate to ensure clean schema
drop table if exists cli_passkey_credentials cascade;

create table cli_passkey_credentials (
  credential_id  text primary key default gen_random_uuid()::text,
  user_id        text not null references users(user_id) on delete cascade,
  device_name    text not null,
  public_key     text not null,
  algorithm      text not null default 'ed25519',
  status         text not null default 'active'
                 check (status in ('active', 'revoked')),
  created_at     timestamptz not null default now(),
  revoked_at     timestamptz,
  unique (user_id, device_name)
);

create index idx_passkey_creds_user on cli_passkey_credentials (user_id, status);

alter table cli_passkey_credentials enable row level security;

-- Uses auth.uid()::text — the standard Supabase way to get the JWT subject
create policy passkey_select_own on cli_passkey_credentials
  for select using (user_id = auth.uid()::text);

create policy passkey_insert_own on cli_passkey_credentials
  for insert with check (user_id = auth.uid()::text);

create policy passkey_update_own on cli_passkey_credentials
  for update using (user_id = auth.uid()::text);

commit;
