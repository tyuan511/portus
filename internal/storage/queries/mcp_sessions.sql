-- name: CreateMCPSession :exec
INSERT INTO mcp_sessions (id, user_id, server_id, purpose, created_at, expires_at)
VALUES ($1, $2, $3, $4, $5, $6);

-- name: GetMCPSession :one
SELECT * FROM mcp_sessions WHERE id = $1;

-- name: CloseMCPSession :execrows
UPDATE mcp_sessions SET closed_at = $3
WHERE id = $1 AND user_id = $2 AND closed_at IS NULL;
