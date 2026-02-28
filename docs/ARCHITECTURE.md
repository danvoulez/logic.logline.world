# UBLX Architecture

This document defines the current architecture and responsibility boundaries.

## 1) High-Level Layers

1. UI Layer (Next.js client components)
- App shell, tabs, component grid, store, and settings UX.
- Primary files:
  - `app/page.tsx`
  - `components/shell/AppShell.tsx`
  - `components/panel/*`
  - `components/component-catalog/*`

2. App API Layer (Next.js route handlers under `app/api/*`)
- Persistence, workspace scoping, config resolution, and gateway proxying.
- Uses server-side DB access and explicit request scoping.

3. Data Layer (Postgres + Drizzle)
- Canonical state for tabs/panels, instances, settings, chat history, status logs.
- Schema/bootstrap:
  - `db/schema.ts`
  - `db/bootstrap.ts`

4. Runtime Layer (LogLine workspace)
- CLI + daemon + shared crates under `logline/`.
- Handles runtime intents and daemon-side operational control.
- Capability flow contract:
  - shared core crates define domain capabilities,
  - CLI is the first interface to implement and validate operator flow,
  - API/MCP surfaces mirror CLI-validated capabilities and must not bypass or outrun CLI flow governance.

5. External/Adjacent Services
- LLM Gateway (`/v1/chat/completions`, `/metrics`, usage/fuel endpoints).
- LAB256-Agent + MCP local tooling.

## 2) Main Data Flows

1. UI -> `/api/*` -> DB
- UI uses React Query hooks from `lib/api/db-hooks.ts`.
- API handlers enforce workspace scoping via `x-workspace-id` or default workspace.

2. UI component config resolution
- UI requests `/api/effective-config/[instanceId]`.
- Server merges app/panel/instance layers and resolves binding tags.

3. UI -> `/api/llm-gateway/*` -> Gateway
- Server proxy route validates target host against allowlist.
- Base URL resolution order:
  1. request header
  2. workspace app setting (`component_defaults.llm_gateway_base_url`)
  3. server env (`LLM_GATEWAY_BASE_URL`)

4. Agent -> Gateway onboarding -> Gateway chat
- LAB256-Agent obtains/rotates per-app key using `CLI_JWT`.
- Agent uses issued key for real completion requests.

## 3) Workspace Isolation

- Workspace ID resolution in `lib/auth/workspace.ts`:
  - `x-workspace-id` header
  - `workspace_id` query
  - fallback `DEFAULT_WORKSPACE_ID` (default: `default`)
- Most API routes are workspace-aware.
- Chat persistence is explicitly workspace-scoped.

## 4) Settings Responsibility Boundaries

- App settings: global defaults and shared secrets/bindings.
- Tab settings: per-tab defaults/overrides.
- Component settings: instance-specific behavior and UI/front props.

Formal behavior is documented in:
- `SETTINGS_CASCADE.md`

## 5) Reliability Boundaries

- Gateway proxy hardening reduces SSRF/open-proxy risk via host allowlist.
- Effective config endpoint provides deterministic resolved state for rendering.
- Remaining runtime risk mainly depends on upstream provider/model availability.

## 6) Current Known Constraints

1. Provider outages/credit failures can still break chat even with valid auth.
2. Route retries in gateway can cause long waits if all candidates are unhealthy.
3. Full auth-to-workspace mapping (signed identity) is still pending roadmap work.

## 7) Design Intent

- Keep business/runtime logic centralized in CLI/daemon/gateway.
- Keep capability definition in shared Rust core crates and make CLI the first execution surface.
- Prevent API/MCP from exposing behavior not already validated in CLI flows.
- Keep UI modular and lightweight, focused on composition and observability.
- Keep settings deterministic via explicit cascade and tag binding resolution.
