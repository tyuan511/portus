-- name: CreateUserGroup :exec
INSERT INTO user_groups (id, name, description, created_at) VALUES ($1, $2, $3, $4);

-- name: GetUserGroup :one
SELECT * FROM user_groups WHERE id = $1;

-- name: ListUserGroups :many
SELECT * FROM user_groups ORDER BY name;

-- name: UpdateUserGroup :execrows
UPDATE user_groups SET name = $2, description = $3 WHERE id = $1;

-- name: DeleteUserGroup :execrows
DELETE FROM user_groups WHERE id = $1;

-- name: AddGroupMember :exec
INSERT INTO user_group_members (group_id, user_id) VALUES ($1, $2)
ON CONFLICT (group_id, user_id) DO NOTHING;

-- name: RemoveGroupMember :execrows
DELETE FROM user_group_members WHERE group_id = $1 AND user_id = $2;

-- name: ListGroupMembers :many
SELECT u.id, u.username, u.display_name, u.role, u.is_active,
       u.must_change_password, u.totp_enabled, u.created_at, u.updated_at, u.last_login_at
FROM users u JOIN user_group_members m ON m.user_id = u.id
WHERE m.group_id = $1 ORDER BY u.username;

-- name: ListGroupsForUser :many
SELECT g.* FROM user_groups g
JOIN user_group_members m ON m.group_id = g.id
WHERE m.user_id = $1 ORDER BY g.name;

-- name: CreateHostGrant :exec
INSERT INTO host_grants (id, user_group_id, group_name,
    can_connect, can_execute, can_transfer, created_by, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8);

-- name: ListHostGrants :many
SELECT g.*, ug.id AS ug_id, ug.name AS ug_name, ug.description AS ug_description,
       ug.created_at AS ug_created_at
FROM host_grants g JOIN user_groups ug ON ug.id = g.user_group_id
ORDER BY g.created_at DESC;

-- name: UpdateHostGrant :execrows
UPDATE host_grants SET can_connect = $2, can_execute = $3, can_transfer = $4 WHERE id = $1;

-- name: DeleteHostGrant :execrows
DELETE FROM host_grants WHERE id = $1;

-- name: CreateApiToken :exec
INSERT INTO api_tokens (id, user_id, name, prefix, token_hash, created_at, expires_at)
VALUES ($1, $2, $3, $4, $5, $6, $7);

-- name: GetApiTokenByHash :one
SELECT * FROM api_tokens WHERE token_hash = $1;

-- name: TouchApiToken :exec
UPDATE api_tokens SET last_used_at = $2 WHERE id = $1;

-- name: ListApiTokensForUser :many
SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC;

-- name: RevokeApiToken :execrows
UPDATE api_tokens SET revoked_at = $3 WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL;

-- name: CleanupApiTokensBefore :execrows
-- Prunes already-revoked or already-expired tokens past the retention cutoff.
DELETE FROM api_tokens
WHERE (revoked_at IS NOT NULL AND revoked_at < $1)
   OR (expires_at IS NOT NULL AND expires_at < $1);

-- name: CreateSession :exec
INSERT INTO sessions (id, user_id, token_hash, ip, user_agent,
    created_at, last_seen_at, expires_at, absolute_expires_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9);

-- name: GetSessionByHash :one
SELECT * FROM sessions WHERE token_hash = $1;

-- name: TouchSession :exec
UPDATE sessions SET last_seen_at = $2, expires_at = $3 WHERE id = $1;

-- name: DeleteSession :execrows
DELETE FROM sessions WHERE id = $1;

-- name: DeleteSessionsForUser :execrows
DELETE FROM sessions WHERE user_id = $1;

-- name: ListSessionsForUser :many
SELECT * FROM sessions WHERE user_id = $1 ORDER BY last_seen_at DESC;

-- name: ListAllSessions :many
SELECT s.*, u.username FROM sessions s
JOIN users u ON u.id = s.user_id
ORDER BY s.last_seen_at DESC;

-- name: CountAllSessions :one
SELECT COUNT(*) FROM sessions;

-- name: CleanupExpiredSessions :execrows
DELETE FROM sessions WHERE expires_at < $1 OR absolute_expires_at < $1;

-- name: CleanupStaleAuthAttempts :execrows
DELETE FROM auth_attempts WHERE created_at < $1;

-- name: RecordAuthAttempt :exec
INSERT INTO auth_attempts (username, ip, success, created_at) VALUES ($1, $2, $3, $4);

-- name: CountFailedAttemptsByUser :one
SELECT COUNT(*) FROM auth_attempts
WHERE username = $1 AND success = 0 AND created_at >= $2;

-- name: CountFailedAttemptsByIP :one
SELECT COUNT(*) FROM auth_attempts
WHERE ip = $1 AND success = 0 AND created_at >= $2;
