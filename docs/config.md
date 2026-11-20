# Configuration Reference

Config file: YAML (`sshield-server.yaml`). Discovery order:
`--config <path>` / `$SSHIELD_CONFIG` / `<data_dir>/sshield-server.yaml`.
A legacy `sshield-server.toml` in the data dir is still discovered as a fallback.
A missing file is not an error — defaults apply. Copy `sshield-server.yaml.example` as a starting point.

Environment overrides use the `SSHIELD_` prefix with dots mapped to underscores
(e.g. `security.lockout_minutes` → `SSHIELD_SECURITY_LOCKOUT_MINUTES`).
Legacy singles still work: `SSHIELD_ADDR`, `SSHIELD_DB_URL`, `SSHIELD_AUDIT_JSONL`, `SSHIELD_VAULT_PATH`.

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
| `data_dir` | `~/.local/share/sshield` | Home for the vault, server key, bootstrap hint |
| `audit_jsonl` | `""` | Optional JSONL mirror of the audit chain |

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

## `vault`

| Key | Default | Notes |
|---|---|---|
| `auto_lock_secs` | `900` | Idle auto-lock for the master key (min 60) |
| `unlock_passphrase_file` | `""` | 0600 file whose contents auto-unlock the vault at startup (headless). Leave empty for manual admin unlock via the web UI |

## `bootstrap` (first boot only, when the users table is empty)

| Key | Default | Notes |
|---|---|---|
| `admin_username` | `admin` | Initial admin account name |
| `admin_password` | `""` | Empty = generate a random 20-char password, print to stderr and write `bootstrap-admin-password.txt` (0600) |
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
| `enforce_totp` | `false` | Require 2FA enrollment at login |
| `mcp_rate_per_min` | `60` | Requests per API token per minute on `/mcp` |
| `token_ttl_hours` | `0` | Default API token lifetime (0 = no expiry) |

## Data files (all 0600)

- `vault.enc` — sealed credentials (AES-256-GCM + Argon2id)
- `server.key` — 32-byte AEAD key encrypting TOTP secrets at rest; **back this up together with the DB**
- `bootstrap-admin-password.txt` — one-time hint, delete after first login
