# Supabase Foundation (System of Record)

This document defines the definitive persistence target for the LogLine/UBLX ecosystem.

## Decision

- Primary database: Supabase Postgres.
- Primary object storage: Supabase Storage buckets.
- Edge/local cache per device: local SQLite (Rust daemon/UI cache + outbox sync).
- Neon remains optional for experiments, not production source-of-truth.

Current Supabase project URL:

- `https://aypxnwofjtdnmtxastti.supabase.co`

## One Project or Many

- Start with one production project and one staging project.
- Inside each project, isolate domains by schema:
  - `core`
  - `observability`
  - `billing`
  - `registry`

This keeps operations simple while preserving strong separation.

## Environment Setup for This App

The app already uses server-side Postgres via `DATABASE_URL`.

1. In Supabase dashboard, copy the Postgres connection string.
2. Set `.env.local`:

```bash
DATABASE_URL="postgresql://postgres.<project-ref>:<password>@<host>:5432/postgres?sslmode=require"
DATABASE_URL_UNPOOLED="postgresql://postgres.<project-ref>:<password>@<host>:5432/postgres?sslmode=require"
DEFAULT_WORKSPACE_ID="default"
```

Notes:

- Use pooled URL for runtime and unpooled URL for migrations if your dashboard provides both.
- Do not commit real secrets.

## Initial SQL Bootstrap (schemas + extensions)

Run in Supabase SQL editor (safe, idempotent):

```sql
create extension if not exists "pgcrypto";

create schema if not exists core;
create schema if not exists observability;
create schema if not exists billing;
create schema if not exists registry;
```

## Migration Strategy from Current State

1. Point `DATABASE_URL` to Supabase and run existing schema bootstrap.
2. Confirm tabs/components/settings read/write paths.
3. Add schema-qualified table placement in next migration wave:
   - app/tabs/components/settings in `core`
   - runtime events/status in `observability`
   - usage/cost rows in `billing`
   - registry metadata in `registry`
4. Keep local SQLite as device outbox/cache and sync to Supabase.

## Storage Convention

Use buckets with workspace prefixes:

- Bucket: `artifacts`
  - `ws/<workspace_id>/artifacts/...`
- Bucket: `logs`
  - `ws/<workspace_id>/logs/...`
- Bucket: `exports`
  - `ws/<workspace_id>/exports/...`

Keep metadata rows in Postgres with object path + checksum/CID references.

## Security Baseline

- Enforce RLS on all shared tables.
- Every row includes `workspace_id`.
- API uses server-side DB access and workspace scoping headers.
- Never expose service role key to frontend clients.

## Apply RBAC + RLS SQL

File:

- `db/sql/001_supabase_rbac_rls.sql`

How:

1. Open Supabase SQL editor for project `aypxnwofjtdnmtxastti`.
2. Run the SQL file.
3. Verify policies exist under Database -> Policies.

Important:

- The SQL expects user/app context from either JWT claims (`sub`, `workspace_id`, `app_id`) or session settings (`app.current_user_id`, `app.current_workspace_id`, `app.current_app_id`).
- For strict DB-level enforcement in runtime, use a non-bypass-RLS DB role for app connections.
