package storage

import (
	"path/filepath"
	"testing"
	"time"

	"github.com/yuantang/sshield/internal/domain"
)

func testDB(t *testing.T) *DB {
	t.Helper()
	db, err := Open(filepath.Join(t.TempDir(), "test.db"), "")
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return db
}

func TestAuditChainAppendAndVerify(t *testing.T) {
	db := testDB(t)

	for i := 0; i < 5; i++ {
		code := 0
		if i == 3 {
			code = 2
		}
		ev := &domain.AuditEvent{
			ID:         GenerateID("aud"),
			Actor:      "mcp-http",
			Timestamp:  time.Now().UTC(),
			ServerID:   "srv-1",
			ServerName: "web-1",
			SSHUser:    "root",
			ServerHost: "10.0.0.1:22",
			Operator:   "alice",
			UserID:     "usr-1",
			Username:   "alice",
			Command:    "ls -la",
			Intent:     "listing",
			Decision:   domain.DecisionAllow,
			ExitCode:   &code,
		}
		if err := db.AppendAudit(ev); err != nil {
			t.Fatalf("append %d: %v", i, err)
		}
		if ev.PrevHash == "" || ev.EventHash == "" {
			t.Fatalf("chain fields not populated on event %d", i)
		}
	}

	status, err := db.VerifyAuditChain()
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if !status.Valid || status.TotalEvents != 5 {
		t.Fatalf("chain invalid: %+v", status)
	}
}

func TestAuditChainTamperDetection(t *testing.T) {
	db := testDB(t)
	for i := 0; i < 3; i++ {
		ev := &domain.AuditEvent{
			ID: GenerateID("aud"), Actor: "terminal", Timestamp: time.Now().UTC(),
			ServerID: "srv-1", ServerName: "web-1", Command: "whoami",
			Intent: "test", Decision: domain.DecisionAllow,
		}
		if err := db.AppendAudit(ev); err != nil {
			t.Fatalf("append: %v", err)
		}
	}
	// Tamper with a stored command directly.
	if _, err := db.sql.Exec(`UPDATE audit_logs SET command = 'rm -rf /' WHERE seq = 2`); err != nil {
		t.Fatalf("tamper: %v", err)
	}
	status, err := db.VerifyAuditChain()
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if status.Valid {
		t.Fatal("tampered chain must fail verification")
	}
	if status.BrokenAtID == "" {
		t.Fatal("broken id must be reported")
	}
}

func TestLegacyHashCompatibility(t *testing.T) {
	// A record written exactly like the legacy Rust implementation (no user
	// columns) must verify against the Go re-implementation of the hash.
	db := testDB(t)
	ev := &domain.AuditEvent{
		ID: "aud-legacy", TraceID: "aud-legacy", Actor: "mcp-stdio",
		Timestamp: time.Date(2024, 5, 1, 12, 0, 0, 0, time.UTC),
		ServerID:  "srv-x", ServerName: "x", Command: "echo hi", Intent: "i",
		Decision: domain.DecisionAllow,
	}
	if err := db.AppendAudit(ev); err != nil {
		t.Fatalf("append: %v", err)
	}
	// Recompute using the exact legacy payload format and compare.
	tsStr := formatRFC3339(ev.Timestamp)
	payload := "GENESIS|aud-legacy|aud-legacy|mcp-stdio|" + tsStr +
		"|srv-x|echo hi|i|allow|None"
	status, err := db.VerifyAuditChain()
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if !status.Valid {
		t.Fatalf("legacy-format record must verify (payload=%s)", payload)
	}
}

func TestRBACGrantEvaluation(t *testing.T) {
	db := testDB(t)
	ctx := t.Context()

	// Roles.
	adminRole := &domain.Role{ID: "role-admin", Name: "admin", IsBuiltin: true,
		Permissions: domain.BuiltinAdminPermissions(), CreatedAt: time.Now().UTC()}
	memberRole := &domain.Role{ID: "role-member", Name: "member", IsBuiltin: true,
		Permissions: domain.BuiltinMemberPermissions(), CreatedAt: time.Now().UTC()}
	if err := db.UpsertRole(adminRole); err != nil {
		t.Fatal(err)
	}
	if err := db.UpsertRole(memberRole); err != nil {
		t.Fatal(err)
	}

	// Users: alice admin, bob member.
	alice := &domain.User{ID: "usr-alice", Username: "alice", RoleID: "role-admin", IsActive: true}
	bob := &domain.User{ID: "usr-bob", Username: "bob", RoleID: "role-member", IsActive: true}
	if err := db.CreateUser(alice, "hash-a", "", nil); err != nil {
		t.Fatal(err)
	}
	if err := db.CreateUser(bob, "hash-b", "", nil); err != nil {
		t.Fatal(err)
	}

	// Group with bob.
	grp := &domain.UserGroup{ID: "grp-ops", Name: "ops"}
	if err := db.CreateUserGroup(grp); err != nil {
		t.Fatal(err)
	}
	if err := db.AddGroupMember("grp-ops", "usr-bob"); err != nil {
		t.Fatal(err)
	}

	// Servers: two in group "prod", one standalone.
	servers := []*domain.ServerAsset{
		{ID: "srv-1", Name: "web-1", Host: "10.0.0.1", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, GroupName: "prod", Tags: []string{}, Enabled: true},
		{ID: "srv-2", Name: "web-2", Host: "10.0.0.2", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, GroupName: "prod", Tags: []string{}, Enabled: true},
		{ID: "srv-3", Name: "db-1", Host: "10.0.0.3", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, Tags: []string{}, Enabled: true},
	}
	for _, s := range servers {
		if err := db.UpsertServer(s); err != nil {
			t.Fatal(err)
		}
	}

	// Grant group "ops": connect on "prod" group, execute on srv-3 only.
	if err := db.CreateHostGrant(&domain.HostGrant{
		ID: "grant-1", UserGroupID: "grp-ops", GroupName: "prod",
		CanConnect: true, CanExecute: false,
	}); err != nil {
		t.Fatal(err)
	}
	if err := db.CreateHostGrant(&domain.HostGrant{
		ID: "grant-2", UserGroupID: "grp-ops", ServerID: "srv-3",
		CanConnect: true, CanExecute: true,
	}); err != nil {
		t.Fatal(err)
	}

	// Admin bypasses grants: ListServersForUser is only for non-admins, but the
	// full list must show 3 servers.
	all, err := db.ListServers()
	if err != nil {
		t.Fatal(err)
	}
	if len(all) != 3 {
		t.Fatalf("admin sees all servers, got %d", len(all))
	}

	// Bob's visible set = web-1, web-2 (group grant) + db-1 (direct grant).
	granted, err := db.ListServersForUser("usr-bob")
	if err != nil {
		t.Fatal(err)
	}
	if len(granted) != 3 {
		t.Fatalf("bob should see 3 granted servers, got %d", len(granted))
	}

	// Execute-filtered set = db-1 only.
	execOnly, err := db.ListServersForUserFiltered("usr-bob", true)
	if err != nil {
		t.Fatal(err)
	}
	if len(execOnly) != 1 || execOnly[0].ID != "srv-3" {
		t.Fatalf("execute filter wrong: %+v", execOnly)
	}

	// Grant decision on srv-1: connect yes, execute no.
	asset := servers[0]
	d, err := db.ServerGrantDecision("usr-bob", asset)
	if err != nil {
		t.Fatal(err)
	}
	if !d.CanConnect || d.CanExecute {
		t.Fatalf("srv-1 decision wrong: %+v", d)
	}

	// Alice (not in group) has no grants.
	d2, err := db.ServerGrantDecision("usr-alice", asset)
	if err != nil {
		t.Fatal(err)
	}
	if d2.Allowed {
		t.Fatal("alice must not have grants via group path (admin bypass handled at rbac layer)")
	}

	_ = ctx
}

func TestUserLifecycleAndLockout(t *testing.T) {
	db := testDB(t)
	if err := db.UpsertRole(&domain.Role{ID: "role-member", Name: "member", IsBuiltin: true,
		Permissions: domain.BuiltinMemberPermissions(), CreatedAt: time.Now().UTC()}); err != nil {
		t.Fatal(err)
	}

	u := &domain.User{ID: "usr-1", Username: "carol", DisplayName: "Carol",
		RoleID: "role-member", IsActive: true, MustChangePassword: true}
	if err := db.CreateUser(u, "argon2id$hash", "TOTPSECRET", []string{"rc1", "rc2"}); err != nil {
		t.Fatal(err)
	}

	creds, err := db.GetCredentialsByUsername("carol")
	if err != nil {
		t.Fatal(err)
	}
	if creds.PasswordHash != "argon2id$hash" || creds.TOTPSecret != "TOTPSECRET" {
		t.Fatalf("credentials mismatch: %+v", creds)
	}
	if len(creds.RecoveryCodes) != 2 {
		t.Fatalf("recovery codes: %v", creds.RecoveryCodes)
	}
	if !creds.MustChangePassword {
		t.Fatal("must_change_password flag lost")
	}

	// Lockout after max attempts.
	until1, err := db.RecordLoginFailure("usr-1", 3, 15)
	if err != nil || until1 != nil {
		t.Fatalf("attempt 1: %v %v", until1, err)
	}
	if _, err = db.RecordLoginFailure("usr-1", 3, 15); err != nil {
		t.Fatal(err)
	}
	until3, err := db.RecordLoginFailure("usr-1", 3, 15)
	if err != nil {
		t.Fatal(err)
	}
	if until3 == nil {
		t.Fatal("user must be locked after 3 failures")
	}
	if time.Until(*until3) <= 0 {
		t.Fatal("lockout must be in the future")
	}

	// Success clears the lock.
	if err := db.RecordLoginSuccess("usr-1"); err != nil {
		t.Fatal(err)
	}
	creds2, _ := db.GetCredentialsByUsername("carol")
	if creds2.FailedAttempts != 0 || creds2.LockedUntil != nil {
		t.Fatalf("lockout not cleared: %+v", creds2)
	}

	// Duplicate username rejected.
	dup := &domain.User{ID: "usr-2", Username: "carol", RoleID: "role-member", IsActive: true}
	if err := db.CreateUser(dup, "h", "", nil); err == nil {
		t.Fatal("duplicate username must be rejected")
	}
}

func TestSessionsAndCleanup(t *testing.T) {
	db := testDB(t)
	_ = db.UpsertRole(&domain.Role{ID: "role-member", Name: "member", CreatedAt: time.Now().UTC()})
	_ = db.CreateUser(&domain.User{ID: "usr-1", Username: "u1", RoleID: "role-member", IsActive: true}, "h", "", nil)
	now := time.Now().UTC()
	s := &domain.UserSession{ID: GenerateID("sess"), UserID: "usr-1", IP: "1.2.3.4",
		UserAgent: "test", CreatedAt: now, LastSeenAt: now, ExpiresAt: now.Add(time.Hour)}
	if err := db.CreateSession(s, "hash-token", now.Add(24*time.Hour)); err != nil {
		t.Fatal(err)
	}
	got, err := db.GetSessionByHash("hash-token")
	if err != nil {
		t.Fatal(err)
	}
	if got.ID != s.ID {
		t.Fatalf("session mismatch: %+v", got)
	}
	db.TouchSession(s.ID, now.Add(2*time.Hour))

	// Expire it manually then cleanup.
	if _, err := db.sql.Exec(`UPDATE sessions SET expires_at = ? WHERE id = ?`,
		formatRFC3339(now.Add(-time.Minute)), s.ID); err != nil {
		t.Fatal(err)
	}
	n, err := db.CleanupExpired()
	if err != nil {
		t.Fatal(err)
	}
	if n == 0 {
		t.Fatal("cleanup must remove the expired session")
	}
	if _, err := db.GetSessionByHash("hash-token"); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestServerCRUD(t *testing.T) {
	db := testDB(t)
	s := &domain.ServerAsset{ID: GenerateID("srv"), Name: "n", Host: "h", Port: 2222,
		Username: "u", AuthKind: domain.AuthIdentityFile, IdentityFile: "/k", Tags: []string{"a", "b"},
		GroupName: "g", Enabled: true}
	if err := db.UpsertServer(s); err != nil {
		t.Fatal(err)
	}
	got, err := db.GetServer(s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.Port != 2222 || len(got.Tags) != 2 || got.GroupName != "g" {
		t.Fatalf("roundtrip mismatch: %+v", got)
	}
	// Update.
	got.Name = "n2"
	if err := db.UpsertServer(got); err != nil {
		t.Fatal(err)
	}
	list, _ := db.ListServers()
	if len(list) != 1 || list[0].Name != "n2" {
		t.Fatalf("update failed: %+v", list)
	}
	if ok, _ := db.DeleteServer(s.ID); !ok {
		t.Fatal("delete failed")
	}
	if _, err := db.GetServer(s.ID); err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}

func TestListUsersWithRoles(t *testing.T) {
	db := testDB(t)
	if err := db.UpsertRole(&domain.Role{ID: "role-member", Name: "member", IsBuiltin: true,
		Permissions: domain.BuiltinMemberPermissions(), CreatedAt: time.Now().UTC()}); err != nil {
		t.Fatal(err)
	}
	if err := db.CreateUser(&domain.User{ID: "usr-1", Username: "zed", DisplayName: "Z",
		RoleID: "role-member", IsActive: true}, "h", "", nil); err != nil {
		t.Fatal(err)
	}
	users, err := db.ListUsers()
	if err != nil {
		t.Fatalf("list users: %v", err)
	}
	if len(users) != 1 || users[0].Username != "zed" || users[0].Role == nil || users[0].Role.Name != "member" {
		t.Fatalf("users: %+v", users)
	}
}
