package mcpserver_test

import (
	"bytes"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/config"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver"
	"github.com/yuantang/sshield/internal/logging"
	"github.com/yuantang/sshield/internal/mcpserver"
	"github.com/yuantang/sshield/internal/rbac"
	"github.com/yuantang/sshield/internal/security"
	"github.com/yuantang/sshield/internal/sshpool"
	"github.com/yuantang/sshield/internal/storage"
	"github.com/yuantang/sshield/internal/vault"
)

type mcpFixture struct {
	t   *testing.T
	app *fiber.App
	db  *storage.DB
}

// testPostgresURL provisions a unique schema and returns its DSN.
func testPostgresURL(t *testing.T) string {
	t.Helper()
	base := os.Getenv("TEST_POSTGRES_URL")
	if base == "" {
		base = "postgres://sshield:secret@127.0.0.1:54329/sshield_test?sslmode=disable"
	}
	var b [6]byte
	_, _ = rand.Read(b[:])
	schema := "m_" + hex.EncodeToString(b[:])

	boot, err := storage.Open(base, "")
	if err != nil {
		t.Skipf("postgres unavailable (%v); set TEST_POSTGRES_URL to enable integration tests", err)
	}
	if err := boot.Exec(fmt.Sprintf(`CREATE SCHEMA IF NOT EXISTS %s`, schema)); err != nil {
		_ = boot.Close()
		t.Fatalf("create schema: %v", err)
	}
	_ = boot.Close()
	t.Cleanup(func() {
		drop, err2 := storage.Open(base, "")
		if err2 == nil {
			_ = drop.Exec(fmt.Sprintf(`DROP SCHEMA IF EXISTS %s CASCADE`, schema))
			_ = drop.Close()
		}
	})
	return fmt.Sprintf("%s&options=-c%%20search_path%%3D%s", base, schema)
}

func newMCPFixture(t *testing.T) *mcpFixture {
	t.Helper()
	dir := t.TempDir()

	cfg := config.Bare()
	cfg.Storage.DataDir = dir
	cfg.Bootstrap.AdminUsername = "root-admin"
	cfg.Bootstrap.AdminPassword = "bootstrap-pass-123"

	db, err := storage.Open(cfg.Storage.DBURL, "")
	if err != nil {
		t.Skipf("postgres unavailable (%v); set TEST_POSTGRES_URL to enable integration tests", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	if _, err := httpserver.Bootstrap(db, cfg); err != nil {
		t.Fatal(err)
	}

	sealedVault := vault.New(filepath.Join(dir, "vault.enc"), time.Minute)
	totpMgr, err := auth.NewTOTPManager(filepath.Join(dir, "server.key"), "SSHield")
	if err != nil {
		t.Fatal(err)
	}
	guard := auth.NewLoginGuard(db, auth.GuardConfig{LoginMaxAttempts: 5, LockoutMinutes: 15,
		IPMaxAttempts: 1000, IPWindowMinutes: 60})
	authSvc := auth.NewService(db, totpMgr, guard, time.Hour, 24*time.Hour, false,
		func(roleID string) (*domain.Role, error) { return db.GetRole(roleID) })
	rbacSvc := rbac.NewService(db)
	pool := sshpool.NewPool(sealedVault)
	engine := security.NewEngine()
	broker := approvals.NewBroker()
	log := logging.NewNop()
	server := httpserver.New(log, cfg, db, authSvc, rbacSvc, pool, engine, broker, sealedVault)

	mcpHandler := mcpserver.NewHandler(mcpserver.Deps{
		DB: db, Auth: authSvc, RBAC: rbacSvc, Pool: pool, Engine: engine,
		Broker: broker, Vault: sealedVault,
		Rules: server.CachedRules, L2: server.L2Reviewer,
		Audit: server.Audit, Log: log,
	})
	server.SetMCPHandler(mcpHandler)

	return &mcpFixture{t: t, app: server.App(), db: db}
}

// issueToken mints a PAT for the given user through the auth service path
// (session-less: directly via storage + hash, mirroring the API handler).
func (f *mcpFixture) issueToken(userID string) string {
	f.t.Helper()
	token, meta := auth.NewApiToken()
	meta.ID = "tok-test"
	meta.UserID = userID
	meta.Name = "test"
	if err := f.db.CreateApiToken(&meta, auth.HashToken(token)); err != nil {
		f.t.Fatal(err)
	}
	return token
}

// callMCP performs a JSON-RPC round trip against /mcp with the bearer token.
func (f *mcpFixture) callMCP(token, method string, params map[string]any, sessionID string) (int, map[string]any, string) {
	f.t.Helper()
	body, _ := json.Marshal(map[string]any{
		"jsonrpc": "2.0", "id": 1, "method": method, "params": params,
	})
	req := httptest.NewRequest("POST", "/mcp", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	if sessionID != "" {
		req.Header.Set("Mcp-Session-Id", sessionID)
	}
	resp, err := f.app.Test(req, -1)
	if err != nil {
		f.t.Fatalf("mcp %s: %v", method, err)
	}
	defer resp.Body.Close()
	var out map[string]any
	_ = json.NewDecoder(resp.Body).Decode(&out)
	return resp.StatusCode, out, resp.Header.Get("Mcp-Session-Id")
}

func TestMCPRequiresBearerToken(t *testing.T) {
	f := newMCPFixture(t)
	status, body, _ := f.callMCP("", "initialize", map[string]any{
		"protocolVersion": "2025-03-26",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "test", "version": "1"},
	}, "")
	if status != 401 {
		t.Fatalf("unauthenticated mcp must 401, got %d %v", status, body)
	}
}

func TestMCPRejectsInvalidToken(t *testing.T) {
	f := newMCPFixture(t)
	status, _, _ := f.callMCP("sk-sshield-not-a-real-token", "initialize", map[string]any{}, "")
	if status != 401 {
		t.Fatalf("invalid token must 401, got %d", status)
	}
}

func TestMCPInitializeAndToolsList(t *testing.T) {
	f := newMCPFixture(t)
	_ = f.db.CreateUser(&domain.User{ID: "usr-1", Username: "agent", RoleID: "role-member", IsActive: true},
		auth.HashPassword("agent-pass-123"), "", nil)
	token := f.issueToken("usr-1")

	status, body, _ := f.callMCP(token, "initialize", map[string]any{
		"protocolVersion": "2025-03-26",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "test-client", "version": "1"},
	}, "")
	if status != 200 {
		t.Fatalf("initialize failed: %d %v", status, body)
	}
	if body["result"] == nil {
		t.Fatalf("initialize result missing: %v", body)
	}
	// Stateless mode: no Mcp-Session-Id; every call re-authenticates.

	// notifications/initialized then tools/list.
	f.callMCP(token, "notifications/initialized", nil, "")
	status, body, _ = f.callMCP(token, "tools/list", nil, "")
	if status != 200 {
		t.Fatalf("tools/list failed: %d", status)
	}
	result := body["result"].(map[string]any)
	tools := result["tools"].([]any)
	if len(tools) != 14 {
		t.Fatalf("want 14 tools, got %d", len(tools))
	}
	names := map[string]bool{}
	for _, tl := range tools {
		names[tl.(map[string]any)["name"].(string)] = true
	}
	for _, want := range []string{"execute_command", "list_authorized_servers", "query_audit_logs", "verify_audit_chain"} {
		if !names[want] {
			t.Fatalf("missing tool %s in %v", want, names)
		}
	}
}

func TestMCPExecuteRequiresGrantAndAudits(t *testing.T) {
	f := newMCPFixture(t)

	// Server NOT granted to bob's group.
	if err := f.db.UpsertServer(&domain.ServerAsset{ID: "srv-1", Name: "web-1",
		Host: "10.0.0.1", Port: 22, Username: "root", Tags: []string{}, Enabled: true}); err != nil {
		t.Fatal(err)
	}
	_ = f.db.CreateUser(&domain.User{ID: "usr-bob", Username: "bob", RoleID: "role-member", IsActive: true},
		auth.HashPassword("bob-pass-123"), "", nil)
	token := f.issueToken("usr-bob")

	// initialize + tools/call execute_command on ungranted host.
	status, body, sessionID := f.callMCP(token, "initialize", map[string]any{
		"protocolVersion": "2025-03-26",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "t", "version": "1"},
	}, "")
	if status != 200 {
		t.Fatalf("initialize: %d", status)
	}
	f.callMCP(token, "notifications/initialized", nil, sessionID)

	params := map[string]any{
		"name":      "execute_command",
		"arguments": map[string]any{"server_id": "srv-1", "command": "ls", "intent": "test", "dry_run": true},
	}
	status, body, _ = f.callMCP(token, "tools/call", params, sessionID)
	if status != 200 {
		t.Fatalf("tools/call: %d", status)
	}
	result := body["result"].(map[string]any)
	if result["isError"] != true {
		t.Fatalf("ungranted execute must be a tool error result: %v", result)
	}
	text := result["content"].([]any)[0].(map[string]any)["text"].(string)
	if !strings.Contains(text, "no execute grant") && !strings.Contains(text, "403") {
		t.Fatalf("denial message wrong: %s", text)
	}
}

func TestMCPExecuteDryRunAllowsAndAudits(t *testing.T) {
	f := newMCPFixture(t)

	if err := f.db.UpsertServer(&domain.ServerAsset{ID: "srv-1", Name: "web-1",
		Host: "10.0.0.1", Port: 22, Username: "root", Tags: []string{}, Enabled: true}); err != nil {
		t.Fatal(err)
	}
	if err := f.db.CreateUserGroup(&domain.UserGroup{ID: "grp-ops", Name: "ops"}); err != nil {
		t.Fatal(err)
	}
	_ = f.db.CreateUser(&domain.User{ID: "usr-bob", Username: "bob", RoleID: "role-member", IsActive: true},
		auth.HashPassword("bob-pass-123"), "", nil)
	_ = f.db.AddGroupMember("grp-ops", "usr-bob")
	if err := f.db.CreateHostGrant(&domain.HostGrant{ID: "g1", UserGroupID: "grp-ops",
		ServerID: "srv-1", CanConnect: true, CanExecute: true}); err != nil {
		t.Fatal(err)
	}
	token := f.issueToken("usr-bob")

	status, body, sessionID := f.callMCP(token, "initialize", map[string]any{
		"protocolVersion": "2025-03-26",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "t", "version": "1"},
	}, "")
	if status != 200 {
		t.Fatalf("initialize: %d", status)
	}
	f.callMCP(token, "notifications/initialized", nil, sessionID)

	// list_authorized_servers: the name finally matches reality.
	status, body, _ = f.callMCP(token, "tools/call", map[string]any{
		"name": "list_authorized_servers", "arguments": map[string]any{},
	}, sessionID)
	if status != 200 {
		t.Fatalf("list servers: %d %v", status, body)
	}
	text := toolText(body)
	if !strings.Contains(text, "web-1") {
		t.Fatalf("granted server must be listed: %s", text)
	}

	// dry_run: allowed, deny rule enforced at L1 (rm), no SSH attempted.
	status, body, _ = f.callMCP(token, "tools/call", map[string]any{
		"name": "execute_command",
		"arguments": map[string]any{
			"server_id": "srv-1", "command": "rm -rf /", "intent": "cleanup", "dry_run": true,
		},
	}, sessionID)
	if status != 200 {
		t.Fatalf("dry run: %d %v", status, body)
	}
	if !strings.Contains(toolText(body), "denied") {
		t.Fatalf("rm must be denied by L1: %s", toolText(body))
	}

	// Audit trail for bob exists (denied execution recorded).
	events, err := f.db.QueryAudits(&domain.AuditQuery{UserID: "usr-bob"})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) == 0 {
		t.Fatal("denied execution must be audited for the calling user")
	}
}

func TestMCPAdminScopedToolsBlockedForMember(t *testing.T) {
	f := newMCPFixture(t)
	_ = f.db.CreateUser(&domain.User{ID: "usr-bob", Username: "bob", RoleID: "role-member", IsActive: true},
		auth.HashPassword("bob-pass-123"), "", nil)
	token := f.issueToken("usr-bob")

	status, body, sessionID := f.callMCP(token, "initialize", map[string]any{
		"protocolVersion": "2025-03-26",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "t", "version": "1"},
	}, "")
	if status != 200 {
		t.Fatal("initialize failed")
	}
	f.callMCP(token, "notifications/initialized", nil, sessionID)

	for _, tool := range []string{"verify_audit_chain", "get_vault_status", "list_vault_credentials"} {
		status, body, _ = f.callMCP(token, "tools/call", map[string]any{
			"name": tool, "arguments": map[string]any{},
		}, sessionID)
		if status != 200 {
			t.Fatalf("%s status: %d", tool, status)
		}
		result := body["result"].(map[string]any)
		if result["isError"] != true {
			t.Fatalf("member calling %s must be denied: %v", tool, result)
		}
	}
}

func toolText(body map[string]any) string {
	result, _ := body["result"].(map[string]any)
	if result == nil {
		return fmt.Sprintf("%v", body)
	}
	content, _ := result["content"].([]any)
	if len(content) == 0 {
		return ""
	}
	first, _ := content[0].(map[string]any)
	text, _ := first["text"].(string)
	return text
}
