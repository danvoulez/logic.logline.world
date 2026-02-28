# UBLX Ecosystem — LLM Start Here

**Read this entire file before doing anything.** This is the normative guide for AI agents working in this codebase.

## Architecture (one sentence)

`logline` is a CLI-only ecosystem: one Rust binary, Supabase as sole infrastructure, Touch ID as hardware root of trust. There is no daemon, no separate backend, no alternative access path.

## Security Model — CLI or Nothing

All infrastructure operations MUST use `logline` CLI commands. There is no other way.

**Why you cannot bypass it:**
1. No `DATABASE_URL` exists anywhere on disk
2. No API tokens exist anywhere on disk
3. All secrets live in macOS Keychain, which requires Touch ID (hardware biometric)
4. `psql` without a connection string does nothing
5. Supabase PostgREST requires a valid JWT that only exists in Keychain
6. You (the LLM) cannot simulate a fingerprint

**What this means for you:**
- NEVER attempt direct database connections
- NEVER create `.env` files with secrets or connection strings
- NEVER hardcode tokens, passwords, or connection strings in any file
- When you need infrastructure access, ask the human to run CLI commands
- The human must approve via Touch ID — this is by design

## Project Structure

```
/Users/ubl-ops/UBLX App/
├── app/                    # Next.js UI (observability dashboard)
│   ├── api/                # API routes (PostgREST proxy, settings)
│   └── page.tsx            # Main UI entry
├── components/             # React components
├── logline/                # Rust workspace (THE ecosystem)
│   └── crates/
│       ├── logline-cli/    # The only binary that matters
│       ├── logline-api/    # Shared models + trait contracts
│       ├── logline-auth/   # JWT/JWKS verification
│       ├── logline-core/   # Domain policy + catalog
│       ├── logline-connectors/  # Connector implementations
│       └── logline-runtime/     # Runtime engine
├── supabase/
│   └── migrations/         # SQL migrations (require review before apply)
├── docs/                   # Documentation
├── .env                    # PUBLIC values only (anon key, URLs)
└── logline.cicd.json       # CI/CD pipeline definition
```

## Key CLI Commands

```bash
# Auth (requires Touch ID)
logline auth unlock              # Open a session (30m default)
logline auth login --passkey     # Login via passkey
logline auth lock                # Lock session immediately
logline auth status              # Check session status

# Pre-flight
logline secrets doctor           # Full health check
logline ready --pipeline prod    # Pipeline readiness

# Database (requires unlocked session + Keychain credentials)
logline db tables                # List tables
logline db query "SELECT ..."    # Execute SQL
logline db verify-rls            # Verify RLS policies

# Migrations (separated from CI/CD)
logline db migrate status        # Show applied vs pending
logline db migrate review        # Review pending migrations (generates receipt)
logline db migrate apply         # Apply migrations (requires review receipt)

# Deploy (requires passkey + non-founder)
logline deploy all --env prod    # Full deploy: Supabase → GitHub → Vercel
logline cicd run --pipeline prod # Run CI/CD pipeline

# Ecosystem
logline app list                 # List apps
logline app create               # Register new app
logline app handshake            # Bidirectional key exchange
logline fuel emit                # Record fuel usage
logline tenant create            # Create tenant
logline tenant allowlist-add     # Add user to allowlist
```

## How to Work on This Codebase

### Editing Rust CLI code
1. Edit files in `logline/crates/logline-cli/src/`
2. Validate: `cd logline && cargo check`
3. The human runs `logline auth unlock` + deploys when ready

### Editing Next.js UI code
1. Edit files in `app/`, `components/`, `lib/`
2. The human runs `npm run dev` to test locally
3. UI is observability only — it cannot modify infrastructure

### Adding database migrations
1. Create a new `.sql` file in `supabase/migrations/` with timestamp prefix
2. The human reviews with `logline db migrate review`
3. The human applies with `logline db migrate apply --env prod`
4. You CANNOT apply migrations directly — no database credentials on disk

### CI/CD pipeline
- Defined in `logline.cicd.json`
- CI/CD blocks if migrations are pending (must apply first)
- Runs via `logline cicd run --pipeline prod`

## What NOT to Do

| Action | Why it fails |
|---|---|
| `psql postgresql://...` | No connection string exists on disk |
| `curl -H "Authorization: Bearer ..."` | No tokens exist on disk |
| Create `.env` with `DATABASE_URL` | Violates security model; `secrets doctor` will flag it |
| Direct Supabase Dashboard edits | Bypasses CLI audit trail |
| Install a separate database | All data lives in one Supabase project |
| Start a daemon or server process | The CLI is the only binary |

## Supabase Infrastructure

Single project: `aypxnwofjtdnmtxastti.supabase.co`

Used for:
- **Auth**: Users, passkeys, sessions (Supabase Auth)
- **Postgres**: All data, RLS on every table
- **PostgREST**: CLI and UI access, always via JWT
- **Realtime**: Future observability live feed
- **Storage**: Future receipts and artifacts

## Quick Mental Model

- The **CLI** is the ecosystem. It does everything.
- The **UI** is observability. It shows data. It cannot change infrastructure.
- **Supabase** is the sole infrastructure. One project, one database, one auth system.
- **Touch ID** is the enforcement. Hardware security that no software can bypass.
- **Secrets Doctor** validates the entire chain. Run it when unsure.
