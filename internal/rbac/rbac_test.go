package rbac

import (
	"errors"
	"testing"

	"github.com/tyuan511/bulwark/internal/domain"
)

type stubStore struct {
	decision domain.GrantDecision
	err      error
}

func (s *stubStore) ListEnabledServers() ([]*domain.ServerAsset, error) { return nil, nil }
func (s *stubStore) ListServersForUserFiltered(string, bool) ([]*domain.ServerAsset, error) {
	return nil, nil
}
func (s *stubStore) ListServersForUserTransferable(string) ([]*domain.ServerAsset, error) {
	return nil, nil
}
func (s *stubStore) ServerGrantDecision(string, *domain.ServerAsset) (domain.GrantDecision, error) {
	return s.decision, s.err
}

func TestCanSkipApprovalDefaultsAndGrants(t *testing.T) {
	member := &domain.Identity{UserID: "usr-1", Username: "bob", Role: domain.BuiltinRoleMember}
	admin := &domain.Identity{UserID: "usr-admin", Username: "admin", Role: domain.BuiltinRoleAdmin, IsAdmin: true}
	asset := &domain.ServerAsset{ID: "srv-1", Name: "web-1"}

	// Default: no approval-bypass configured -> both channels require approval.
	svc := NewService(&stubStore{decision: domain.GrantDecision{
		Allowed: true, CanConnect: true, CanExecute: true, CanTransfer: true,
	}})
	if svc.CanSkipTerminalApproval(member, asset) {
		t.Fatal("default grant must require terminal approval")
	}
	if svc.CanSkipMCPApproval(member, asset) {
		t.Fatal("default grant must require MCP approval")
	}

	// Terminal no-approval only: terminal skips, MCP still requires approval.
	svc = NewService(&stubStore{decision: domain.GrantDecision{
		Allowed: true, CanConnect: true, CanExecute: true, TerminalNoApproval: true,
	}})
	if !svc.CanSkipTerminalApproval(member, asset) {
		t.Fatal("terminal_no_approval with can_connect must skip terminal approval")
	}
	if svc.CanSkipMCPApproval(member, asset) {
		t.Fatal("mcp_no_approval is false, must still require MCP approval")
	}

	// MCP no-approval only: MCP skips, terminal still requires approval.
	svc = NewService(&stubStore{decision: domain.GrantDecision{
		Allowed: true, CanConnect: true, CanExecute: true, MCPNoApproval: true,
	}})
	if svc.CanSkipTerminalApproval(member, asset) {
		t.Fatal("terminal_no_approval is false, must still require terminal approval")
	}
	if !svc.CanSkipMCPApproval(member, asset) {
		t.Fatal("mcp_no_approval with can_execute must skip MCP approval")
	}

	// Missing base capability (can_connect / can_execute false) must not skip.
	svc = NewService(&stubStore{decision: domain.GrantDecision{
		TerminalNoApproval: true, MCPNoApproval: true,
	}})
	if svc.CanSkipTerminalApproval(member, asset) {
		t.Fatal("cannot skip terminal approval without can_connect")
	}
	if svc.CanSkipMCPApproval(member, asset) {
		t.Fatal("cannot skip MCP approval without can_execute")
	}

	// Admin has no grant row and defaults to requiring approval.
	svc = NewService(&stubStore{decision: domain.GrantDecision{
		Allowed: true, CanConnect: true, CanExecute: true, TerminalNoApproval: true, MCPNoApproval: true,
	}})
	if svc.CanSkipTerminalApproval(admin, asset) {
		t.Fatal("admin must default to requiring terminal approval")
	}
	if svc.CanSkipMCPApproval(admin, asset) {
		t.Fatal("admin must default to requiring MCP approval")
	}

	// Storage error fails closed (requires approval).
	svc = NewService(&stubStore{err: errors.New("db down")})
	if svc.CanSkipTerminalApproval(member, asset) || svc.CanSkipMCPApproval(member, asset) {
		t.Fatal("storage error must fail closed and require approval")
	}

	// Nil inputs fail closed.
	if svc.CanSkipTerminalApproval(nil, asset) || svc.CanSkipTerminalApproval(member, nil) {
		t.Fatal("nil inputs must fail closed")
	}
	if svc.CanSkipMCPApproval(nil, asset) || svc.CanSkipMCPApproval(member, nil) {
		t.Fatal("nil inputs must fail closed")
	}
}
