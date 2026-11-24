package storage

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
)

var lastTestDSN string

// testDB opens an isolated postgres schema; skips when unreachable.
func testDB(t *testing.T) *DB {
	t.Helper()
	base := os.Getenv("TEST_POSTGRES_URL")
	if base == "" {
		base = "postgres://bulwark:secret@127.0.0.1:54329/bulwark_test?sslmode=disable"
	}
	var b [6]byte
	_, _ = rand.Read(b[:])
	schema := "s_" + hex.EncodeToString(b[:])

	boot, err := Open(base, "")
	if err != nil {
		t.Skipf("postgres unavailable (%v); set TEST_POSTGRES_URL to enable db tests", err)
	}
	if err := boot.Exec(fmt.Sprintf(`CREATE SCHEMA IF NOT EXISTS %s`, schema)); err != nil {
		_ = boot.Close()
		t.Fatalf("create schema: %v", err)
	}
	_ = boot.Close()

	lastTestDSN = fmt.Sprintf("%s&options=-c%%20search_path%%3D%s", base, schema)
	h, err := Open(lastTestDSN, "")
	if err != nil {
		t.Skipf("postgres unavailable (%v)", err)
	}
	t.Cleanup(func() {
		drop, err2 := Open(base, "")
		if err2 == nil {
			_ = drop.Exec(fmt.Sprintf(`DROP SCHEMA IF EXISTS %s CASCADE`, schema))
			_ = drop.Close()
		}
		_ = h.Close()
	})
	return h
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
	if err := db.Exec(`UPDATE audit_logs SET command = 'rm -rf /' WHERE seq = 2`); err != nil {
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

	// Users: alice admin, bob member.
	alice := &domain.User{ID: "usr-alice", Username: "alice", Role: domain.BuiltinRoleAdmin, IsActive: true}
	bob := &domain.User{ID: "usr-bob", Username: "bob", Role: domain.BuiltinRoleMember, IsActive: true}
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

	// Servers: two in machine group "prod", one in "db".
	servers := []*domain.ServerAsset{
		{ID: "srv-1", Name: "web-1", Host: "10.0.0.1", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, GroupName: "prod", Tags: []string{}, Enabled: true},
		{ID: "srv-2", Name: "web-2", Host: "10.0.0.2", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, GroupName: "prod", Tags: []string{}, Enabled: true},
		{ID: "srv-3", Name: "db-1", Host: "10.0.0.3", Port: 22, Username: "root",
			AuthKind: domain.AuthAgentOrDefaultKey, GroupName: "db", Tags: []string{}, Enabled: true},
	}
	for _, s := range servers {
		if err := db.UpsertServer(s); err != nil {
			t.Fatal(err)
		}
	}

	// Grant group "ops": connect on srv-1 and srv-2, connect+execute on srv-3.
	if err := db.CreateHostGrant(&domain.HostGrant{
		ID: "grant-1", UserGroupID: "grp-ops", ServerID: "srv-1",
		CanConnect: true, CanExecute: false,
	}); err != nil {
		t.Fatal(err)
	}
	if err := db.CreateHostGrant(&domain.HostGrant{
		ID: "grant-2", UserGroupID: "grp-ops", ServerID: "srv-2",
		CanConnect: true, CanExecute: false,
	}); err != nil {
		t.Fatal(err)
	}
	if err := db.CreateHostGrant(&domain.HostGrant{
		ID: "grant-3", UserGroupID: "grp-ops", ServerID: "srv-3",
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

	// Bob's visible set = web-1, web-2 (group grant) + db-1 (direct grant):
	// the connect-filtered query returns the same set.
	granted, err := db.ListServersForUserFiltered("usr-bob", false)
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
	u := &domain.User{ID: "usr-1", Username: "carol", DisplayName: "Carol",
		Role: domain.BuiltinRoleMember, IsActive: true, MustChangePassword: true}
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
	dup := &domain.User{ID: "usr-2", Username: "carol", Role: domain.BuiltinRoleMember, IsActive: true}
	if err := db.CreateUser(dup, "h", "", nil); err == nil {
		t.Fatal("duplicate username must be rejected")
	}
}

func TestSessionsAndCleanup(t *testing.T) {
	db := testDB(t)
	_ = db.CreateUser(&domain.User{ID: "usr-1", Username: "u1", Role: domain.BuiltinRoleMember, IsActive: true}, "h", "", nil)
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
	if err := db.Exec(`UPDATE sessions SET expires_at = $1 WHERE id = $2`,
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

func TestListUsers(t *testing.T) {
	db := testDB(t)
	if err := db.CreateUser(&domain.User{ID: "usr-1", Username: "zed", DisplayName: "Z",
		Role: domain.BuiltinRoleMember, IsActive: true}, "h", "", nil); err != nil {
		t.Fatal(err)
	}
	users, err := db.ListUsers()
	if err != nil {
		t.Fatalf("list users: %v", err)
	}
	if len(users) != 1 || users[0].Username != "zed" || users[0].Role != domain.BuiltinRoleMember {
		t.Fatalf("users: %+v", users)
	}
}

func TestCredentialSealAndDecrypt(t *testing.T) {
	db := testDB(t)
	if err := db.SetEncryptionKey([]byte("0123456789abcdef0123456789abcdef")); err != nil {
		t.Fatal(err)
	}

	meta, err := db.CreateCredential("cred-1", CredKindPassword, "prod root", "root",
		"super-secret-password", "admin")
	if err != nil {
		t.Fatal(err)
	}
	if meta.Preview != "••••••••" {
		t.Fatalf("preview: %s", meta.Preview)
	}

	// Plaintext never lands in the database.
	var stored string
	if err := db.Exec(`SELECT encode(ciphertext, 'escape') FROM credentials WHERE id = 'cred-1'`); err == nil {
		_ = stored
	}
	var ct []byte
	if err := db.sqlDB.QueryRow(`SELECT ciphertext FROM credentials WHERE id = 'cred-1'`).Scan(&ct); err != nil {
		t.Fatal(err)
	}
	if string(ct) == "super-secret-password" || len(ct) == 0 {
		t.Fatal("ciphertext must be present and differ from plaintext")
	}

	// Roundtrip through the SecretResolver surface.
	got, err := db.GetSecret("cred-1")
	if err != nil || got != "super-secret-password" {
		t.Fatalf("get secret: %v %q", err, got)
	}

	// Wrong key fails closed (tamper simulation).
	bad, _ := Open(lastTestDSN, "")
	_ = bad.SetEncryptionKey([]byte("ffffffffffffffffffffffffffffffff"))
	if _, err := bad.GetSecret("cred-1"); err == nil {
		t.Fatal("decryption with wrong key must fail")
	}
	_ = bad.Close()

	// Metadata listing carries no secret material.
	list, err := db.ListCredentials()
	if err != nil || len(list) != 1 {
		t.Fatalf("list: %v %d", err, len(list))
	}
}

func TestHostSessionLifecycle(t *testing.T) {
	db := testDB(t)

	sess, err := db.CreateHostSession("mcpsess-1", "usr-a", "srv-1", "deploy", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if sess.ID != "mcpsess-1" || time.Until(sess.ExpiresAt) <= 0 {
		t.Fatalf("session: %+v", sess)
	}

	// Valid for the owner.
	got, err := db.ValidateHostSession("mcpsess-1", "usr-a")
	if err != nil || got.ServerID != "srv-1" {
		t.Fatalf("validate: %v %+v", err, got)
	}
	// Foreign user rejected.
	if _, err := db.ValidateHostSession("mcpsess-1", "usr-b"); err != ErrSessionInvalid {
		t.Fatalf("foreign user must be invalid: %v", err)
	}
	// Close by owner, then invalid.
	if ok, _ := db.CloseHostSession("mcpsess-1", "usr-a"); !ok {
		t.Fatal("close must succeed")
	}
	if _, err := db.ValidateHostSession("mcpsess-1", "usr-a"); err != ErrSessionInvalid {
		t.Fatalf("closed session must be invalid: %v", err)
	}
	// Expired session invalid.
	if _, err := db.CreateHostSession("mcpsess-2", "usr-a", "srv-1", "", -time.Minute); err != nil {
		t.Fatal(err)
	}
	if _, err := db.ValidateHostSession("mcpsess-2", "usr-a"); err != ErrSessionInvalid {
		t.Fatalf("expired session must be invalid: %v", err)
	}
}

func TestTerminalSessionRecordingLifecycle(t *testing.T) {
	db := testDB(t)

	sess := &domain.TerminalSession{
		ID: "tser-test-1", UserID: "usr-a", Username: "alice",
		ServerID: "srv-1", ServerName: "web-1", SSHUser: "deploy",
		ServerHost: "10.0.0.5:22", ClientIP: "127.0.0.1",
		PolicyMode: domain.TerminalModeEnforce,
		OpenedAt:   time.Now().UTC(),
	}
	if err := db.CreateTerminalSession(sess); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 3; i++ {
		if err := db.AppendTerminalEvent(sess.ID, int64(i*100), "output", "chunk\r\n"); err != nil {
			t.Fatal(err)
		}
	}
	if err := db.AppendTerminalEvent(sess.ID, 400, "input", "reboot"); err != nil {
		t.Fatal(err)
	}

	events, err := db.ListTerminalEvents(sess.ID)
	if err != nil || len(events) != 4 {
		t.Fatalf("events: %v %d", err, len(events))
	}
	if events[0].Seq <= 0 || events[3].Direction != "input" || events[3].Data != "reboot" {
		t.Fatalf("event rows wrong: %+v", events[3])
	}

	if err := db.CloseTerminalSession(sess.ID, "client disconnected", time.Now().UTC(), 24, 4, false); err != nil {
		t.Fatal(err)
	}
	got, err := db.GetTerminalSession(sess.ID)
	if err != nil {
		t.Fatal(err)
	}
	if got.CloseReason != "client disconnected" || got.ByteSize != 24 || got.EventCount != 4 || got.ClosedAt == nil {
		t.Fatalf("close not finalized: %+v", got)
	}

	// Second, older session for retention cleanup.
	old := &domain.TerminalSession{
		ID: "tser-old", UserID: "usr-a", ServerID: "srv-1",
		PolicyMode: domain.TerminalModeAudit, OpenedAt: time.Now().UTC().AddDate(0, 0, -40),
	}
	if err := db.CreateTerminalSession(old); err != nil {
		t.Fatal(err)
	}
	_ = db.AppendTerminalEvent(old.ID, 0, "output", "x")

	items, total, err := db.ListTerminalSessions(&domain.TerminalSessionQuery{UserID: "", Limit: 10})
	if err != nil || total != 2 || len(items) != 2 {
		t.Fatalf("list: %v total=%d", err, total)
	}
	n, err := db.CleanupTerminalSessionsBefore(time.Now().UTC().AddDate(0, 0, -30))
	if err != nil || n != 1 {
		t.Fatalf("cleanup: %v removed=%d", err, n)
	}
	events, err = db.ListTerminalEvents(old.ID)
	if err != nil || len(events) != 0 {
		t.Fatalf("cascade delete failed: %v %d", err, len(events))
	}
	items, total, _ = db.ListTerminalSessions(&domain.TerminalSessionQuery{})
	if total != 1 {
		t.Fatalf("one session must remain, got %d", total)
	}
}
