# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 2026-11-21

### Added

- Terminal policy enforcement: submitted interactive terminal lines are routed
  through the L1/L2/approval pipeline (`terminal.enforce`), with full TTY output
  recording and replay from the Audit page (`terminal.record`).
- SFTP file manager in the web UI (browse, upload, download, transfer authorization).
- Audit log filtering, export, pagination and a statistics dashboard.
- Approval history, including the requester's reason on each request.
- `GET /metrics` Prometheus endpoint (`metrics.enabled`, off by default).
- Webhook notifications for approval, policy-denial and file-transfer events
  (`notifications.webhook_url`).
- Docker packaging: multi-stage `Dockerfile`, reference `docker-compose.yml`
  (PostgreSQL 16 + bulwark-server) and GitHub Actions CI.
- CLI: `-config <path>` flag for explicit configuration files and `-version`
  to print the build version (also injected via ldflags into
  `internal/buildinfo.Version` and reported by `GET /api/health`).
- Password/TOTP enforcement gates for sensitive operations.
- Credential rename plus a dedicated credentials management page.
- Tab persistence across reloads in the web UI.
- Terminal UX: in-session search, adjustable font size, clipboard integration
  and automatic reconnect.
- `host.transfer` authorization bit controlling SFTP transfer permission.
- Retention policies (`retention.mcp_session_days`, `retention.terminal_days`,
  `retention.api_token_days`) for pruning old sessions and revoked tokens.
- Rotating on-disk log files (`logging.file` with size/backup/age limits).
- Backup & restore runbook ([docs/backup.md](docs/backup.md)): PostgreSQL dumps
  and `server.key` handling.

### Fixed

- `auth_attempts` cleanup window bug: brute-force counters were wiped every
  10 minutes instead of expiring per the lockout window.
- Bootstrap with an explicit admin password no longer marks the account for a
  forced password change.
- IP rate-limit buckets are no longer cleared wholesale (per-IP windows are
  preserved).
- Interactive terminals could bypass the policy engine; every submitted line is
  now evaluated.
- `PUT /api/servers/:id` no longer silently creates a server when the id does
  not exist.
- Grants can no longer reference non-existent machine groups.

### Changed

- `GET /api/audit-logs` and `GET /api/mcp-sessions` now return a pagination
  envelope (items + total + cursor) instead of bare arrays.
- L2 protocol enum corrected to `openai_chat` / `openai_responses`.
- Terminal WebSocket `ready` frame now carries a `policy` field, and a new
  `notice` frame type signals server-side policy/approval events.

### Removed

- Dead vault configuration keys and unreachable vault code paths.
- Native `confirm`/`prompt` browser dialogs (replaced by in-app dialogs).
