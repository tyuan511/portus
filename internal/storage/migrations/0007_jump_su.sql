-- Support shell-based jump host traversal (`su` + `ssh`) in addition to
-- standard SSH direct-tcpip ProxyJump.

ALTER TABLE servers ADD COLUMN IF NOT EXISTS jump_mode TEXT NOT NULL DEFAULT '';
ALTER TABLE servers ADD COLUMN IF NOT EXISTS jump_su_username TEXT NOT NULL DEFAULT '';
ALTER TABLE servers ADD COLUMN IF NOT EXISTS jump_su_credential_id TEXT NOT NULL DEFAULT '';
