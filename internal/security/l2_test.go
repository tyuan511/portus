package security

import (
	"context"
	"testing"
	"time"

	"github.com/yuantang/sshield/internal/domain"
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
