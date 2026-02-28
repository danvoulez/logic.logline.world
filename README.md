# logline

CLI-only ecosystem. One binary, Supabase direct, Touch ID enforced.

## Crates
- `logline-api`: shared models + trait contracts
- `logline-auth`: JWT/JWKS verification + tenant/cookie helpers
- `logline-core`: domain policy + catalog validation
- `logline-connectors`: connector implementations/factory
- `logline-runtime`: runtime engine orchestration
- `logline-cli`: the CLI (the only binary that matters)

## Quickstart
```bash
cargo run -p logline-cli -- init
cargo run -p logline-cli -- status
cargo run -p logline-cli -- auth login --email you@example.com
cargo run -p logline-cli -- auth login --passkey
cargo run -p logline-cli -- auth unlock
cargo run -p logline-cli -- secrets doctor
cargo run -p logline-cli -- ready --pipeline prod
```

## Security Model
- All secrets live in macOS Keychain, gated by Touch ID
- No secrets on disk. Ever. No `auth.json`, no `passkey.json`, no `DATABASE_URL` in `.env`
- Every infra command requires: Touch ID unlock + passkey login + non-founder identity
- `logline secrets doctor` validates the entire chain

## Key Commands
```bash
logline auth unlock              # Touch ID session (30m default)
logline auth login --passkey     # Supabase Auth via passkey
logline secrets set <key>        # Store credential in Keychain
logline secrets doctor           # Full system health check
logline ready --pipeline prod    # Pre-flight readiness check
logline deploy all --env prod    # Full deploy: Supabase → GitHub → Vercel
logline cicd run --pipeline prod # CI/CD pipeline execution
logline db verify-rls            # RLS policy verification gate
logline migrate review           # Schema diff before applying migrations
logline migrate apply --env prod # Apply migrations (requires review)
```

## Notes
- CLI loads `connections.toml` from `~/.config/logline` by default
- If config files are missing, falls back to an in-code demo catalog
- Supabase helper commands wrap the `supabase` CLI with Keychain token injection
