-- Migration 003: Supabase auth hooks
-- - before_user_created: blocks signups not on tenant allowlist
-- - custom_access_token: injects tenant_id, app_id claims if available
-- Depends on: 001_base_tables

begin;

-- ─── before_user_created hook ─────────────────────────────────────────────────
-- Called by Supabase before creating a new user.
-- Input: { "user": { "email": "...", "user_metadata": { "tenant_slug": "..." } } }
-- Must return: { "decision": "continue" } or raise an exception to block.

-- Functions live in `app` schema (auth schema requires superuser on hosted Supabase).
-- Hooks configured via Management API: PATCH /v1/projects/{ref}/config/auth
-- Using explicit grants instead of SECURITY DEFINER (per Supabase best practices).

create or replace function app.before_user_created(event jsonb)
returns jsonb
language plpgsql
stable
set search_path = public
as $$
declare
  v_email          text;
  v_email_norm     text;
  v_tenant_slug    text;
  v_tenant_id      text;
  v_allowlist_row  record;
begin
  v_email       := event->'user'->>'email';
  v_email_norm  := lower(trim(v_email));
  v_tenant_slug := event->'user'->'user_metadata'->>'tenant_slug';

  -- If no tenant_slug provided, block signup (mandatory tenant membership).
  if v_tenant_slug is null or v_tenant_slug = '' then
    raise exception 'tenant_slug is required in user_metadata' using errcode = 'P0001';
  end if;

  -- Resolve tenant.
  select tenant_id into v_tenant_id
  from tenants
  where slug = v_tenant_slug
  limit 1;

  if v_tenant_id is null then
    raise exception 'unknown tenant slug: %', v_tenant_slug using errcode = 'P0001';
  end if;

  -- Check email allowlist (if allowlist has entries for this tenant).
  -- If tenant has no allowlist entries at all, signup is open for that tenant
  -- (tenant admin must explicitly restrict by adding allowlist rows).
  if exists (
    select 1 from tenant_email_allowlist where tenant_id = v_tenant_id
  ) then
    select * into v_allowlist_row
    from tenant_email_allowlist
    where tenant_id = v_tenant_id
      and email_normalized = v_email_norm
      and (expires_at is null or expires_at > now())
    limit 1;

    if not found then
      raise exception 'email % is not on the allowlist for tenant %', v_email, v_tenant_slug
        using errcode = 'P0001';
    end if;
  end if;

  return jsonb_build_object('decision', 'continue');
end;
$$;

-- ─── custom_access_token hook ─────────────────────────────────────────────────
-- Injects workspace_id / app_id into the JWT if the user has a primary membership.
-- Input: { "user_id": "...", "claims": {...} }

create or replace function app.custom_access_token(event jsonb)
returns jsonb
language plpgsql
stable
set search_path = public
as $$
declare
  v_user_id    text;
  v_tenant_id  text;
  v_app_id     text;
  v_claims     jsonb;
begin
  v_user_id := event->>'user_id';
  v_claims  := event->'claims';

  -- Resolve first tenant membership (alphabetical for determinism).
  select tenant_id into v_tenant_id
  from tenant_memberships
  where user_id = v_user_id
  order by tenant_id asc
  limit 1;

  if v_tenant_id is not null then
    v_claims := v_claims || jsonb_build_object('workspace_id', v_tenant_id);

    -- Resolve first app membership for this tenant.
    select app_id into v_app_id
    from app_memberships
    where user_id = v_user_id
      and tenant_id = v_tenant_id
    order by app_id asc
    limit 1;

    if v_app_id is not null then
      v_claims := v_claims || jsonb_build_object('app_id', v_app_id);
    end if;
  end if;

  return jsonb_build_object('claims', v_claims);
end;
$$;

-- ─── post-signup membership provisioning function ─────────────────────────────
-- Called by /v1/auth/onboard/claim after Supabase creates the user.
-- Creates user row + tenant/app memberships from the allowlist defaults.

create or replace function app.provision_user_membership(
  p_user_id     text,
  p_email       text,
  p_display_name text,
  p_tenant_slug text
)
returns jsonb
language plpgsql
security definer
as $$
declare
  v_tenant_id      text;
  v_allowlist_row  record;
  v_tenant_role    text;
  v_app_defaults   jsonb;
  v_app_entry      jsonb;
  v_app_id         text;
  v_app_role       text;
begin
  -- Resolve tenant.
  select tenant_id into v_tenant_id
  from tenants
  where slug = p_tenant_slug
  limit 1;

  if v_tenant_id is null then
    return jsonb_build_object('ok', false, 'error', 'unknown tenant slug');
  end if;

  -- Upsert user record.
  insert into users (user_id, email, display_name, created_at)
  values (p_user_id, p_email, p_display_name, now())
  on conflict (user_id) do update
    set email = excluded.email,
        display_name = excluded.display_name;

  -- Determine role from allowlist or default to 'member'.
  v_tenant_role := 'member';
  v_app_defaults := '[]';

  select * into v_allowlist_row
  from tenant_email_allowlist
  where tenant_id = v_tenant_id
    and email_normalized = lower(trim(p_email))
    and (expires_at is null or expires_at > now())
  limit 1;

  if found then
    v_tenant_role  := coalesce(v_allowlist_row.role_default, 'member');
    v_app_defaults := coalesce(v_allowlist_row.app_defaults, '[]');
  end if;

  -- Upsert tenant membership.
  insert into tenant_memberships (tenant_id, user_id, role, created_at)
  values (v_tenant_id, p_user_id, v_tenant_role, now())
  on conflict (tenant_id, user_id) do nothing;

  -- Upsert app memberships from defaults.
  for v_app_entry in select jsonb_array_elements(v_app_defaults)
  loop
    v_app_id   := v_app_entry->>'app_id';
    v_app_role := coalesce(v_app_entry->>'role', 'member');

    if v_app_id is not null and exists (
      select 1 from apps where app_id = v_app_id and tenant_id = v_tenant_id
    ) then
      insert into app_memberships (app_id, tenant_id, user_id, role, created_at)
      values (v_app_id, v_tenant_id, p_user_id, v_app_role, now())
      on conflict (app_id, tenant_id, user_id) do nothing;
    end if;
  end loop;

  return jsonb_build_object(
    'ok', true,
    'tenant_id', v_tenant_id,
    'role', v_tenant_role
  );
end;
$$;

-- Auth system needs execute + table access (replaces SECURITY DEFINER)
grant usage on schema app to supabase_auth_admin;
grant execute on function app.before_user_created(jsonb) to supabase_auth_admin;
grant execute on function app.custom_access_token(jsonb) to supabase_auth_admin;
grant select on tenants to supabase_auth_admin;
grant select on tenant_email_allowlist to supabase_auth_admin;
grant select on tenant_memberships to supabase_auth_admin;
grant select on app_memberships to supabase_auth_admin;

-- Lock down: prevent direct RPC calls from API users
revoke execute on function app.before_user_created(jsonb) from authenticated, anon, public;
revoke execute on function app.custom_access_token(jsonb) from authenticated, anon, public;

commit;
