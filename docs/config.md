# Configuration Reference

The server binary takes `-config <path>` (YAML/TOML) and `-version` (prints the build
version and exits). Without `-config`, the file is discovered via `$SSHIELD_CONFIG`,
then `<data_dir>/sshield-server.yaml`. A legacy `sshield-server.toml` in the data dir
is still discovered as a fallback. A missing file is not an error — defaults apply.
Copy `sshield-server.yaml.example` as a starting point.

Environment overrides use the `SSHIELD_` prefix with dots mapped to underscores
(e.g. `security.lockout_minutes` → `SSHIELD_SECURITY_LOCKOUT_MINUTES`,
`terminal.enforce` → `SSHIELD_TERMINAL_ENFORCE`).
Legacy singles still work: `SSHIELD_ADDR`, `SSHIELD_DB_URL`, `SSHIELD_AUDIT_JSONL`.

## `server`

| Key | Default | Notes |
|---|---|---|
| `bind` | `127.0.0.1:8787` | Listen address. Expose on `0.0.0.0` only behind TLS (see deploy.md) |
| `allowed_origins` | `[]` | Browser origins for cross-origin API access. Empty = same-origin only (recommended) |
| `tls_cert`, `tls_key` | `""` | Optional built-in HTTPS; otherwise terminate TLS at your reverse proxy |

## `storage`

| Key | Default | Notes |
|---|---|---|
| `db_url` | `postgres://sshield:sshield@127.0.0.1:5432/sshield?sslmode=disable` | PostgreSQL connection string (pgx URL) |
| `data_dir` | `~/.local/share/sshield` | Home for the server key, bootstrap hint and (by default) the log file |
| `audit_jsonl` | `""` | Optional JSONL mirror of the audit chain (0600 file, appended per event) |

PostgreSQL is the only supported backend; all data access goes through
**sqlc-generated queries** (`internal/storage/db`), with the schema embedded in
the binary (`internal/storage/migrations`, applied idempotently at boot).
Create the database before first boot — tables are created automatically:

```sql
CREATE DATABASE sshield;
-- or with a dedicated role:
CREATE ROLE sshield LOGIN PASSWORD 'secret';
CREATE DATABASE sshield OWNER sshield;
```

## `bootstrap` (first boot only, when the users table is empty)

| Key | Default | Notes |
|---|---|---|
| `admin_username` | `admin` | Initial admin account name |
| `admin_password` | `""` | Empty = generate a random 20-char password, print to stderr and write `bootstrap-admin-password.txt` (0600). An explicit password does **not** force a change at first login |
| `admin_password_file` | `""` | Read the initial password from a 0600 file instead |

The generated password forces a change at first login. Existing databases are never re-bootstrapped.

## `security`

| Key | Default | Notes |
|---|---|---|
| `session_idle_minutes` | `720` | Idle session timeout (min 5) |
| `session_max_hours` | `168` | Absolute session lifetime |
| `login_max_attempts` | `5` | Failed attempts (persisted) before username lockout |
| `lockout_minutes` | `15` | Username lockout window |
| `ip_max_attempts` / `ip_window_minutes` | `20` / `60` | Per-IP sliding-window block |
| `enforce_totp` | `false` | **Enforced when set**: after login, accounts without an enrolled TOTP may only enroll TOTP (plus view `me`, logout, change password) until enrollment completes |
| `mcp_rate_per_min` | `60` | Requests per API token per minute on `/mcp` |
| `mcp_session_ttl_minutes` | `30` | Lifetime of each server-issued MCP host session |
| `token_ttl_hours` | `0` | Default API token lifetime (0 = no expiry) |

## `terminal`

| Key | Default | Notes |
|---|---|---|
| `enforce` | `false` | Route every submitted interactive-terminal line through the L1/L2/approval pipeline before it reaches the remote shell. Input is line-buffered server-side; remote tab completion is not available in this mode. Default (false) = audit-only: record but do not block |
| `record` | `true` | Capture the full PTY output stream (plus submitted lines) for replay from the Audit page |
| `record_max_bytes` | `10485760` (10 MiB) | Cap the recorded output per session; beyond it the recording is truncated with a marker |

## `retention` (checked by the internal cleanup loop every 10 minutes)

| Key | Default | Notes |
|---|---|---|
| `mcp_session_days` | `0` | Prune MCP host-session registry rows older than N days. `0` = keep forever |
| `terminal_days` | `0` | Prune recorded terminal sessions (and their events) older than N days. `0` = keep forever |
| `api_token_days` | `0` | Prune revoked/expired API tokens older than N days. `0` = keep forever |

Independent of these: expired web sessions and stale login-attempt records are always
pruned (login attempts are kept for 24 hours). Audit events are never pruned.

## `logging`

| Key | Default | Notes |
|---|---|---|
| `file` | `""` | Additionally mirror logs to a rotating file (lumberjack). Empty = stdout only. Relative paths resolve against `storage.data_dir` |
| `max_size_mb` | `100` | Max size of each rotated log file |
| `max_backups` | `7` | Number of rotated files to retain |
| `max_age_days` | `28` | Max age of rotated files |

## `metrics`

| Key | Default | Notes |
|---|---|---|
| `enabled` | `false` | Expose `GET /metrics` in Prometheus text format (unauthenticated; scrape from localhost or your proxy) |

## `notifications`

| Key | Default | Notes |
|---|---|---|
| `webhook_url` | `""` | Webhook receiving JSON POSTs (`{type, time, summary, fields}`) for `approval.requested`, `approval.decided`, `policy.terminal_denied`, `policy.mcp_denied`, `file.uploaded`. Empty = disabled. Delivery is fire-and-forget: failures are logged, never surfaced to callers |
| `timeout_secs` | `5` | Timeout for each webhook POST |

## Data files (all 0600)

- `server.key` — 32-byte AEAD key sealing per-host SSH credentials and TOTP secrets at
  rest; the **single encryption root**. Back it up together with the database (see
  [backup.md](backup.md)); losing it makes stored credentials and enrolled TOTP secrets
  unrecoverable
- `bootstrap-admin-password.txt` — one-time hint, delete after first login
