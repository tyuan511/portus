# Security Model

Threat focus: **the server is internet-adjacent and will be brute-forced; a single compromise
must not expose every service at once.**

## Defense layers

### Authentication
- Passwords hashed with **Argon2id** (32 MiB, t=2, p=2, PHC format). Unknown usernames burn an
  identical dummy verification to defeat timing-based account enumeration; login errors are generic.
- **Brute-force guard, persisted in PostgreSQL** (survives restarts): per-username lockout after
  `login_max_attempts` failures for `lockout_minutes`; per-IP sliding-window block after
  `ip_max_attempts` failures inside `ip_window_minutes`. Every attempt is recorded and audited.
- **TOTP 2FA** (opt-in per user): RFC 6238, ±1 step window, 8 one-time recovery codes stored as
  SHA-256 hashes and consumed on use. TOTP secrets are encrypted at rest with the 0600
  `server.key` (AES-256-GCM). With `security.enforce_totp = true` the gate is **active**: after
  login, accounts that have not enrolled may only enroll TOTP (plus view `me`, logout, change
  password) until enrollment completes.
- Admin password resets issue a one-time temporary password and force a change at next login.
- **Mandatory gates after login**: accounts flagged `must_change_password` may only change their
  password (plus view `me`, logout, UI preference settings) until they do. Both gates are
  enforced server-side on every API call, not just in the UI.

### Sessions & tokens
- Web sessions: 256-bit opaque tokens, **only SHA-256 hashes stored**, HttpOnly + SameSite=Lax
  cookies, Secure flag when TLS, idle + absolute expiry, rotation at login.
- Password change, admin deactivation, and role changes **revoke all live sessions** of that user.
- Users can inspect and kill their own sessions; admins can force sign-out anyone.
- MCP access uses **separate personal access tokens** (`sk-sshield-…`, SHA-256 at rest, shown
  once, revocable, expiring, `last_used` tracked). Web cookies are never accepted on `/mcp`;
  tokens are never accepted on the web API. Blast radius of a leaked browser session and a
  leaked agent token are therefore disjoint.

### Authorization (RBAC)
- Two built-in roles with permission sets fixed in code (not editable): `admin` manages
  everything; `member` uses hosts and MCP (read/connect/execute/transfer + `mcp.access`).
  Host access requires an explicit **grant** of a machine group to a user group with
  `connect` / `execute` levels, evaluated server-side on every REST call, websocket upgrade,
  and MCP tool invocation. Members cannot even list ungranted hosts, and host payloads omit
  all credential metadata.
- **File transfer is authorized separately**: every SFTP operation additionally requires the
  `host.transfer` permission (members have it by default) **and** a `can_transfer` grant bit
  on the machine-group grant. `connect`/`execute` alone never allow file transfers; admins
  bypass the grant bit but keep the permission requirement.
- Deactivating a user or changing their role immediately drops their sessions.

### Transport & browser attack surface
- CORS disabled by default (same-origin only); configurable origin allowlist when needed.
- CSRF defense-in-depth: mutating requests must present a same-origin `Origin`/`Referer`
  header on top of SameSite=Lax cookies.
- Security headers: `X-Content-Type-Options`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`,
  `Cache-Control: no-store` on API responses.

### Command pipeline (blast-radius containment)
1. **L1** — bash AST parse (mvdan/sh, fail-closed: parse failure = deny), rule matching with
   priority + strictness ordering, write-redirection escalation, wrapper unwrapping (sudo/doas/
   nohup/env/command/time), full recursion into `$(…)`, backticks, process substitution and
   parameter expansions. Unknown commands default to **ask_approval**.
2. **L2** — optional AI semantic review (OpenAI/Anthropic/SystemOne protocols) with risk
   thresholds; any provider failure fails closed to ask_approval.
3. **Human approval** — ask_approval commands block until an admin with `approvals.decide`
   responds via the live websocket feed, or times out. Decisions (including a reason) are
   audited and visible in the approval history.
4. **Execution** — masked output (private keys, AWS keys, `sk-` tokens, Bearer tokens,
   `PASSWORD=…` env lines, DB connection strings) before anything reaches agents or audits.

### Interactive terminal
- **`terminal.enforce = true`**: the websocket input stream is line-buffered server-side and
  every Enter-terminated line is pushed through the same L1/L2/approval pipeline as MCP
  executions before reaching the remote shell. A denied line is **never executed**: the remote
  line buffer is killed with an injected Ctrl+U (0x15) — the Enter byte is withheld — and a
  notice with the denial reason is shown in the terminal. Multi-line pastes are evaluated line
  by line, in order. Trade-off: remote tab completion / line editing are not available in this
  mode (typed characters are still echoed and forwarded).
- **`terminal.enforce = false` (default)**: audit-only — everything is recorded and audited but
  nothing is blocked.
- **`terminal.record = true` (default)**: the full PTY output stream plus submitted lines are
  recorded (capped at `terminal.record_max_bytes`, then truncated) into
  `terminal_sessions`/`terminal_events` for replay from the Audit page.

### Audit integrity
- Every execution (MCP and terminal), file operation, login (success and failure), admin action
  and approval decision is appended to a **SHA-256 hash chain** (`prev_hash` → `event_hash`).
  Verification endpoint recomputes the chain and pinpoints the first tampered record.
- Records carry `user_id` + `username`; members can only query their own trail, admins see all.
- The audit log has no delete/update API. Enable `storage.audit_jsonl` to mirror every event to
  a 0600 JSONL file as an off-database copy (see [backup.md](backup.md)).

### Secrets at rest
- Per-host credentials (SSH passwords / private keys) are sealed with **AES-256-GCM under
  `server.key`** and stored in PostgreSQL — a stolen database dump alone is mathematically
  undecryptable. The credential id is bound as AEAD associated data so rows cannot be swapped.
- `server.key` is the **single encryption root**: a 32-byte random file (0600) created on first
  boot that seals both SSH credentials and TOTP enrollment secrets. Plaintext secrets exist only
  in process memory during SSH auth.
- **Losing `server.key` makes every stored credential and enrolled TOTP secret unrecoverable**
  (users would have to re-enter credentials and re-enroll TOTP). Back it up together with the
  database — see [backup.md](backup.md) for the procedure and why the key and the DB dump should
  not be stored in the same place.
- Trade-off to understand: anyone who compromises the server host (filesystem + API) can
  decrypt credentials — that is inherent to server-side-managed credentials. Recovery of the
  "separation" requires an external KMS/HSM integration.

## Residual risks & recommendations
- Run behind a TLS-terminating reverse proxy (see deploy.md); enable `tls_cert`/`tls_key` if bare.
- Keep `bind` on a management network or localhost when possible.
- Back up the PostgreSQL database and `server.key` (0600) regularly; keep the key copy separate
  from the DB dump (see [backup.md](backup.md)).
- Host-key verification is currently accept-and-log (TOFU store planned); place SSH targets on
  a trusted network path or add strict checking before exposing to hostile networks.
- `GET /metrics` (when `metrics.enabled`) is unauthenticated — scrape it from localhost or
  restrict it at the reverse proxy.
