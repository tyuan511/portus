-- name: CreateCredential :exec
INSERT INTO credentials (id, kind, label, username, nonce, ciphertext, created_by, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8);

-- name: GetCredential :one
SELECT * FROM credentials WHERE id = $1;

-- name: ListCredentials :many
SELECT id, kind, label, username, created_by, created_at FROM credentials ORDER BY created_at DESC;

-- name: DeleteCredential :execrows
DELETE FROM credentials WHERE id = $1;

-- name: CountCredentials :one
SELECT COUNT(*) FROM credentials;
