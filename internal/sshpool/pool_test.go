package sshpool

import (
	"testing"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
)

func asset(id, jumpID string) *domain.ServerAsset {
	return &domain.ServerAsset{ID: id, Name: id, Host: "10.0.0." + id[len(id)-1:], Port: 22,
		Username: "root", AuthKind: domain.AuthAgentOrDefaultKey, JumpHostID: jumpID, Enabled: true}
}

func TestResolveJumpChainLinear(t *testing.T) {
	all := map[string]*domain.ServerAsset{
		"srv-a": asset("srv-a", "srv-b"), // target uses jump srv-b
		"srv-b": asset("srv-b", "srv-c"), // which uses jump srv-c
		"srv-c": asset("srv-c", ""),
	}
	chain, err := ResolveJumpChain(all["srv-a"], all)
	if err != nil {
		t.Fatalf("resolve: %v", err)
	}
	if len(chain) != 3 || chain[0].ID != "srv-c" || chain[1].ID != "srv-b" || chain[2].ID != "srv-a" {
		t.Fatalf("chain order wrong: %v", chain)
	}
}

func TestResolveJumpChainSingle(t *testing.T) {
	all := map[string]*domain.ServerAsset{"srv-a": asset("srv-a", "")}
	chain, err := ResolveJumpChain(all["srv-a"], all)
	if err != nil || len(chain) != 1 {
		t.Fatalf("single hop chain: %v %v", chain, err)
	}
}

func TestResolveJumpChainMissingHost(t *testing.T) {
	all := map[string]*domain.ServerAsset{"srv-a": asset("srv-a", "srv-missing")}
	_, err := ResolveJumpChain(all["srv-a"], all)
	if err == nil {
		t.Fatal("missing jump host must error")
	}
	if apierr.From(err).Code != apierr.CodeNotFound {
		t.Fatalf("want not-found code, got %v", apierr.From(err).Code)
	}
}

func TestResolveJumpChainCycle(t *testing.T) {
	all := map[string]*domain.ServerAsset{
		"srv-a": asset("srv-a", "srv-b"),
		"srv-b": asset("srv-b", "srv-a"),
	}
	_, err := ResolveJumpChain(all["srv-a"], all)
	if err == nil {
		t.Fatal("cyclic chain must error")
	}
}
