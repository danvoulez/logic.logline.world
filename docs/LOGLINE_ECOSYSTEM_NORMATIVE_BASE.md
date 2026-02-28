# LogLine Ecosystem Normative Base

This document defines non-negotiable architecture rules and boundaries for the ecosystem.
It exists to preserve clarity and speed, not bureaucracy.

## 1) What We Are

- A Rust-first runtime where business logic lives in CLI/daemon crates.
- A Next.js UI system focused on presentation and operator flow.
- A centralized control-plane model for integrations, policy, identity, observability, and billing derivation.
- A contract-first system: explicit, versioned APIs/events over hidden coupling.
- A practical trust model: enough evidence to operate safely, not audit theater.

## 2) What We Are Not

- Not a UI-first logic architecture.
- Not a set of app-specific security and billing islands.
- Not a free-form integration mesh that bypasses central checks.
- Not a compliance simulator where process replaces product value.

## 3) Core Invariants (MUST)

- Business logic MUST have a single authority in Rust runtime/control plane.
- Domain capabilities MUST be defined in shared Rust core crates before transport/interface exposure.
- CLI MUST be the first interface to implement and validate operator flow for new capabilities.
- API/MCP adapters MUST mirror CLI-validated capabilities and MUST NOT expose behavior that bypasses CLI governance.
- UI handlers MUST remain transport adapters/proxies, not domain engines.
- Identity scope MUST resolve to `tenant_id`, `app_id`, and `user_id` before protected operations.
- Sensitive mutations MUST pass `auth -> policy -> execution -> audit`.
- Usage events MUST be normalized, idempotent, and append-only.
- Pricing/billing outputs MUST be reproducible from usage ledger + pricing version.
- Cross-tenant access MUST be denied by default.
- Breaking API changes MUST be versioned and sunset-managed.

## 4) Sovereignty Boundaries

Sovereignty/context separation is enforced at runtime, not naming convention.

- Personal context and platform context MUST NOT share operational identity.
- Credentials/tokens/keys MUST be context-specific and non-transferable by default.
- Audit trails MUST preserve context origin.
- Cross-context actions MUST produce explicit reconciliation/audit evidence.

## 5) Homeostasis Principle

When pressure rises, the system must preserve operability and evidence continuity.

- Preserve evidence under conflict instead of overwriting history.
- Prefer containment and explicit correction over silent mutation.
- Keep platform behavior explainable by receipts and policy references.

## 6) Security and Capability Baseline

- Capability checks happen at runtime against scope and limits.
- Any bypass path around central gate is an architectural defect.
- Key identity actions require rotation support and provenance trails.
- Security incidents must emit evidence sufficient for forensic reconstruction.

## 7) Operational-First Audit Posture

Audit is enabling infrastructure, not mission identity.

- Critical path actions require strong immutable audit.
- Normal operations use lightweight structured telemetry.
- Low-risk interactions should not be burdened by heavy audit friction.

See `LOGLINE_OPERATING_POSTURE.md` for execution tiers.

## 8) Should / Should Not

Should:
- Keep policy and billing centralized.
- Keep contracts explicit and testable.
- Keep observability operator-friendly.
- Keep architecture decisions reversible through versioning.

Should not:
- Duplicate logic in Next.js route handlers.
- Put billing formulas inside product apps.
- Trust unverified scope from clients.
- Allow implicit cross-context impersonation.

## 9) Adoption Status

Adopted now:
- Rust authority + UI proxy model.
- Central policy direction.
- Normalized usage-to-billing principle.
- Operational-first audit posture.

Deferred:
- Full federated conflict protocol automation.
- Advanced multi-context reconciliation UX.

## 10) Testable Checks (CI/Review)

- `MUST-001`: no new app route contains DB-backed domain logic for Rust-owned resources.
- `MUST-001A`: new capability PRs include core crate implementation before API/MCP exposure.
- `MUST-001B`: API/MCP handlers reference existing CLI/core use-case, not duplicated domain logic.
- `MUST-002`: mutating endpoints enforce access resolution and role checks.
- `MUST-003`: billing artifacts reference usage event IDs and pricing version.
- `MUST-004`: protected actions write audit evidence.
- `MUST-005`: contract changes include versioning impact note.
