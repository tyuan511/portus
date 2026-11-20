# Security Model

Threat focus: **the server is internet-adjacent and will be brute-forced; a single compromise
must not expose every service at once.**

## Defense layers

### Authentication
- Passwords hashed with **Argon2id** (32 MiB, t=2, p=2, PHC format). Unknown usernames burn an
  identical dummy verification to defeat timing-based account enumeration; login errors are generic.
- **Brute-force guard, persisted in SQLite** (survives restarts): per-username lockout after
  `login_max_attempts` failures for `lockout_minutes`; per-IP sliding-window block after
  `ip_max_attempts` failures inside `ip_window_minutes`. Every attempt is recorded and audited.
- **TOTP 2FA** (opt-in per user, `security.enforce_totp` to mandate): RFC 6238, ±1 step window,
  8 one-time recovery codes stored as SHA-256 hashes and consumed on use. TOTP secrets are
  encrypted at rest with the 0600 `server.key` (AES-256-GCM).
- Admin password resets issue a one-time temporary password and force a change at next login.

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
  everything; `member` only uses hosts and MCP. Host access requires an explicit **grant**
  of a machine group to a user group with `connect` / `execute` levels, evaluated
  server-side on every REST call, websocket upgrade, and MCP tool invocation. Members
  cannot even list ungranted hosts, and host payloads omit all credential metadata.
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
   responds via the live websocket feed, or times out.
4. **Execution** — masked output (private keys, AWS keys, `sk-` tokens, Bearer tokens,
   `PASSWORD=…` env lines, DB connection strings) before anything reaches agents or audits.

### Audit integrity
- Every execution, login (success and failure), admin action, vault operation and approval
  decision is appended to a **SHA-256 hash chain** (`prev_hash` → `event_hash`). Verification
  endpoint recomputes the chain and pinpoints the first tampered record.
- Records carry `user_id` + `username`; members can only query their own trail, admins see all.
- The audit log has no delete/update API.

### Secrets at rest
- Per-host credentials (SSH passwords / private keys) are sealed with **AES-256-GCM under
  the server key** and stored in PostgreSQL — a stolen database dump alone is mathematically
  undecryptable. The credential id is bound as AEAD associated data so rows cannot be swapped.
- `server.key` is a 32-byte random file (0600) created on first boot; it also seals TOTP
  enrollment secrets. Plaintext secrets exist only in process memory during SSH auth.
- Trade-off to understand: anyone who compromises the server host (filesystem + API) can
  decrypt credentials — that is inherent to server-side-managed credentials. The former
  master-passphrase vault was removed in favor of this zero-friction model; recovery of the
  "separation" requires an external KMS/HSM integration.

## Residual risks & recommendations
- Run behind a TLS-terminating reverse proxy (see deploy.md); enable `tls_cert`/`tls_key` if bare.
- Keep `bind` on a management network or localhost when possible.
- Set `vault.unlock_passphrase_file` only on headless single-purpose hosts; otherwise keep the
  vault locked and unlock on demand — a compromised server still cannot read credentials while
  the vault is locked.
- Back up `sshield.db` + `vault.enc` + `server.key` together; losing `server.key` invalidates
  enrolled TOTP secrets (users re-enroll, no password impact).
- Host-key verification is currently accept-and-log (TOFU store planned); place SSH targets on
  a trusted network path or add strict checking before exposing to hostile networks.
