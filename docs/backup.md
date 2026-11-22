# Backup & Restore

SSHield keeps its state in exactly two places, and both must be backed up:

1. **The PostgreSQL database** — hosts, credentials rows (encrypted), users, grants, sessions,
   audit chain, MCP host sessions, terminal recordings.
2. **`server.key`** — the 32-byte AEAD root key in `storage.data_dir` (default
   `~/.local/share/sshield`, 0600). It decrypts the credential rows and TOTP secrets inside
   that database.

Everything else in the data dir is disposable (`bootstrap-admin-password.txt` is a one-time hint).

## 1. Database backup (pg_dump)

```bash
pg_dump --format=custom --file=/secure/backups/sshield-$(date +%F).dump \
  "postgres://sshield:secret@127.0.0.1:5432/sshield?sslmode=disable"
```

- The custom format supports `pg_restore`, parallel restore and selective restore.
- **Schema layout**: all SSHield tables live in the default `public` schema (the embedded
  migrations issue plain `CREATE TABLE` statements) — no extra `--schema` handling needed.
- Schedule it with cron/systemd timers; keep dumps offline if the server is internet-adjacent.
- Terminal recordings (`terminal_sessions` / `terminal_events`) are usually the largest tables.
  Either accept the size or bound them with `retention.terminal_days` before backing up.

## 2. `server.key` backup

```bash
cp -p ~/.local/share/sshield/server.key /secure/backups/sshield-server.key
chmod 600 /secure/backups/sshield-server.key
```

- Keep the mode at **0600** and own it by the service user when restoring.
- **Losing `server.key` is fatal for secrets**: every stored SSH credential and enrolled TOTP
  secret becomes undecryptable. Passwords, sessions, grants and the audit chain survive, but
  hosts must have their credentials re-entered and users re-enroll TOTP. There is no recovery.
- **Do not store the key copy in the same place as the DB dump.** The dump alone is
  undecryptable by design; a dump plus its key together equals full credential compromise.
  Separate locations (ideally separate systems) preserve that boundary.

## 3. Restore runbook (new machine)

1. **Install the binary** (and the data dir, e.g. `/var/lib/sshield`).
2. **Restore `server.key`** into the data dir first:
   ```bash
   install -m 600 -o sshield -g sshield /secure/backups/sshield-server.key \
     /var/lib/sshield/server.key
   ```
3. **Start PostgreSQL** and create the role/database if fresh:
   ```sql
   CREATE ROLE sshield LOGIN PASSWORD 'secret';
   CREATE DATABASE sshield OWNER sshield;
   ```
4. **Restore the dump** into the empty database:
   ```bash
   pg_restore --dbname="postgres://sshield:secret@127.0.0.1:5432/sshield?sslmode=disable" \
     --no-owner --role=sshield /secure/backups/sshield-2026-01-31.dump
   ```
5. **Start sshield-server.** Boot applies pending schema migrations automatically (a dump from
   an older release restores cleanly and then upgrades); the bootstrap sequence is a no-op
   because the users table is already populated.
6. **Verify**: `GET /api/health` returns `status: ok` plus the build `version`; sign in and run
   *Audit → Verify chain* to confirm the hash chain came through intact.

Restoring a database **without** `server.key` yields a server that starts but cannot decrypt
any credential or TOTP secret — treat the key as the more critical of the two artifacts.

## 4. Retention

Backups only stay manageable if growth is bounded. Review in `sshield-server.yaml`:

- `retention.terminal_days` — recorded PTY sessions (usually the bulk of the data)
- `retention.mcp_session_days` — MCP host-session registry rows
- `retention.api_token_days` — revoked/expired API tokens

`0` = keep forever. Expired web sessions and login-attempt records (>24h) are always pruned
internally; audit events are never pruned.

## 5. Audit JSONL mirror (extra insurance)

Set `storage.audit_jsonl` to a file path and every audit event is appended (0600) to it as it
is chained into the database. This off-database copy:

- survives total database loss (a dump only helps if you took one *after* the events happened),
- is plain JSON, one event per line, readable with `jq`/grep when the DB is unavailable,
- does **not** replace `pg_dump` — it carries no users/grants/hosts, and the verification
  endpoint recomputes the chain inside the database. Treat it as an independent evidentiary
  copy of the audit trail, and back the file up like any other log.

Combine all three — scheduled `pg_dump`, an off-host `server.key` copy, and the JSONL mirror —
for a restore story that covers routine failure, host loss, and post-incident forensics.
