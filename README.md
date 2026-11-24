# Bulwark Enterprise

Multi-user, RBAC-hardened SSH operations platform: **Go server + PostgreSQL + embedded Web UI + MCP endpoint**.

Teams connect to granted hosts through an audited web terminal; AI agents operate the same
hosts through MCP — every command passes the layered security pipeline (L1 shell-AST rules →
optional L2 AI semantic review → human approval → execution with output masking) and lands in
a tamper-evident audit chain.

## Quick start

```bash
make build            # builds web/ then the single static binary
./bin/bulwark-server  # listens on 127.0.0.1:8787
```

The server talks to **PostgreSQL only**. Quick container for evaluation:

```bash
docker run -d --name bulwark-pg -e POSTGRES_PASSWORD=secret -e POSTGRES_USER=bulwark \
  -e POSTGRES_DB=bulwark -p 5432:5432 postgres:16-alpine
```

For a full containerized deployment see [docs/deploy.md](docs/deploy.md) (Docker Compose).

First boot bootstraps (idempotent):

- the initial admin account — password comes from `bootstrap.admin_password`,
  `bootstrap.admin_password_file`, or is **generated** and printed to stderr plus
  `~/.local/share/bulwark/bootstrap-admin-password.txt` (0600, delete after first login)
- the default L1 command rule set

Open http://127.0.0.1:8787, sign in as the bootstrap admin, change the password, then:
create **user groups**, grant **machine groups** to them in *Access*, and invite
**users** (role: admin or member). Members see exactly what is granted to them — nothing else.

## What's inside

| Area | Details |
|---|---|
| Auth | Argon2id passwords, DB-backed sessions (HttpOnly cookie, idle+absolute expiry, revocation on password change/disable), TOTP 2FA with hashed recovery codes, brute-force guard (per-user lockout + per-IP sliding window, persisted). Mandatory gates: accounts flagged `must_change_password` may only change their password, and with `security.enforce_totp` unenrolled accounts may only enroll |
| RBAC | Built-in `admin` / `member` roles (permission sets fixed in code, not editable), user groups granted machine groups with connect / execute / **transfer** levels (`can_transfer` grant bit); enforced on REST, websockets, and every MCP tool call |
| Terminal | Interactive xterm.js terminal over websocket. `terminal.enforce` routes every submitted line through L1/L2/approval before it reaches the remote shell (denied lines are wiped with Ctrl+U and explained in a notice); `terminal.record` records the full PTY stream (size-capped) for replay from the Audit page |
| File manager | Browser SFTP file management (browse / download / upload / mkdir / rename / delete) gated by the `host.transfer` permission plus a `can_transfer` grant; every operation is audited |
| Approvals | Live websocket approval queue for `ask_approval` commands, decision **history** endpoint, decisions carry an optional reason, requests/decisions mirrored to webhooks |
| Audit | Full hash chain (SHA-256 `prev_hash`), per-user attribution, admins see everything (members have no audit surface), chain verification endpoint, filterable `/api/audit-logs` (`{items,total}`), optional JSONL mirror file |
| MCP | Official Go SDK, streamable HTTP at `/mcp`, bearer PAT only (never cookies), stateless JSON mode, per-token rate limit, **14 tools** RBAC-scoped |
| SSH | ProxyJump chains with cycle detection, layered credential fallback, PTY terminals over websocket, exec with timeout + output masking, SFTP subsystem for file operations |
| Credentials | Per-host passwords/keys sealed with AES-256-GCM under the server key and stored in PostgreSQL — a stolen database dump alone is undecryptable |
| Notifications | Optional webhook for `approval.requested`, `approval.decided`, `policy.terminal_denied`, `policy.mcp_denied`, `file.uploaded` |
| Observability | Optional Prometheus `GET /metrics`, optional rotating log file (lumberjack) |
| Storage | PostgreSQL only — sqlc-generated queries, versioned migrations applied idempotently at boot, retention pruning for MCP sessions / terminal recordings / revoked API tokens |
| Web | React 19 + Vite + Tailwind v4 SPA (light/dark themes), embedded into the binary via `go:embed`, permission-aware navigation |

## Layout

```
cmd/bulwark-server/    entrypoint (-config <path>, -version)
internal/
  apierr/              typed error codes + unified HTTP envelope
  approvals/           human-in-the-loop broker with WS fan-out
  auth/                passwords, sessions, TOTP, PATs, brute-force guard
  buildinfo/           version injected at build time (-ldflags)
  config/              viper YAML + BULWARK_* env overrides
  domain/              shared models (snake_case JSON contract)
  httpserver/          fiber app, middleware, handlers, WS hubs, embedded SPA
  logging/             zap setup with optional rotating file output
  mcpserver/           MCP tools (14) + per-call identity
  metrics/             Prometheus registry + /metrics handler
  notify/              webhook notifier (fire-and-forget)
  rbac/                permission + host-grant evaluation (incl. transfer bit)
  security/            L1 engine (mvdan/sh), masking, L2 reviewers
  serverkey/           32-byte AEAD root key (server.key)
  sshpool/             connection pool, jump chains, exec, PTY, SFTP client
  storage/             sqlc queries (db/), domain mapping, audit hash chain
  webui/               go:embed target for web/dist
web/                   React frontend source
```

## Docs

- [docs/config.md](docs/config.md) — full configuration reference
- [docs/security.md](docs/security.md) — security model & hardening checklist
- [docs/mcp.md](docs/mcp.md) — connecting MCP clients with personal access tokens
- [docs/deploy.md](docs/deploy.md) — production deployment (TLS, reverse proxy, Docker)
- [docs/backup.md](docs/backup.md) — PostgreSQL backups, `server.key`, restore runbook

## Development

```bash
make dev    # go server on :8787 + vite dev server on :5173 (proxy /api,/ws,/mcp)
make test   # go test ./...
```

Legacy Rust/Tauri single-user implementation: archived on the `rust-legacy` branch.
