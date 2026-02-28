# API Contracts

This document defines the app-side API surface under `app/api/*`.

## Conventions

- Base URL: same origin as Next.js app (`http://localhost:3000` in local dev).
- Content type: `application/json` unless proxying upstream content.
- Most routes are protected by `requireAccess()` with one of:
- `read`
- `write`
- `private_read`

## Identity and Scoping

Identity sources:
- JWT mode (`AUTH_PROVIDER_MODE=jwt`): `Authorization: Bearer <supabase_jwt>`.
- Compat mode (`AUTH_PROVIDER_MODE=compat`): `x-user-id` or `user_id` query.
- `SUPABASE_ACCOUNT_TOKEN` / `SUPABASE_ACCESS_TOKEN` are CLI/account tokens and are not accepted as API Bearer JWTs.

Scope sources:
- Tenant/workspace: `x-workspace-id` or `workspace_id` query, default `DEFAULT_WORKSPACE_ID`.
- App: `x-app-id` or `app_id` query, default `DEFAULT_APP_ID`.
- In JWT mode, token claims (`workspace_id`, `app_id`) can fill defaults.

Permission effects:
- `read`: member/app_admin can read.
- `write`: app_admin only.
- `private_read`: app_admin only.

## `/api/*` Product Routes

### Panels

- `GET /api/panels` -> list ordered panels with components (`read`).
- `POST /api/panels` body `{ "name": "New Tab" }` -> create panel (`write`).
- `PATCH /api/panels/:panelId` body `{ "name": "Ops", "position": 1 }` (`write`).
- `DELETE /api/panels/:panelId` -> delete panel (`write`).

### Panel Components

- `GET /api/panels/:panelId/components` -> list instances (`read`).
- `POST /api/panels/:panelId/components` body `{ "componentId": "chat-ai" }` (`write`).
- `PATCH /api/panels/:panelId/components/:instanceId` body with `rect` and/or `front_props` (`write`).
- `DELETE /api/panels/:panelId/components/:instanceId` (`write`).

### Instance and Cascade Config

- `GET /api/instance-configs/:instanceId` -> instance private config (`private_read`).
- `PUT /api/instance-configs/:instanceId` -> upsert private config (`write`).
- `GET /api/panel-settings/:panelId` -> tab settings (`read`).
- `PUT /api/panel-settings/:panelId` -> update tab settings (`write`).
- `GET /api/effective-config/:instanceId` -> cascade payload (`private_read`).

`/api/effective-config/:instanceId` includes:
- `layers`
- `effective`
- `bindings`
- `binding_sources`
- `missing_required_tags`

### App Settings, Tab Meta, Store

- `GET /api/settings` -> app-scoped settings map (`private_read`).
- `PATCH /api/settings` body `{ "key": "...", "value": ... }` (`write`).
- `GET /api/tab-meta/:panelId` (`read`).
- `PUT /api/tab-meta/:panelId` body `{ "icon": "...", "label": "...", "shortcut": 1 }` (`write`).
- `GET /api/installed-components` (`read`).
- `POST /api/installed-components` body `{ "componentId": "..." }` (`write`).
- `DELETE /api/installed-components/:componentId` (`write`).

### Chat and Status Log

- `GET /api/chat?session_id=...` -> list messages (`read`).
- `POST /api/chat` -> append message (`write`).
- `GET /api/status-log?limit=50` -> workspace+app scoped rows (`read`).
- `POST /api/status-log` body `{ "service_name": "...", "status": "...", "latency_ms": 42 }` (`write`).

### Proxies

- `ANY /api/llm-gateway/*` (`private_read`):
- Host allowlist enforced by `LLM_GATEWAY_ALLOWED_HOSTS`.
- Base URL resolution: header `x-llm-gateway-base-url` -> scoped setting `component_defaults.llm_gateway_base_url` -> env `LLM_GATEWAY_BASE_URL`.
- `ANY /api/logline/*` (`read`): server-side daemon proxy.

## `/api/v1/*` Auth and Governance Routes

### Auth and Onboarding

- `GET /api/v1/auth/whoami`
- Returns user profile, tenant memberships, app memberships, capabilities.
- Accepts JWT, or compat fallback (`x-logline-token`/`x-user-id`).

- `POST /api/v1/auth/tenant/resolve`
- Body: `{ "slug": "tenant-slug" }`
- Returns tenant metadata and `has_allowlist`.

- `POST /api/v1/auth/onboard/claim`
- Requires JWT.
- Body: `{ "tenant_slug": "...", "display_name": "..." }`
- Provisions user + memberships from allowlist defaults.

### CLI QR Auth

- `POST /api/v1/cli/auth/challenge`
- Body: `{ "device_name": "..." }` optional.
- Returns `challenge_id`, `nonce`, `challenge_url`, `expires_at`.

- `GET /api/v1/cli/auth/challenge/:challengeId/status`
- Returns challenge status and session token when approved.

- `POST /api/v1/cli/auth/challenge/:challengeId/approve`
- Requires JWT.
- Body: `{ "action": "approve" | "deny" }`.

### User-Owned Keys

- `GET /api/v1/apps/:appId/keys/user` (`read`).
- `POST /api/v1/apps/:appId/keys/user` (`read` to identify user/app scope).
- Body: `{ "provider": "...", "key_label": "...", "encrypted_key": "...", "metadata": {} }`.
- Tenant/user/app are derived from access context, not trusted from payload.

### Founder Signed Intents

- `POST /api/v1/founder/keys/register`
- Requires JWT + `founder` capability.
- Body: `{ "public_key": "<hex>", "algorithm": "ed25519" }`.

- `POST /api/v1/founder/intents/verify`
- Requires JWT + `founder`.
- Verifies signature, nonce freshness, expiry, then stores verified intent.

- `POST /api/v1/founder/actions/execute`
- Requires JWT + `founder`.
- Executes previously verified intent and appends immutable audit trail.

## Common Status Codes

- `200` or `201`: success.
- `400`: malformed input.
- `401`: missing/invalid auth.
- `403`: membership/permission denied.
- `404`: scoped resource not found.
- `409`: conflict (for example replay or non-pending challenge).
- `410`: expired auth challenge/intent.
- `500`: server error.
- `502`: upstream proxy error.
