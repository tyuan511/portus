package storage

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/storage/db"
)

// CountUsers returns the number of user accounts (bootstrap detection).
func (h *DB) CountUsers() (int, error) {
	n, err := h.q.CountUsers(context.Background())
	return int(n), err
}

// --- users ---

type userCredentialsRow struct {
	ID                 string
	Username           string
	DisplayName        string
	PasswordHash       string
	Role               string
	IsActive           int32
	MustChangePassword int32
	TotpSecret         string
	TotpEnabled        int32
	RecoveryCodesJson  string
	FailedAttempts     int32
	LockedUntil        sql.NullString
}

// UserCredentials is the internal authentication record (hash + TOTP material).
type UserCredentials struct {
	UserID             string
	Username           string
	DisplayName        string
	PasswordHash       string
	Role               string
	IsActive           bool
	TOTPSecret         string
	TOTPEnabled        bool
	RecoveryCodes      []string
	FailedAttempts     int
	LockedUntil        *time.Time
	MustChangePassword bool
}

func credentialsFromRow(r userCredentialsRow) *UserCredentials {
	c := &UserCredentials{
		UserID:             r.ID,
		Username:           r.Username,
		DisplayName:        r.DisplayName,
		PasswordHash:       r.PasswordHash,
		Role:               r.Role,
		IsActive:           r.IsActive == 1,
		TOTPSecret:         r.TotpSecret,
		TOTPEnabled:        r.TotpEnabled == 1,
		FailedAttempts:     int(r.FailedAttempts),
		MustChangePassword: r.MustChangePassword == 1,
	}
	_ = json.Unmarshal([]byte(r.RecoveryCodesJson), &c.RecoveryCodes)
	if r.LockedUntil.Valid {
		if t, ok := parseRFC3339(r.LockedUntil.String); ok {
			c.LockedUntil = &t
		}
	}
	return c
}

func userToDomain(u db.User) *domain.User {
	usr := &domain.User{
		ID:                 u.ID,
		Username:           u.Username,
		DisplayName:        u.DisplayName,
		Role:               u.Role,
		IsActive:           u.IsActive == 1,
		MustChangePassword: u.MustChangePassword == 1,
		TOTPEnabled:        u.TotpEnabled == 1,
	}
	usr.CreatedAt, _ = parseRFC3339(u.CreatedAt)
	usr.UpdatedAt, _ = parseRFC3339(u.UpdatedAt)
	if u.LastLoginAt.Valid {
		if t, ok := parseRFC3339(u.LastLoginAt.String); ok {
			usr.LastLoginAt = &t
		}
	}
	return usr
}

// CreateUser inserts a new user account.
func (h *DB) CreateUser(u *domain.User, passwordHash, totpSecret string, recoveryCodes []string) error {
	if recoveryCodes == nil {
		recoveryCodes = []string{}
	}
	conv, _ := json.Marshal(recoveryCodes)
	now := nowRFC3339()
	err := h.q.CreateUser(context.Background(), db.CreateUserParams{
		ID:                 u.ID,
		Username:           u.Username,
		DisplayName:        u.DisplayName,
		PasswordHash:       passwordHash,
		Role:               u.Role,
		IsActive:           int32(boolToInt(u.IsActive)),
		MustChangePassword: int32(boolToInt(u.MustChangePassword)),
		TotpSecret:         totpSecret,
		TotpEnabled:        int32(boolToInt(u.TOTPEnabled)),
		RecoveryCodesJson:  string(conv),
		FailedAttempts:     0,
		CreatedAt:          now,
		UpdatedAt:          now,
	})
	if err != nil {
		if h.isUniqueViolation(err) {
			return errors.New("username already exists")
		}
		return fmt.Errorf("create user: %w", err)
	}
	return nil
}

// GetUser fetches a user by id (no secrets).
func (h *DB) GetUser(id string) (*domain.User, error) {
	u, err := h.q.GetUserByID(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get user: %w", err)
	}
	return userToDomain(u), nil
}


// GetCredentialsByUsername loads the authentication record for login.
func (h *DB) GetCredentialsByUsername(username string) (*UserCredentials, error) {
	r, err := h.q.GetUserCredentialsByUsername(context.Background(), username)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get credentials: %w", err)
	}
	return credentialsFromRow(userCredentialsRow{
		ID: r.ID, Username: r.Username, DisplayName: r.DisplayName,
		PasswordHash: r.PasswordHash, Role: r.Role,
		IsActive: r.IsActive, MustChangePassword: r.MustChangePassword,
		TotpSecret: r.TotpSecret, TotpEnabled: r.TotpEnabled,
		RecoveryCodesJson: r.RecoveryCodesJson, FailedAttempts: r.FailedAttempts,
		LockedUntil: r.LockedUntil,
	}), nil
}

// GetCredentialsByID loads the authentication record by user id.
func (h *DB) GetCredentialsByID(id string) (*UserCredentials, error) {
	r, err := h.q.GetUserCredentialsByID(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get credentials: %w", err)
	}
	return credentialsFromRow(userCredentialsRow{
		ID: r.ID, Username: r.Username, DisplayName: r.DisplayName,
		PasswordHash: r.PasswordHash, Role: r.Role,
		IsActive: r.IsActive, MustChangePassword: r.MustChangePassword,
		TotpSecret: r.TotpSecret, TotpEnabled: r.TotpEnabled,
		RecoveryCodesJson: r.RecoveryCodesJson, FailedAttempts: r.FailedAttempts,
		LockedUntil: r.LockedUntil,
	}), nil
}

// ConsumeRecoveryCode replaces the stored recovery code hashes after one is
// used (the consumed code is removed from the list by the caller).
func (h *DB) ConsumeRecoveryCode(id string, remainingHashes []string) error {
	if remainingHashes == nil {
		remainingHashes = []string{}
	}
	conv, _ := json.Marshal(remainingHashes)
	err := h.q.ConsumeRecoveryCode(context.Background(), db.ConsumeRecoveryCodeParams{
		ID:                id,
		RecoveryCodesJson: string(conv),
		UpdatedAt:         nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("consume recovery code: %w", err)
	}
	return nil
}

// ListUsers returns all accounts.
func (h *DB) ListUsers() ([]*domain.User, error) {
	rows, err := h.q.ListUsers(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list users: %w", err)
	}
	out := make([]*domain.User, 0, len(rows))
	for _, row := range rows {
		out = append(out, userToDomain(row))
	}
	return out, nil
}

// UpdateUserProfile mutates display name / role / active / must-change flags.
// All fields are set; callers hydrate the current values first.
func (h *DB) UpdateUserProfile(id string, displayName *string, role *string, isActive *bool, mustChange *bool) error {
	current, err := h.GetUser(id)
	if err != nil {
		return err
	}
	name := current.DisplayName
	if displayName != nil {
		name = *displayName
	}
	roleName := current.Role
	if role != nil {
		roleName = *role
	}
	active := current.IsActive
	if isActive != nil {
		active = *isActive
	}
	mustChangeFlag := current.MustChangePassword
	if mustChange != nil {
		mustChangeFlag = *mustChange
	}
	err = h.q.UpdateUserProfile(context.Background(), db.UpdateUserProfileParams{
		ID:                 id,
		DisplayName:        name,
		Role:               roleName,
		IsActive:           int32(boolToInt(active)),
		MustChangePassword: int32(boolToInt(mustChangeFlag)),
		UpdatedAt:          nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("update user: %w", err)
	}
	return nil
}

// SetUserPassword replaces the password hash, clears failure counters and
// optionally forces a password change at next login.
func (h *DB) SetUserPassword(id, hash string, mustChange bool) error {
	n, err := h.q.SetUserPassword(context.Background(), db.SetUserPasswordParams{
		ID:                 id,
		PasswordHash:       hash,
		MustChangePassword: int32(boolToInt(mustChange)),
		UpdatedAt:          nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("set password: %w", err)
	}
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// SetUserTOTP stores (or clears) TOTP material.
func (h *DB) SetUserTOTP(id string, secret string, enabled bool, recoveryCodes []string) error {
	if recoveryCodes == nil {
		recoveryCodes = []string{}
	}
	conv, _ := json.Marshal(recoveryCodes)
	err := h.q.SetUserTOTP(context.Background(), db.SetUserTOTPParams{
		ID:                id,
		TotpSecret:        secret,
		TotpEnabled:       int32(boolToInt(enabled)),
		RecoveryCodesJson: string(conv),
		UpdatedAt:         nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("set totp: %w", err)
	}
	return nil
}

// RecordLoginSuccess resets failure counters and stamps last_login_at.
func (h *DB) RecordLoginSuccess(id string) error {
	err := h.q.RecordLoginSuccess(context.Background(), db.RecordLoginSuccessParams{
		ID:          id,
		LastLoginAt: nullString(nowRFC3339()),
	})
	if err != nil {
		return fmt.Errorf("record login success: %w", err)
	}
	return nil
}

// RecordLoginFailure increments the failure counter and applies lockout when
// maxAttempts is reached; returns the new locked-until time (nil = not locked).
func (h *DB) RecordLoginFailure(id string, maxAttempts, lockoutMinutes int) (*time.Time, error) {
	n, err := h.q.BumpFailedAttempts(context.Background(), db.BumpFailedAttemptsParams{
		ID:        id,
		UpdatedAt: nowRFC3339(),
	})
	if err != nil {
		return nil, fmt.Errorf("bump failed attempts: %w", err)
	}
	if n == 0 {
		return nil, ErrNotFound
	}
	failed, err := h.q.GetUserFailedAttempts(context.Background(), id)
	if err != nil {
		return nil, fmt.Errorf("get failed attempts: %w", err)
	}
	if int(failed) < maxAttempts {
		return nil, nil
	}
	lockUntil := nowUTC().Add(time.Duration(lockoutMinutes) * time.Minute)
	if err := h.q.SetUserLockedUntil(context.Background(), db.SetUserLockedUntilParams{
		ID:          id,
		LockedUntil: nullString(formatRFC3339(lockUntil)),
	}); err != nil {
		return nil, fmt.Errorf("lock user: %w", err)
	}
	return &lockUntil, nil
}

// DeleteUser removes an account.
func (h *DB) DeleteUser(id string) (bool, error) {
	n, err := h.q.DeleteUser(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete user: %w", err)
	}
	return n > 0, nil
}

// isUniqueViolation detects duplicate-key errors across drivers.
func (h *DB) isUniqueViolation(err error) bool {
	if err == nil {
		return false
	}
	msg := err.Error()
	return containsFold(msg, "unique") || containsFold(msg, "duplicate key") || containsFold(msg, "duplicate entry")
}

func containsFold(s, sub string) bool {
	return len(sub) > 0 && len(s) >= len(sub) && indexFold(s, sub) >= 0
}

func indexFold(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if equalFold(s[i:i+len(sub)], sub) {
			return i
		}
	}
	return -1
}

func equalFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := 0; i < len(a); i++ {
		ca, cb := a[i], b[i]
		if 'A' <= ca && ca <= 'Z' {
			ca += 32
		}
		if 'A' <= cb && cb <= 'Z' {
			cb += 32
		}
		if ca != cb {
			return false
		}
	}
	return true
}

// --- user groups ---

// CreateUserGroup inserts a group.
func (h *DB) CreateUserGroup(g *domain.UserGroup) error {
	if g.CreatedAt.IsZero() {
		g.CreatedAt = nowUTC()
	}
	err := h.q.CreateUserGroup(context.Background(), db.CreateUserGroupParams{
		ID:          g.ID,
		Name:        g.Name,
		Description: g.Description,
		CreatedAt:   formatRFC3339(g.CreatedAt),
	})
	if err != nil {
		if h.isUniqueViolation(err) {
			return errors.New("group name already exists")
		}
		return fmt.Errorf("create group: %w", err)
	}
	return nil
}

// GetUserGroup fetches one group.
func (h *DB) GetUserGroup(id string) (*domain.UserGroup, error) {
	g, err := h.q.GetUserGroup(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get group: %w", err)
	}
	out := &domain.UserGroup{ID: g.ID, Name: g.Name, Description: g.Description}
	out.CreatedAt, _ = parseRFC3339(g.CreatedAt)
	return out, nil
}

// ListUserGroups returns all groups.
func (h *DB) ListUserGroups() ([]*domain.UserGroup, error) {
	rows, err := h.q.ListUserGroups(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list groups: %w", err)
	}
	out := make([]*domain.UserGroup, 0, len(rows))
	for _, g := range rows {
		ug := &domain.UserGroup{ID: g.ID, Name: g.Name, Description: g.Description}
		ug.CreatedAt, _ = parseRFC3339(g.CreatedAt)
		out = append(out, ug)
	}
	return out, nil
}

// UpdateUserGroup mutates name/description.
func (h *DB) UpdateUserGroup(id string, name *string, description *string) error {
	current, err := h.GetUserGroup(id)
	if err != nil {
		return err
	}
	newName := current.Name
	if name != nil {
		newName = *name
	}
	newDesc := current.Description
	if description != nil {
		newDesc = *description
	}
	n, err := h.q.UpdateUserGroup(context.Background(), db.UpdateUserGroupParams{
		ID:          id,
		Name:        newName,
		Description: newDesc,
	})
	if err != nil {
		if h.isUniqueViolation(err) {
			return errors.New("group name already exists")
		}
		return fmt.Errorf("update group: %w", err)
	}
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteUserGroup removes a group (grants cascade).
func (h *DB) DeleteUserGroup(id string) (bool, error) {
	n, err := h.q.DeleteUserGroup(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete group: %w", err)
	}
	return n > 0, nil
}

// AddGroupMember links a user to a group (idempotent).
func (h *DB) AddGroupMember(groupID, userID string) error {
	err := h.q.AddGroupMember(context.Background(), db.AddGroupMemberParams{
		GroupID: groupID,
		UserID:  userID,
	})
	if err != nil {
		return fmt.Errorf("add group member: %w", err)
	}
	return nil
}

// RemoveGroupMember unlinks a user from a group.
func (h *DB) RemoveGroupMember(groupID, userID string) (bool, error) {
	n, err := h.q.RemoveGroupMember(context.Background(), db.RemoveGroupMemberParams{
		GroupID: groupID,
		UserID:  userID,
	})
	if err != nil {
		return false, fmt.Errorf("remove group member: %w", err)
	}
	return n > 0, nil
}

// ListGroupMembers returns the users of a group.
func (h *DB) ListGroupMembers(groupID string) ([]*domain.User, error) {
	rows, err := h.q.ListGroupMembers(context.Background(), groupID)
	if err != nil {
		return nil, fmt.Errorf("list group members: %w", err)
	}
	out := make([]*domain.User, 0, len(rows))
	for _, u := range rows {
		member := &domain.User{
			ID:                 u.ID,
			Username:           u.Username,
			DisplayName:        u.DisplayName,
			Role:               u.Role,
			IsActive:           u.IsActive == 1,
			MustChangePassword: u.MustChangePassword == 1,
			TOTPEnabled:        u.TotpEnabled == 1,
		}
		member.CreatedAt, _ = parseRFC3339(u.CreatedAt)
		member.UpdatedAt, _ = parseRFC3339(u.UpdatedAt)
		if u.LastLoginAt.Valid {
			if t, ok := parseRFC3339(u.LastLoginAt.String); ok {
				member.LastLoginAt = &t
			}
		}
		out = append(out, member)
	}
	return out, nil
}


// --- host grants ---

// CreateHostGrant inserts an access grant.
func (h *DB) CreateHostGrant(g *domain.HostGrant) error {
	if g.CreatedAt.IsZero() {
		g.CreatedAt = nowUTC()
	}
	if g.GroupName == "" {
		return errors.New("grant must target a machine group")
	}
	err := h.q.CreateHostGrant(context.Background(), db.CreateHostGrantParams{
		ID:          g.ID,
		UserGroupID: g.UserGroupID,
		GroupName:   g.GroupName,
		CanConnect:  int32(boolToInt(g.CanConnect)),
		CanExecute:  int32(boolToInt(g.CanExecute)),
		CanTransfer: int32(boolToInt(g.CanTransfer)),
		CreatedBy:   g.CreatedBy,
		CreatedAt:   formatRFC3339(g.CreatedAt),
	})
	if err != nil {
		return fmt.Errorf("create grant: %w", err)
	}
	return nil
}

// ListHostGrants returns all grants with their groups hydrated.
func (h *DB) ListHostGrants() ([]*domain.HostGrant, error) {
	rows, err := h.q.ListHostGrants(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list grants: %w", err)
	}
	out := make([]*domain.HostGrant, 0, len(rows))
	for _, g := range rows {
		grant := &domain.HostGrant{
			ID:          g.ID,
			UserGroupID: g.UserGroupID,
			GroupName:   g.GroupName,
			CanConnect:  g.CanConnect == 1,
			CanExecute:  g.CanExecute == 1,
			CanTransfer: g.CanTransfer == 1,
			CreatedBy:   g.CreatedBy,
		}
		grant.CreatedAt, _ = parseRFC3339(g.CreatedAt)
		ug := &domain.UserGroup{ID: g.UgID, Name: g.UgName, Description: g.UgDescription}
		ug.CreatedAt, _ = parseRFC3339(g.UgCreatedAt)
		grant.UserGroup = ug
		out = append(out, grant)
	}
	return out, nil
}

// UpdateHostGrant toggles the connect/execute/transfer bits.
func (h *DB) UpdateHostGrant(id string, canConnect, canExecute, canTransfer *bool) error {
	grants, err := h.ListHostGrants()
	if err != nil {
		return err
	}
	var current *domain.HostGrant
	for _, g := range grants {
		if g.ID == id {
			current = g
			break
		}
	}
	if current == nil {
		return ErrNotFound
	}
	connect := current.CanConnect
	if canConnect != nil {
		connect = *canConnect
	}
	execute := current.CanExecute
	if canExecute != nil {
		execute = *canExecute
	}
	transfer := current.CanTransfer
	if canTransfer != nil {
		transfer = *canTransfer
	}
	n, err := h.q.UpdateHostGrant(context.Background(), db.UpdateHostGrantParams{
		ID:          id,
		CanConnect:  int32(boolToInt(connect)),
		CanExecute:  int32(boolToInt(execute)),
		CanTransfer: int32(boolToInt(transfer)),
	})
	if err != nil {
		return fmt.Errorf("update grant: %w", err)
	}
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteHostGrant removes a grant.
func (h *DB) DeleteHostGrant(id string) (bool, error) {
	n, err := h.q.DeleteHostGrant(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete grant: %w", err)
	}
	return n > 0, nil
}

// --- api tokens ---

// CreateApiToken stores a token record (hash only).
func (h *DB) CreateApiToken(t *domain.ApiToken, tokenHash string) error {
	if t.CreatedAt.IsZero() {
		t.CreatedAt = nowUTC()
	}
	var expires sql.NullString
	if t.ExpiresAt != nil {
		expires = nullString(formatRFC3339(*t.ExpiresAt))
	}
	err := h.q.CreateApiToken(context.Background(), db.CreateApiTokenParams{
		ID:        t.ID,
		UserID:    t.UserID,
		Name:      t.Name,
		Prefix:    t.Prefix,
		TokenHash: tokenHash,
		CreatedAt: formatRFC3339(t.CreatedAt),
		ExpiresAt: expires,
	})
	if err != nil {
		return fmt.Errorf("create token: %w", err)
	}
	return nil
}

// GetApiTokenByHash resolves a bearer token (sha256 hex) to its record.
func (h *DB) GetApiTokenByHash(hash string) (*domain.ApiToken, error) {
	t, err := h.q.GetApiTokenByHash(context.Background(), hash)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get token: %w", err)
	}
	out := &domain.ApiToken{
		ID:     t.ID,
		UserID: t.UserID,
		Name:   t.Name,
		Prefix: t.Prefix,
	}
	out.CreatedAt, _ = parseRFC3339(t.CreatedAt)
	if t.LastUsedAt.Valid {
		if v, ok := parseRFC3339(t.LastUsedAt.String); ok {
			out.LastUsedAt = &v
		}
	}
	if t.ExpiresAt.Valid {
		if v, ok := parseRFC3339(t.ExpiresAt.String); ok {
			out.ExpiresAt = &v
		}
	}
	if t.RevokedAt.Valid {
		if v, ok := parseRFC3339(t.RevokedAt.String); ok {
			out.RevokedAt = &v
		}
	}
	return out, nil
}

// TouchApiToken updates last_used_at.
func (h *DB) TouchApiToken(id string) {
	_ = h.q.TouchApiToken(context.Background(), db.TouchApiTokenParams{
		ID:         id,
		LastUsedAt: nullString(nowRFC3339()),
	})
}

// ListApiTokensForUser returns the user's tokens (no hashes).
func (h *DB) ListApiTokensForUser(userID string) ([]*domain.ApiToken, error) {
	rows, err := h.q.ListApiTokensForUser(context.Background(), userID)
	if err != nil {
		return nil, fmt.Errorf("list tokens: %w", err)
	}
	out := make([]*domain.ApiToken, 0, len(rows))
	for _, t := range rows {
		out = append(out, tokenToDomain(t))
	}
	return out, nil
}

func tokenToDomain(t db.ApiToken) *domain.ApiToken {
	out := &domain.ApiToken{
		ID:     t.ID,
		UserID: t.UserID,
		Name:   t.Name,
		Prefix: t.Prefix,
	}
	out.CreatedAt, _ = parseRFC3339(t.CreatedAt)
	if t.LastUsedAt.Valid {
		if v, ok := parseRFC3339(t.LastUsedAt.String); ok {
			out.LastUsedAt = &v
		}
	}
	if t.ExpiresAt.Valid {
		if v, ok := parseRFC3339(t.ExpiresAt.String); ok {
			out.ExpiresAt = &v
		}
	}
	if t.RevokedAt.Valid {
		if v, ok := parseRFC3339(t.RevokedAt.String); ok {
			out.RevokedAt = &v
		}
	}
	return out
}

// RevokeApiToken marks a token revoked (only by its owner).
func (h *DB) RevokeApiToken(id, userID string) (bool, error) {
	n, err := h.q.RevokeApiToken(context.Background(), db.RevokeApiTokenParams{
		ID:        id,
		UserID:    userID,
		RevokedAt: nullString(nowRFC3339()),
	})
	if err != nil {
		return false, fmt.Errorf("revoke token: %w", err)
	}
	return n > 0, nil
}

// --- sessions ---

// CreateSession stores a web session (token stored as sha256 hash).
func (h *DB) CreateSession(s *domain.UserSession, tokenHash string, absoluteExpires time.Time) error {
	err := h.q.CreateSession(context.Background(), db.CreateSessionParams{
		ID:                s.ID,
		UserID:            s.UserID,
		TokenHash:         tokenHash,
		Ip:                s.IP,
		UserAgent:         s.UserAgent,
		CreatedAt:         formatRFC3339(s.CreatedAt),
		LastSeenAt:        formatRFC3339(s.LastSeenAt),
		ExpiresAt:         formatRFC3339(s.ExpiresAt),
		AbsoluteExpiresAt: formatRFC3339(absoluteExpires),
	})
	if err != nil {
		return fmt.Errorf("create session: %w", err)
	}
	return nil
}

// GetSessionByHash resolves a session cookie value (sha256 hex) to the session.
func (h *DB) GetSessionByHash(hash string) (*domain.UserSession, error) {
	s, err := h.q.GetSessionByHash(context.Background(), hash)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get session: %w", err)
	}
	out := &domain.UserSession{
		ID:        s.ID,
		UserID:    s.UserID,
		IP:        s.Ip,
		UserAgent: s.UserAgent,
	}
	out.CreatedAt, _ = parseRFC3339(s.CreatedAt)
	out.LastSeenAt, _ = parseRFC3339(s.LastSeenAt)
	out.ExpiresAt, _ = parseRFC3339(s.ExpiresAt)
	return out, nil
}

// TouchSession slides the idle expiry window forward.
func (h *DB) TouchSession(id string, newExpiry time.Time) {
	_ = h.q.TouchSession(context.Background(), db.TouchSessionParams{
		ID:         id,
		LastSeenAt: nowRFC3339(),
		ExpiresAt:  formatRFC3339(newExpiry),
	})
}

// DeleteSession removes one session.
func (h *DB) DeleteSession(id string) (bool, error) {
	n, err := h.q.DeleteSession(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete session: %w", err)
	}
	return n > 0, nil
}

// DeleteSessionsForUser removes every session of a user.
func (h *DB) DeleteSessionsForUser(userID string) (int64, error) {
	n, err := h.q.DeleteSessionsForUser(context.Background(), userID)
	if err != nil {
		return 0, fmt.Errorf("delete sessions: %w", err)
	}
	return n, nil
}

func sessionRowsToDomain(rows []db.Session) []*domain.UserSession {
	out := make([]*domain.UserSession, 0, len(rows))
	for _, s := range rows {
		item := &domain.UserSession{
			ID:        s.ID,
			UserID:    s.UserID,
			IP:        s.Ip,
			UserAgent: s.UserAgent,
		}
		item.CreatedAt, _ = parseRFC3339(s.CreatedAt)
		item.LastSeenAt, _ = parseRFC3339(s.LastSeenAt)
		item.ExpiresAt, _ = parseRFC3339(s.ExpiresAt)
		out = append(out, item)
	}
	return out
}

// ListSessionsForUser returns a user's active sessions.
func (h *DB) ListSessionsForUser(userID string) ([]*domain.UserSession, error) {
	rows, err := h.q.ListSessionsForUser(context.Background(), userID)
	if err != nil {
		return nil, fmt.Errorf("list sessions: %w", err)
	}
	return sessionRowsToDomain(rows), nil
}

// ListAllSessions returns every active session (admin) with usernames joined.
func (h *DB) ListAllSessions() ([]*domain.UserSession, error) {
	rows, err := h.q.ListAllSessions(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list all sessions: %w", err)
	}
	out := make([]*domain.UserSession, 0, len(rows))
	for _, s := range rows {
		item := &domain.UserSession{
			ID:        s.ID,
			UserID:    s.UserID,
			Username:  s.Username,
			IP:        s.Ip,
			UserAgent: s.UserAgent,
		}
		item.CreatedAt, _ = parseRFC3339(s.CreatedAt)
		item.LastSeenAt, _ = parseRFC3339(s.LastSeenAt)
		item.ExpiresAt, _ = parseRFC3339(s.ExpiresAt)
		out = append(out, item)
	}
	return out, nil
}

// authAttemptsRetention bounds how long login-attempt evidence is kept. It must
// exceed the largest brute-force window (per-IP + per-user) so the rate limiter
// keeps its counters between cleanup ticks.
const authAttemptsRetention = 24 * time.Hour

// CleanupExpired removes expired sessions and stale auth attempts.
func (h *DB) CleanupExpired() (int64, error) {
	now := nowRFC3339()
	sessions, err := h.q.CleanupExpiredSessions(context.Background(), now)
	if err != nil {
		return 0, fmt.Errorf("cleanup sessions: %w", err)
	}
	cutoff := time.Now().UTC().Add(-authAttemptsRetention).Format(time.RFC3339)
	attempts, err := h.q.CleanupStaleAuthAttempts(context.Background(), cutoff)
	if err != nil {
		return sessions, fmt.Errorf("cleanup attempts: %w", err)
	}
	return sessions + attempts, nil
}

// --- auth attempts (brute-force evidence) ---

// RecordAuthAttempt persists one login attempt outcome.
func (h *DB) RecordAuthAttempt(username, ip string, success bool) error {
	err := h.q.RecordAuthAttempt(context.Background(), db.RecordAuthAttemptParams{
		Username:  username,
		Ip:        ip,
		Success:   int32(boolToInt(success)),
		CreatedAt: nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("record auth attempt: %w", err)
	}
	return nil
}

// CountFailedAttemptsByUser counts recent failures for a username in the window.
func (h *DB) CountFailedAttemptsByUser(username string, since time.Time) (int, error) {
	n, err := h.q.CountFailedAttemptsByUser(context.Background(), db.CountFailedAttemptsByUserParams{
		Username:  username,
		CreatedAt: formatRFC3339(since),
	})
	return int(n), err
}

// CountFailedAttemptsByIP counts recent failures for a source IP in the window.
func (h *DB) CountFailedAttemptsByIP(ip string, since time.Time) (int, error) {
	n, err := h.q.CountFailedAttemptsByIP(context.Background(), db.CountFailedAttemptsByIPParams{
		Ip:        ip,
		CreatedAt: formatRFC3339(since),
	})
	return int(n), err
}
