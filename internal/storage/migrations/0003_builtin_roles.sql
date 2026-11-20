-- Built-in roles: the editable roles table is replaced by a plain role column
-- on users holding one of the two fixed names ('admin' | 'member'). Host
-- grants are machine-group scoped only (per-server grants are dropped).

ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'member'
    CHECK (role IN ('admin', 'member'));
UPDATE users SET role = COALESCE(
    (SELECT r.name FROM roles r WHERE r.id = users.role_id), 'member');
ALTER TABLE users DROP COLUMN role_id;
DROP TABLE IF EXISTS roles;

CREATE TABLE host_grants_builtin (
    id TEXT PRIMARY KEY,
    user_group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
    group_name TEXT NOT NULL,
    can_connect INTEGER NOT NULL DEFAULT 0,
    can_execute INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
INSERT INTO host_grants_builtin (id, user_group_id, group_name, can_connect, can_execute, created_by, created_at)
SELECT id, user_group_id, group_name, can_connect, can_execute, created_by, created_at
FROM host_grants WHERE group_name <> '';
DROP TABLE host_grants;
ALTER TABLE host_grants_builtin RENAME TO host_grants;
CREATE INDEX IF NOT EXISTS idx_host_grants_group ON host_grants(user_group_id);
