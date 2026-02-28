# Ecosystem Phase 0 Approved v1

Approval date: 2026-02-27
Approval owner: UBL Founder
Source form: `ECOSYSTEM_PHASE0_DECISION_FORM.md`

## Decision Summary

Phase 0 result: Approved as a whole.

## D-0.1 HQ Product Boundary

Status: Approved

Approved statement:
- HQ owns all cross-app concerns: integrations, identity/auth, policy, observability, metering, pricing/billing derivation.
- App surfaces own domain UX and local presentation behavior only.
- shared Rust core crates define capabilities first.
- CLI is first implementation/validation surface for new capability flow.
- API/MCP may expose only capabilities already validated through CLI flow governance.

Exception rule:
- temporary exception allowed only with expiry date and migration plan.

## D-0.2 Canonical Identity Scope

Status: Approved

Approved statement:
- canonical scope tuple: `tenant_id`, `app_id`, `user_id`.
- protected operations must resolve scope before read/write.
- client-provided scope is untrusted until validated.

Approved precedence:
1. validated token/session claims
2. trusted server context
3. header/query only when explicitly allowed by mode

## D-0.3 Risk Posture by Tier

Status: Approved

Approved statement:
- posture: `operational-first, evidence-sufficient`.
- Tier A strict, Tier B light, Tier C fast.

Approved defaults:
- Tier A: auth + policy + immutable audit receipt.
- Tier B: auth/access + structured telemetry.
- Tier C: minimal telemetry, low friction.

Red lines:
- no auth bypass for protected mutations
- no policy bypass for sensitive actions
- no billing-impact writes without evidence
- no cross-tenant leakage

## D-0.4 Pricing Philosophy

Status: Approved

Approved statement:
- apps emit normalized fuel usage only.
- pricing/billing logic is centralized and versioned.
- apps must not self-price customers.

Approved fuel event core fields:
- `event_id`
- `idempotency_key`
- `tenant_id`
- `app_id`
- `user_id`
- `units`
- `unit_type`
- `occurred_at`
- `source`

Billing invariant:
- every invoice line item references source usage events + `pricing_version`.

## Execution Start Order

1. 0.1 Boundary
2. 0.2 Identity
3. 0.3 Tiered risk posture
4. 1.2 Shared middleware chain
5. 3.1 Policy decision contract
6. 3.2 Usage ledger canon
7. 3.3 Pricing derivation engine
