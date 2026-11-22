package domain

import "time"

// Policy modes for interactive terminal sessions.
const (
	TerminalModeAudit   = "audit"
	TerminalModeEnforce = "enforce"
)

// TerminalSession is one recorded interactive PTY session (replayable from
// the Audit page).
type TerminalSession struct {
	ID          string     `json:"id"`
	UserID      string     `json:"user_id"`
	Username    string     `json:"username"`
	ServerID    string     `json:"server_id"`
	ServerName  string     `json:"server_name"`
	SSHUser     string     `json:"ssh_user"`
	ServerHost  string     `json:"server_host"`
	ClientIP    string     `json:"client_ip"`
	PolicyMode  string     `json:"policy_mode"`
	OpenedAt    time.Time  `json:"opened_at"`
	ClosedAt    *time.Time `json:"closed_at,omitempty"`
	CloseReason string     `json:"close_reason"`
	ByteSize    int64      `json:"byte_size"`
	EventCount  int64      `json:"event_count"`
	Truncated   bool       `json:"truncated"`
}

// TerminalEvent is one chunk of a recorded session. Direction is "output"
// (remote PTY stream), "input" (submitted command line) or "notice"
// (server-side policy message).
type TerminalEvent struct {
	Seq       int64  `json:"seq"`
	TsMs      int64  `json:"ts_ms"`
	Direction string `json:"direction"`
	Data      string `json:"data"`
}

// TerminalSessionQuery filters recorded terminal sessions.
type TerminalSessionQuery struct {
	UserID   string `json:"user_id,omitempty"`
	ServerID string `json:"server_id,omitempty"`
	Username string `json:"username,omitempty"`
	Limit    int    `json:"limit,omitempty"`
	Offset   int    `json:"offset,omitempty"`
}
