package auth

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"

	"github.com/tyuan511/bulwark/internal/domain"
)

// SessionCookieName is the HttpOnly cookie holding the web session token.
const SessionCookieName = "bulwark_session"

// TokenPrefix is the recognizable prefix for API tokens.
const TokenPrefix = "sk-bulwark-"

// HashToken returns the SHA-256 hex digest stored in the database (never the
// raw token).
func HashToken(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

// NewSessionToken generates a 256-bit opaque session token.
func NewSessionToken() string { return randomHex(32) }

// NewApiToken generates a new personal access token with display prefix.
func NewApiToken() (token string, meta domain.ApiToken) {
	secret := randomHex(24)
	token = TokenPrefix + secret
	prefix := token[:len(TokenPrefix)+8] // sk-bulwark-XXXXXXXX…
	return token, domain.ApiToken{Prefix: prefix}
}

// GenerateRecoveryCodes creates n one-time TOTP recovery codes; returned in
// plaintext once, hash them before storage.
func GenerateRecoveryCodes(n int) []string {
	codes := make([]string, n)
	for i := range codes {
		codes[i] = randomHex(5) + "-" + randomHex(5)
	}
	return codes
}

// HashRecoveryCodes hashes each code for storage.
func HashRecoveryCodes(codes []string) []string {
	out := make([]string, len(codes))
	for i, c := range codes {
		out[i] = HashToken(strings.ToLower(strings.TrimSpace(c)))
	}
	return out
}

// MatchRecoveryCode checks a code against the stored hashes and returns the
// index of the matched (consumed) code, or -1.
func MatchRecoveryCode(hashed []string, code string) int {
	normalized := HashToken(strings.ToLower(strings.TrimSpace(code)))
	for i, h := range hashed {
		if h == normalized {
			return i
		}
	}
	return -1
}

func randomHex(n int) string {
	return hex.EncodeToString(randomBytes(n))
}
