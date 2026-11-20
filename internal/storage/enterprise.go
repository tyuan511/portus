package storage

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/yuantang/sshield/internal/domain"
)

// --- roles ---

const roleCols = `id, name, permissions_json, is_builtin, description, created_at`

func scanRole(row interface{ Scan(...any) error }) (*domain.Role, error) {
	var r domain.Role
	var permsJSON string
	var builtin int
	if err := row.Scan(&r.ID, &r.Name, &permsJSON, &builtin, &r.Description, &r.CreatedAt); err != nil {
		return nil, err
	}
	_ = json.Unmarshal([]byte(permsJSON), &r.Permissions)
	if r.Permissions == nil {
		r.Permissions = []domain.Permission{}
	}
	r.IsBuiltin = intToBool(builtin)
	return &r, nil
}

// UpsertRole inserts or updates a role.
func (db *DB) UpsertRole(r *domain.Role) error {
	if r.Permissions == nil {
		r.Permissions = []domain.Permission{}
	}
	if r.CreatedAt.IsZero() {
		r.CreatedAt = nowUTC()
	}
	perms, _ := json.Marshal(r.Permissions)
	_, err := db.sql.Exec(`INSERT INTO roles (id, name, permissions_json, is_builtin, description, created_at)
		VALUES (?,?,?,?,?,?)
		ON CONFLICT(id) DO UPDATE SET name=excluded.name, permissions_json=excluded.permissions_json,
		is_builtin=excluded.is_builtin, description=excluded.description`,
		r.ID, r.Name, string(perms), boolToInt(r.IsBuiltin), r.Description, formatRFC3339(r.CreatedAt))
	if err != nil {
		return fmt.Errorf("upsert role: %w", err)
	}
	return nil
}

// GetRole fetches a role by id.
func (db *DB) GetRole(id string) (*domain.Role, error) {
	row := db.sql.QueryRow(`SELECT `+roleCols+` FROM roles WHERE id = ?`, id)
	r, err := scanRole(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	return r, err
}

// ListRoles returns all roles.
func (db *DB) ListRoles() ([]*domain.Role, error) {
	rows, err := db.sql.Query(`SELECT ` + roleCols + ` FROM roles ORDER BY is_builtin DESC, name`)
	if err != nil {
		return nil, fmt.Errorf("list roles: %w", err)
	}
	defer rows.Close()
	var out []*domain.Role
	for rows.Next() {
		r, err := scanRole(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// DeleteRole removes a non-builtin role; fails when users still reference it.
func (db *DB) DeleteRole(id string) (bool, error) {
	var builtin int
	var users int
	if err := db.sql.QueryRow(`SELECT is_builtin FROM roles WHERE id = ?`, id).Scan(&builtin); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return false, ErrNotFound
		}
		return false, err
	}
	if intToBool(builtin) {
		return false, errors.New("built-in roles cannot be deleted")
	}
	if err := db.sql.QueryRow(`SELECT COUNT(*) FROM users WHERE role_id = ?`, id).Scan(&users); err != nil {
		return false, err
	}
	if users > 0 {
		return false, fmt.Errorf("role still assigned to %d user(s)", users)
	}
	return db.execDelete(`DELETE FROM roles WHERE id = ?`, id)
}

// CountUsers returns the number of user accounts (bootstrap detection).
func (db *DB) CountUsers() (int, error) {
	var n int
	err := db.sql.QueryRow(`SELECT COUNT(*) FROM users`).Scan(&n)
	return n, err
}

// --- users ---

const userCols = `id, username, display_name, password_hash, role_id, is_active,
	must_change_password, totp_secret, totp_enabled, recovery_codes_json,
	failed_attempts, locked_until, created_at, updated_at, last_login_at`

type userRow struct {
	ID, Username, DisplayName, PasswordHash, RoleID string
	IsActive, MustChange, TOTPEnabled               int
	TOTPSecret, RecoveryJSON                        string
	FailedAttempts                                  int
	LockedUntil                                     sql.NullString
	CreatedAt, UpdatedAt                            string
	LastLoginAt                                     sql.NullString
}

func scanUser(row interface{ Scan(...any) error }) (*userRow, error) {
	var u userRow
	err := row.Scan(&u.ID, &u.Username, &u.DisplayName, &u.PasswordHash, &u.RoleID,
		&u.IsActive, &u.MustChange, &u.TOTPSecret, &u.TOTPEnabled, &u.RecoveryJSON,
		&u.FailedAttempts, &u.LockedUntil, &u.CreatedAt, &u.UpdatedAt, &u.LastLoginAt)
	return &u, err
}

func (u *userRow) toUser(withSecret bool) *domain.User {
	usr := &domain.User{
		ID: u.ID, Username: u.Username, DisplayName: u.DisplayName, RoleID: u.RoleID,
		IsActive: intToBool(u.IsActive), MustChangePassword: intToBool(u.MustChange),
		TOTPEnabled: intToBool(u.TOTPEnabled),
	}
	if t, ok := parseRFC3339(u.CreatedAt); ok {
		usr.CreatedAt = t
	}
	if t, ok := parseRFC3339(u.UpdatedAt); ok {
		usr.UpdatedAt = t
	}
	if u.LastLoginAt.Valid {
		if t, ok := parseRFC3339(u.LastLoginAt.String); ok {
			usr.LastLoginAt = &t
		}
	}
	_ = withSecret // TOTP secret is never exposed through domain.User
	return usr
}

// UserCredentials is the internal authentication record (hash + TOTP material).
type UserCredentials struct {
	UserID           string
	Username         string
	PasswordHash     string
	RoleID           string
	IsActive         bool
	TOTPSecret       string
	TOTPEnabled      bool
	RecoveryCodes    []string
	FailedAttempts   int
	LockedUntil      *time.Time
	MustChangePassword bool
}

func (u *userRow) toCredentials() *UserCredentials {
	c := &UserCredentials{
		UserID: u.ID, Username: u.Username, PasswordHash: u.PasswordHash, RoleID: u.RoleID,
		IsActive: intToBool(u.IsActive), TOTPSecret: u.TOTPSecret, TOTPEnabled: intToBool(u.TOTPEnabled),
		FailedAttempts: u.FailedAttempts, MustChangePassword: intToBool(u.MustChange),
	}
	_ = json.Unmarshal([]byte(u.RecoveryJSON), &c.RecoveryCodes)
	if u.LockedUntil.Valid {
		if t, ok := parseRFC3339(u.LockedUntil.String); ok {
			c.LockedUntil = &t
		}
	}
	return c
}

// CreateUser inserts a new user account.
func (db *DB) CreateUser(u *domain.User, passwordHash, totpSecret string, recoveryCodes []string) error {
	if recoveryCodes == nil {
		recoveryCodes = []string{}
	}
	conv, _ := json.Marshal(recoveryCodes)
	now := nowRFC3339()
	_, err := db.sql.Exec(`INSERT INTO users (id, username, display_name, password_hash, role_id,
		is_active, must_change_password, totp_secret, totp_enabled, recovery_codes_json,
		failed_attempts, created_at, updated_at)
		VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		u.ID, u.Username, u.DisplayName, passwordHash, u.RoleID,
		boolToInt(u.IsActive), boolToInt(u.MustChangePassword), totpSecret,
		boolToInt(u.TOTPEnabled), string(conv), 0, now, now)
	if err != nil {
		if isUniqueViolation(err) {
			return errors.New("username already exists")
		}
		return fmt.Errorf("create user: %w", err)
	}
	return nil
}

// GetUser fetches a user by id (no secrets).
func (db *DB) GetUser(id string) (*domain.User, error) {
	row := db.sql.QueryRow(`SELECT `+userCols+` FROM users WHERE id = ?`, id)
	u, err := scanUser(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return u.toUser(false), nil
}

// GetUserByUsername fetches a user by unique username.
func (db *DB) GetUserByUsername(username string) (*domain.User, error) {
	row := db.sql.QueryRow(`SELECT `+userCols+` FROM users WHERE username = ?`, username)
	u, err := scanUser(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return u.toUser(false), nil
}

// GetCredentialsByUsername loads the authentication record for login.
func (db *DB) GetCredentialsByUsername(username string) (*UserCredentials, error) {
	row := db.sql.QueryRow(`SELECT `+userCols+` FROM users WHERE username = ?`, username)
	u, err := scanUser(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return u.toCredentials(), nil
}

// ListUsers returns all accounts with their roles hydrated.
func (db *DB) ListUsers() ([]*domain.User, error) {
	rows, err := db.sql.Query(`SELECT u.` + qualifyCols(userCols, "u") + `, r.` +
		qualifyCols(roleCols, "r") + `
		FROM users u JOIN roles r ON r.id = u.role_id ORDER BY u.created_at`)
	if err != nil {
		return nil, fmt.Errorf("list users: %w", err)
	}
	defer rows.Close()

	var out []*domain.User
	for rows.Next() {
		u, r, err := scanUserWithRole(rows)
		if err != nil {
			return nil, err
		}
		u.Role = r
		out = append(out, u)
	}
	return out, rows.Err()
}

func scanUserWithRole(rows *sql.Rows) (*domain.User, *domain.Role, error) {
	var u userRow
	var r domain.Role
	var permsJSON string
	var builtin int
	var roleCreated string
	err := rows.Scan(&u.ID, &u.Username, &u.DisplayName, &u.PasswordHash, &u.RoleID,
		&u.IsActive, &u.MustChange, &u.TOTPSecret, &u.TOTPEnabled, &u.RecoveryJSON,
		&u.FailedAttempts, &u.LockedUntil, &u.CreatedAt, &u.UpdatedAt, &u.LastLoginAt,
		&r.ID, &r.Name, &permsJSON, &builtin, &r.Description, &roleCreated)
	if err != nil {
		return nil, nil, err
	}
	_ = json.Unmarshal([]byte(permsJSON), &r.Permissions)
	r.IsBuiltin = intToBool(builtin)
	if r.Permissions == nil {
		r.Permissions = []domain.Permission{}
	}
	return u.toUser(false), &r, nil
}

// qualifyCols prefixes each comma-separated column with a table alias.
func qualifyCols(cols, alias string) string {
	parts := strings.Split(cols, ",")
	for i, p := range parts {
		parts[i] = alias + "." + strings.TrimSpace(p)
	}
	return strings.Join(parts, ", ")
}

// UpdateUserProfile mutates display name / role / active flag / must-change flag.
func (db *DB) UpdateUserProfile(id string, displayName *string, roleID *string, isActive *bool, mustChange *bool) error {
	sets := []string{"updated_at = ?"}
	args := []any{nowRFC3339()}
	if displayName != nil {
		sets = append(sets, "display_name = ?")
		args = append(args, *displayName)
	}
	if roleID != nil {
		sets = append(sets, "role_id = ?")
		args = append(args, *roleID)
	}
	if isActive != nil {
		sets = append(sets, "is_active = ?")
		args = append(args, boolToInt(*isActive))
	}
	if mustChange != nil {
		sets = append(sets, "must_change_password = ?")
		args = append(args, boolToInt(*mustChange))
	}
	args = append(args, id)
	res, err := db.sql.Exec(`UPDATE users SET `+joinSets(sets)+` WHERE id = ?`, args...)
	if err != nil {
		return fmt.Errorf("update user: %w", err)
	}
	n, _ := res.RowsAffected()
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// SetUserPassword replaces the password hash, clears failure counters and
// optionally forces a password change at next login.
func (db *DB) SetUserPassword(id, hash string, mustChange bool) error {
	res, err := db.sql.Exec(`UPDATE users SET password_hash = ?, must_change_password = ?,
		failed_attempts = 0, locked_until = NULL, updated_at = ? WHERE id = ?`,
		hash, boolToInt(mustChange), nowRFC3339(), id)
	if err != nil {
		return fmt.Errorf("set password: %w", err)
	}
	n, _ := res.RowsAffected()
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// SetUserTOTP stores (or clears) TOTP material.
func (db *DB) SetUserTOTP(id string, secret string, enabled bool, recoveryCodes []string) error {
	if recoveryCodes == nil {
		recoveryCodes = []string{}
	}
	conv, _ := json.Marshal(recoveryCodes)
	_, err := db.sql.Exec(`UPDATE users SET totp_secret = ?, totp_enabled = ?,
		recovery_codes_json = ?, updated_at = ? WHERE id = ?`,
		secret, boolToInt(enabled), string(conv), nowRFC3339(), id)
	if err != nil {
		return fmt.Errorf("set totp: %w", err)
	}
	return nil
}

// RecordLoginSuccess resets failure counters and stamps last_login_at.
func (db *DB) RecordLoginSuccess(id string) error {
	_, err := db.sql.Exec(`UPDATE users SET failed_attempts = 0, locked_until = NULL,
		last_login_at = ?, updated_at = ? WHERE id = ?`, nowRFC3339(), nowRFC3339(), id)
	return err
}

// RecordLoginFailure increments the failure counter and applies lockout when
// maxAttempts is reached; returns the new locked-until time (nil = not locked).
func (db *DB) RecordLoginFailure(id string, maxAttempts, lockoutMinutes int) (*time.Time, error) {
	var failed int
	if err := db.sql.QueryRow(`UPDATE users SET failed_attempts = failed_attempts + 1,
		updated_at = ? WHERE id = ? RETURNING failed_attempts`, nowRFC3339(), id).Scan(&failed); err != nil {
		return nil, err
	}
	if failed < maxAttempts {
		return nil, nil
	}
	lockUntil := nowUTC().Add(time.Duration(lockoutMinutes) * time.Minute)
	if _, err := db.sql.Exec(`UPDATE users SET locked_until = ? WHERE id = ?`,
		formatRFC3339(lockUntil), id); err != nil {
		return nil, err
	}
	return &lockUntil, nil
}

// DeleteUser removes an account.
func (db *DB) DeleteUser(id string) (bool, error) {
	return db.execDelete(`DELETE FROM users WHERE id = ?`, id)
}

// --- user groups ---

// CreateUserGroup inserts a group.
func (db *DB) CreateUserGroup(g *domain.UserGroup) error {
	if g.CreatedAt.IsZero() {
		g.CreatedAt = nowUTC()
	}
	_, err := db.sql.Exec(`INSERT INTO user_groups (id, name, description, created_at)
		VALUES (?,?,?,?)`, g.ID, g.Name, g.Description, formatRFC3339(g.CreatedAt))
	if err != nil {
		if isUniqueViolation(err) {
			return errors.New("group name already exists")
		}
		return fmt.Errorf("create group: %w", err)
	}
	return nil
}

// GetUserGroup fetches one group.
func (db *DB) GetUserGroup(id string) (*domain.UserGroup, error) {
	row := db.sql.QueryRow(`SELECT id, name, description, created_at FROM user_groups WHERE id = ?`, id)
	var g domain.UserGroup
	var created string
	err := row.Scan(&g.ID, &g.Name, &g.Description, &created)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	g.CreatedAt, _ = parseRFC3339(created)
	return &g, nil
}

// ListUserGroups returns all groups.
func (db *DB) ListUserGroups() ([]*domain.UserGroup, error) {
	rows, err := db.sql.Query(`SELECT id, name, description, created_at FROM user_groups ORDER BY name`)
	if err != nil {
		return nil, fmt.Errorf("list groups: %w", err)
	}
	defer rows.Close()
	var out []*domain.UserGroup
	for rows.Next() {
		var g domain.UserGroup
		var created string
		if err := rows.Scan(&g.ID, &g.Name, &g.Description, &created); err != nil {
			return nil, err
		}
		g.CreatedAt, _ = parseRFC3339(created)
		out = append(out, &g)
	}
	return out, rows.Err()
}

// UpdateUserGroup mutates name/description.
func (db *DB) UpdateUserGroup(id string, name *string, description *string) error {
	sets, args := []string{}, []any{}
	if name != nil {
		sets, args = append(sets, "name = ?"), append(args, *name)
	}
	if description != nil {
		sets, args = append(sets, "description = ?"), append(args, *description)
	}
	if len(sets) == 0 {
		return nil
	}
	args = append(args, id)
	res, err := db.sql.Exec(`UPDATE user_groups SET `+joinSets(sets)+` WHERE id = ?`, args...)
	if err != nil {
		if isUniqueViolation(err) {
			return errors.New("group name already exists")
		}
		return err
	}
	n, _ := res.RowsAffected()
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteUserGroup removes a group (grants cascade).
func (db *DB) DeleteUserGroup(id string) (bool, error) {
	return db.execDelete(`DELETE FROM user_groups WHERE id = ?`, id)
}

// AddGroupMember links a user to a group (idempotent).
func (db *DB) AddGroupMember(groupID, userID string) error {
	_, err := db.sql.Exec(`INSERT OR IGNORE INTO user_group_members (group_id, user_id) VALUES (?,?)`,
		groupID, userID)
	return err
}

// RemoveGroupMember unlinks a user from a group.
func (db *DB) RemoveGroupMember(groupID, userID string) (bool, error) {
	return db.execDelete(`DELETE FROM user_group_members WHERE group_id = ? AND user_id = ?`, groupID, userID)
}

// ListGroupMembers returns the users of a group.
func (db *DB) ListGroupMembers(groupID string) ([]*domain.User, error) {
	rows, err := db.sql.Query(`SELECT u.id, u.username, u.display_name, u.role_id, u.is_active,
		u.must_change_password, u.totp_enabled, u.created_at, u.updated_at, u.last_login_at
		FROM users u JOIN user_group_members m ON m.user_id = u.id
		WHERE m.group_id = ? ORDER BY u.username`, groupID)
	if err != nil {
		return nil, fmt.Errorf("list group members: %w", err)
	}
	defer rows.Close()
	var out []*domain.User
	for rows.Next() {
		var u domain.User
		var lastLogin sql.NullString
		var created, updated string
		if err := rows.Scan(&u.ID, &u.Username, &u.DisplayName, &u.RoleID, &u.IsActive,
			&u.MustChangePassword, &u.TOTPEnabled, &created, &updated, &lastLogin); err != nil {
			return nil, err
		}
		u.CreatedAt, _ = parseRFC3339(created)
		u.UpdatedAt, _ = parseRFC3339(updated)
		if lastLogin.Valid {
			if t, ok := parseRFC3339(lastLogin.String); ok {
				u.LastLoginAt = &t
			}
		}
		out = append(out, &u)
	}
	return out, rows.Err()
}

// ListGroupsForUser returns the groups a user belongs to.
func (db *DB) ListGroupsForUser(userID string) ([]*domain.UserGroup, error) {
	rows, err := db.sql.Query(`SELECT g.id, g.name, g.description, g.created_at
		FROM user_groups g JOIN user_group_members m ON m.group_id = g.id
		WHERE m.user_id = ? ORDER BY g.name`, userID)
	if err != nil {
		return nil, fmt.Errorf("groups for user: %w", err)
	}
	defer rows.Close()
	var out []*domain.UserGroup
	for rows.Next() {
		var g domain.UserGroup
		var created string
		if err := rows.Scan(&g.ID, &g.Name, &g.Description, &created); err != nil {
			return nil, err
		}
		g.CreatedAt, _ = parseRFC3339(created)
		out = append(out, &g)
	}
	return out, rows.Err()
}

// --- host grants ---

// CreateHostGrant inserts an access grant.
func (db *DB) CreateHostGrant(g *domain.HostGrant) error {
	if g.CreatedAt.IsZero() {
		g.CreatedAt = nowUTC()
	}
	if (g.ServerID == "") == (g.GroupName == "") {
		return errors.New("grant must target exactly one of server_id or group_name")
	}
	_, err := db.sql.Exec(`INSERT INTO host_grants (id, user_group_id, server_id, group_name,
		can_connect, can_execute, created_by, created_at) VALUES (?,?,?,?,?,?,?,?)`,
		g.ID, g.UserGroupID, g.ServerID, g.GroupName,
		boolToInt(g.CanConnect), boolToInt(g.CanExecute), g.CreatedBy, formatRFC3339(g.CreatedAt))
	if err != nil {
		return fmt.Errorf("create grant: %w", err)
	}
	return nil
}

// ListHostGrants returns all grants with their groups hydrated.
func (db *DB) ListHostGrants() ([]*domain.HostGrant, error) {
	rows, err := db.sql.Query(`SELECT g.id, g.user_group_id, g.server_id, g.group_name,
		g.can_connect, g.can_execute, g.created_by, g.created_at,
		ug.id, ug.name, ug.description, ug.created_at
		FROM host_grants g JOIN user_groups ug ON ug.id = g.user_group_id
		ORDER BY g.created_at DESC`)
	if err != nil {
		return nil, fmt.Errorf("list grants: %w", err)
	}
	defer rows.Close()
	var out []*domain.HostGrant
	for rows.Next() {
		var grant domain.HostGrant
		var ug domain.UserGroup
		var created, ugCreated string
		if err := rows.Scan(&grant.ID, &grant.UserGroupID, &grant.ServerID, &grant.GroupName,
			&grant.CanConnect, &grant.CanExecute, &grant.CreatedBy, &created,
			&ug.ID, &ug.Name, &ug.Description, &ugCreated); err != nil {
			return nil, err
		}
		grant.CreatedAt, _ = parseRFC3339(created)
		ug.CreatedAt, _ = parseRFC3339(ugCreated)
		grant.UserGroup = &ug
		out = append(out, &grant)
	}
	return out, rows.Err()
}

// UpdateHostGrant toggles the connect/execute bits.
func (db *DB) UpdateHostGrant(id string, canConnect, canExecute *bool) error {
	sets, args := []string{}, []any{}
	if canConnect != nil {
		sets, args = append(sets, "can_connect = ?"), append(args, boolToInt(*canConnect))
	}
	if canExecute != nil {
		sets, args = append(sets, "can_execute = ?"), append(args, boolToInt(*canExecute))
	}
	if len(sets) == 0 {
		return nil
	}
	args = append(args, id)
	res, err := db.sql.Exec(`UPDATE host_grants SET `+joinSets(sets)+` WHERE id = ?`, args...)
	if err != nil {
		return err
	}
	n, _ := res.RowsAffected()
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteHostGrant removes a grant.
func (db *DB) DeleteHostGrant(id string) (bool, error) {
	return db.execDelete(`DELETE FROM host_grants WHERE id = ?`, id)
}

// GrantsForServer lists the grants targeting a specific server (admin UI).
func (db *DB) GrantsForServer(serverID, groupName string) ([]*domain.HostGrant, error) {
	all, err := db.ListHostGrants()
	if err != nil {
		return nil, err
	}
	var out []*domain.HostGrant
	for _, g := range all {
		if (g.ServerID != "" && g.ServerID == serverID) || (g.GroupName != "" && g.GroupName == groupName) {
			out = append(out, g)
		}
	}
	return out, nil
}

// --- api tokens ---

// CreateApiToken stores a token record (hash only).
func (db *DB) CreateApiToken(t *domain.ApiToken, tokenHash string) error {
	if t.CreatedAt.IsZero() {
		t.CreatedAt = nowUTC()
	}
	var expires any
	if t.ExpiresAt != nil {
		expires = formatRFC3339(*t.ExpiresAt)
	}
	_, err := db.sql.Exec(`INSERT INTO api_tokens (id, user_id, name, prefix, token_hash,
		created_at, expires_at) VALUES (?,?,?,?,?,?,?)`,
		t.ID, t.UserID, t.Name, t.Prefix, tokenHash, formatRFC3339(t.CreatedAt), expires)
	if err != nil {
		return fmt.Errorf("create token: %w", err)
	}
	return nil
}

// GetApiTokenByHash resolves a bearer token (sha256 hex) to its record.
func (db *DB) GetApiTokenByHash(hash string) (*domain.ApiToken, error) {
	row := db.sql.QueryRow(`SELECT id, user_id, name, prefix, created_at, last_used_at,
		expires_at, revoked_at FROM api_tokens WHERE token_hash = ?`, hash)
	var t domain.ApiToken
	var created string
	var lastUsed, expires, revoked sql.NullString
	err := row.Scan(&t.ID, &t.UserID, &t.Name, &t.Prefix, &created, &lastUsed, &expires, &revoked)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	t.CreatedAt, _ = parseRFC3339(created)
	if lastUsed.Valid {
		if v, ok := parseRFC3339(lastUsed.String); ok {
			t.LastUsedAt = &v
		}
	}
	if expires.Valid {
		if v, ok := parseRFC3339(expires.String); ok {
			t.ExpiresAt = &v
		}
	}
	if revoked.Valid {
		if v, ok := parseRFC3339(revoked.String); ok {
			t.RevokedAt = &v
		}
	}
	return &t, nil
}

// TouchApiToken updates last_used_at.
func (db *DB) TouchApiToken(id string) {
	_, _ = db.sql.Exec(`UPDATE api_tokens SET last_used_at = ? WHERE id = ?`, nowRFC3339(), id)
}

// ListApiTokensForUser returns the user's tokens (no hashes).
func (db *DB) ListApiTokensForUser(userID string) ([]*domain.ApiToken, error) {
	rows, err := db.sql.Query(`SELECT id, user_id, name, prefix, created_at, last_used_at,
		expires_at, revoked_at FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC`, userID)
	if err != nil {
		return nil, fmt.Errorf("list tokens: %w", err)
	}
	defer rows.Close()
	var out []*domain.ApiToken
	for rows.Next() {
		var t domain.ApiToken
		var created string
		var lastUsed, expires, revoked sql.NullString
		if err := rows.Scan(&t.ID, &t.UserID, &t.Name, &t.Prefix, &created, &lastUsed, &expires, &revoked); err != nil {
			return nil, err
		}
		t.CreatedAt, _ = parseRFC3339(created)
		if lastUsed.Valid {
			if v, ok := parseRFC3339(lastUsed.String); ok {
				t.LastUsedAt = &v
			}
		}
		if expires.Valid {
			if v, ok := parseRFC3339(expires.String); ok {
				t.ExpiresAt = &v
			}
		}
		if revoked.Valid {
			if v, ok := parseRFC3339(revoked.String); ok {
				t.RevokedAt = &v
			}
		}
		out = append(out, &t)
	}
	return out, rows.Err()
}

// RevokeApiToken marks a token revoked (only by its owner).
func (db *DB) RevokeApiToken(id, userID string) (bool, error) {
	return db.execRowsAffected(`UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND user_id = ?
		AND revoked_at IS NULL`, nowRFC3339(), id, userID)
}

// --- sessions ---

// CreateSession stores a web session (token stored as sha256 hash).
func (db *DB) CreateSession(s *domain.UserSession, tokenHash string, absoluteExpires time.Time) error {
	_, err := db.sql.Exec(`INSERT INTO sessions (id, user_id, token_hash, ip, user_agent,
		created_at, last_seen_at, expires_at, absolute_expires_at) VALUES (?,?,?,?,?,?,?,?,?)`,
		s.ID, s.UserID, tokenHash, s.IP, s.UserAgent,
		formatRFC3339(s.CreatedAt), formatRFC3339(s.LastSeenAt),
		formatRFC3339(s.ExpiresAt), formatRFC3339(absoluteExpires))
	if err != nil {
		return fmt.Errorf("create session: %w", err)
	}
	return nil
}

// GetSessionByHash resolves a session cookie value (sha256 hex) to the session.
func (db *DB) GetSessionByHash(hash string) (*domain.UserSession, error) {
	row := db.sql.QueryRow(`SELECT id, user_id, ip, user_agent, created_at, last_seen_at,
		expires_at FROM sessions WHERE token_hash = ?`, hash)
	var s domain.UserSession
	var created, lastSeen, expires string
	err := row.Scan(&s.ID, &s.UserID, &s.IP, &s.UserAgent, &created, &lastSeen, &expires)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	s.CreatedAt, _ = parseRFC3339(created)
	s.LastSeenAt, _ = parseRFC3339(lastSeen)
	s.ExpiresAt, _ = parseRFC3339(expires)
	return &s, nil
}

// TouchSession slides the idle expiry window forward.
func (db *DB) TouchSession(id string, newExpiry time.Time) {
	_, _ = db.sql.Exec(`UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?`,
		nowRFC3339(), formatRFC3339(newExpiry), id)
}

// DeleteSession removes one session.
func (db *DB) DeleteSession(id string) (bool, error) {
	return db.execDelete(`DELETE FROM sessions WHERE id = ?`, id)
}

// DeleteSessionsForUser removes every session of a user (password change /
// account disable). Returns the number removed.
func (db *DB) DeleteSessionsForUser(userID string) (int64, error) {
	res, err := db.sql.Exec(`DELETE FROM sessions WHERE user_id = ?`, userID)
	if err != nil {
		return 0, err
	}
	n, _ := res.RowsAffected()
	return n, nil
}

// ListSessionsForUser returns a user's active sessions.
func (db *DB) ListSessionsForUser(userID string) ([]*domain.UserSession, error) {
	return db.querySessions(`SELECT id, user_id, ip, user_agent, created_at, last_seen_at, expires_at
		FROM sessions WHERE user_id = ? ORDER BY last_seen_at DESC`, userID)
}

// ListAllSessions returns every active session (admin).
func (db *DB) ListAllSessions() ([]*domain.UserSession, error) {
	return db.querySessions(`SELECT id, user_id, ip, user_agent, created_at, last_seen_at, expires_at
		FROM sessions ORDER BY last_seen_at DESC`)
}

func (db *DB) querySessions(q string, args ...any) ([]*domain.UserSession, error) {
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []*domain.UserSession
	for rows.Next() {
		var s domain.UserSession
		var created, lastSeen, expires string
		if err := rows.Scan(&s.ID, &s.UserID, &s.IP, &s.UserAgent, &created, &lastSeen, &expires); err != nil {
			return nil, err
		}
		s.CreatedAt, _ = parseRFC3339(created)
		s.LastSeenAt, _ = parseRFC3339(lastSeen)
		s.ExpiresAt, _ = parseRFC3339(expires)
		out = append(out, &s)
	}
	return out, rows.Err()
}

// CleanupExpired removes expired sessions, tokens beyond retention and stale
// auth attempts; returns the number of rows removed.
func (db *DB) CleanupExpired() (int64, error) {
	now := nowRFC3339()
	var total int64
	for _, q := range []string{
		`DELETE FROM sessions WHERE expires_at < ? OR absolute_expires_at < ?`,
		`DELETE FROM auth_attempts WHERE created_at < ?`,
	} {
		res, err := db.sql.Exec(q, now, now)
		if err != nil {
			return total, err
		}
		n, _ := res.RowsAffected()
		total += n
	}
	return total, nil
}

// --- auth attempts (brute-force evidence) ---

// RecordAuthAttempt persists one login attempt outcome.
func (db *DB) RecordAuthAttempt(username, ip string, success bool) error {
	_, err := db.sql.Exec(`INSERT INTO auth_attempts (username, ip, success, created_at)
		VALUES (?,?,?,?)`, username, ip, boolToInt(success), nowRFC3339())
	return err
}

// CountFailedAttemptsByUser counts recent failures for a username in the window.
func (db *DB) CountFailedAttemptsByUser(username string, since time.Time) (int, error) {
	return db.countWhere(`SELECT COUNT(*) FROM auth_attempts WHERE username = ? AND success = 0 AND created_at >= ?`,
		username, formatRFC3339(since))
}

// CountFailedAttemptsByIP counts recent failures for a source IP in the window.
func (db *DB) CountFailedAttemptsByIP(ip string, since time.Time) (int, error) {
	return db.countWhere(`SELECT COUNT(*) FROM auth_attempts WHERE ip = ? AND success = 0 AND created_at >= ?`,
		ip, formatRFC3339(since))
}

func (db *DB) countWhere(q string, args ...any) (int, error) {
	var n int
	if err := db.sql.QueryRow(q, args...).Scan(&n); err != nil {
		return 0, err
	}
	return n, nil
}

// --- small helpers ---

func joinSets(sets []string) string {
	out := ""
	for i, s := range sets {
		if i > 0 {
			out += ", "
		}
		out += s
	}
	return out
}

// execRowsAffected wraps Exec and reports whether any row changed.
func (db *DB) execRowsAffected(q string, args ...any) (bool, error) {
	res, err := db.sql.Exec(q, args...)
	if err != nil {
		return false, err
	}
	n, _ := res.RowsAffected()
	return n > 0, nil
}

// isUniqueViolation detects modernc/sqlite UNIQUE constraint errors.
func isUniqueViolation(err error) bool {
	return err != nil && strings.Contains(strings.ToLower(err.Error()), "unique")
}
