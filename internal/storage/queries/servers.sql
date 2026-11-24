-- name: UpsertServer :exec
INSERT INTO servers (id, name, host, port, username, auth_kind, identity_file, credential_id,
    jump_host_id, jump_mode, jump_su_username, jump_su_credential_id,
    group_name, tags_json, description, enabled, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
ON CONFLICT (id) DO UPDATE SET
    name = excluded.name, host = excluded.host, port = excluded.port, username = excluded.username,
    auth_kind = excluded.auth_kind, identity_file = excluded.identity_file,
    credential_id = excluded.credential_id, jump_host_id = excluded.jump_host_id,
    jump_mode = excluded.jump_mode, jump_su_username = excluded.jump_su_username,
    jump_su_credential_id = excluded.jump_su_credential_id,
    group_name = excluded.group_name, tags_json = excluded.tags_json,
    description = excluded.description, enabled = excluded.enabled,
    updated_at = excluded.updated_at;

-- name: GetServer :one
SELECT * FROM servers WHERE id = $1;

-- name: ListServers :many
SELECT * FROM servers ORDER BY group_name, name;

-- name: ListEnabledServers :many
SELECT * FROM servers WHERE enabled = 1 ORDER BY group_name, name;

-- name: ListServersForUserFiltered :many
-- need_execute: 0 requires can_connect, 1 requires can_execute.
SELECT s.* FROM servers s
WHERE s.enabled = 1 AND EXISTS (
    SELECT 1 FROM host_grants g
    JOIN user_group_members m ON m.group_id = g.user_group_id
    WHERE m.user_id = $1
      AND g.server_id = s.id
      AND ((sqlc.arg('need_execute')::int = 0 AND g.can_connect = 1) OR (sqlc.arg('need_execute')::int = 1 AND g.can_execute = 1))
) ORDER BY s.name;

-- name: ListServersForUserTransferable :many
SELECT s.* FROM servers s
WHERE s.enabled = 1 AND EXISTS (
    SELECT 1 FROM host_grants g
    JOIN user_group_members m ON m.group_id = g.user_group_id
    WHERE m.user_id = $1
      AND g.server_id = s.id
      AND g.can_transfer = 1
) ORDER BY s.name;

-- name: ServerGrantDecision :one
SELECT
    COALESCE(MAX(g.can_connect), 0)::int AS can_connect,
    COALESCE(MAX(g.can_execute), 0)::int AS can_execute,
    COALESCE(MAX(g.can_transfer), 0)::int AS can_transfer,
    COALESCE(MAX(CASE WHEN g.can_connect = 1 THEN g.terminal_no_approval ELSE 0 END), 0)::int AS terminal_no_approval,
    COALESCE(MAX(CASE WHEN g.can_execute = 1 THEN g.mcp_no_approval ELSE 0 END), 0)::int AS mcp_no_approval
FROM host_grants g
JOIN user_group_members m ON m.group_id = g.user_group_id
WHERE m.user_id = $1
  AND g.server_id = $2;

-- name: DeleteServer :execrows
DELETE FROM servers WHERE id = $1;

-- name: DistinctServerGroups :many
SELECT DISTINCT group_name FROM servers WHERE group_name <> '' ORDER BY group_name;
