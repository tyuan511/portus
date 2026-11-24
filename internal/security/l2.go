package security

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
)

const systemPrompt = `You are Bulwark L2 Security Reviewer.
Evaluate whether the shell command matches the stated AI intent and assess operational risk.
Respond strictly in JSON with keys:
- "decision": "allow" | "ask_approval" | "deny"
- "risk_score": integer 0..100
- "intent_aligned": boolean
- "reasoning": concise explanation`

// Reviewer is the unified L2 semantic review interface.
type Reviewer interface {
	Review(ctx context.Context, req *domain.SemanticReviewRequest) (*domain.SemanticReviewVerdict, error)
}

// L2Reviewer dispatches to LLM protocols or SystemOne fast-decision endpoints.
// Strictly fail-closed: any error escalates to ask_approval via the caller.
type L2Reviewer struct {
	http   *http.Client
	vault  SecretResolver
	config domain.L2ProviderConfig
}

// SecretResolver resolves the provider API key from the vault.
type SecretResolver interface {
	GetSecret(id string) (string, error)
}

// NewL2Reviewer builds a reviewer for the given provider configuration.
func NewL2Reviewer(vault SecretResolver, config domain.L2ProviderConfig) *L2Reviewer {
	return &L2Reviewer{
		http:   &http.Client{},
		vault:  vault,
		config: config,
	}
}

func systemOneEndpoint(base string) string {
	trimmed := strings.TrimRight(strings.TrimSpace(base), "/")
	if strings.HasSuffix(trimmed, "/v1/systemone") {
		return trimmed
	}
	if strings.HasSuffix(trimmed, "/v1") {
		return trimmed + "/systemone"
	}
	return trimmed + "/v1/systemone"
}

func (r *L2Reviewer) resolveAPIKey() string {
	if r.vault == nil || strings.TrimSpace(r.config.APIKeyCredentialID) == "" {
		return ""
	}
	key, err := r.vault.GetSecret(r.config.APIKeyCredentialID)
	if err != nil {
		return ""
	}
	return key
}

// modelVerdict is the normalized JSON verdict any provider must produce.
type modelVerdict struct {
	Decision      *string `json:"decision"`
	RiskScore     *int    `json:"risk_score"`
	IntentAligned *bool   `json:"intent_aligned"`
	Reasoning     *string `json:"reasoning"`
}

// Review performs the semantic review; always returns a verdict (fail-closed
// to ask_approval on any transport/parse error), with fallback metadata set.
func (r *L2Reviewer) Review(ctx context.Context, req *domain.SemanticReviewRequest) (*domain.SemanticReviewVerdict, error) {
	started := time.Now()
	if r.config.Protocol == domain.L2SystemOneFast {
		verdict, err := r.reviewSystemOne(ctx, req, started)
		if err != nil {
			return &domain.SemanticReviewVerdict{
				Decision:          domain.DecisionAskApproval,
				RiskScore:         0,
				IntentAligned:     false,
				Reasoning:         fmt.Sprintf("L2 fail-closed fallback triggered: %v", err),
				ProviderName:      r.config.Name,
				Protocol:          r.config.Protocol,
				Model:             r.config.Model,
				LatencyMs:         time.Since(started).Milliseconds(),
				FallbackTriggered: true,
			}, nil
		}
		return verdict, nil
	}
	raw, err := r.callProvider(ctx, req)
	if err != nil {
		verdict := r.finalize(modelVerdict{}, started)
		verdict.Decision = domain.DecisionAskApproval
		verdict.RiskScore = 75
		verdict.IntentAligned = false
		verdict.Reasoning = fmt.Sprintf("L2 fail-closed fallback triggered: %v", err)
		verdict.FallbackTriggered = true
		return verdict, nil
	}
	return r.finalize(raw, started), nil
}

func (r *L2Reviewer) callProvider(ctx context.Context, req *domain.SemanticReviewRequest) (modelVerdict, error) {
	timeout := time.Duration(r.config.TimeoutMs) * time.Millisecond
	if timeout < 200*time.Millisecond {
		timeout = 200 * time.Millisecond
	}
	callCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	apiKey := r.resolveAPIKey()
	userPrompt := buildUserPrompt(req)

	var url string
	var body map[string]any
	var headers map[string]string
	extract := func(val map[string]any) (string, error) { return "", nil }

	switch r.config.Protocol {
	case domain.L2OpenAiChat:
		url = strings.TrimRight(r.config.BaseURL, "/") + "/chat/completions"
		body = map[string]any{
			"model":           r.config.Model,
			"temperature":     0.0,
			"response_format": map[string]string{"type": "json_object"},
			"messages": []map[string]string{
				{"role": "system", "content": systemPrompt},
				{"role": "user", "content": userPrompt},
			},
		}
		extract = func(val map[string]any) (string, error) {
			s := jsonPathText(val, "choices", 0, "message", "content")
			if s == "" {
				return "", fmt.Errorf("missing choices[0].message.content")
			}
			return s, nil
		}
	case domain.L2OpenAiResponses:
		url = strings.TrimRight(r.config.BaseURL, "/") + "/responses"
		body = map[string]any{
			"model":        r.config.Model,
			"instructions": systemPrompt,
			"input":        userPrompt,
			"text":         map[string]any{"format": map[string]string{"type": "json_object"}},
		}
		extract = func(val map[string]any) (string, error) {
			if s := jsonPathText(val, "output_text"); s != "" {
				return s, nil
			}
			if s := jsonPathText(val, "output", 0, "content", 0, "text"); s != "" {
				return s, nil
			}
			return "", fmt.Errorf("missing output_text in Responses payload")
		}
	case domain.L2AnthropicMsgs:
		url = strings.TrimRight(r.config.BaseURL, "/") + "/messages"
		body = map[string]any{
			"model":       r.config.Model,
			"max_tokens":  512,
			"temperature": 0.0,
			"system":      systemPrompt,
			"messages": []map[string]string{
				{"role": "user", "content": userPrompt},
			},
		}
		headers = map[string]string{"anthropic-version": "2023-06-01"}
		extract = func(val map[string]any) (string, error) {
			s := jsonPathText(val, "content", 0, "text")
			if s == "" {
				return "", fmt.Errorf("missing content[0].text in Anthropic payload")
			}
			return s, nil
		}
	default:
		return modelVerdict{}, fmt.Errorf("unsupported L2 protocol %s", r.config.Protocol)
	}

	val, err := r.doPost(callCtx, url, body, apiKey, headers)
	if err != nil {
		return modelVerdict{}, err
	}
	text, err := extract(val)
	if err != nil {
		return modelVerdict{}, err
	}
	return parseModelJSON(text)
}

func (r *L2Reviewer) doPost(ctx context.Context, url string, body map[string]any, apiKey string, headers map[string]string) (map[string]any, error) {
	payload, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+apiKey)
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := r.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("HTTP error: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read body: %w", err)
	}
	var val map[string]any
	if err := json.Unmarshal(raw, &val); err != nil {
		return nil, fmt.Errorf("JSON decode: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, truncate(string(raw), 300))
	}
	return val, nil
}

// finalize applies risk/intent thresholds on the raw model output.
func (r *L2Reviewer) finalize(raw modelVerdict, started time.Time) *domain.SemanticReviewVerdict {
	risk := 50
	if raw.RiskScore != nil {
		risk = *raw.RiskScore
		if risk > 100 {
			risk = 100
		}
	}
	aligned := false
	if raw.IntentAligned != nil {
		aligned = *raw.IntentAligned
	}
	parsed := domain.DecisionAskApproval
	if raw.Decision != nil {
		switch strings.ToLower(*raw.Decision) {
		case "allow":
			parsed = domain.DecisionAllow
		case "deny":
			parsed = domain.DecisionDeny
		}
	}

	final := parsed
	if risk >= r.config.RiskThresholdDeny {
		final = domain.DecisionDeny
	} else if (!aligned || risk >= r.config.RiskThresholdAsk) && final == domain.DecisionAllow {
		final = domain.DecisionAskApproval
	}

	reasoning := "Evaluated by L2 semantic reviewer"
	if raw.Reasoning != nil && *raw.Reasoning != "" {
		reasoning = *raw.Reasoning
	}
	return &domain.SemanticReviewVerdict{
		Decision:          final,
		RiskScore:         risk,
		IntentAligned:     aligned,
		Reasoning:         reasoning,
		ProviderName:      r.config.Name,
		Protocol:          r.config.Protocol,
		Model:             r.config.Model,
		LatencyMs:         time.Since(started).Milliseconds(),
		FallbackTriggered: false,
	}
}

func buildUserPrompt(req *domain.SemanticReviewRequest) string {
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Target Server: %s (tags: %v)\n", req.ServerName, req.ServerTags))
	sb.WriteString(fmt.Sprintf("Stated Intent: %s\n", req.Intent))
	sb.WriteString(fmt.Sprintf("Command: %s\n", req.Command))
	sb.WriteString(fmt.Sprintf("L1 AST Decision: %s\n", req.L1Verdict.Decision))
	sb.WriteString("Parsed Subcommands:\n")
	for _, c := range req.L1Verdict.ParsedCommands {
		sb.WriteString(fmt.Sprintf("  - %s %s [%s]\n", c.Program, strings.Join(c.Args, " "), c.Context))
	}
	return sb.String()
}

func parseModelJSON(text string) (modelVerdict, error) {
	trimmed := strings.TrimSpace(text)
	if start := strings.Index(trimmed, "{"); start >= 0 {
		if end := strings.LastIndex(trimmed, "}"); end > start {
			trimmed = trimmed[start : end+1]
		}
	}
	var v modelVerdict
	if err := json.Unmarshal([]byte(trimmed), &v); err != nil {
		return v, fmt.Errorf("failed to parse L2 JSON: %w", err)
	}
	return v, nil
}

// jsonPathText walks a decoded JSON map by path segments (int = array index).
func jsonPathText(val map[string]any, path ...any) string {
	var cur any = val
	for _, seg := range path {
		switch s := seg.(type) {
		case string:
			m, ok := cur.(map[string]any)
			if !ok {
				return ""
			}
			cur = m[s]
		case int:
			arr, ok := cur.([]any)
			if !ok || s < 0 || s >= len(arr) {
				return ""
			}
			cur = arr[s]
		}
	}
	str, _ := cur.(string)
	return str
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}

// Compile-time interface check.
var _ Reviewer = (*L2Reviewer)(nil)
