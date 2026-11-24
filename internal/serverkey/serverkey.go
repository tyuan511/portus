// Package serverkey owns the process-local AEAD root key file (server.key):
// 32 random bytes, 0600, created on first boot. It encrypts every secret
// stored server-side (TOTP enrollment secrets, per-host SSH credentials).
// A stolen database dump without this file is undecryptable.
package serverkey

import (
	"crypto/rand"
	"fmt"
	"os"
	"path/filepath"
)

// LoadOrCreate returns the 32-byte key at path, creating it (0600) when absent.
func LoadOrCreate(path string) ([]byte, error) {
	if raw, err := os.ReadFile(path); err == nil { // #nosec G304 -- configured key path
		if len(raw) != 32 {
			return nil, fmt.Errorf("server key %s has wrong size (%d bytes, want 32)", path, len(raw))
		}
		return raw, nil
	}
	key := make([]byte, 32)
	if _, err := rand.Read(key); err != nil {
		return nil, fmt.Errorf("generate server key: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, fmt.Errorf("create server key dir: %w", err)
	}
	if err := os.WriteFile(path, key, 0o600); err != nil {
		return nil, fmt.Errorf("write server key: %w", err)
	}
	return key, nil
}
