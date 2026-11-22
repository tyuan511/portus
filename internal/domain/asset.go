package domain

import "time"

// AuthKind is the authentication method metadata for a server asset
// (secrets are never stored here).
type AuthKind string

const (
	// AuthAgentOrDefaultKey uses the local SSH agent or default keys.
	AuthAgentOrDefaultKey AuthKind = "agent_or_default_key"
	// AuthIdentityFile uses a specific private key file path.
	AuthIdentityFile AuthKind = "identity_file"
	// AuthKeyringPassword uses an encrypted vault credential referenced by id.
	AuthKeyringPassword AuthKind = "keyring_password"
)

// ServerAsset is a managed SSH server or bastion jump host.
type ServerAsset struct {
	ID           string   `json:"id"`
	Name         string   `json:"name"`
	Host         string   `json:"host"`
	Port         int      `json:"port"`
	Username     string   `json:"username"`
	AuthKind     AuthKind `json:"auth_kind"`
	IdentityFile string   `json:"identity_file,omitempty"`
	// CredentialID references an encrypted credential in the vault (never a raw secret).
	CredentialID string `json:"credential_id,omitempty"`
	JumpHostID   string `json:"jump_host_id,omitempty"`
	// GroupName is the host group name (e.g. "Production", "Bastion", "Staging").
	GroupName   string   `json:"group_name,omitempty"`
	Tags        []string `json:"tags"`
	Description string   `json:"description,omitempty"`
	Enabled     bool     `json:"enabled"`
}

// AuthorizedServerInfo is the projection of a server asset exposed to MCP
// agents and non-privileged API consumers (no auth metadata).
type AuthorizedServerInfo struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Host        string   `json:"host"`
	Port        int      `json:"port"`
	Username    string   `json:"username"`
	JumpHostID  string   `json:"jump_host_id,omitempty"`
	GroupName   string   `json:"group_name,omitempty"`
	Tags        []string `json:"tags"`
	Description string   `json:"description,omitempty"`
}

func FromAsset(a *ServerAsset) AuthorizedServerInfo {
	return AuthorizedServerInfo{
		ID:          a.ID,
		Name:        a.Name,
		Host:        a.Host,
		Port:        a.Port,
		Username:    a.Username,
		JumpHostID:  a.JumpHostID,
		GroupName:   a.GroupName,
		Tags:        a.Tags,
		Description: a.Description,
	}
}

// VaultCredentialKind enumerates credential types stored in the sealed vault.
type VaultCredentialKind string

const (
	CredPassword   VaultCredentialKind = "password"
	CredPrivateKey VaultCredentialKind = "private_key"
	CredAPIKey     VaultCredentialKind = "api_key"
)

// VaultCredentialMeta is non-secret metadata for a vault credential.
type VaultCredentialMeta struct {
	ID       string              `json:"id"`
	Label    string              `json:"label"`
	Kind     VaultCredentialKind `json:"kind"`
	Username string              `json:"username,omitempty"`
	// Preview is a masked preview (e.g. "sk-***9a2f"), never the plaintext secret.
	Preview   string    `json:"preview"`
	CreatedAt time.Time `json:"created_at"`
}


// CommandSnippet is a reusable command snippet.
type CommandSnippet struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Command     string   `json:"command"`
	Description string   `json:"description,omitempty"`
	Tags        []string `json:"tags"`
}

// CommandOutput is the result of executing a remote command over SSH.
type CommandOutput struct {
	ExitCode   int    `json:"exit_code"`
	Stdout     string `json:"stdout"`
	Stderr     string `json:"stderr"`
	DurationMs int64  `json:"duration_ms"`
}

// ApprovalDecision is the outcome of a human approval request.
type ApprovalDecision string

const (
	ApprovalApproved ApprovalDecision = "approved"
	ApprovalRejected ApprovalDecision = "rejected"
	ApprovalTimedOut ApprovalDecision = "timed_out"
)

// ApprovalRequest is sent to the human approval handler when a command
// is classified as ask_approval.
type ApprovalRequest struct {
	ID         string    `json:"id"`
	TraceID    string    `json:"trace_id"`
	CreatedAt  time.Time `json:"created_at"`
	ServerID   string    `json:"server_id"`
	ServerName string    `json:"server_name"`
	Host       string    `json:"host"`
	// Username is the remote SSH login user (e.g. "root").
	Username    string                 `json:"username"`
	Command     string                 `json:"command"`
	Intent      string                 `json:"intent"`
	Verdict     CommandVerdict         `json:"verdict"`
	L2Verdict   *SemanticReviewVerdict `json:"l2_verdict,omitempty"`
	TimeoutSecs int64                  `json:"timeout_secs"`
	// RequestedBy is the platform user that initiated the execution (multi-user).
	UserID      string `json:"user_id,omitempty"`
	RequestedBy string `json:"requested_by,omitempty"`
}
