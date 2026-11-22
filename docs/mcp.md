# Connecting MCP Clients

The MCP endpoint is served at **`/mcp`** (streamable HTTP, stateless JSON mode).
It authenticates exclusively with **personal access tokens (PATs)** — browser sessions are
never accepted there, and tokens are never accepted by the web API.

## 1. Issue a token

Sign in to the web UI → **Settings → API Tokens** → create a token (name it after the client,
e.g. `claude-code`). The plaintext `sk-sshield-…` value is shown **once**. Tokens require the
`mcp.access` permission (members have it by default).

## 2. Configure your client

Any streamable-HTTP MCP client works. Authorization header + JSON responses:

```jsonc
// e.g. Claude Code / ZCode MCP config
{
  "mcpServers": {
    "sshield": {
      "type": "http",
      "url": "http://your-sshield-host:8787/mcp",
      "headers": { "Authorization": "Bearer sk-sshield-xxxxxxxx…" }
    }
  }
}
```

Requirements: `Content-Type: application/json` and
`Accept: application/json, text/event-stream` (the SDK enforces both).

## 3. Host sessions (required for execution)

`execute_command` no longer takes a server directly. The agent must first obtain a
**server-issued host session** — this is the temporary per-host authorization:

```jsonc
// 1. Request authorization for a host (RBAC-checked, TTL-bounded)
{"name": "open_host_session", "arguments": {"server_id": "srv-x", "purpose": "deploy v1.2"}}
// → {"session_id": "mcpsess-…", "expires_at": "…"}   (TTL: security.mcp_session_ttl_minutes, default 30)

// 2. Execute through the session (host derived from it)
{"name": "execute_command", "arguments": {"session_id": "mcpsess-…", "command": "uptime", "intent": "health check"}}

// 3. Optional early close
{"name": "close_host_session", "arguments": {"session_id": "mcpsess-…"}}
```

Sessions are rejected when expired, closed, foreign or when the underlying grant was
revoked (grants are re-checked at execution time). Every event — open, each execution,
close — carries the session id and appears in the web console's
**Audit → MCP Sessions** replay view (step / auto-play through the whole task).

## 4. Tools and how RBAC scopes them

| Tool | Scoping |
|---|---|
| `list_authorized_servers` | only hosts granted to the caller (connect or execute) |
| `get_server_details`, `test_server_connection` | connect grant required |
| `execute_command` | live host session (whose open required `host.execute` + execute grant) |
| `open_host_session`, `close_host_session` | `host.execute` permission **and** execute grant on that host |
| `evaluate_command`, `list_snippets`, `list_security_rules` | any authenticated caller |
| `query_audit_logs`, `get_audit_stats` | scoped to the caller's own trail unless `audit.view_all` |
| `verify_audit_chain` | `audit.view_all` (admin) |
| `list_pending_approvals` | `approvals.decide` (admin) |
| `get_l2_config` | any authenticated caller |
| — 14 tools total — | includes open/close_host_session and get_l2_config; vault tools were removed earlier |

`execute_command` runs the full pipeline: L1 AST rules → optional L2 review → human approval
(surfaced live in the web UI's Approvals page) → SSH execution with masked output → audit
record attributed to the token owner.

## 5. Operations

- Per-token rate limit: `security.mcp_rate_per_min` (default 60 req/min) → HTTP 429.
- Revoking a token (Settings → API Tokens) cuts off the client immediately.
- `last_used_at` is tracked per token; expired or revoked tokens return 401.
