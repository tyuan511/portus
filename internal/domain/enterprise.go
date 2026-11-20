package domain

import "time"

// Permission is a fine-grained capability key checked by the RBAC layer.
type Permission string

const (
	PermHostRead        Permission = "host.read"
	PermHostConnect     Permission = "host.connect"
	PermHostExecute     Permission = "host.execute"
	PermHostManage      Permission = "host.manage"
	PermVaultManage     Permission = "vault.manage"
	PermRulesRead       Permission = "rules.read"
	PermRulesManage     Permission = "rules.manage"
	PermSnippetsUse     Permission = "snippets.use"
	PermSnippetsManage  Permission = "snippets.manage"
	PermAuditViewSelf   Permission = "audit.view_self"
	PermAuditViewAll    Permission = "audit.view_all"
	PermApprovalsDecide Permission = "approvals.decide"
	PermMCPAccess       Permission = "mcp.access"
	PermUsersManage     Permission = "users.manage"
	PermAccessManage    Permission = "access.manage"
	PermSettingsManage  Permission = "settings.manage"
)

// AllPermissions lists every permission key. RBAC is an internal mechanism:
// roles are built in and never shown to users as an editable matrix.
func AllPermissions() []Permission {
	return []Permission{
		PermHostRead, PermHostConnect, PermHostExecute, PermHostManage,
		PermVaultManage, PermRulesRead, PermRulesManage,
		PermSnippetsUse, PermSnippetsManage,
		PermAuditViewSelf, PermAuditViewAll, PermApprovalsDecide, PermMCPAccess,
		PermUsersManage, PermAccessManage, PermSettingsManage,
	}
}

// Names of the two built-in roles. Permission sets are fixed in code.
const (
	BuiltinRoleAdmin  = "admin"
	BuiltinRoleMember = "member"
)

// AllRoles lists the built-in roles in UI order.
func AllRoles() []string { return []string{BuiltinRoleAdmin, BuiltinRoleMember} }

// IsValidRole reports whether name is one of the built-in roles.
func IsValidRole(name string) bool {
	return name == BuiltinRoleAdmin || name == BuiltinRoleMember
}

// RolePermissions returns the fixed permission set of a built-in role
// (nil for unknown names).
func RolePermissions(name string) []Permission {
	switch name {
	case BuiltinRoleAdmin:
		return AllPermissions()
	case BuiltinRoleMember:
		// Members only consume the platform: connect to hosts granted via
		// user groups, execute commands, and use MCP.
		return []Permission{PermHostRead, PermHostConnect, PermHostExecute, PermMCPAccess}
	default:
		return nil
	}
}

// User is a platform account. Role is one of the built-in role names.
type User struct {
	ID          string `json:"id"`
	Username    string `json:"username"`
	DisplayName string `json:"display_name"`
	Role        string `json:"role"`
	IsActive    bool   `json:"is_active"`
	// MustChangePassword forces a password change at next login.
	MustChangePassword bool       `json:"must_change_password"`
	TOTPEnabled        bool       `json:"totp_enabled"`
	CreatedAt          time.Time  `json:"created_at"`
	UpdatedAt          time.Time  `json:"updated_at"`
	LastLoginAt        *time.Time `json:"last_login_at,omitempty"`
}

// UserDetails adds administrative fields never returned in bulk listings.
type UserDetails struct {
	User
	FailedAttempts int        `json:"failed_attempts"`
	LockedUntil    *time.Time `json:"locked_until,omitempty"`
}

// UserGroup is a set of users that host grants target.
type UserGroup struct {
	ID          string    `json:"id"`
	Name        string    `json:"name"`
	Description string    `json:"description,omitempty"`
	CreatedAt   time.Time `json:"created_at"`
}

// UserGroupMember links a user into a group.
type UserGroupMember struct {
	GroupID string `json:"group_id"`
	UserID  string `json:"user_id"`
	User    *User  `json:"user,omitempty"`
}

// HostGrant authorizes a user group against a machine group (the group_name
// of servers). Access is provisioned at machine-group granularity only.
type HostGrant struct {
	ID          string     `json:"id"`
	UserGroupID string     `json:"user_group_id"`
	UserGroup   *UserGroup `json:"user_group,omitempty"`
	GroupName   string     `json:"group_name"`
	CanConnect  bool       `json:"can_connect"`
	CanExecute  bool       `json:"can_execute"`
	CreatedBy   string     `json:"created_by,omitempty"`
	CreatedAt   time.Time  `json:"created_at"`
}

// GrantDecision is the effective access level of a user on a specific host.
type GrantDecision struct {
	Allowed    bool `json:"allowed"`
	CanConnect bool `json:"can_connect"`
	CanExecute bool `json:"can_execute"`
}

// ApiToken is a personal access token used by MCP clients (per-user).
type ApiToken struct {
	ID         string     `json:"id"`
	UserID     string     `json:"user_id"`
	Name       string     `json:"name"`
	Prefix     string     `json:"prefix"`
	CreatedAt  time.Time  `json:"created_at"`
	LastUsedAt *time.Time `json:"last_used_at,omitempty"`
	ExpiresAt  *time.Time `json:"expires_at,omitempty"`
	RevokedAt  *time.Time `json:"revoked_at,omitempty"`
}

// UserSession is an active web session (token hash is never exposed).
type UserSession struct {
	ID         string    `json:"id"`
	UserID     string    `json:"user_id"`
	IP         string    `json:"ip"`
	UserAgent  string    `json:"user_agent"`
	CreatedAt  time.Time `json:"created_at"`
	LastSeenAt time.Time `json:"last_seen_at"`
	ExpiresAt  time.Time `json:"expires_at"`
}

// Identity is the resolved per-request authenticated principal.
// It is attached to request contexts (HTTP, WS, MCP) and flows into audits.
type Identity struct {
	UserID      string       `json:"user_id"`
	Username    string       `json:"username"`
	DisplayName string       `json:"display_name"`
	Role        string       `json:"role"`
	Permissions []Permission `json:"permissions"`
	// IsAdmin is true only for the built-in admin role (bypasses grants).
	IsAdmin bool `json:"is_admin"`
	// TOTPEnabled reports whether the account has a second factor enrolled.
	TOTPEnabled bool `json:"totp_enabled"`
}

func (i *Identity) HasPermission(p Permission) bool {
	for _, perm := range i.Permissions {
		if perm == p {
			return true
		}
	}
	return false
}

// TokenAuthKind distinguishes how an identity was authenticated.
type TokenAuthKind string

const (
	AuthKindWebSession TokenAuthKind = "web_session"
	AuthKindApiToken   TokenAuthKind = "api_token"
	AuthKindLocal      TokenAuthKind = "local"
)
