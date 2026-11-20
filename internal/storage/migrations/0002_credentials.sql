-- Encrypted per-host credentials. Plaintext secrets never touch the database:
-- ciphertext = AES-256-GCM(server.key, nonce, secret, aad = credential id).
-- server.key is a 32-byte random file (0600) on the server host; a stolen
-- database dump alone is mathematically undecryptable.

CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,          -- password | private_key | api_key
    label TEXT NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    nonce BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
