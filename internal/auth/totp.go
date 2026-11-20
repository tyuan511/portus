package auth

import (
	"crypto/aes"
	"crypto/cipher"
	"encoding/hex"
	"sync"
	"time"

	"github.com/pquerna/otp"
	"github.com/pquerna/otp/totp"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/serverkey"
)

// TOTPManager handles second-factor enrollment and verification. Secrets are
// sealed at rest with the server key file (AES-256-GCM), which is generated
// with 0600 permissions on first boot.
type TOTPManager struct {
	keyPath string
	mu      sync.Mutex
	key     []byte
	issuer  string
}

// NewTOTPManager loads (or creates) the server key at keyPath.
// Prefer NewTOTPManagerWithKey when the process already owns the shared
// server key (same file encrypts credentials too).
func NewTOTPManager(keyPath, issuer string) (*TOTPManager, error) {
	key, err := serverkey.LoadOrCreate(keyPath)
	if err != nil {
		return nil, err
	}
	return NewTOTPManagerWithKey(key, issuer), nil
}

// NewTOTPManagerWithKey builds a manager over an existing server key.
func NewTOTPManagerWithKey(key []byte, issuer string) *TOTPManager {
	return &TOTPManager{key: key, issuer: issuer}
}

func (m *TOTPManager) seal(plaintext string) string {
	block, err := aes.NewCipher(m.key)
	if err != nil {
		panic(err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		panic(err)
	}
	nonce := randomBytes(12)
	ct := gcm.Seal(nil, nonce, []byte(plaintext), []byte("sshield-totp-v1"))
	return hex.EncodeToString(nonce) + ":" + hex.EncodeToString(ct)
}

func (m *TOTPManager) unseal(sealed string) (string, bool) {
	parts := splitOnce(sealed, ":")
	if len(parts) != 2 {
		return "", false
	}
	nonce, err1 := hex.DecodeString(parts[0])
	ct, err2 := hex.DecodeString(parts[1])
	if err1 != nil || err2 != nil {
		return "", false
	}
	block, err := aes.NewCipher(m.key)
	if err != nil {
		return "", false
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", false
	}
	pt, err := gcm.Open(nil, nonce, ct, []byte("sshield-totp-v1"))
	if err != nil {
		return "", false
	}
	return string(pt), true
}

func splitOnce(s, sep string) []string {
	for i := 0; i+len(sep) <= len(s); i++ {
		if s[i:i+len(sep)] == sep {
			return []string{s[:i], s[i+len(sep):]}
		}
	}
	return []string{s}
}

// Setup generates a fresh TOTP secret and otpauth URL for enrollment.
func (m *TOTPManager) Setup(username string) (secret string, otpauthURL string, err error) {
	key, err := totp.Generate(totp.GenerateOpts{
		Issuer:      m.issuer,
		AccountName: username,
		Period:      30,
		Digits:      otp.DigitsSix,
		Algorithm:   otp.AlgorithmSHA1,
	})
	if err != nil {
		return "", "", apierr.Internal("generate TOTP secret", err)
	}
	return key.Secret(), key.URL(), nil
}

// Validate checks a 6-digit code against the secret (±1 time step window).
func (m *TOTPManager) Validate(secret, code string) bool {
	if len(code) != 6 {
		return false
	}
	ok, err := totp.ValidateCustom(code, secret, time.Now(), totp.ValidateOpts{
		Period: 30, Skew: 1, Digits: otp.DigitsSix, Algorithm: otp.AlgorithmSHA1,
	})
	return err == nil && ok
}

// Seal wraps a raw secret for DB storage.
func (m *TOTPManager) Seal(secret string) string { return m.seal(secret) }

// Unseal recovers the raw secret from DB storage.
func (m *TOTPManager) Unseal(sealed string) (string, bool) { return m.unseal(sealed) }
