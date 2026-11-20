package vault

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/yuantang/sshield/internal/domain"
)

func TestVaultLifecycle(t *testing.T) {
	path := filepath.Join(t.TempDir(), "vault.enc")
	v := New(path, 0)

	if got := v.Status().State; got != domain.VaultUninitialized {
		t.Fatalf("initial state: %s", got)
	}
	if err := v.Initialize("short"); err == nil {
		t.Fatal("short passphrase must be rejected")
	}
	if err := v.Initialize("correct-horse-battery-staple"); err != nil {
		t.Fatalf("initialize: %v", err)
	}
	if err := v.Initialize("another-passphrase"); err == nil {
		t.Fatal("re-initialize must fail")
	}
	if got := v.Status().State; got != domain.VaultUnlocked {
		t.Fatalf("state after init: %s", got)
	}

	meta, err := v.StoreSecret("", "OpenAI Key", domain.CredAPIKey, "", "sk-proj-1234567890abcdef")
	if err != nil {
		t.Fatalf("store: %v", err)
	}
	if meta.Preview != "sk-p***cdef" {
		t.Fatalf("preview: %s", meta.Preview)
	}

	secret, err := v.GetSecret(meta.ID)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if secret != "sk-proj-1234567890abcdef" {
		t.Fatalf("roundtrip: %s", secret)
	}

	// Lock wipes the key.
	v.Lock()
	if got := v.Status().State; got != domain.VaultLocked {
		t.Fatalf("state after lock: %s", got)
	}
	if _, err := v.GetSecret(meta.ID); err == nil {
		t.Fatal("get must fail while locked")
	}
	// Metadata still readable while locked.
	metas, err := v.ListCredentialsMeta()
	if err != nil || len(metas) != 1 {
		t.Fatalf("meta while locked: %v %d", err, len(metas))
	}

	// Wrong passphrase rejected.
	if err := v.Unlock("wrong-passphrase"); err == nil {
		t.Fatal("wrong passphrase must fail")
	}
	// Correct passphrase unlocks and round-trips.
	if err := v.Unlock("correct-horse-battery-staple"); err != nil {
		t.Fatalf("unlock: %v", err)
	}
	secret, err = v.GetSecret(meta.ID)
	if err != nil || secret != "sk-proj-1234567890abcdef" {
		t.Fatalf("after unlock: %v %s", err, secret)
	}

	// Delete.
	ok, err := v.DeleteSecret(meta.ID)
	if err != nil || !ok {
		t.Fatalf("delete: %v %v", ok, err)
	}
	if ok, _ := v.DeleteSecret(meta.ID); ok {
		t.Fatal("second delete must report false")
	}
}

func TestVaultAutoLock(t *testing.T) {
	path := filepath.Join(t.TempDir(), "vault.enc")
	v := New(path, time.Second)
	if err := v.Initialize("auto-lock-passphrase"); err != nil {
		t.Fatal(err)
	}
	meta, err := v.StoreSecret("", "k", domain.CredPassword, "u", "s3cret")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := v.GetSecret(meta.ID); err != nil {
		t.Fatal(err)
	}
	// Simulate idle expiry by rewinding last_used.
	v.mu.Lock()
	v.lastUsed = v.lastUsed.Add(-2 * time.Second)
	v.mu.Unlock()
	if _, err := v.GetSecret(meta.ID); err == nil {
		t.Fatal("idle vault must auto-lock")
	}
	if got := v.Status().State; got != domain.VaultLocked {
		t.Fatalf("state after idle: %s", got)
	}
}

func TestVaultTamperDetection(t *testing.T) {
	path := filepath.Join(t.TempDir(), "vault.enc")
	v := New(path, 0)
	_ = v.Initialize("tamper-check-passphrase")
	meta, _ := v.StoreSecret("", "k", domain.CredAPIKey, "", "value-123456789")
	_ = v.Unlock("tamper-check-passphrase")

	// Flip a byte in the stored ciphertext.
	raw, _ := os.ReadFile(path)
	for i := len(raw) - 1; i >= 0; i-- {
		if raw[i] != '"' && raw[i] != '}' {
			raw[i] ^= 0x01
			break
		}
	}
	_ = os.WriteFile(path, raw, 0o600)
	v.cachedFile = nil // force re-read

	if _, err := v.GetSecret(meta.ID); err == nil {
		t.Fatal("tampered ciphertext must fail to decrypt")
	}
}
