# SSHield Enterprise

Multi-user, RBAC-hardened SSH operations platform: **Go server + embedded Web UI + MCP endpoint**.

Teams connect to granted hosts through an audited web terminal; AI agents operate the same
hosts through MCP — every command passes the layered security pipeline (L1 shell-AST rules →
optional L2 AI semantic review → human approval → execution with output masking) and lands in
a tamper-evident audit chain.

## Quick start

```bash
make build            # builds web/ then the single static binary
./bin/sshield-server  # listens on 127.0.0.1:8787
```

Requires PostgreSQL (the only backend). Quick container for evaluation:

```bash
docker run -d --name sshield-pg -e POSTGRES_PASSWORD=secret -e POSTGRES_USER=sshield \
  -e POSTGRES_DB=sshield -p 5432:5432 postgres:16-alpine
```

First boot bootstraps (idempotent):

- the initial admin account — password comes from `bootstrap.admin_password`,
  `bootstrap.admin_password_file`, or is **generated** and printed to stderr plus
  `~/.local/share/sshield/bootstrap-admin-password.txt` (0600, delete after first login)
- the default L1 command rule set

Open http://127.0.0.1:8787, sign in as the bootstrap admin, change the password, then:
create **user groups**, grant **machine groups** to them in *Access*, and invite
**users** (role: admin or member). Members see exactly what is granted to them — nothing else.

## What's inside

| Area | Details |
|---|---|
| Auth | Argon2id passwords, DB-backed sessions (HttpOnly cookie, idle+absolute expiry, revocation on password change/disable), TOTP 2FA with hashed recovery codes, brute-force guard (per-user lockout + per-IP sliding window, persisted) |
| RBAC | Built-in `admin` / `member` roles (permission sets fixed in code, not editable), user groups granted machine groups with connect/execute levels; enforced on REST, websockets, and every MCP tool call |
| Audit | Full hash chain (SHA-256 `prev_hash`), per-user attribution, admins see everything (members have no audit surface), chain verification endpoint |
| MCP | Official Go SDK, streamable HTTP at `/mcp`, bearer PAT only (never cookies), stateless JSON mode, per-token rate limit, 12 tools RBAC-scoped |
| SSH | ProxyJump chains with cycle detection, credential fallback vault→identity-file→agent→default keys, PTY terminals over websocket, exec with timeout + output masking |
| Credentials | Per-host passwords/keys sealed with AES-256-GCM under the server key and stored in PostgreSQL — a stolen database dump alone is undecryptable |
| Databases | SQLite (default, cgo-free), MySQL, PostgreSQL — same schema, versioned migrations, chosen by config |
| Web | React 19 + Vite + Tailwind v4 SPA, 17 themes, embedded into the binary via `go:embed`, xterm.js terminal, permission-aware navigation |

## Layout

```
cmd/sshield-server/    entrypoint
internal/
  config/              viper YAML + SSHIELD_* env overrides
  domain/              shared models (snake_case JSON contract)
  storage/             sqlc queries (db/), domain mapping, audit hash chain
  vault/               sealed credential vault
  security/            L1 engine (mvdan/sh), masking, L2 reviewers
  sshpool/             connection pool, jump chains, exec, PTY
  auth/                passwords, sessions, TOTP, PATs, brute-force guard
  rbac/                permission + host-grant evaluation
  approvals/           human-in-the-loop broker with WS fan-out
  mcpserver/           MCP tools + per-call identity
  httpserver/          fiber app, middleware, handlers, WS hubs, embedded SPA
  webui/               go:embed target for web/dist
web/                   React frontend source
```

## Docs

- [docs/config.md](docs/config.md) — full configuration reference
- [docs/security.md](docs/security.md) — security model & hardening checklist
- [docs/mcp.md](docs/mcp.md) — connecting MCP clients with personal access tokens
- [docs/deploy.md](docs/deploy.md) — production deployment (TLS, reverse proxy)

## Development

```bash
make dev    # go server on :8787 + vite dev server on :5173 (proxy /api,/ws,/mcp)
make test   # go test ./...
```

Legacy Rust/Tauri single-user implementation: archived on the `rust-legacy` branch.
