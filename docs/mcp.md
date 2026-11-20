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

## 3. Tools and how RBAC scopes them

| Tool | Scoping |
|---|---|
| `list_authorized_servers` | only hosts granted to the caller (connect or execute) |
| `get_server_details`, `test_server_connection` | connect grant required |
| `execute_command` | `host.execute` permission **and** execute grant on that host |
| `evaluate_command`, `list_snippets`, `list_security_rules` | any authenticated caller |
| `query_audit_logs`, `get_audit_stats` | scoped to the caller's own trail unless `audit.view_all` |
| `verify_audit_chain` | `audit.view_all` (admin) |
| `list_pending_approvals` | `approvals.decide` (admin) |
| `get_vault_status`, `list_vault_credentials` | `vault.manage` (admin) |

`execute_command` runs the full pipeline: L1 AST rules → optional L2 review → human approval
(surfaced live in the web UI's Approvals page) → SSH execution with masked output → audit
record attributed to the token owner.

## 4. Operations

- Per-token rate limit: `security.mcp_rate_per_min` (default 60 req/min) → HTTP 429.
- Revoking a token (Settings → API Tokens) cuts off the client immediately.
- `last_used_at` is tracked per token; expired or revoked tokens return 401.
