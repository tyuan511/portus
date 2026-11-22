-- File transfer (SFTP) authorization bit alongside connect/execute. The
-- grant model stays user-group × host-group; transfer is a third capability.

ALTER TABLE host_grants ADD COLUMN IF NOT EXISTS can_transfer INTEGER NOT NULL DEFAULT 0;

-- Free-text reason captured when an approval is decided, surfaced in the
-- approval history and audit trail.
ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS note TEXT NOT NULL DEFAULT '';
