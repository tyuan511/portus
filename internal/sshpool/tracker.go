package sshpool

import (
	"strings"
)

// maxTrackedCommand caps audit-tracked command length per submission.
const maxTrackedCommand = 4096

// CommandTracker watches raw PTY input bytes and extracts "submitted"
// commands (Enter-terminated lines) for the audit trail. It skips ANSI
// escape/CSI sequences and handles backspace edits, mirroring the behavior
// of the legacy implementation.
type CommandTracker struct {
	line strings.Builder
}

func NewCommandTracker() *CommandTracker { return &CommandTracker{} }

// Feed consumes raw input bytes and returns commands submitted by pressing
// Enter. The bytes themselves must still be forwarded to the PTY.
func (t *CommandTracker) Feed(data []byte) []string {
	var submitted []string
	i := 0
	for i < len(data) {
		b := data[i]
		switch {
		case b == 0x1b: // ESC — skip the escape sequence
			i += skipEscape(data[i:])
		case b == '\r' || b == '\n':
			if cmd := strings.TrimSpace(t.line.String()); cmd != "" {
				submitted = append(submitted, truncate(cmd, maxTrackedCommand))
			}
			t.line.Reset()
			i++
		case b == 0x7f || b == 0x08: // backspace
			s := t.line.String()
			if s != "" {
				t.line.Reset()
				t.line.WriteString(s[:len(s)-1])
			}
			i++
		case b == 0x15: // Ctrl+U — clear line
			t.line.Reset()
			i++
		case b == 0x03: // Ctrl+C — discard line
			t.line.Reset()
			i++
		case b >= 0x20 && b < 0x7f, b >= 0xa0: // printable + UTF-8 passthrough
			t.line.WriteByte(b)
			i++
		default:
			i++ // other control bytes
		}
	}
	return submitted
}

// skipEscape returns how many bytes the ANSI escape sequence occupies.
func skipEscape(data []byte) int {
	if len(data) < 2 {
		return len(data)
	}
	switch data[1] {
	case '[': // CSI ... final byte 0x40-0x7E
		for i := 2; i < len(data); i++ {
			if data[i] >= 0x40 && data[i] <= 0x7e {
				return i + 1
			}
		}
		return len(data)
	case 'O': // SS3
		if len(data) >= 3 {
			return 3
		}
		return len(data)
	default:
		return 2
	}
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
