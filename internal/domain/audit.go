package domain

import "time"

// AuditEvent is a tamper-evident, fully traceable audit log entry.
type AuditEvent struct {
	ID      string `json:"id"`
	TraceID string `json:"trace_id,omitempty"`
	// Actor is the execution channel (e.g. "mcp-http", "terminal", "auth").
	Actor      string    `json:"actor"`
	Timestamp  time.Time `json:"timestamp"`
	ServerID   string    `json:"server_id"`
	ServerName string    `json:"server_name"`
	// SSHUser is the target SSH login username on the remote host.
	SSHUser    string `json:"ssh_user,omitempty"`
	ServerHost string `json:"server_host,omitempty"`
	// Operator is the acting identity string (platform username, OS user, or channel id).
	Operator string `json:"operator,omitempty"`
	// UserID/Username identify the authenticated platform user (multi-user edition).
	// Empty for legacy/local events. These participate in the hash chain only when
	// non-empty, so pre-existing chain records remain verifiable.
	UserID   string `json:"user_id,omitempty"`
	Username string `json:"username,omitempty"`

	Command        string                 `json:"command"`
	Intent         string                 `json:"intent"`
	DryRun         bool                   `json:"dry_run"`
	Decision       PolicyDecision         `json:"decision"`
	L1Verdict      *CommandVerdict        `json:"l1_verdict,omitempty"`
	L2Verdict      *SemanticReviewVerdict `json:"l2_verdict,omitempty"`
	ApprovalID     string                 `json:"approval_id,omitempty"`
	ApprovalStatus ApprovalDecision       `json:"approval_status,omitempty"`
	ExitCode       *int                   `json:"exit_code,omitempty"`
	DurationMs     *int64                 `json:"duration_ms,omitempty"`
	StdoutPreview  string                 `json:"stdout_preview,omitempty"`
	StderrPreview  string                 `json:"stderr_preview,omitempty"`
	Error          string                 `json:"error,omitempty"`
	// PrevHash is the SHA-256 of the previous record ("GENESIS" for the first).
	PrevHash string `json:"prev_hash"`
	// EventHash links prev_hash + canonical event fields.
	EventHash string `json:"event_hash"`
}

// AuditChainStatus is the verification result for the audit hash chain.
type AuditChainStatus struct {
	TotalEvents int    `json:"total_events"`
	Valid       bool   `json:"valid"`
	BrokenAtID  string `json:"broken_at_id,omitempty"`
}

// AuditQuery filters audit log queries.
type AuditQuery struct {
	Q          string         `json:"q,omitempty"`
	ServerID   string         `json:"server_id,omitempty"`
	SSHUser    string         `json:"ssh_user,omitempty"`
	Actor      string         `json:"actor,omitempty"`
	UserID     string         `json:"user_id,omitempty"`
	Username   string         `json:"username,omitempty"`
	Decision   PolicyDecision `json:"decision,omitempty"`
	OnlyFailed bool           `json:"only_failed,omitempty"`
	From       *time.Time     `json:"from,omitempty"`
	To         *time.Time     `json:"to,omitempty"`
	Limit      int            `json:"limit,omitempty"`
	Offset     int            `json:"offset,omitempty"`
}

// AuditCountBucket is a generic name/count aggregation bucket.
type AuditCountBucket struct {
	Key   string `json:"key"`
	Count int64  `json:"count"`
}

// AuditServerBucket aggregates executions per server.
type AuditServerBucket struct {
	ServerID    string `json:"server_id"`
	ServerName  string `json:"server_name"`
	Count       int64  `json:"count"`
	FailedCount int64  `json:"failed_count"`
}

// AuditDailyBucket aggregates per-day activity.
type AuditDailyBucket struct {
	Date   string `json:"date"`
	Total  int64  `json:"total"`
	Denied int64  `json:"denied"`
	Failed int64  `json:"failed"`
}

// AuditStats aggregates recorded command executions.
type AuditStats struct {
	TotalCommands int64               `json:"total_commands"`
	AllowedCount  int64               `json:"allowed_count"`
	ApprovalCount int64               `json:"approval_count"`
	DeniedCount   int64               `json:"denied_count"`
	FailedCount   int64               `json:"failed_count"`
	AvgDurationMs *float64            `json:"avg_duration_ms"`
	ByActor       []AuditCountBucket  `json:"by_actor"`
	ByUser        []AuditCountBucket  `json:"by_user"`
	ByServer      []AuditServerBucket `json:"by_server"`
	TopCommands   []AuditCountBucket  `json:"top_commands"`
	DailyTrend    []AuditDailyBucket  `json:"daily_trend"`
}

// TruncatePreview truncates long output previews for audit storage.
func TruncatePreview(s string, maxChars int) string {
	runes := []rune(s)
	if len(runes) <= maxChars {
		return s
	}
	return string(runes[:maxChars]) + "...[truncated]"
}
