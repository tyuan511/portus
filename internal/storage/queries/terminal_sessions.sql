-- name: InsertTerminalSession :exec
INSERT INTO terminal_sessions (id, user_id, username, server_id, server_name, ssh_user,
    server_host, client_ip, policy_mode, opened_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10);

-- name: AppendTerminalEvent :exec
INSERT INTO terminal_events (session_id, ts_ms, direction, data)
VALUES ($1, $2, $3, $4);

-- name: CloseTerminalSession :execrows
UPDATE terminal_sessions SET closed_at = $2, close_reason = $3,
    byte_size = $4, event_count = $5, truncated = $6
WHERE id = $1;

-- name: GetTerminalSession :one
SELECT * FROM terminal_sessions WHERE id = $1;

-- name: ListTerminalSessions :many
-- NULL user_id = admin scope (audit.view_all); otherwise own sessions only.
SELECT * FROM terminal_sessions
WHERE (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('username')::text IS NULL OR username = sqlc.narg('username')::text)
ORDER BY opened_at DESC
LIMIT sqlc.narg('lim')::bigint
OFFSET sqlc.narg('offs')::bigint;

-- name: CountTerminalSessions :one
SELECT COUNT(*) FROM terminal_sessions
WHERE (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('username')::text IS NULL OR username = sqlc.narg('username')::text);

-- name: SelectTerminalEvents :many
SELECT seq, ts_ms, direction, data FROM terminal_events
WHERE session_id = $1 ORDER BY seq ASC;

-- name: CleanupTerminalSessionsBefore :execrows
-- Recordings expire as a unit; terminal_events rows go with them via cascade.
DELETE FROM terminal_sessions WHERE opened_at < $1;
