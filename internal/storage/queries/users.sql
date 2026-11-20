-- name: CreateUser :exec
INSERT INTO users (id, username, display_name, password_hash, role_id,
    is_active, must_change_password, totp_secret, totp_enabled, recovery_codes_json,
    failed_attempts, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13);

-- name: GetUserByID :one
SELECT * FROM users WHERE id = $1;

-- name: GetUserByUsername :one
SELECT * FROM users WHERE username = $1;

-- name: GetUserCredentialsByUsername :one
SELECT id, username, display_name, password_hash, role_id, is_active,
       must_change_password, totp_secret, totp_enabled, recovery_codes_json,
       failed_attempts, locked_until
FROM users WHERE username = $1;

-- name: GetUserCredentialsByID :one
SELECT id, username, display_name, password_hash, role_id, is_active,
       must_change_password, totp_secret, totp_enabled, recovery_codes_json,
       failed_attempts, locked_until
FROM users WHERE id = $1;

-- name: ListUsersWithRoles :many
SELECT u.*, r.name AS role_name, r.permissions_json AS role_permissions_json,
       r.is_builtin AS role_is_builtin, r.description AS role_description,
       r.created_at AS role_created_at
FROM users u JOIN roles r ON r.id = u.role_id
ORDER BY u.created_at;

-- name: UpdateUserProfile :exec
UPDATE users SET display_name = $2, role_id = $3, is_active = $4,
    must_change_password = $5, updated_at = $6
WHERE id = $1;

-- name: SetUserPassword :execrows
UPDATE users SET password_hash = $2, must_change_password = $3,
    failed_attempts = 0, locked_until = NULL, updated_at = $4
WHERE id = $1;

-- name: SetUserTOTP :exec
UPDATE users SET totp_secret = $2, totp_enabled = $3,
    recovery_codes_json = $4, updated_at = $5
WHERE id = $1;

-- name: RecordLoginSuccess :exec
UPDATE users SET failed_attempts = 0, locked_until = NULL,
    last_login_at = $2, updated_at = $2
WHERE id = $1;

-- name: BumpFailedAttempts :execrows
UPDATE users SET failed_attempts = failed_attempts + 1, updated_at = $2
WHERE id = $1;

-- name: GetUserFailedAttempts :one
SELECT failed_attempts FROM users WHERE id = $1;

-- name: SetUserLockedUntil :exec
UPDATE users SET locked_until = $2 WHERE id = $1;

-- name: DeleteUser :execrows
DELETE FROM users WHERE id = $1;

-- name: ConsumeRecoveryCode :exec
UPDATE users SET recovery_codes_json = $2, updated_at = $3 WHERE id = $1;

-- name: CountUsers :one
SELECT COUNT(*) FROM users;

-- name: GetRoleByID :one
SELECT * FROM roles WHERE id = $1;

-- name: UpsertRole :exec
INSERT INTO roles (id, name, permissions_json, is_builtin, description, created_at)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (id) DO UPDATE SET
    name = excluded.name, permissions_json = excluded.permissions_json,
    is_builtin = excluded.is_builtin, description = excluded.description;

-- name: ListRoles :many
SELECT * FROM roles ORDER BY is_builtin DESC, name;

-- name: GetRoleBuiltin :one
SELECT is_builtin FROM roles WHERE id = $1;

-- name: CountRoleUsers :one
SELECT COUNT(*) FROM users WHERE role_id = $1;

-- name: DeleteRole :execrows
DELETE FROM roles WHERE id = $1;
