# Audit Report: 21-Phase Supabase Migration

**Date:** 2026-02-28
**Scope:** All work completed in the [CLI + Supabase Direct Pass](d3f0ea77-e46f-44dc-9596-bd3161b7e043) session, cross-referenced against current Supabase documentation and industry best practices.

---

## Executive Summary

The migration from Vercel Postgres (Neon) to Supabase, including the CLI rewrite for direct PostgREST access, was **architecturally sound**. The core decisions — shared Supabase project, RLS-enforced multi-tenancy, PostgREST from CLI, auth hooks in a custom schema — are all validated patterns. However, the audit identifies **7 concrete improvements** (3 security, 2 performance, 2 reliability) that should be applied.

### Verdict by Area

| Area | Status | Issues |
|---|---|---|
| Database connection strings | CORRECT | Minor: `max: 1` in runtime is limiting |
| Drizzle ORM integration | CORRECT | - |
| RLS policies | MOSTLY CORRECT | Performance: missing `(SELECT auth.uid())` optimization |
| Auth hooks (migration 003) | NEEDS FIX | Security: `security definer` should be removed; missing `REVOKE` grants |
| CLI Supabase client | CORRECT | Security: plaintext token storage should use OS keychain |
| Ed25519 passkeys | CORRECT | Security: private key file should use keychain |
| Founder bootstrap | CORRECT | - |
| Fuel events (append-only) | MOSTLY CORRECT | Missing `REVOKE UPDATE, DELETE` as primary defense |

---

## 1. Database Connection Strings

### What was done
- Pooled: `postgresql://postgres.aypxnwofjtdnmtxastti:***@aws-1-eu-west-1.pooler.supabase.com:6543/postgres`
- Direct: `postgresql://postgres:***@db.aypxnwofjtdnmtxastti.supabase.co:5432/postgres`
- Pooled for runtime, direct for migrations.

### Verdict: CORRECT

The format, port assignments, and username differences (`postgres.ref` for pooler vs `postgres` for direct) are all per Supabase documentation. The `aws-1` region prefix was correctly identified after the `aws-0` error.

### Minor improvement

In `db/index.ts`, `max: 1` limits runtime connection pool to a single connection. For serverless (Vercel), this is fine per-invocation, but if the app ever moves to a long-lived server, this should be bumped.

```typescript
// db/index.ts line 12 — fine for Vercel serverless, but document the assumption
const client = postgres(connectionString, {
  prepare: false,  // required for Supavisor transaction mode
  max: 1,          // one connection per serverless invocation
});
```

**No action needed** — current setup is correct for Vercel.

---

## 2. Drizzle ORM Integration

### What was done
- `drizzle.config.ts` prioritizes `DATABASE_URL_UNPOOLED` for migrations.
- `postgres` (Postgres.js) as the driver with `prepare: false`.
- Tables defined in `db/schema.ts`, pushed with `drizzle-kit push`.

### Verdict: CORRECT

This follows both the [Supabase Drizzle guide](https://supabase.com/docs/guides/database/drizzle) and the [Drizzle Supabase tutorial](https://orm.drizzle.team/docs/tutorials/drizzle-with-supabase) exactly.

### Recommendation: Add `schemaFilter`

Drizzle should filter to avoid introspecting Supabase internal schemas during `drizzle-kit pull` or `drizzle-kit push`:

```typescript
// drizzle.config.ts — add this
tablesFilter: ['!auth.*', '!storage.*', '!realtime.*', '!_*'],
```

This prevents conflicts if Supabase adds internal tables. Low priority since `push` currently works fine.

---

## 3. RLS Policies (Migration 002) — PERFORMANCE FIX NEEDED

### What was done
- All 22 tables have RLS enabled.
- Helper functions (`app.is_tenant_member`, `app.is_app_admin`, etc.) encapsulate membership checks.
- Policies use these helpers consistently.

### Issue: `app.current_user_id()` called per-row instead of cached

Per [Supabase RLS performance guide](https://supabase.com/docs/guides/troubleshooting/rls-performance-and-best-practices-Z5Jjwv), wrapping `auth.uid()` (or your equivalent) in a subselect forces PostgreSQL to evaluate it once and cache it via initPlan, instead of re-evaluating per row.

**Current (called per-row):**
```sql
create policy users_select_self on users
  for select using (user_id = app.current_user_id());
```

**Recommended (called once, cached):**
```sql
create policy users_select_self on users
  for select using (user_id = (SELECT app.current_user_id()));
```

This applies to every policy that calls `app.current_user_id()` directly. The impact scales with table size — on small tables it's negligible, on large tables (like `fuel_events`, `chat_messages`) it can be **10-100x slower** without the subselect.

### Issue: Helper functions missing `SECURITY DEFINER` or `SET search_path`

The helper functions in migration 002 (e.g. `app.is_tenant_member`) do **not** use `SECURITY DEFINER`. This means they execute as the calling role (`authenticated`). Since the tables they query (`tenant_memberships`, `app_memberships`) themselves have RLS policies, this creates **nested RLS evaluation** — potentially causing circular or N+1 policy checks.

**Two valid approaches:**

**Option A — `SECURITY DEFINER` with `SET search_path` (recommended):**
```sql
create or replace function app.is_tenant_member(target_tenant text)
returns boolean language sql stable
security definer
set search_path = public
as $$
  select exists (
    select 1 from tenant_memberships tm
    where tm.tenant_id = target_tenant
      and tm.user_id = (SELECT app.current_user_id())
  );
$$;
```

**Option B — Bypass RLS explicitly for the helper's internal queries** by granting the function owner bypass privileges on the specific tables. More complex, less common.

Option A is how MakerKit, Supabase's own examples, and most production multi-tenant setups handle it.

### Severity: MEDIUM — affects query performance at scale.

---

## 4. Auth Hooks (Migration 003) — SECURITY FIX NEEDED

### What was done
- Functions created in `app` schema (correct — `auth` schema is locked on hosted Supabase).
- Hooks enabled via Management API `PATCH /v1/projects/{ref}/config/auth` (correct).
- URI format `pg-functions://postgres/app/before_user_created` (correct).

### Issue 1: `SECURITY DEFINER` should be REMOVED

Both hook functions use `security definer`. Per Supabase's [official auth hooks documentation](https://supabase.com/docs/guides/auth/auth-hooks):

> "For security, we recommend against the use of the `security definer` tag. The `security definer` tag specifies that the function is to be executed with the privileges of the user that owns it. When a function is created via the Supabase dashboard with the tag, it will have the extensive permissions of the `postgres` role which make it easier for undesirable actions to occur."

**Current:**
```sql
create or replace function app.before_user_created(event jsonb)
returns jsonb language plpgsql security definer as $$
```

**Recommended:**
```sql
create or replace function app.before_user_created(event jsonb)
returns jsonb language plpgsql as $$
```

Then grant explicit access to the tables the hook needs:

```sql
-- Allow supabase_auth_admin to read the tables the hooks query
GRANT SELECT ON tenants TO supabase_auth_admin;
GRANT SELECT ON tenant_email_allowlist TO supabase_auth_admin;
GRANT SELECT ON tenant_memberships TO supabase_auth_admin;
GRANT SELECT ON app_memberships TO supabase_auth_admin;
GRANT USAGE ON SCHEMA app TO supabase_auth_admin;
```

### Issue 2: Missing `REVOKE` from public roles

The functions should be explicitly locked down:

```sql
REVOKE EXECUTE ON FUNCTION app.before_user_created(jsonb) FROM authenticated, anon, public;
REVOKE EXECUTE ON FUNCTION app.custom_access_token(jsonb) FROM authenticated, anon, public;
```

Without this, any authenticated user could call these functions directly via PostgREST RPC, which could be used to probe tenant slugs or email allowlists.

### Issue 3: `provision_user_membership` correctly uses `SECURITY DEFINER`

This function **should** keep `security definer` since it's called by the Next.js API route (which runs as `authenticated` role via user JWT) but needs to insert into RLS-protected tables on behalf of the user being created. This is correct as-is.

### Severity: HIGH — security misconfiguration in auth hooks.

---

## 5. CLI Supabase Client (`supabase.rs`) — SECURITY IMPROVEMENT

### What was done
- Full HTTP client for Supabase Auth + PostgREST.
- Token refresh with 30-second buffer before expiry.
- Tokens stored at `~/.config/logline/auth.json` with `chmod 0600`.

### Verdict: Functionally correct, security could be better.

### Issue: Plaintext token/key storage

`auth.json` and `passkey_ed25519.key` are stored as plaintext files with `0600` permissions. This protects against other OS users but not against any process running as the current user (malware, compromised npm packages, etc.).

**How major CLIs handle this:**

| CLI | Method |
|---|---|
| GitHub CLI (`gh`) | OS keychain (macOS Keychain, Windows Credential Manager) via `--secure-storage` |
| Supabase CLI | Native credentials storage (keyring) |
| AWS CLI | Plaintext `~/.aws/credentials` (recommends STS temp tokens) |
| gcloud | Plaintext `~/.config/gcloud/` |

**Recommended:** Use the `keyring` crate (cross-platform: macOS Keychain, Linux Secret Service, Windows Credential Locker):

```rust
// Cargo.toml
keyring = "3"

// Usage
let entry = keyring::Entry::new("logline-cli", "auth_tokens")?;
entry.set_password(&serde_json::to_string(&stored_auth)?)?;
// ...
let json = entry.get_password()?;
let auth: StoredAuth = serde_json::from_str(&json)?;
```

Fall back to file-based storage with `0600` only when keychain is unavailable (headless servers, CI).

### Severity: MEDIUM — acceptable for v1 but should be upgraded.

---

## 6. Ed25519 Passkeys

### What was done
- Ed25519 keypair generation via `ed25519-dalek`.
- Touch ID gating via spawning a Swift `LAContext` script.
- Public key registered via PostgREST insert.

### Verdict: CORRECT for the use case.

Ed25519 is appropriate for SSH-style CLI authentication. WebAuthn/FIDO2 would require browser interaction, which adds friction to a terminal workflow. The approach is similar to how SSH keys work — generate locally, register public key server-side.

### Minor improvements
1. **Private key storage**: Same as above — use macOS Keychain via `security-framework` or `keyring` crate instead of plaintext file.
2. **Touch ID integration**: The Swift-spawning approach works but is fragile. Consider using the `security-framework` crate directly for macOS Keychain + biometric access control, which gives native Touch ID prompts without spawning a subprocess.

---

## 7. Founder Bootstrap

### What was done
- Uses `SUPABASE_SERVICE_ROLE_KEY` from env var (never hardcoded).
- One-time atomic creation of user, tenant, memberships, capabilities, HQ app.
- Designed to be idempotent.

### Verdict: CORRECT

This is the documented approach for breaking the chicken-and-egg problem with RLS. The key is only used once, from a trusted environment, and the CLI prompts for it rather than embedding it.

### Recommendation
Print a warning after successful bootstrap advising key rotation:

```
⚠ Bootstrap complete. Consider rotating SUPABASE_SERVICE_ROLE_KEY
  in the Supabase Dashboard → Settings → API → Service Role Key.
```

---

## 8. Fuel Events (Append-Only Ledger) — IMPROVEMENT NEEDED

### What was done
- Trigger `no_modify_fuel` blocks `UPDATE` and `DELETE` on `fuel_events`.
- RLS policies allow insert by app members, select by app admins.

### Issue: Trigger alone is insufficient for append-only guarantee

The trigger approach is defense-in-depth but not the primary defense. A superuser or role with `TRIGGER` privilege can drop the trigger. The primary defense should be **revoking UPDATE/DELETE permissions** at the PostgreSQL grant level:

**Add to migration 004:**
```sql
REVOKE UPDATE, DELETE ON fuel_events FROM authenticated, anon;
GRANT INSERT, SELECT ON fuel_events TO authenticated;
```

This makes the table append-only at the permission layer (which cannot be bypassed without superuser), with the trigger as a safety net.

### Severity: LOW — the trigger works, but the REVOKE provides a stronger guarantee.

---

## Summary of Recommended Changes

### HIGH Priority (Security)

| # | What | Where | Impact |
|---|---|---|---|
| 1 | Remove `SECURITY DEFINER` from auth hook functions | migration 003 | Prevents privilege escalation via auth hooks |
| 2 | Add `REVOKE EXECUTE` from public roles on hook functions | migration 003 | Prevents direct RPC probing of allowlists |
| 3 | Add `GRANT SELECT` on required tables to `supabase_auth_admin` | migration 003 | Hooks work without SECURITY DEFINER |

### MEDIUM Priority (Performance + Security)

| # | What | Where | Impact |
|---|---|---|---|
| 4 | Wrap `app.current_user_id()` in `(SELECT ...)` subselect | migration 002 policies | 10-100x RLS performance on large tables |
| 5 | Add `SECURITY DEFINER` + `SET search_path` to helper functions | migration 002 helpers | Prevents nested RLS evaluation |
| 6 | Migrate CLI token storage to OS keychain (`keyring` crate) | `supabase.rs` | Secrets protected by OS credential manager |

### LOW Priority (Hardening)

| # | What | Where | Impact |
|---|---|---|---|
| 7 | Add `REVOKE UPDATE, DELETE` on `fuel_events` | migration 004 | Stronger append-only guarantee |
| 8 | Add `schemaFilter` to `drizzle.config.ts` | drizzle.config.ts | Prevents introspecting Supabase internal schemas |
| 9 | Post-bootstrap key rotation warning in CLI | `main.rs` | Security hygiene reminder |

---

## What Was Done Right (Highlights)

1. **Connection strings**: Correct pooled vs direct split, `prepare: false` for Supavisor.
2. **Drizzle + Postgres.js**: Exactly per official docs, not using `@supabase/supabase-js` as a DB driver (common mistake).
3. **CLI → PostgREST architecture**: RLS enforced regardless of client, no raw Postgres credentials in the binary.
4. **Auth hooks in `app` schema**: Correct workaround for hosted Supabase's locked `auth` schema.
5. **Management API for hooks**: Programmatic configuration instead of manual Dashboard clicks.
6. **Hook URI format**: `pg-functions://postgres/app/function_name` is exactly correct.
7. **Token auto-refresh**: 30-second buffer before expiry prevents race conditions.
8. **Founder bootstrap with service_role**: Standard pattern, env-var-only, never embedded.
9. **Append-only trigger**: Valid defense-in-depth pattern.
10. **Idempotent bootstrap**: Safe to run multiple times.
