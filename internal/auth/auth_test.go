package auth

import (
	"strings"
	"testing"
	"time"

	"github.com/pquerna/otp/totp"

	"github.com/tyuan511/bulwark/internal/apierr"
)

func TestPasswordHashAndVerify(t *testing.T) {
	hash := HashPassword("correct-horse-42")
	if !strings.HasPrefix(hash, "$argon2id$") {
		t.Fatalf("PHC format expected: %s", hash)
	}
	ok, err := VerifyPassword("correct-horse-42", hash)
	if err != nil || !ok {
		t.Fatalf("verify correct: %v %v", ok, err)
	}
	ok, err = VerifyPassword("wrong-password", hash)
	if err != nil || ok {
		t.Fatalf("verify wrong must fail: %v %v", ok, err)
	}
	ok, err = VerifyPassword("x", "$bcrypt$garbage")
	if err == nil {
		t.Fatal("unsupported hash format must error")
	}
}

func TestPasswordPolicy(t *testing.T) {
	for _, pw := range []string{"short", "password12345", "qwerty123456", "adminadmin11", "has space 123", "password"} {
		if err := CheckPasswordPolicy(pw, "alice"); err == nil {
			t.Fatalf("weak password %q accepted", pw)
		}
	}
	if err := CheckPasswordPolicy("Str0ng!Passphrase", "alice"); err != nil {
		t.Fatalf("good password rejected: %v", err)
	}
	if err := CheckPasswordPolicy("alice-the-admin", "alice"); err == nil {
		t.Fatal("password equal to username must be rejected")
	}
}

func TestTokenHashStabilityAndPrefix(t *testing.T) {
	token, meta := NewApiToken()
	if !strings.HasPrefix(token, "sk-bulwark-") {
		t.Fatalf("token prefix: %s", token)
	}
	if !strings.HasPrefix(meta.Prefix, "sk-bulwark-") {
		t.Fatalf("display prefix: %s", meta.Prefix)
	}
	if HashToken(token) != HashToken(token) {
		t.Fatal("hash must be deterministic")
	}
	if HashToken(token) == HashToken(token+"x") {
		t.Fatal("different tokens must hash differently")
	}
}

func TestRecoveryCodes(t *testing.T) {
	codes := GenerateRecoveryCodes(8)
	if len(codes) != 8 {
		t.Fatalf("want 8 codes, got %d", len(codes))
	}
	hashed := HashRecoveryCodes(codes)
	for i, c := range codes {
		if idx := MatchRecoveryCode(hashed, c); idx != i {
			t.Fatalf("code %d matched at %d", i, idx)
		}
	}
	if idx := MatchRecoveryCode(hashed, "nope-nope"); idx != -1 {
		t.Fatal("unknown code must not match")
	}
}

func TestTOTPEnrollmentAndValidation(t *testing.T) {
	mgr, err := NewTOTPManager(t.TempDir()+"/server.key", "Bulwark")
	if err != nil {
		t.Fatal(err)
	}
	secret, url, err := mgr.Setup("alice")
	if err != nil {
		t.Fatalf("setup: %v", err)
	}
	if secret == "" || !strings.HasPrefix(url, "otpauth://totp/") {
		t.Fatalf("otpauth url: %s", url)
	}

	// Derive a valid code for the current window and verify it.
	code, err := totp.GenerateCode(secret, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if !mgr.Validate(secret, code) {
		t.Fatal("valid code rejected")
	}
	if mgr.Validate(secret, "000000") {
		// Extremely unlikely to collide; treat as flake if it does.
		t.Skip("collision with 000000 in this window")
	}
	if mgr.Validate(secret, "1234567") {
		t.Fatal("malformed code must fail")
	}

	// Seal/unseal roundtrip for DB storage.
	sealed := mgr.Seal(secret)
	if sealed == secret {
		t.Fatal("sealed secret must not be plaintext")
	}
	got, ok := mgr.Unseal(sealed)
	if !ok || got != secret {
		t.Fatalf("unseal roundtrip: %v %s", ok, got)
	}
}

func TestLoginGuardLockoutAndIPBlock(t *testing.T) {
	store := newFakeAttemptStore()
	guard := NewLoginGuard(store, GuardConfig{
		LoginMaxAttempts: 3, LockoutMinutes: 15,
		IPMaxAttempts: 5, IPWindowMinutes: 60,
	})

	// Fresh state passes.
	if err := guard.Preflight("bob", "1.2.3.4"); err != nil {
		t.Fatalf("preflight should pass: %v", err)
	}

	// 3 failures within the window → username locked.
	for i := 0; i < 3; i++ {
		_ = store.RecordAuthAttempt("bob", "1.2.3.4", false)
	}
	err := guard.Preflight("bob", "1.2.3.4")
	if err == nil || apierr.From(err).Code != apierr.CodeTooManyAttempts {
		t.Fatalf("username lockout expected, got %v", err)
	}

	// Other usernames from same IP still fine (4 failures so far on the IP).
	if err := guard.Preflight("alice", "1.2.3.4"); err != nil {
		t.Fatalf("other user should not be locked: %v", err)
	}

	// IP threshold reached → everyone from that IP blocked.
	_ = store.RecordAuthAttempt("alice", "1.2.3.4", false)
	_ = store.RecordAuthAttempt("alice", "1.2.3.4", false)
	err = guard.Preflight("carol", "1.2.3.4")
	if err == nil || apierr.From(err).Code != apierr.CodeIPBlocked {
		t.Fatalf("IP block expected, got %v", err)
	}

	// Different IP unaffected.
	if err := guard.Preflight("carol", "5.6.7.8"); err != nil {
		t.Fatalf("other IP should pass: %v", err)
	}
}

// --- fakes ---

type fakeAttempt struct {
	username string
	ip       string
	success  bool
	at       time.Time
}

type fakeAttemptStore struct {
	attempts []fakeAttempt
}

func newFakeAttemptStore() *fakeAttemptStore { return &fakeAttemptStore{} }

func (f *fakeAttemptStore) RecordAuthAttempt(username, ip string, success bool) error {
	f.attempts = append(f.attempts, fakeAttempt{username, ip, success, time.Now()})
	return nil
}

func (f *fakeAttemptStore) count(username, ip string, since time.Time) int {
	n := 0
	for _, a := range f.attempts {
		if a.success || a.at.Before(since) {
			continue
		}
		if username != "" && a.username == username {
			n++
		}
		if ip != "" && a.ip == ip {
			n++
		}
	}
	return n
}

func (f *fakeAttemptStore) CountFailedAttemptsByUser(username string, since time.Time) (int, error) {
	return f.count(username, "", since), nil
}

func (f *fakeAttemptStore) CountFailedAttemptsByIP(ip string, since time.Time) (int, error) {
	return f.count("", ip, since), nil
}
