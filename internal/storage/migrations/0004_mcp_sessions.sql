-- MCP session replay: group audit events by an agent-supplied session id so
-- users can review (replay) every operation of one agent task in order.

ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS session_id TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_audit_logs_session ON audit_logs(session_id);

-- Server-issued temporary host authorizations for MCP agents. open_host_session
-- validates RBAC and mints a session; execute_command requires a live session
-- and every operation inside it lands in audit_logs with the same session_id.
CREATE TABLE IF NOT EXISTS mcp_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    server_id TEXT NOT NULL,
    purpose TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    closed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_mcp_sessions_user ON mcp_sessions(user_id);
