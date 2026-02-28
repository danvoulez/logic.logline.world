# Deployment Guide

This project supports multiple deployment topologies. Pick one based on your operational goal.

## 1) Topology A: Vercel UI + External Services

Use when:
- You want fast UI deploys.
- Backend services (gateway/daemon/agent) run elsewhere.

Required env in Vercel:
- `DATABASE_URL`
- `DATABASE_URL_UNPOOLED` (optional but recommended for migration tooling)
- `DEFAULT_WORKSPACE_ID` (optional, default is `default`)
- `DEFAULT_APP_ID` (optional, default is `ublx`)
- `AUTH_PROVIDER_MODE` (`jwt` recommended in production)
- `RBAC_STRICT` (`1` recommended in production)
- `SUPABASE_JWT_SECRET` (required in JWT mode)
- `LLM_GATEWAY_BASE_URL` (optional fallback)
- `LLM_GATEWAY_ALLOWED_HOSTS`
- `LOGLINE_DAEMON_URL` (if using `/api/logline/*`)
- `LOGLINE_DAEMON_TOKEN` (if daemon is token-protected)

**Upload secrets with Vercel CLI** (when already logged in with `vercel login`):

```bash
# From project root. Prompts for value; use Production, Preview, Development as needed.
vercel env add LOGLINE_DAEMON_TOKEN
vercel env add SUPABASE_JWT_SECRET
vercel env add DATABASE_URL
# Add other env vars the same way: vercel env add <NAME>
```

Redeploy after changing env so new values are picked up.

Notes:
- App API routes run in Vercel server context.
- Gateway proxy route enforces host allowlist.

## 2) Topology B: Local Full Stack (Mac mini)

Use when:
- You run UBLX + gateway + agent on one machine.
- You want local-first operations with PM2.

Core services:
- `next dev` / Next.js app
- `llm-gateway` (PM2)
- `agent-256` (PM2)
- optional `logline-daemon` + cloudflared tunnel

Useful commands:

```bash
cd "/Users/ubl-ops/UBLX App"
npm run dev

pm2 restart llm-gateway --update-env
pm2 startOrReload /Users/ubl-ops/LAB256-Agent/ecosystem.config.cjs --only agent-256
```

## 3) Topology C: Local Backend + Remote Mobile Access

Use when:
- iPhone/macOS clients should reach local daemon/gateway through a stable public hostname.

Reference:
- `logline/docs/REMOTE_IPHONE_SETUP.md`

High level:
1. Run daemon locally with token auth.
2. Expose daemon through Cloudflare tunnel hostname.
3. Use session-token flow for mobile clients.

## 4) Minimal Env Matrix

### UBLX app (`/Users/ubl-ops/UBLX App/.env.local`)

```env
DATABASE_URL=...
DATABASE_URL_UNPOOLED=...
DEFAULT_WORKSPACE_ID=default
DEFAULT_APP_ID=ublx
AUTH_PROVIDER_MODE=jwt
RBAC_STRICT=1
SUPABASE_JWT_SECRET=...
LLM_GATEWAY_BASE_URL=https://api.logline.world
LLM_GATEWAY_ALLOWED_HOSTS=api.logline.world,localhost,127.0.0.1
LOGLINE_DAEMON_URL=https://logline.voulezvous.tv
LOGLINE_DAEMON_TOKEN=...
```

### LAB256 agent (`/Users/ubl-ops/LAB256-Agent/agent/.env`)

```env
AGENT_PORT=4256
AGENT_TOKEN=...
LLM_GATEWAY_URL=http://localhost:7700/v1
LLM_GATEWAY_MODE=premium
CLI_JWT=...
LLM_GATEWAY_APP_NAME=lab256-agent
MCP_LOCAL_URL=http://127.0.0.1:4257/mcp
```

### Gateway (`/Users/ubl-ops/.llm-gateway/config.toml`)

- `api_key` (admin key)
- `[security] onboarding_jwt_secret`, `onboarding_jwt_audience`

## 5) Post-Deploy Validation

App:

```bash
curl -sS https://<your-ui>/api/panels | jq
```

Gateway proxy:

```bash
curl -sS https://<your-ui>/api/llm-gateway/v1/fuel | jq
```

Agent:

```bash
curl -sS -H "Authorization: Bearer <AGENT_TOKEN>" http://127.0.0.1:4256/health | jq
```

Expected:
- Agent health reports `gateway.auth_mode: "onboarded"` in onboarding mode.

## 6) Rollback Checklist

1. Keep backup copies of `.env` and config files before edits.
2. Use PM2 restart per service (not full machine restart first).
3. Revert changed env/config and restart affected service.
4. Confirm health endpoints before reopening traffic.
