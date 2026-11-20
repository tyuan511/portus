-- name: UpsertRule :exec
INSERT INTO security_rules (id, name, match_kind, pattern, decision, description, enabled, priority)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (id) DO UPDATE SET
    name = excluded.name, match_kind = excluded.match_kind, pattern = excluded.pattern,
    decision = excluded.decision, description = excluded.description,
    enabled = excluded.enabled, priority = excluded.priority;

-- name: ListRules :many
SELECT * FROM security_rules ORDER BY priority DESC;

-- name: DeleteRule :execrows
DELETE FROM security_rules WHERE id = $1;

-- name: CountRules :one
SELECT COUNT(*) FROM security_rules;

-- name: GetL2Config :one
SELECT * FROM l2_configs ORDER BY id LIMIT 1;

-- name: UpsertL2Config :exec
INSERT INTO l2_configs (id, name, protocol, base_url, model, api_key_credential_id,
    timeout_ms, risk_threshold_ask, risk_threshold_deny, enabled)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
ON CONFLICT (id) DO UPDATE SET
    name = excluded.name, protocol = excluded.protocol, base_url = excluded.base_url,
    model = excluded.model, api_key_credential_id = excluded.api_key_credential_id,
    timeout_ms = excluded.timeout_ms, risk_threshold_ask = excluded.risk_threshold_ask,
    risk_threshold_deny = excluded.risk_threshold_deny, enabled = excluded.enabled;

-- name: UpsertSnippet :exec
INSERT INTO snippets (id, name, command, description, tags_json)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (id) DO UPDATE SET
    name = excluded.name, command = excluded.command,
    description = excluded.description, tags_json = excluded.tags_json;

-- name: ListSnippets :many
SELECT * FROM snippets ORDER BY name;

-- name: DeleteSnippet :execrows
DELETE FROM snippets WHERE id = $1;

-- name: GetSetting :one
SELECT value FROM app_settings WHERE key = $1;

-- name: SetSetting :exec
INSERT INTO app_settings (key, value, updated_at) VALUES ($1, $2, $3)
ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at;
