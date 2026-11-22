-- name: InsertAudit :exec
INSERT INTO audit_logs (id, trace_id, actor, timestamp, server_id, server_name, ssh_user,
    server_host, operator, user_id, username, session_id, command, intent, dry_run, decision,
    l1_verdict_json, l2_verdict_json, approval_id, approval_status, exit_code, duration_ms,
    stdout_preview, stderr_preview, error, note, prev_hash, event_hash)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28);

-- name: ListMCPSessions :many
-- Sessions are views over the audit trail: every event carrying the same
-- agent-supplied session_id. NULL user_id = all users (admin scope).
SELECT session_id,
    MAX(username)::text AS username,
    MIN(timestamp)::text AS first_at,
    MAX(timestamp)::text AS last_at,
    COUNT(*)::bigint AS event_count,
    COALESCE(STRING_AGG(DISTINCT NULLIF(server_name, ''), ', '), '')::text AS servers
FROM audit_logs
WHERE session_id <> ''
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
GROUP BY session_id
ORDER BY MAX(seq) DESC
LIMIT sqlc.narg('lim')::bigint
OFFSET sqlc.narg('offs')::bigint;

-- name: CountMCPSessions :one
SELECT COUNT(DISTINCT session_id) FROM audit_logs
WHERE session_id <> ''
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text);

-- name: SelectAuditBySession :many
-- Ordered replay stream for one session; NULL user_id = admin scope.
SELECT * FROM audit_logs
WHERE session_id = sqlc.arg('session_id')::text
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
ORDER BY seq ASC;

-- name: GetAuditTailHash :one
SELECT event_hash FROM audit_logs ORDER BY seq DESC LIMIT 1;

-- name: SelectAuditLogs :many
-- All filters are optional: pass NULL to disable a dimension.
-- actor semantics: NULL = all; 'mcp' = every mcp-* channel;
-- 'terminal'/'manual' = interactive actors; anything else = exact match.
SELECT * FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('ssh_user')::text IS NULL OR ssh_user = sqlc.narg('ssh_user')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('username')::text IS NULL OR username = sqlc.narg('username')::text)
  AND (sqlc.narg('decision')::text IS NULL OR decision = sqlc.narg('decision')::text)
  AND (sqlc.narg('actor')::text IS NULL OR
       CASE WHEN sqlc.narg('actor')::text = 'mcp' THEN actor LIKE 'mcp%'
            WHEN sqlc.narg('actor')::text IN ('terminal', 'manual') THEN actor IN ('terminal', 'manual')
            ELSE actor = sqlc.narg('actor')::text END)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
  AND (
    sqlc.narg('q')::text IS NULL OR command ILIKE '%' || sqlc.narg('q')::text || '%'
    OR intent ILIKE '%' || sqlc.narg('q')::text || '%'
    OR server_name ILIKE '%' || sqlc.narg('q')::text || '%'
    OR ssh_user ILIKE '%' || sqlc.narg('q')::text || '%'
    OR server_host ILIKE '%' || sqlc.narg('q')::text || '%'
    OR operator ILIKE '%' || sqlc.narg('q')::text || '%'
    OR trace_id ILIKE '%' || sqlc.narg('q')::text || '%'
    OR username ILIKE '%' || sqlc.narg('q')::text || '%'
  )
  AND (sqlc.narg('only_failed')::int IS NULL OR sqlc.narg('only_failed')::int = 0 OR
       ((exit_code IS NOT NULL AND exit_code <> 0) OR (error IS NOT NULL AND error <> '')))
ORDER BY seq DESC
LIMIT sqlc.narg('lim')::bigint
OFFSET sqlc.narg('offs')::bigint;

-- name: CountAuditLogs :one
-- Total row count for the SelectAuditLogs filter set (pagination totals).
SELECT COUNT(*) FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('ssh_user')::text IS NULL OR ssh_user = sqlc.narg('ssh_user')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('username')::text IS NULL OR username = sqlc.narg('username')::text)
  AND (sqlc.narg('decision')::text IS NULL OR decision = sqlc.narg('decision')::text)
  AND (sqlc.narg('actor')::text IS NULL OR
       CASE WHEN sqlc.narg('actor')::text = 'mcp' THEN actor LIKE 'mcp%'
            WHEN sqlc.narg('actor')::text IN ('terminal', 'manual') THEN actor IN ('terminal', 'manual')
            ELSE actor = sqlc.narg('actor')::text END)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
  AND (
    sqlc.narg('q')::text IS NULL OR command ILIKE '%' || sqlc.narg('q')::text || '%'
    OR intent ILIKE '%' || sqlc.narg('q')::text || '%'
    OR server_name ILIKE '%' || sqlc.narg('q')::text || '%'
    OR ssh_user ILIKE '%' || sqlc.narg('q')::text || '%'
    OR server_host ILIKE '%' || sqlc.narg('q')::text || '%'
    OR operator ILIKE '%' || sqlc.narg('q')::text || '%'
    OR trace_id ILIKE '%' || sqlc.narg('q')::text || '%'
    OR username ILIKE '%' || sqlc.narg('q')::text || '%'
  )
  AND (sqlc.narg('only_failed')::int IS NULL OR sqlc.narg('only_failed')::int = 0 OR
       ((exit_code IS NOT NULL AND exit_code <> 0) OR (error IS NOT NULL AND error <> '')));

-- name: SelectAuditChain :many
SELECT * FROM audit_logs ORDER BY seq ASC;

-- name: SelectApprovalEvents :many
-- Resolved approval requests, newest first — the approval history feed.
SELECT id, trace_id, timestamp, server_id, server_name, ssh_user, user_id, username,
       command, intent, decision, approval_id, approval_status, note,
       l1_verdict_json, l2_verdict_json, error
FROM audit_logs
WHERE approval_id <> ''
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('status')::text IS NULL OR approval_status = sqlc.narg('status')::text)
ORDER BY seq DESC
LIMIT sqlc.narg('lim')::bigint
OFFSET sqlc.narg('offs')::bigint;

-- name: CountApprovalEvents :one
SELECT COUNT(*) FROM audit_logs
WHERE approval_id <> ''
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('status')::text IS NULL OR approval_status = sqlc.narg('status')::text);

-- name: AuditStatsTotals :one
SELECT
    COUNT(*) AS total_commands,
    COALESCE(SUM(CASE WHEN decision = 'allow' THEN 1 ELSE 0 END), 0)::bigint AS allowed_count,
    COALESCE(SUM(CASE WHEN approval_status = 'approved' THEN 1 ELSE 0 END), 0)::bigint AS approval_count,
    COALESCE(SUM(CASE WHEN decision = 'deny' THEN 1 ELSE 0 END), 0)::bigint AS denied_count,
    COALESCE(SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code <> 0) OR (error IS NOT NULL AND error <> '') THEN 1 ELSE 0 END), 0)::bigint AS failed_count,
    COALESCE(AVG(duration_ms), 0)::float8 AS avg_duration_ms
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text);

-- name: AuditStatsByActor :many
SELECT actor::text AS key, COUNT(*)::bigint AS count
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
GROUP BY actor ORDER BY COUNT(*) DESC LIMIT 10;

-- name: AuditStatsByUser :many
SELECT COALESCE(NULLIF(username, ''), 'legacy')::text AS key, COUNT(*)::bigint AS count
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
GROUP BY username ORDER BY COUNT(*) DESC LIMIT 10;

-- name: AuditStatsByServer :many
SELECT server_id, MAX(server_name)::text AS server_name, COUNT(*)::bigint AS count,
    COALESCE(SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code <> 0) OR (error IS NOT NULL AND error <> '') THEN 1 ELSE 0 END), 0)::bigint AS failed_count
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
GROUP BY server_id ORDER BY COUNT(*) DESC LIMIT 20;

-- name: AuditStatsTopCommands :many
SELECT command::text AS key, COUNT(*)::bigint AS count
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
GROUP BY command ORDER BY COUNT(*) DESC LIMIT 10;

-- name: AuditStatsDailyTrend :many
SELECT substr(timestamp, 1, 10)::text AS date, COUNT(*)::bigint AS total,
    COALESCE(SUM(CASE WHEN decision = 'deny' THEN 1 ELSE 0 END), 0)::bigint AS denied,
    COALESCE(SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code <> 0) OR (error IS NOT NULL AND error <> '') THEN 1 ELSE 0 END), 0)::bigint AS failed
FROM audit_logs
WHERE (sqlc.narg('server_id')::text IS NULL OR server_id = sqlc.narg('server_id')::text)
  AND (sqlc.narg('user_id')::text IS NULL OR user_id = sqlc.narg('user_id')::text)
  AND (sqlc.narg('from_ts')::text IS NULL OR timestamp >= sqlc.narg('from_ts')::text)
  AND (sqlc.narg('to_ts')::text IS NULL OR timestamp <= sqlc.narg('to_ts')::text)
GROUP BY date ORDER BY date DESC LIMIT 30;
