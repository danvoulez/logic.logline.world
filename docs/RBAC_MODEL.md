# RBAC Model (Tenant + App + User)

This app now enforces access at three levels:

1. `tenant` (mapped to `workspace_id`)
2. `app` (`app_id`)
3. `user` (`user_id`)

## Roles

Tenant role:

- `member`
- `admin`

App role:

- `member`
- `app_admin`

## Effective rules

- User must belong to at least one tenant.
- User must be a member of the active tenant.
- User must be a member of the active app.
- `member` cannot mutate app/tenant data.
- Private reads (settings secrets/effective bindings/instance private config) require `app_admin`.
- Tenant `admin` alone does not grant private read.

## Headers used by API

- `x-workspace-id`: tenant/workspace context
- `x-app-id`: app context
- `x-user-id`: user context

Client hooks now send all three headers by default.

## Development bootstrap

In non-strict mode, a local dev principal is auto-bootstrapped:

- user: `local-dev`
- app: `ublx`
- tenant: current workspace id
- roles: tenant `admin`, app `app_admin`

Set `RBAC_STRICT=1` to disable this bootstrap and require real memberships.

