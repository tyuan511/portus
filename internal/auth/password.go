// Package auth provides password hashing (Argon2id), web sessions, personal
// access tokens, TOTP second factor, and brute-force guards.
package auth

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"fmt"
	"strings"

	"golang.org/x/crypto/argon2"

	"github.com/tyuan511/bulwark/internal/apierr"
)

// Argon2id parameters for password hashing (login path runs frequently, so
// parameters are tuned lighter than the vault master key derivation).
const (
	pwTime    = 2
	pwMemoryK = 32 * 1024 // 32 MiB
	pwThreads = 2
	pwKeyLen  = 32
	pwSaltLen = 16
)

// HashPassword produces a PHC-format Argon2id hash:
// $argon2id$v=19$m=32768,t=2,p=2$<salt>$<hash>
func HashPassword(password string) string {
	salt := randomBytes(pwSaltLen)
	key := argon2.IDKey([]byte(password), salt, pwTime, pwMemoryK, pwThreads, pwKeyLen)
	return fmt.Sprintf("$argon2id$v=%d$m=%d,t=%d,p=%d$%s$%s",
		argon2.Version, pwMemoryK, pwTime, pwThreads,
		base64.RawStdEncoding.EncodeToString(salt),
		base64.RawStdEncoding.EncodeToString(key))
}

// VerifyPassword checks a password against a PHC-format hash in constant time.
// Unknown formats return an error; callers treat that as a failed verification
// (legacy hashes force a reset through password change).
func VerifyPassword(password, encoded string) (bool, error) {
	parts := strings.Split(encoded, "$")
	if len(parts) != 6 || parts[1] != "argon2id" {
		return false, fmt.Errorf("unsupported password hash format")
	}
	var version int
	if _, err := fmt.Sscanf(parts[2], "v=%d", &version); err != nil {
		return false, fmt.Errorf("bad hash version: %w", err)
	}
	var m uint32
	var t uint32
	var p uint8
	if _, err := fmt.Sscanf(parts[3], "m=%d,t=%d,p=%d", &m, &t, &p); err != nil {
		return false, fmt.Errorf("bad hash params: %w", err)
	}
	salt, err := base64.RawStdEncoding.DecodeString(parts[4])
	if err != nil {
		return false, fmt.Errorf("bad hash salt: %w", err)
	}
	want, err := base64.RawStdEncoding.DecodeString(parts[5])
	if err != nil {
		return false, fmt.Errorf("bad hash digest: %w", err)
	}
	got := argon2.IDKey([]byte(password), salt, t, m, p, uint32(len(want)))
	return subtle.ConstantTimeCompare(got, want) == 1, nil
}

// DummyVerify burns comparable CPU time for unknown users so login timing
// does not leak account existence.
func DummyVerify() {
	salt := make([]byte, pwSaltLen)
	_ = argon2.IDKey([]byte("timing-equalizer"), salt, pwTime, pwMemoryK, pwThreads, pwKeyLen)
}

// CheckPasswordPolicy enforces the enterprise password policy.
func CheckPasswordPolicy(password, username string) error {
	if len(password) < 10 {
		return apierr.Unprocessable("password must be at least 10 characters")
	}
	if len(password) > 128 {
		return apierr.Unprocessable("password must be at most 128 characters")
	}
	if strings.Contains(strings.ToLower(password), strings.ToLower(username)) {
		return apierr.Unprocessable("password must not contain the username")
	}
	if strings.ContainsAny(password, " \t\n") {
		return apierr.Unprocessable("password must not contain whitespace")
	}
	lower := strings.ToLower(password)
	for _, common := range []string{"password", "qwerty", "12345678", "letmein", "admin", "welcome", "iloveyou"} {
		if strings.Contains(lower, common) {
			return apierr.Unprocessable("password contains a banned pattern")
		}
	}
	return nil
}

func randomBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return b
}
