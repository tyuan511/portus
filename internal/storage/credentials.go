package storage

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"database/sql"
	"errors"
	"fmt"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/storage/db"
)

// CredentialKind mirrors domain vault kinds; kept as plain strings in SQL.
const (
	CredKindPassword   = "password"
	CredKindPrivateKey = "private_key"
	CredKindAPIKey     = "api_key"
)

var errNoEncryptionKey = errors.New("storage: encryption key not configured")

// SetEncryptionKey wires the process-local server key used to seal credential
// secrets at rest. Without it, credential APIs fail closed.
func (h *DB) SetEncryptionKey(key []byte) error {
	if len(key) != 32 {
		return apierr.Internal("encryption key must be 32 bytes", nil)
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.encKey = key
	return nil
}

// sealSecret encrypts a plaintext secret with the server key; the credential
// id is bound as AEAD associated data so rows cannot be swapped.
func (h *DB) sealSecret(id, plaintext string) (nonce, ciphertext []byte, err error) {
	h.mu.RLock()
	key := h.encKey
	h.mu.RUnlock()
	if len(key) != 32 {
		return nil, nil, apierr.Internal("encryption key not configured", nil)
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, nil, err
	}
	nonce = make([]byte, gcm.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, nil, err
	}
	return nonce, gcm.Seal(nil, nonce, []byte(plaintext), []byte(id)), nil
}

func (h *DB) openSecret(id string, nonce, ciphertext []byte) (string, error) {
	h.mu.RLock()
	key := h.encKey
	h.mu.RUnlock()
	if len(key) != 32 {
		return "", apierr.Internal("encryption key not configured", nil)
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return "", err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}
	pt, err := gcm.Open(nil, nonce, ciphertext, []byte(id))
	if err != nil {
		return "", apierr.VaultError("credential decryption failed (tampered or wrong server key)")
	}
	return string(pt), nil
}

// CreateCredential seals and stores a per-host secret (password / private key
// / API key). Plaintext is only ever held in memory during encryption.
func (h *DB) CreateCredential(id, kind, label, username, secret, createdBy string) (*domain.VaultCredentialMeta, error) {
	nonce, ciphertext, err := h.sealSecret(id, secret)
	if err != nil {
		return nil, err
	}
	err = h.q.CreateCredential(context.Background(), db.CreateCredentialParams{
		ID:         id,
		Kind:       kind,
		Label:      label,
		Username:   username,
		Nonce:      nonce,
		Ciphertext: ciphertext,
		CreatedBy:  createdBy,
		CreatedAt:  nowRFC3339(),
	})
	if err != nil {
		return nil, fmt.Errorf("create credential: %w", err)
	}
	return &domain.VaultCredentialMeta{
		ID:        id,
		Label:     label,
		Kind:      domain.VaultCredentialKind(kind),
		Username:  username,
		Preview:   previewSecret(secret, kind),
		CreatedAt: nowUTC(),
	}, nil
}

// GetSecret decrypts a credential. Implements sshpool.SecretResolver and
// security.SecretResolver, so SSH auth and the L2 reviewer consume stored
// secrets transparently.
func (h *DB) GetSecret(id string) (string, error) {
	row, err := h.q.GetCredential(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return "", apierr.NotFoundf("credential %s not found", id)
	}
	if err != nil {
		return "", fmt.Errorf("get credential: %w", err)
	}
	return h.openSecret(id, row.Nonce, row.Ciphertext)
}

// CredentialMeta is the non-secret projection returned by the API.
type CredentialMeta struct {
	ID        string `json:"id"`
	Kind      string `json:"kind"`
	Label     string `json:"label"`
	Username  string `json:"username,omitempty"`
	CreatedBy string `json:"created_by,omitempty"`
	CreatedAt string `json:"created_at"`
}

// ListCredentials returns metadata for all credentials (never secrets).
func (h *DB) ListCredentials() ([]CredentialMeta, error) {
	rows, err := h.q.ListCredentials(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list credentials: %w", err)
	}
	out := make([]CredentialMeta, 0, len(rows))
	for _, r := range rows {
		out = append(out, CredentialMeta{
			ID:        r.ID,
			Kind:      r.Kind,
			Label:     r.Label,
			Username:  r.Username,
			CreatedBy: r.CreatedBy,
			CreatedAt: r.CreatedAt,
		})
	}
	return out, nil
}

// DeleteCredential removes a credential; returns whether a row was deleted.
func (h *DB) DeleteCredential(id string) (bool, error) {
	n, err := h.q.DeleteCredential(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete credential: %w", err)
	}
	return n > 0, nil
}

// previewSecret builds the masked listing preview.
func previewSecret(secret, kind string) string {
	switch kind {
	case CredKindPassword:
		return "••••••••"
	case CredKindPrivateKey:
		return fmt.Sprintf("PEM Key (%d bytes)", len(secret))
	default:
		r := []rune(secret)
		if len(r) <= 8 {
			return "***"
		}
		return string(r[:4]) + "***" + string(r[len(r)-4:])
	}
}
