# Security Guide

This document defines the minimum security posture for UBLX, gateway proxying, and agent onboarding.

## 1) Secret Handling Rules

1. Never commit live secrets/tokens.
2. Use `.env*` for local runtime only.
3. Keep production secrets in deployment secret managers (for example Vercel env).
4. Rotate compromised/old keys immediately.

Sensitive values include:
- DB credentials (`DATABASE_URL`)
- gateway admin key / app keys / user provider keys
- `CLI_JWT` and onboarding secrets
- daemon bootstrap token
- `SUPABASE_JWT_SECRET` and `SUPABASE_SERVICE_ROLE_KEY`
- `SUPABASE_ACCOUNT_TOKEN` / `SUPABASE_ACCESS_TOKEN` (Supabase CLI/account tokens)

## 2) Identity and Scope Isolation

- API access is resolved by `requireAccess()` using:
- user identity
- tenant/workspace
- app
- role and permission (`read`, `write`, `private_read`)

Identity resolution:
- JWT mode (`AUTH_PROVIDER_MODE=jwt`): `Authorization: Bearer <supabase_jwt>`.
- Compat mode (`AUTH_PROVIDER_MODE=compat`): header/query fallback for local dev.

Token semantics:
- `SUPABASE_ACCOUNT_TOKEN` / `SUPABASE_ACCESS_TOKEN` are for Supabase CLI/account operations.
- They are not valid user auth credentials for `/api/v1/auth/*` or daemon `/v1/auth/*`.
- User-facing auth endpoints require a Supabase user access JWT in `Authorization: Bearer <jwt>`.

Scope resolution:
- `x-workspace-id` / `workspace_id` query / default.
- `x-app-id` / `app_id` query / default.
- JWT `workspace_id` and `app_id` claims can fill defaults.

## 3) Gateway Proxy Controls (`/api/llm-gateway/*`)

Controls currently enforced:
- allowlisted target hosts (`LLM_GATEWAY_ALLOWED_HOSTS` + defaults)
- URL protocol validation (`http/https`)
- fallback base URL from trusted server-side settings/env

Why:
- Prevent open-proxy/SSRF behavior.

## 4) Authentication Boundaries

### UBLX app API
- protected by app-level access checks
- private routes (`/api/settings`, `/api/effective-config/*`, `/api/instance-configs/*`) require app_admin-level permissions

### Logline daemon proxy (`/api/logline/*`)
- upstream daemon should enforce token/JWT auth

### `/api/v1/*` auth/governance routes
- onboarding routes validate Supabase JWTs
- founder routes require explicit `founder` capability
- CLI QR flow must use short-lived challenges and expiration checks

### LAB256 agent
- protected by `AGENT_TOKEN`
- gateway auth should use onboarding flow:
  - `CLI_JWT` -> `/v1/onboarding/sync` -> issued app key

## 5) Onboarding Security

Gateway side:
- set `[security].onboarding_jwt_secret`
- set `[security].onboarding_jwt_audience`

Agent side:
- provide `CLI_JWT` with short TTL where possible
- avoid hardcoded static `LLM_GATEWAY_KEY`

## 6) PM2 Environment Hygiene

Risk:
- inherited PM2 env can override local `.env` expectations.

Mitigation:
- explicitly set dangerous overrides in PM2 ecosystem config.
- for onboarding mode, force:
  - `LLM_GATEWAY_KEY=""`

## 7) Transport Exposure

If exposing local services via tunnel:
- keep health endpoints minimal/public.
- require auth for all control routes.
- prefer short-lived session tokens for mobile clients.

## 8) Incident Response (Minimum)

If secret leakage suspected:
1. Revoke/rotate affected keys and tokens.
2. Restart impacted services with updated env.
3. Audit logs for unauthorized requests.
4. Verify auth mode and health endpoints.
5. Document incident in changelog/ops notes.

## 9) Immediate Security Backlog

1. Encrypt `user_provider_keys.encrypted_key` values at application layer with key rotation support.
2. Add rate limiting for `/api/v1/cli/auth/*`, `/api/v1/founder/*`, and proxy endpoints.
3. Add audit logging for `/api/settings` writes and auth mode transitions.
4. Add CI secret scanning and policy checks for docs/examples.
