package security

import (
	"regexp"
)

// Masking patterns redact secrets from command output before it reaches AI
// agents, the web UI, or audit previews.
type maskingRule struct {
	re           *regexp.Regexp
	replacement  string
	dollarGroups bool // replacement uses $n group references
}

var maskingRules = []maskingRule{
	{regexp.MustCompile(`(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----`), "[REDACTED_PRIVATE_KEY]", false},
	{regexp.MustCompile(`\bAKIA[0-9A-Z]{16}\b`), "[REDACTED_AWS_ACCESS_KEY]", false},
	{regexp.MustCompile(`\bsk-(?:proj-|ant-|live-|test-)?[A-Za-z0-9_-]{20,}\b`), "[REDACTED_API_KEY]", false},
	{regexp.MustCompile(`(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{16,}`), "${1}[REDACTED_TOKEN]", true},
	{regexp.MustCompile(`(?im)^([A-Z0-9_]*(?:PASSWORD|PASSWD|SECRET|TOKEN|API_KEY|ACCESS_KEY|PRIVATE_KEY)[A-Z0-9_]*\s*=\s*)([^\s#]+)`), "${1}[REDACTED]", true},
	{regexp.MustCompile(`(?i)((?:postgres|postgresql|mysql|mongodb|redis)://[^:\s/]+:)([^@\s]+)(@)`), "${1}[REDACTED]${3}", true},
}

// MaskSensitiveOutput redacts secrets from stdout/stderr content.
func MaskSensitiveOutput(input string) string {
	out := input
	for _, r := range maskingRules {
		out = r.re.ReplaceAllString(out, r.replacement)
	}
	return out
}

// MaskSensitiveBytes is the []byte convenience wrapper.
func MaskSensitiveBytes(b []byte) []byte {
	return []byte(MaskSensitiveOutput(string(b)))
}
