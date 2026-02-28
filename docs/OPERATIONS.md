# Operations Guide

This document is for day-to-day runtime operations on UBLX + LogLine services.

## 1) Process Inventory (PM2)

List all services:

```bash
pm2 list
```

Common processes used by this project:
- `agent-256`
- `agent-256-mcp`
- `llm-gateway`
- `logline-daemon`
- `ubl-cloudflared` (if tunnel is active)

## 2) Core Restart Commands

Gateway:

```bash
pm2 restart llm-gateway --update-env
```

LAB256 agent:

```bash
pm2 startOrReload /Users/ubl-ops/LAB256-Agent/ecosystem.config.cjs --only agent-256
```

Logline daemon:

```bash
pm2 restart logline-daemon --update-env
```

Save PM2 process list after stable changes:

```bash
pm2 save
```

## 3) Logs and Live Debug

Agent logs:

```bash
pm2 logs agent-256 --lines 100 --nostream
```

Gateway logs:

```bash
pm2 logs llm-gateway --lines 120 --nostream
```

Daemon logs:

```bash
pm2 logs logline-daemon --lines 120 --nostream
```

## 4) Health Checks

App API health-by-function:

```bash
curl -sS http://localhost:3000/api/panels | jq
curl -sS http://localhost:3000/api/settings | jq
```

Agent health:

```bash
set -a; source /Users/ubl-ops/LAB256-Agent/agent/.env; set +a
curl -sS -H "Authorization: Bearer $AGENT_TOKEN" http://127.0.0.1:4256/health | jq
```

Expected in health:
- `ok: true`
- `gateway.ok: true`
- `gateway.auth_mode: onboarded` (for onboarding mode)

Gateway direct onboarding probe:

```bash
curl -sS -H "Authorization: Bearer <CLI_JWT>" \
  -H "Content-Type: application/json" \
  -d '{"app_name":"lab256-agent"}' \
  http://127.0.0.1:7700/v1/onboarding/sync | jq
```

## 5) Configuration Surfaces

Primary runtime files:
- Agent env: `/Users/ubl-ops/LAB256-Agent/agent/.env`
- Agent PM2 app config: `/Users/ubl-ops/LAB256-Agent/ecosystem.config.cjs`
- Gateway config: `/Users/ubl-ops/.llm-gateway/config.toml`
- UBLX app env: `/Users/ubl-ops/UBLX App/.env.local`

## 6) Persistence + Data Safety

UBLX app persistence:
- Postgres via `DATABASE_URL`.

Agent memory:
- SQLite default `~/.lab256-agent/memory.db`.
- Gateway key cache: `~/.lab256-agent/gateway-key.json`.

Before risky changes:
1. Backup `.env` files.
2. Backup gateway config.
3. Export PM2 state (`pm2 save`).

## 7) Security Hygiene

- Never commit real keys or tokens.
- Prefer onboarding (`CLI_JWT`) over static long-lived app keys.
- Keep gateway host allowlists strict in app env:
  - `LLM_GATEWAY_ALLOWED_HOSTS`

## 8) Current Operational Risks

1. Upstream provider credits or local model availability can cause `502 upstream_error`.
2. Gateway candidate retries can increase latency when all routes are degraded.
3. PM2 inherited env can override local `.env` expectations if not explicitly managed.

## 9) Escalation Order

When chat path fails:
1. Validate agent health/auth mode.
2. Validate gateway onboarding endpoint.
3. Validate direct gateway completion request.
4. Check upstream provider/local model status.
5. Apply deterministic mode or fallback policy.

## 10) Related Docs

- `LAB256_AGENT_GATEWAY_RUNBOOK.md`
- `TROUBLESHOOTING.md`
- `API_CONTRACTS.md`
