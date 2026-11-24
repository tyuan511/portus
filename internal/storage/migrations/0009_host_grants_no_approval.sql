-- Granular approval-bypass flags on host grants:
-- terminal_no_approval: skip human approval gate for interactive Web Terminal commands
-- mcp_no_approval: skip human approval gate for MCP execute_command calls
-- Default 0 (false): both channels go through approval unless explicitly configured.

ALTER TABLE host_grants ADD COLUMN IF NOT EXISTS terminal_no_approval INTEGER NOT NULL DEFAULT 0;
ALTER TABLE host_grants ADD COLUMN IF NOT EXISTS mcp_no_approval INTEGER NOT NULL DEFAULT 0;
