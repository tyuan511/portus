-- Bind host_grants directly to servers (server_id) instead of machine group_name.
-- Existing group_name-based grants are expanded onto matching servers so live
-- deployments preserve effective access on upgrade.

CREATE TABLE host_grants_v2 (
    id TEXT PRIMARY KEY,
    user_group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
    server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    can_connect INTEGER NOT NULL DEFAULT 0,
    can_execute INTEGER NOT NULL DEFAULT 0,
    can_transfer INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    UNIQUE (user_group_id, server_id)
);

INSERT INTO host_grants_v2 (
    id, user_group_id, server_id, can_connect, can_execute, can_transfer, created_by, created_at
)
SELECT
    MIN(g.id) || ':' || s.id,
    g.user_group_id,
    s.id,
    MAX(g.can_connect),
    MAX(g.can_execute),
    MAX(g.can_transfer),
    MIN(g.created_by),
    MIN(g.created_at)
FROM host_grants g
JOIN servers s ON g.group_name <> '' AND g.group_name = s.group_name
GROUP BY g.user_group_id, s.id;

DROP TABLE host_grants;
ALTER TABLE host_grants_v2 RENAME TO host_grants;
CREATE INDEX IF NOT EXISTS idx_host_grants_group ON host_grants(user_group_id);
CREATE INDEX IF NOT EXISTS idx_host_grants_server ON host_grants(server_id);
