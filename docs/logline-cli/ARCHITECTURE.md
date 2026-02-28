# Logline CLI v1 Architecture

## Core Principle
- Runtime is the brain.
- CLI is the primary control surface.
- UI (desktop/web/iPhone) is a remote control that sends intents and reads state.

## Runtime Boundaries
1. Domain Logic (`logline-core`)
- Rules, validation, policies, deterministic decision logic.

2. Execution (`logline-runtime`)
- Intent execution, retries/backoff, scheduler, state transitions, audit events.

3. Connectors (`logline-connectors`)
- Backend adapters behind a stable trait contract.
- Supports multiple auth modes (`api_key`, `bearer`, `mtls`) and endpoint URLs.

4. Surfaces
- CLI (`logline-cli`) for operators.
- Daemon API (`logline-daemon`) for UI/mobile clients.

## Precedence Model
- CLI flags > env vars > profile config > defaults.

## Profiles
- Named profiles: `local`, `staging`, `prod`, custom.
- Profile selects active backend and runtime policy.

## API/Daemon Contract (v1)
- `GET /v1/health`
- `GET /v1/status`
- `GET /v1/events?since=<cursor>`
- `POST /v1/intents/run`
- `POST /v1/intents/stop`
- `GET /v1/profiles`
- `POST /v1/profiles/select`
- `GET /v1/backends`
- `POST /v1/backends/test`
- `GET /v1/config/effective`

## Security Baseline
- Secrets stored in system keychain/vault; config stores references.
- Daemon API protected by local token/session.
- Optional read-only token for mobile UI.
- All mutating intents emit audit events.

## Recommended Workspace Structure
- `crates/logline-core`
- `crates/logline-runtime`
- `crates/logline-connectors`
- `crates/logline-api`
- `crates/logline-daemon`
- `crates/logline-cli`

## Implementation Order
1. `logline-api` + shared models.
2. Connector trait + one concrete backend.
3. Runtime intent executor.
4. CLI commands wired to runtime.
5. Daemon API wrapping runtime.
6. UI/mobile client as daemon consumer.
