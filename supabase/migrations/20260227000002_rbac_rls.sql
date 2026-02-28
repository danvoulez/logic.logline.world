-- Migration 002: RBAC helper functions and RLS policies
-- Depends on: 001_base_tables

begin;

-- ─── Role-check helpers ───────────────────────────────────────────────────────

create or replace function app.is_tenant_member(target_tenant text)
returns boolean language sql stable
security definer set search_path = public
as $$
  select exists (
    select 1 from tenant_memberships tm
    where tm.tenant_id = target_tenant
      and tm.user_id = (select app.current_user_id())
  );
$$;

create or replace function app.is_tenant_admin(target_tenant text)
returns boolean language sql stable
security definer set search_path = public
as $$
  select exists (
    select 1 from tenant_memberships tm
    where tm.tenant_id = target_tenant
      and tm.user_id = (select app.current_user_id())
      and tm.role = 'admin'
  );
$$;

create or replace function app.is_app_member(target_tenant text, target_app text)
returns boolean language sql stable
security definer set search_path = public
as $$
  select exists (
    select 1 from app_memberships am
    where am.tenant_id = target_tenant
      and am.app_id = target_app
      and am.user_id = (select app.current_user_id())
  );
$$;

create or replace function app.is_app_admin(target_tenant text, target_app text)
returns boolean language sql stable
security definer set search_path = public
as $$
  select exists (
    select 1 from app_memberships am
    where am.tenant_id = target_tenant
      and am.app_id = target_app
      and am.user_id = (select app.current_user_id())
      and am.role = 'app_admin'
  );
$$;

create or replace function app.has_capability(cap text)
returns boolean language sql stable
security definer set search_path = public
as $$
  select exists (
    select 1 from user_capabilities uc
    where uc.user_id = (select app.current_user_id())
      and uc.capability = cap
  );
$$;

-- ─── Enable RLS on all tables ─────────────────────────────────────────────────

alter table users enable row level security;
alter table tenants enable row level security;
alter table apps enable row level security;
alter table tenant_memberships enable row level security;
alter table app_memberships enable row level security;
alter table user_capabilities enable row level security;
alter table tenant_email_allowlist enable row level security;
alter table user_provider_keys enable row level security;
alter table cli_auth_challenges enable row level security;
alter table founder_signing_keys enable row level security;
alter table protected_intents enable row level security;
alter table protected_action_audit enable row level security;
alter table panels enable row level security;
alter table panel_components enable row level security;
alter table instance_configs enable row level security;
alter table panel_settings enable row level security;
alter table tab_meta enable row level security;
alter table chat_messages enable row level security;
alter table app_settings enable row level security;
alter table installed_components enable row level security;
alter table service_status_log enable row level security;

-- ─── users ───────────────────────────────────────────────────────────────────

drop policy if exists users_select_self on users;
create policy users_select_self on users
  for select using (user_id = (select app.current_user_id()));

-- ─── tenants ─────────────────────────────────────────────────────────────────

drop policy if exists tenants_select_member on tenants;
create policy tenants_select_member on tenants
  for select using (app.is_tenant_member(tenant_id));

drop policy if exists tenants_insert_founder on tenants;
create policy tenants_insert_founder on tenants
  for insert with check (app.has_capability('founder'));

-- ─── apps ────────────────────────────────────────────────────────────────────

drop policy if exists apps_select_member on apps;
create policy apps_select_member on apps
  for select using (app.is_app_member(tenant_id, app_id));

drop policy if exists apps_insert_admin on apps;
create policy apps_insert_admin on apps
  for insert with check (app.is_tenant_admin(tenant_id));

-- ─── tenant_memberships ───────────────────────────────────────────────────────

drop policy if exists tenant_memberships_select_self on tenant_memberships;
create policy tenant_memberships_select_self on tenant_memberships
  for select using (user_id = (select app.current_user_id()));

drop policy if exists tenant_memberships_admin_manage on tenant_memberships;
create policy tenant_memberships_admin_manage on tenant_memberships
  for all using (app.is_tenant_admin(tenant_id))
  with check (app.is_tenant_admin(tenant_id));

-- ─── app_memberships ─────────────────────────────────────────────────────────

drop policy if exists app_memberships_select_self on app_memberships;
create policy app_memberships_select_self on app_memberships
  for select using (user_id = (select app.current_user_id()));

drop policy if exists app_memberships_admin_manage on app_memberships;
create policy app_memberships_admin_manage on app_memberships
  for all using (app.is_app_admin(tenant_id, app_id))
  with check (app.is_app_admin(tenant_id, app_id));

-- ─── user_capabilities ───────────────────────────────────────────────────────

drop policy if exists user_capabilities_select_self on user_capabilities;
create policy user_capabilities_select_self on user_capabilities
  for select using (user_id = (select app.current_user_id()));

-- ─── tenant_email_allowlist ───────────────────────────────────────────────────

drop policy if exists email_allowlist_admin on tenant_email_allowlist;
create policy email_allowlist_admin on tenant_email_allowlist
  for all using (app.is_tenant_admin(tenant_id))
  with check (app.is_tenant_admin(tenant_id));

-- ─── user_provider_keys ───────────────────────────────────────────────────────

drop policy if exists user_provider_keys_owner on user_provider_keys;
create policy user_provider_keys_owner on user_provider_keys
  for all using (user_id = (select app.current_user_id()))
  with check (user_id = (select app.current_user_id()));

-- ─── cli_auth_challenges ──────────────────────────────────────────────────────
-- Anyone can create a challenge; only owner can read own challenge status.

drop policy if exists cli_challenges_select_owner on cli_auth_challenges;
create policy cli_challenges_select_owner on cli_auth_challenges
  for select using (
    user_id = (select app.current_user_id())
    or user_id is null
  );

-- ─── founder_signing_keys ────────────────────────────────────────────────────

drop policy if exists founder_keys_owner on founder_signing_keys;
create policy founder_keys_owner on founder_signing_keys
  for select using (user_id = (select app.current_user_id()));

-- ─── protected_intents ───────────────────────────────────────────────────────

drop policy if exists protected_intents_actor on protected_intents;
create policy protected_intents_actor on protected_intents
  for select using (actor_user_id = (select app.current_user_id()));

-- ─── protected_action_audit ──────────────────────────────────────────────────

drop policy if exists audit_select_founder on protected_action_audit;
create policy audit_select_founder on protected_action_audit
  for select using (app.has_capability('founder'));

-- ─── panels ──────────────────────────────────────────────────────────────────

drop policy if exists panels_select_member on panels;
create policy panels_select_member on panels
  for select using (app.is_app_member(workspace_id, app_id));

drop policy if exists panels_insert_admin on panels;
create policy panels_insert_admin on panels
  for insert with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists panels_update_admin on panels;
create policy panels_update_admin on panels
  for update using (app.is_app_admin(workspace_id, app_id))
  with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists panels_delete_admin on panels;
create policy panels_delete_admin on panels
  for delete using (app.is_app_admin(workspace_id, app_id));

-- ─── panel_components ────────────────────────────────────────────────────────

drop policy if exists panel_components_select_member on panel_components;
create policy panel_components_select_member on panel_components
  for select using (
    exists (
      select 1 from panels p
      where p.panel_id = panel_components.panel_id
        and app.is_app_member(p.workspace_id, p.app_id)
    )
  );

drop policy if exists panel_components_insert_admin on panel_components;
create policy panel_components_insert_admin on panel_components
  for insert with check (
    exists (
      select 1 from panels p
      where p.panel_id = panel_components.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists panel_components_update_admin on panel_components;
create policy panel_components_update_admin on panel_components
  for update
  using (
    exists (
      select 1 from panels p
      where p.panel_id = panel_components.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  )
  with check (
    exists (
      select 1 from panels p
      where p.panel_id = panel_components.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists panel_components_delete_admin on panel_components;
create policy panel_components_delete_admin on panel_components
  for delete using (
    exists (
      select 1 from panels p
      where p.panel_id = panel_components.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

-- ─── instance_configs (private): app_admin only ───────────────────────────────

drop policy if exists instance_configs_select_admin on instance_configs;
create policy instance_configs_select_admin on instance_configs
  for select using (
    exists (
      select 1 from panel_components pc
      join panels p on p.panel_id = pc.panel_id
      where pc.instance_id = instance_configs.instance_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists instance_configs_insert_admin on instance_configs;
create policy instance_configs_insert_admin on instance_configs
  for insert with check (
    exists (
      select 1 from panel_components pc
      join panels p on p.panel_id = pc.panel_id
      where pc.instance_id = instance_configs.instance_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists instance_configs_update_admin on instance_configs;
create policy instance_configs_update_admin on instance_configs
  for update
  using (
    exists (
      select 1 from panel_components pc
      join panels p on p.panel_id = pc.panel_id
      where pc.instance_id = instance_configs.instance_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  )
  with check (
    exists (
      select 1 from panel_components pc
      join panels p on p.panel_id = pc.panel_id
      where pc.instance_id = instance_configs.instance_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists instance_configs_delete_admin on instance_configs;
create policy instance_configs_delete_admin on instance_configs
  for delete using (
    exists (
      select 1 from panel_components pc
      join panels p on p.panel_id = pc.panel_id
      where pc.instance_id = instance_configs.instance_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

-- ─── panel_settings (private): app_admin only ────────────────────────────────

drop policy if exists panel_settings_select_admin on panel_settings;
create policy panel_settings_select_admin on panel_settings
  for select using (
    exists (
      select 1 from panels p
      where p.panel_id = panel_settings.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

drop policy if exists panel_settings_write_admin on panel_settings;
create policy panel_settings_write_admin on panel_settings
  for all
  using (
    exists (
      select 1 from panels p
      where p.panel_id = panel_settings.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  )
  with check (
    exists (
      select 1 from panels p
      where p.panel_id = panel_settings.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

-- ─── tab_meta ─────────────────────────────────────────────────────────────────

drop policy if exists tab_meta_select_member on tab_meta;
create policy tab_meta_select_member on tab_meta
  for select using (
    exists (
      select 1 from panels p
      where p.panel_id = tab_meta.panel_id
        and app.is_app_member(p.workspace_id, p.app_id)
    )
  );

drop policy if exists tab_meta_write_admin on tab_meta;
create policy tab_meta_write_admin on tab_meta
  for all
  using (
    exists (
      select 1 from panels p
      where p.panel_id = tab_meta.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  )
  with check (
    exists (
      select 1 from panels p
      where p.panel_id = tab_meta.panel_id
        and app.is_app_admin(p.workspace_id, p.app_id)
    )
  );

-- ─── chat_messages ────────────────────────────────────────────────────────────

drop policy if exists chat_messages_select_member on chat_messages;
create policy chat_messages_select_member on chat_messages
  for select using (app.is_app_member(workspace_id, app_id));

drop policy if exists chat_messages_insert_admin on chat_messages;
create policy chat_messages_insert_admin on chat_messages
  for insert with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists chat_messages_update_admin on chat_messages;
create policy chat_messages_update_admin on chat_messages
  for update using (app.is_app_admin(workspace_id, app_id))
  with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists chat_messages_delete_admin on chat_messages;
create policy chat_messages_delete_admin on chat_messages
  for delete using (app.is_app_admin(workspace_id, app_id));

-- ─── app_settings (private, key-scoped) ──────────────────────────────────────

drop policy if exists app_settings_select_admin on app_settings;
create policy app_settings_select_admin on app_settings
  for select using (
    key like ('ws:' || app.current_workspace_id() || ':app:' || app.current_app_id() || ':%')
    and app.is_app_admin(app.current_workspace_id(), app.current_app_id())
  );

drop policy if exists app_settings_write_admin on app_settings;
create policy app_settings_write_admin on app_settings
  for all
  using (
    key like ('ws:' || app.current_workspace_id() || ':app:' || app.current_app_id() || ':%')
    and app.is_app_admin(app.current_workspace_id(), app.current_app_id())
  )
  with check (
    key like ('ws:' || app.current_workspace_id() || ':app:' || app.current_app_id() || ':%')
    and app.is_app_admin(app.current_workspace_id(), app.current_app_id())
  );

-- ─── installed_components ─────────────────────────────────────────────────────

drop policy if exists installed_components_select_member on installed_components;
create policy installed_components_select_member on installed_components
  for select using (
    component_id like (app.current_workspace_id() || ':' || app.current_app_id() || '::%')
    and app.is_app_member(app.current_workspace_id(), app.current_app_id())
  );

drop policy if exists installed_components_write_admin on installed_components;
create policy installed_components_write_admin on installed_components
  for all
  using (
    component_id like (app.current_workspace_id() || ':' || app.current_app_id() || '::%')
    and app.is_app_admin(app.current_workspace_id(), app.current_app_id())
  )
  with check (
    component_id like (app.current_workspace_id() || ':' || app.current_app_id() || '::%')
    and app.is_app_admin(app.current_workspace_id(), app.current_app_id())
  );

-- ─── service_status_log ───────────────────────────────────────────────────────

drop policy if exists service_status_select_member on service_status_log;
create policy service_status_select_member on service_status_log
  for select using (app.is_app_member(workspace_id, app_id));

drop policy if exists service_status_insert_admin on service_status_log;
create policy service_status_insert_admin on service_status_log
  for insert with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists service_status_update_admin on service_status_log;
create policy service_status_update_admin on service_status_log
  for update using (app.is_app_admin(workspace_id, app_id))
  with check (app.is_app_admin(workspace_id, app_id));

drop policy if exists service_status_delete_admin on service_status_log;
create policy service_status_delete_admin on service_status_log
  for delete using (app.is_app_admin(workspace_id, app_id));

commit;
