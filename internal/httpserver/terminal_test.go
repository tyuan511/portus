package httpserver

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/tyuan511/bulwark/internal/approvals"
	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/rbac"
	"github.com/tyuan511/bulwark/internal/security"
)

// The interactive-terminal gate is the enforcement point for policy on PTY
// sessions (enforce mode only — audit mode forwards via CommandTracker);
// these tests pin down its byte-level contract.
func TestTerminalGateInactiveEnforce(t *testing.T) {
	// With enforcement off the gate forwards everything untouched and never
	// invokes evaluation.
	g := newTerminalGate(false)
	var submitted []string
	forward := g.process([]byte("ls -la\r"), func(line string) bool {
		submitted = append(submitted, line)
		return true
	})
	if len(submitted) != 0 {
		t.Fatalf("audit mode must not evaluate lines: %v", submitted)
	}
	if string(forward) != "ls -la\r" {
		t.Fatalf("audit mode forwards everything verbatim: %q", forward)
	}
}

func TestTerminalGateAllow(t *testing.T) {
	g := newTerminalGate(true)
	var submitted []string
	forward := g.process([]byte("echo hi\r"), func(line string) bool {
		submitted = append(submitted, line)
		return true
	})
	if len(submitted) != 1 || submitted[0] != "echo hi" {
		t.Fatalf("enforce mode must evaluate the line: %v", submitted)
	}
	// Allowed line: typed bytes AND the Enter reach the remote shell.
	if string(forward) != "echo hi\r" {
		t.Fatalf("allowed line must pass through with Enter: %q", forward)
	}
}

func TestTerminalGateDeny(t *testing.T) {
	g := newTerminalGate(true)
	forward := g.process([]byte("rm -rf /\r"), func(line string) bool {
		return false // policy deny
	})
	// Deny: Enter withheld, remote line buffer killed with Ctrl+U (0x15) so
	// the command can never execute.
	if !strings.HasSuffix(string(forward), "\x15") {
		t.Fatalf("denied line must be killed with Ctrl+U: %q", forward)
	}
	if strings.Contains(string(forward), "\r") {
		t.Fatalf("denied line must not receive Enter: %q", forward)
	}
}

func TestTerminalGateMultiLinePaste(t *testing.T) {
	g := newTerminalGate(true)
	var submitted []string
	forward := g.process([]byte("safe-cmd\rdangerous\r"), func(line string) bool {
		submitted = append(submitted, line)
		return line == "safe-cmd" // deny the second line
	})
	if len(submitted) != 2 {
		t.Fatalf("each pasted line must be evaluated in order: %v", submitted)
	}
	if submitted[0] != "safe-cmd" || submitted[1] != "dangerous" {
		t.Fatalf("wrong evaluation order: %v", submitted)
	}
	// First line fully forwarded (incl. Enter), second killed with Ctrl+U.
	if string(forward) != "safe-cmd\rdangerous\x15" {
		t.Fatalf("mixed paste result wrong: %q", forward)
	}
}

func TestTerminalGateEmptyEnterAndControl(t *testing.T) {
	g := newTerminalGate(true)
	// Empty Enter passes through untouched (no policy check needed).
	forward := g.process([]byte("\r"), func(string) bool {
		t.Fatal("empty line must not be evaluated")
		return true
	})
	if string(forward) != "\r" {
		t.Fatalf("empty Enter must pass through: %q", forward)
	}
	// Ctrl+C clears the pending line and forwards untouched.
	g.process([]byte("partial"), func(string) bool { return true })
	forward = g.process([]byte{0x03}, func(string) bool {
		t.Fatal("Ctrl+C must not trigger evaluation")
		return true
	})
	if string(forward) != "\x03" {
		t.Fatalf("Ctrl+C must forward verbatim: %q", forward)
	}
	if g.pending() != "" {
		t.Fatalf("Ctrl+C must clear the pending line: %q", g.pending())
	}
}

func TestTerminalGateAcrossFrames(t *testing.T) {
	g := newTerminalGate(true)
	// The pending line accumulates across websocket frames.
	g.process([]byte("ver"), func(string) bool { return true })
	g.process([]byte("y long"), func(string) bool { return true })
	var submitted []string
	g.process([]byte(" command\r"), func(line string) bool {
		submitted = append(submitted, line)
		return true
	})
	if len(submitted) != 1 || submitted[0] != "very long command" {
		t.Fatalf("cross-frame line reconstruction wrong: %v", submitted)
	}
}

type stubRBACStore struct {
	decision domain.GrantDecision
}

func (s *stubRBACStore) ListEnabledServers() ([]*domain.ServerAsset, error) { return nil, nil }
func (s *stubRBACStore) ListServersForUserFiltered(string, bool) ([]*domain.ServerAsset, error) {
	return nil, nil
}
func (s *stubRBACStore) ListServersForUserTransferable(string) ([]*domain.ServerAsset, error) {
	return nil, nil
}
func (s *stubRBACStore) ServerGrantDecision(string, *domain.ServerAsset) (domain.GrantDecision, error) {
	return s.decision, nil
}

func TestEvaluateTerminalLineApprovalBypass(t *testing.T) {
	member := &domain.Identity{UserID: "usr-bob", Username: "bob", Role: domain.BuiltinRoleMember}
	asset := &domain.ServerAsset{ID: "srv-1", Name: "web-1", Host: "10.0.0.1", Port: 22, Username: "root"}

	// With terminal_no_approval=true: an unmatched command (L1 ask_approval)
	// skips the approval broker and is allowed, while a deny rule still blocks.
	srv := &Server{
		engine: security.NewEngine(),
		broker: approvals.NewBroker(),
		rbac: rbac.NewService(&stubRBACStore{decision: domain.GrantDecision{
			Allowed: true, CanConnect: true, TerminalNoApproval: true,
		}}),
		rulesCache:  security.DefaultRules(),
		rulesLoaded: time.Now(),
		l2Config:    &domain.L2ProviderConfig{Enabled: false},
		l2Loaded:    time.Now(),
	}

	v := srv.evaluateTerminalLine(context.Background(), member, asset, "systemctl restart nginx")
	if v.Decision != domain.DecisionAllow {
		t.Fatalf("expected ask_approval command to be allowed when terminal_no_approval is set, got %s", v.Decision)
	}
	if v.ApprovalID != "" {
		t.Fatalf("expected no approval request when terminal_no_approval is set, got %s", v.ApprovalID)
	}

	vDeny := srv.evaluateTerminalLine(context.Background(), member, asset, "rm -rf /tmp/foo")
	if vDeny.Decision != domain.DecisionDeny {
		t.Fatalf("deny rule must still block even with terminal_no_approval, got %s", vDeny.Decision)
	}
}
