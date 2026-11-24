package security

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
)

func intPtr(i int) *int       { return &i }
func boolPtr(b bool) *bool    { return &b }
func strPtr(s string) *string { return &s }
func testNow() time.Time      { return time.Now() }

func testReviewer(config domain.L2ProviderConfig) *L2Reviewer {
	return NewL2Reviewer(nil, config)
}

func baseL2Config() domain.L2ProviderConfig {
	cfg := domain.DefaultL2ProviderConfig()
	cfg.Enabled = true
	cfg.RiskThresholdAsk = 40
	cfg.RiskThresholdDeny = 85
	return cfg
}

func TestThresholdsEscalateHighRiskToDeny(t *testing.T) {
	r := testReviewer(baseL2Config())
	allow := "allow"
	deny := "deny"
	out := r.finalize(modelVerdict{Decision: &allow, RiskScore: intPtr(90), IntentAligned: boolPtr(true), Reasoning: strPtr("Risky")}, testNow())
	if out.Decision != domain.DecisionDeny {
		t.Fatalf("risk 90 must deny even when model says allow, got %s", out.Decision)
	}
	_ = deny
}

func TestThresholdsEscalateMisalignedIntent(t *testing.T) {
	r := testReviewer(baseL2Config())
	allow := "allow"
	out := r.finalize(modelVerdict{Decision: &allow, RiskScore: intPtr(15), IntentAligned: boolPtr(false), Reasoning: strPtr("Mismatch")}, testNow())
	if out.Decision != domain.DecisionAskApproval {
		t.Fatalf("misaligned intent must ask, got %s", out.Decision)
	}
}

func TestThresholdsLowRiskAlignedAllows(t *testing.T) {
	r := testReviewer(baseL2Config())
	allow := "allow"
	out := r.finalize(modelVerdict{Decision: &allow, RiskScore: intPtr(5), IntentAligned: boolPtr(true), Reasoning: strPtr("OK")}, testNow())
	if out.Decision != domain.DecisionAllow {
		t.Fatalf("aligned low-risk allow must stay allow, got %s", out.Decision)
	}
}

func TestFailClosedWhenProviderUnreachable(t *testing.T) {
	cfg := baseL2Config()
	cfg.Protocol = domain.L2SystemOneFast
	cfg.BaseURL = "http://127.0.0.1:59999/unreachable"
	cfg.TimeoutMs = 250
	r := testReviewer(cfg)

	req := &domain.SemanticReviewRequest{
		ServerName: "prod-db",
		ServerTags: []string{"prod"},
		Command:    "ls -la",
		Intent:     "list files",
		L1Verdict:  domain.CommandVerdict{Decision: domain.DecisionAllow, ParseOK: true},
	}
	v, err := r.Review(context.Background(), req)
	if err != nil {
		t.Fatalf("review must not error on unreachable provider: %v", err)
	}
	if !v.FallbackTriggered || v.Decision != domain.DecisionAskApproval {
		t.Fatalf("fail-closed verdict wrong: %+v", v)
	}
}

func TestParseModelJSONExtractsEmbeddedJSON(t *testing.T) {
	v, err := parseModelJSON(`Sure! {"decision": "deny", "risk_score": 95, "intent_aligned": false, "reasoning": "bad"} end`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if v.Decision == nil || *v.Decision != "deny" || v.RiskScore == nil || *v.RiskScore != 95 {
		t.Fatalf("parsed verdict wrong: %+v", v)
	}
}

func TestSystemOneEndpointResolution(t *testing.T) {
	cases := []struct {
		in   string
		want string
	}{
		{"https://fastvibe.dev", "https://fastvibe.dev/v1/systemone"},
		{"https://fastvibe.dev/", "https://fastvibe.dev/v1/systemone"},
		{"https://fastvibe.dev/v1", "https://fastvibe.dev/v1/systemone"},
		{"https://fastvibe.dev/v1/systemone", "https://fastvibe.dev/v1/systemone"},
	}
	for _, tc := range cases {
		if got := systemOneEndpoint(tc.in); got != tc.want {
			t.Fatalf("systemOneEndpoint(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestSystemOneAllowAndEscalateByViolationMass(t *testing.T) {
	var lastReq map[string]any
	var nextResp map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/systemone" {
			http.Error(w, "wrong path: "+r.URL.Path, http.StatusNotFound)
			return
		}
		_ = json.NewDecoder(r.Body).Decode(&lastReq)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(nextResp)
	}))
	defer srv.Close()

	cfg := baseL2Config()
	cfg.Protocol = domain.L2SystemOneFast
	cfg.BaseURL = srv.URL
	cfg.Model = "jev-latest"
	cfg.RiskThresholdAsk = 30 // 30% violation mass threshold
	r := testReviewer(cfg)

	req := &domain.SemanticReviewRequest{
		ServerName: "prod-1",
		ServerTags: []string{"prod"},
		Command:    "ls -la",
		Intent:     "inspect directory",
		L1Verdict:  domain.CommandVerdict{Decision: domain.DecisionAllow, ParseOK: true},
	}

	// 1. Safe command: routine_work 96%, negative mass 4% < 30% -> allow.
	nextResp = map[string]any{
		"model": "jev-1.14.0",
		"answers": map[string]any{
			"outcome": map[string]any{
				"type":       "choice",
				"choice":     "routine_work",
				"confidence": 0.96,
				"probabilities": map[string]any{
					"routine_work":             0.96,
					"irreversible_destruction": 0.02,
					"secret_exposure":          0.02,
				},
			},
		},
	}
	v, err := r.Review(context.Background(), req)
	if err != nil {
		t.Fatal(err)
	}
	if v.Decision != domain.DecisionAllow || v.FallbackTriggered || v.RiskScore != 0 {
		t.Fatalf("expected allow without risk_score, got %+v", v)
	}
	if !strings.Contains(v.Reasoning, "routine_work") {
		t.Fatalf("expected reasoning to name routine_work, got %q", v.Reasoning)
	}
	if _, ok := lastReq["questions"]; !ok {
		t.Fatalf("request must contain SystemOne questions, got %+v", lastReq)
	}

	// 2. Split violation mass: routine_work wins argmax (65%), but two negative
	// categories sum to 35% >= 30% -> must escalate to ask_approval.
	nextResp = map[string]any{
		"model": "jev-1.14.0",
		"answers": map[string]any{
			"outcome": map[string]any{
				"type":       "choice",
				"choice":     "routine_work",
				"confidence": 0.65,
				"probabilities": map[string]any{
					"routine_work":             0.65,
					"irreversible_destruction": 0.20,
					"security_weakened":        0.15,
				},
			},
		},
	}
	v2, err := r.Review(context.Background(), req)
	if err != nil {
		t.Fatal(err)
	}
	if v2.Decision != domain.DecisionAskApproval || v2.FallbackTriggered {
		t.Fatalf("expected escalate on 35%% violation mass, got %+v", v2)
	}
	if !strings.Contains(v2.Reasoning, "35%") || !strings.Contains(v2.Reasoning, "irreversible_destruction") {
		t.Fatalf("unexpected reasoning: %q", v2.Reasoning)
	}
}
