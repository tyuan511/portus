// Package domain holds the shared domain models for Bulwark enterprise edition.
// JSON field names use snake_case to keep the HTTP API contract stable with the
// web frontend and MCP clients.
package domain

// PolicyDecision is the outcome of the layered security policy for a command.
type PolicyDecision string

const (
	// DecisionAllow means safe to execute automatically.
	DecisionAllow PolicyDecision = "allow"
	// DecisionAskApproval requires explicit human-in-the-loop confirmation.
	DecisionAskApproval PolicyDecision = "ask_approval"
	// DecisionDeny is strictly forbidden and blocked immediately.
	DecisionDeny PolicyDecision = "deny"
)

// MoreRestrictive returns the stricter of two decisions
// (deny > ask_approval > allow), mirroring the Rust `max` semantics.
func MoreRestrictive(a, b PolicyDecision) PolicyDecision {
	rank := func(d PolicyDecision) int {
		switch d {
		case DecisionAllow:
			return 0
		case DecisionAskApproval:
			return 1
		case DecisionDeny:
			return 2
		}
		return 1
	}
	if rank(a) >= rank(b) {
		return a
	}
	return b
}

// RuleMatchKind describes how a security rule matches an extracted command.
type RuleMatchKind string

const (
	// MatchProgramExact matches the exact program name (e.g. "ls", "rm").
	MatchProgramExact RuleMatchKind = "program_exact"
	// MatchCommandPrefix matches program + leading subcommand (e.g. "docker ps").
	MatchCommandPrefix RuleMatchKind = "command_prefix"
	// MatchRegex matches the full reconstructed command against a regex.
	MatchRegex RuleMatchKind = "regex"
)

// SecurityRule is a configurable L1 security rule.
type SecurityRule struct {
	ID          string         `json:"id"`
	Name        string         `json:"name"`
	MatchKind   RuleMatchKind  `json:"match_kind"`
	Pattern     string         `json:"pattern"`
	Decision    PolicyDecision `json:"decision"`
	Description string         `json:"description,omitempty"`
	Enabled     bool           `json:"enabled"`
	Priority    int            `json:"priority"`
}

// ParsedCommandInfo is a single command invocation extracted from the shell AST.
type ParsedCommandInfo struct {
	Program     string         `json:"program"`
	Args        []string       `json:"args"`
	Raw         string         `json:"raw"`
	Context     string         `json:"context"`
	Decision    PolicyDecision `json:"decision"`
	MatchedRule string         `json:"matched_rule,omitempty"`
}

// CommandVerdict is the result of the L1 AST security evaluation.
type CommandVerdict struct {
	Decision       PolicyDecision      `json:"decision"`
	ParseOK        bool                `json:"parse_ok"`
	ParsedCommands []ParsedCommandInfo `json:"parsed_commands"`
	MatchedRules   []string            `json:"matched_rules"`
	Reason         string              `json:"reason"`
}

// L2Protocol enumerates the supported L2 semantic reviewer protocols.
type L2Protocol string

const (
	L2OpenAiChat      L2Protocol = "openai_chat"
	L2OpenAiResponses L2Protocol = "openai_responses"
	L2AnthropicMsgs   L2Protocol = "anthropic_messages"
	L2SystemOneFast   L2Protocol = "system_one_fast"
)

// L2ProviderConfig configures the L2 AI semantic review provider.
// The API key itself lives in the vault, referenced by credential id.
type L2ProviderConfig struct {
	ID                 string     `json:"id"`
	Name               string     `json:"name"`
	Protocol           L2Protocol `json:"protocol"`
	BaseURL            string     `json:"base_url"`
	Model              string     `json:"model"`
	APIKeyCredentialID string     `json:"api_key_credential_id,omitempty"`
	TimeoutMs          int64      `json:"timeout_ms"`
	RiskThresholdAsk   int        `json:"risk_threshold_ask"`
	RiskThresholdDeny  int        `json:"risk_threshold_deny"`
	Enabled            bool       `json:"enabled"`
}

// DefaultL2ProviderConfig returns the default (disabled) L2 configuration.
func DefaultL2ProviderConfig() L2ProviderConfig {
	return L2ProviderConfig{
		ID:                "default-l2",
		Name:              "Default L2 Reviewer",
		Protocol:          L2SystemOneFast,
		BaseURL:           "https://fastvibe.dev/v1/systemone",
		Model:             "jev-latest",
		TimeoutMs:         5000,
		RiskThresholdAsk:  30,
		RiskThresholdDeny: 85,
		Enabled:           false,
	}
}

// SemanticReviewRequest is the input to the unified L2 reviewer interface.
type SemanticReviewRequest struct {
	ServerName string         `json:"server_name"`
	ServerTags []string       `json:"server_tags"`
	Command    string         `json:"command"`
	Intent     string         `json:"intent"`
	L1Verdict  CommandVerdict `json:"l1_verdict"`
}

// SemanticReviewVerdict is the structured output from any L2 reviewer.
type SemanticReviewVerdict struct {
	Decision          PolicyDecision `json:"decision"`
	RiskScore         int            `json:"risk_score"`
	IntentAligned     bool           `json:"intent_aligned"`
	Reasoning         string         `json:"reasoning"`
	ProviderName      string         `json:"provider_name"`
	Protocol          L2Protocol     `json:"protocol"`
	Model             string         `json:"model"`
	LatencyMs         int64          `json:"latency_ms"`
	FallbackTriggered bool           `json:"fallback_triggered"`
}
