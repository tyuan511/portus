//! Storage layer supporting SQLite (Phase 1/2 local/single-node) and PostgreSQL
//! via `sqlx`, with tamper-evident SHA-256 audit hash chaining and JSONL export.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use sshield_core::{
    ApprovalDecision, AuditChainStatus, AuditEvent, AuthKind, CommandSnippet, CommandVerdict,
    L2Protocol, L2ProviderConfig, PolicyDecision, Result, RuleMatchKind, SecurityRule,
    SemanticReviewVerdict, ServerAsset, SshieldError,
};
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
    audit_jsonl_path: Option<PathBuf>,
}

impl Storage {
    /// Connect to a SQLite database and run embedded schema migrations.
    pub async fn connect(database_url: &str, audit_jsonl_path: Option<PathBuf>) -> Result<Self> {
        if let Some(file_path) = database_url.strip_prefix("sqlite://")
            && file_path != ":memory:"
            && !file_path.is_empty()
        {
            let path = Path::new(file_path);
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    SshieldError::Storage(format!("Failed to create DB directory: {e}"))
                })?;
            }
        }

        let options: SqliteConnectOptions = database_url
            .parse::<SqliteConnectOptions>()
            .map_err(|e| SshieldError::Storage(format!("Invalid SQLite URL `{database_url}`: {e}")))?
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to connect SQLite: {e}")))?;

        let storage = Self {
            pool,
            audit_jsonl_path,
        };
        storage.migrate().await?;
        Ok(storage)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                username TEXT NOT NULL,
                auth_kind TEXT NOT NULL,
                identity_file TEXT,
                credential_id TEXT,
                jump_host_id TEXT,
                group_name TEXT,
                tags_json TEXT NOT NULL,
                description TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS security_rules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                match_kind TEXT NOT NULL,
                pattern TEXT NOT NULL,
                decision TEXT NOT NULL,
                description TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS l2_configs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                protocol TEXT NOT NULL,
                base_url TEXT NOT NULL,
                model TEXT NOT NULL,
                api_key_credential_id TEXT,
                timeout_ms INTEGER NOT NULL,
                risk_threshold_ask INTEGER NOT NULL,
                risk_threshold_deny INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS snippets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                description TEXT,
                tags_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_logs (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT UNIQUE NOT NULL,
                trace_id TEXT NOT NULL DEFAULT '',
                actor TEXT NOT NULL DEFAULT 'mcp',
                timestamp TEXT NOT NULL,
                server_id TEXT NOT NULL,
                server_name TEXT NOT NULL,
                command TEXT NOT NULL,
                intent TEXT NOT NULL,
                dry_run INTEGER NOT NULL,
                decision TEXT NOT NULL,
                l1_verdict_json TEXT,
                l2_verdict_json TEXT,
                approval_id TEXT,
                approval_status TEXT,
                exit_code INTEGER,
                duration_ms INTEGER,
                stdout_preview TEXT,
                stderr_preview TEXT,
                error TEXT,
                prev_hash TEXT NOT NULL DEFAULT 'GENESIS',
                event_hash TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_audit_logs_ts ON audit_logs(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_logs_server ON audit_logs(server_id);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Migration failed: {e}")))?;

        // Idempotent column migrations for pre-existing Phase 1 SQLite databases
        let alters = [
            "ALTER TABLE servers ADD COLUMN credential_id TEXT",
            "ALTER TABLE servers ADD COLUMN group_name TEXT",
            "ALTER TABLE audit_logs ADD COLUMN trace_id TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE audit_logs ADD COLUMN actor TEXT NOT NULL DEFAULT 'mcp'",
            "ALTER TABLE audit_logs ADD COLUMN l1_verdict_json TEXT",
            "ALTER TABLE audit_logs ADD COLUMN l2_verdict_json TEXT",
            "ALTER TABLE audit_logs ADD COLUMN approval_id TEXT",
            "ALTER TABLE audit_logs ADD COLUMN prev_hash TEXT NOT NULL DEFAULT 'GENESIS'",
            "ALTER TABLE audit_logs ADD COLUMN event_hash TEXT NOT NULL DEFAULT ''",
        ];
        for stmt in alters {
            let _ = sqlx::query(stmt).execute(&self.pool).await;
        }

        Ok(())
    }

    pub async fn seed_default_rules_if_empty(&self, defaults: &[SecurityRule]) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM security_rules")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to count rules: {e}")))?;

        if count == 0 {
            for rule in defaults {
                self.upsert_rule(rule).await?;
            }
        }
        Ok(())
    }

    pub async fn list_servers(&self) -> Result<Vec<ServerAsset>> {
        let rows = sqlx::query(
            "SELECT id, name, host, port, username, auth_kind, identity_file, credential_id, jump_host_id, group_name, tags_json, description, enabled FROM servers ORDER BY name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to list servers: {e}")))?;

        rows.into_iter().map(row_to_server).collect()
    }

    pub async fn get_server(&self, id: &str) -> Result<Option<ServerAsset>> {
        let row = sqlx::query(
            "SELECT id, name, host, port, username, auth_kind, identity_file, credential_id, jump_host_id, group_name, tags_json, description, enabled FROM servers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to get server: {e}")))?;

        row.map(row_to_server).transpose()
    }

    pub async fn upsert_server(&self, asset: &ServerAsset) -> Result<()> {
        let auth_kind_str = serde_json::to_string(&asset.auth_kind)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        let tags_json = serde_json::to_string(&asset.tags)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO servers (id, name, host, port, username, auth_kind, identity_file, credential_id, jump_host_id, group_name, tags_json, description, enabled, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                host = excluded.host,
                port = excluded.port,
                username = excluded.username,
                auth_kind = excluded.auth_kind,
                identity_file = excluded.identity_file,
                credential_id = excluded.credential_id,
                jump_host_id = excluded.jump_host_id,
                group_name = excluded.group_name,
                tags_json = excluded.tags_json,
                description = excluded.description,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&asset.id)
        .bind(&asset.name)
        .bind(&asset.host)
        .bind(asset.port as i64)
        .bind(&asset.username)
        .bind(auth_kind_str.trim_matches('"'))
        .bind(&asset.identity_file)
        .bind(&asset.credential_id)
        .bind(&asset.jump_host_id)
        .bind(&asset.group_name)
        .bind(tags_json)
        .bind(&asset.description)
        .bind(if asset.enabled { 1i64 } else { 0i64 })
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to upsert server: {e}")))?;

        Ok(())
    }

    pub async fn delete_server(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM servers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to delete server: {e}")))?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn list_rules(&self) -> Result<Vec<SecurityRule>> {
        let rows = sqlx::query(
            "SELECT id, name, match_kind, pattern, decision, description, enabled, priority FROM security_rules ORDER BY priority DESC, name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to list rules: {e}")))?;

        rows.into_iter().map(row_to_rule).collect()
    }

    pub async fn upsert_rule(&self, rule: &SecurityRule) -> Result<()> {
        let match_kind_str = serde_json::to_string(&rule.match_kind)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        let decision_str = serde_json::to_string(&rule.decision)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO security_rules (id, name, match_kind, pattern, decision, description, enabled, priority)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                match_kind = excluded.match_kind,
                pattern = excluded.pattern,
                decision = excluded.decision,
                description = excluded.description,
                enabled = excluded.enabled,
                priority = excluded.priority
            "#,
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(match_kind_str.trim_matches('"'))
        .bind(&rule.pattern)
        .bind(decision_str.trim_matches('"'))
        .bind(&rule.description)
        .bind(if rule.enabled { 1i64 } else { 0i64 })
        .bind(rule.priority as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to upsert rule: {e}")))?;

        Ok(())
    }

    pub async fn delete_rule(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM security_rules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to delete rule: {e}")))?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_l2_config(&self) -> Result<L2ProviderConfig> {
        let row = sqlx::query(
            "SELECT id, name, protocol, base_url, model, api_key_credential_id, timeout_ms, risk_threshold_ask, risk_threshold_deny, enabled FROM l2_configs LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to load L2 config: {e}")))?;

        match row {
            Some(r) => row_to_l2_config(r),
            None => Ok(L2ProviderConfig::default()),
        }
    }

    pub async fn upsert_l2_config(&self, cfg: &L2ProviderConfig) -> Result<()> {
        let protocol_str = serde_json::to_string(&cfg.protocol)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO l2_configs (id, name, protocol, base_url, model, api_key_credential_id, timeout_ms, risk_threshold_ask, risk_threshold_deny, enabled)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                protocol = excluded.protocol,
                base_url = excluded.base_url,
                model = excluded.model,
                api_key_credential_id = excluded.api_key_credential_id,
                timeout_ms = excluded.timeout_ms,
                risk_threshold_ask = excluded.risk_threshold_ask,
                risk_threshold_deny = excluded.risk_threshold_deny,
                enabled = excluded.enabled
            "#,
        )
        .bind(&cfg.id)
        .bind(&cfg.name)
        .bind(protocol_str.trim_matches('"'))
        .bind(&cfg.base_url)
        .bind(&cfg.model)
        .bind(&cfg.api_key_credential_id)
        .bind(cfg.timeout_ms as i64)
        .bind(cfg.risk_threshold_ask as i64)
        .bind(cfg.risk_threshold_deny as i64)
        .bind(if cfg.enabled { 1i64 } else { 0i64 })
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to save L2 config: {e}")))?;

        Ok(())
    }

    pub async fn list_snippets(&self) -> Result<Vec<CommandSnippet>> {
        let rows = sqlx::query("SELECT id, name, command, description, tags_json FROM snippets ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to list snippets: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let tags_json: String = r.get("tags_json");
                CommandSnippet {
                    id: r.get("id"),
                    name: r.get("name"),
                    command: r.get("command"),
                    description: r.get("description"),
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                }
            })
            .collect())
    }

    pub async fn upsert_snippet(&self, snippet: &CommandSnippet) -> Result<()> {
        let tags_json = serde_json::to_string(&snippet.tags)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO snippets (id, name, command, description, tags_json)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                command = excluded.command,
                description = excluded.description,
                tags_json = excluded.tags_json
            "#,
        )
        .bind(&snippet.id)
        .bind(&snippet.name)
        .bind(&snippet.command)
        .bind(&snippet.description)
        .bind(tags_json)
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to upsert snippet: {e}")))?;
        Ok(())
    }

    pub async fn delete_snippet(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM snippets WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| SshieldError::Storage(format!("Failed to delete snippet: {e}")))?;
        Ok(res.rows_affected() > 0)
    }

    /// Append a tamper-evident audit event chained to the previous `event_hash` via SHA-256.
    pub async fn append_audit(&self, event: &AuditEvent) -> Result<AuditEvent> {
        let mut chained = event.clone();
        if chained.trace_id.is_empty() {
            chained.trace_id = chained.id.clone();
        }

        let prev_hash: String = sqlx::query_scalar(
            "SELECT event_hash FROM audit_logs ORDER BY seq DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to fetch prev_hash: {e}")))?
        .unwrap_or_else(|| "GENESIS".to_string());

        chained.prev_hash = prev_hash;
        chained.event_hash = compute_event_hash(&chained);

        let decision_str = serde_json::to_string(&chained.decision)
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        let approval_str = chained
            .approval_status
            .map(|s| serde_json::to_string(&s))
            .transpose()
            .map_err(|e| SshieldError::Storage(e.to_string()))?
            .map(|s| s.trim_matches('"').to_string());
        let l1_json = chained
            .l1_verdict
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| SshieldError::Storage(e.to_string()))?;
        let l2_json = chained
            .l2_verdict
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| SshieldError::Storage(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                id, trace_id, actor, timestamp, server_id, server_name, command, intent, dry_run,
                decision, l1_verdict_json, l2_verdict_json, approval_id, approval_status,
                exit_code, duration_ms, stdout_preview, stderr_preview, error, prev_hash, event_hash
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&chained.id)
        .bind(&chained.trace_id)
        .bind(&chained.actor)
        .bind(chained.timestamp.to_rfc3339())
        .bind(&chained.server_id)
        .bind(&chained.server_name)
        .bind(&chained.command)
        .bind(&chained.intent)
        .bind(if chained.dry_run { 1i64 } else { 0i64 })
        .bind(decision_str.trim_matches('"'))
        .bind(l1_json)
        .bind(l2_json)
        .bind(&chained.approval_id)
        .bind(approval_str)
        .bind(chained.exit_code.map(|c| c as i64))
        .bind(chained.duration_ms.map(|d| d as i64))
        .bind(&chained.stdout_preview)
        .bind(&chained.stderr_preview)
        .bind(&chained.error)
        .bind(&chained.prev_hash)
        .bind(&chained.event_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to insert audit event: {e}")))?;

        if let Some(ref jsonl_path) = self.audit_jsonl_path {
            if let Some(parent) = jsonl_path.parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Ok(line) = serde_json::to_string(&chained)
                && let Ok(mut file) = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(jsonl_path)
                    .await
            {
                let _ = file.write_all(format!("{line}\n").as_bytes()).await;
            }
        }

        Ok(chained)
    }

    pub async fn list_audits(&self, limit: i64, server_id: Option<&str>) -> Result<Vec<AuditEvent>> {
        let rows = if let Some(sid) = server_id {
            sqlx::query(
                "SELECT id, trace_id, actor, timestamp, server_id, server_name, command, intent, dry_run, decision, l1_verdict_json, l2_verdict_json, approval_id, approval_status, exit_code, duration_ms, stdout_preview, stderr_preview, error, prev_hash, event_hash FROM audit_logs WHERE server_id = ? ORDER BY seq DESC LIMIT ?",
            )
            .bind(sid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, trace_id, actor, timestamp, server_id, server_name, command, intent, dry_run, decision, l1_verdict_json, l2_verdict_json, approval_id, approval_status, exit_code, duration_ms, stdout_preview, stderr_preview, error, prev_hash, event_hash FROM audit_logs ORDER BY seq DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| SshieldError::Storage(format!("Failed to list audit logs: {e}")))?;

        rows.into_iter().map(row_to_audit).collect()
    }

    /// Verify the SHA-256 hash chain across all persisted audit records.
    pub async fn verify_audit_chain(&self) -> Result<AuditChainStatus> {
        let rows = sqlx::query(
            "SELECT id, trace_id, actor, timestamp, server_id, server_name, command, intent, dry_run, decision, l1_verdict_json, l2_verdict_json, approval_id, approval_status, exit_code, duration_ms, stdout_preview, stderr_preview, error, prev_hash, event_hash FROM audit_logs ORDER BY seq ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| SshieldError::Storage(format!("Failed to scan audit chain: {e}")))?;

        let events: Vec<AuditEvent> = rows.into_iter().map(row_to_audit).collect::<Result<_>>()?;
        let mut expected_prev = "GENESIS".to_string();

        for ev in &events {
            if ev.prev_hash != expected_prev || ev.event_hash != compute_event_hash(ev) {
                return Ok(AuditChainStatus {
                    total_events: events.len(),
                    valid: false,
                    broken_at_id: Some(ev.id.clone()),
                });
            }
            expected_prev = ev.event_hash.clone();
        }

        Ok(AuditChainStatus {
            total_events: events.len(),
            valid: true,
            broken_at_id: None,
        })
    }
}

fn compute_event_hash(ev: &AuditEvent) -> String {
    let mut hasher = Sha256::new();
    let payload = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}",
        ev.prev_hash,
        ev.id,
        ev.trace_id,
        ev.actor,
        ev.timestamp.to_rfc3339(),
        ev.server_id,
        ev.command,
        ev.intent,
        ev.decision,
        ev.exit_code
    );
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

fn row_to_server(row: sqlx::sqlite::SqliteRow) -> Result<ServerAsset> {
    let auth_raw: String = row.get("auth_kind");
    let auth_kind: AuthKind = serde_json::from_str(&format!("\"{auth_raw}\""))
        .unwrap_or(AuthKind::AgentOrDefaultKey);
    let tags_json: String = row.get("tags_json");
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let port: i64 = row.get("port");
    let enabled: i64 = row.get("enabled");

    Ok(ServerAsset {
        id: row.get("id"),
        name: row.get("name"),
        host: row.get("host"),
        port: port as u16,
        username: row.get("username"),
        auth_kind,
        identity_file: row.get("identity_file"),
        credential_id: row.get("credential_id"),
        jump_host_id: row.get("jump_host_id"),
        group_name: row.get("group_name"),
        tags,
        description: row.get("description"),
        enabled: enabled != 0,
    })
}

fn row_to_rule(row: sqlx::sqlite::SqliteRow) -> Result<SecurityRule> {
    let mk_raw: String = row.get("match_kind");
    let match_kind: RuleMatchKind = serde_json::from_str(&format!("\"{mk_raw}\""))
        .map_err(|e| SshieldError::Storage(format!("Invalid match_kind `{mk_raw}`: {e}")))?;
    let dec_raw: String = row.get("decision");
    let decision: PolicyDecision = serde_json::from_str(&format!("\"{dec_raw}\""))
        .map_err(|e| SshieldError::Storage(format!("Invalid decision `{dec_raw}`: {e}")))?;
    let enabled: i64 = row.get("enabled");
    let priority: i64 = row.get("priority");

    Ok(SecurityRule {
        id: row.get("id"),
        name: row.get("name"),
        match_kind,
        pattern: row.get("pattern"),
        decision,
        description: row.get("description"),
        enabled: enabled != 0,
        priority: priority as i32,
    })
}

fn row_to_l2_config(row: sqlx::sqlite::SqliteRow) -> Result<L2ProviderConfig> {
    let proto_raw: String = row.get("protocol");
    let protocol: L2Protocol = serde_json::from_str(&format!("\"{proto_raw}\""))
        .unwrap_or(L2Protocol::SystemOneFast);
    let timeout_ms: i64 = row.get("timeout_ms");
    let risk_ask: i64 = row.get("risk_threshold_ask");
    let risk_deny: i64 = row.get("risk_threshold_deny");
    let enabled: i64 = row.get("enabled");

    Ok(L2ProviderConfig {
        id: row.get("id"),
        name: row.get("name"),
        protocol,
        base_url: row.get("base_url"),
        model: row.get("model"),
        api_key_credential_id: row.get("api_key_credential_id"),
        timeout_ms: timeout_ms as u64,
        risk_threshold_ask: risk_ask as u8,
        risk_threshold_deny: risk_deny as u8,
        enabled: enabled != 0,
    })
}

fn row_to_audit(row: sqlx::sqlite::SqliteRow) -> Result<AuditEvent> {
    let ts_raw: String = row.get("timestamp");
    let timestamp = DateTime::parse_from_rfc3339(&ts_raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let dec_raw: String = row.get("decision");
    let decision: PolicyDecision = serde_json::from_str(&format!("\"{dec_raw}\""))
        .unwrap_or(PolicyDecision::Deny);
    let app_raw: Option<String> = row.get("approval_status");
    let approval_status = app_raw
        .and_then(|s| serde_json::from_str::<ApprovalDecision>(&format!("\"{s}\"")).ok());
    let l1_raw: Option<String> = row.get("l1_verdict_json");
    let l1_verdict = l1_raw.and_then(|s| serde_json::from_str::<CommandVerdict>(&s).ok());
    let l2_raw: Option<String> = row.get("l2_verdict_json");
    let l2_verdict = l2_raw.and_then(|s| serde_json::from_str::<SemanticReviewVerdict>(&s).ok());

    let dry_run: i64 = row.get("dry_run");
    let exit_code: Option<i64> = row.get("exit_code");
    let duration_ms: Option<i64> = row.get("duration_ms");

    Ok(AuditEvent {
        id: row.get("id"),
        trace_id: row.get("trace_id"),
        actor: row.get("actor"),
        timestamp,
        server_id: row.get("server_id"),
        server_name: row.get("server_name"),
        command: row.get("command"),
        intent: row.get("intent"),
        dry_run: dry_run != 0,
        decision,
        l1_verdict,
        l2_verdict,
        approval_id: row.get("approval_id"),
        approval_status,
        exit_code: exit_code.map(|c| c as i32),
        duration_ms: duration_ms.map(|d| d as u64),
        stdout_preview: row.get("stdout_preview"),
        stderr_preview: row.get("stderr_preview"),
        error: row.get("error"),
        prev_hash: row.get("prev_hash"),
        event_hash: row.get("event_hash"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn crud_and_tamper_evident_audit_chain() {
        let storage = Storage::connect("sqlite::memory:", None).await.unwrap();

        let asset = ServerAsset {
            id: "srv-1".into(),
            name: "prod-web".into(),
            host: "10.0.0.10".into(),
            port: 22,
            username: "ubuntu".into(),
            auth_kind: AuthKind::AgentOrDefaultKey,
            identity_file: None,
            credential_id: None,
            jump_host_id: None,
            group_name: Some("Production".into()),
            tags: vec!["prod".into()],
            description: Some("Web server".into()),
            enabled: true,
        };
        storage.upsert_server(&asset).await.unwrap();
        assert_eq!(storage.list_servers().await.unwrap().len(), 1);

        let ev1 = AuditEvent {
            id: "aud-1".into(),
            trace_id: "tr-1".into(),
            actor: "mcp-stdio".into(),
            timestamp: Utc::now(),
            server_id: "srv-1".into(),
            server_name: "prod-web".into(),
            command: "uptime".into(),
            intent: "Check load".into(),
            dry_run: false,
            decision: PolicyDecision::Allow,
            l1_verdict: None,
            l2_verdict: None,
            approval_id: None,
            approval_status: None,
            exit_code: Some(0),
            duration_ms: Some(42),
            stdout_preview: Some("up 10 days".into()),
            stderr_preview: None,
            error: None,
            prev_hash: String::new(),
            event_hash: String::new(),
        };
        let saved1 = storage.append_audit(&ev1).await.unwrap();
        assert_eq!(saved1.prev_hash, "GENESIS");

        let mut ev2 = ev1.clone();
        ev2.id = "aud-2".into();
        ev2.trace_id = "tr-2".into();
        let saved2 = storage.append_audit(&ev2).await.unwrap();
        assert_eq!(saved2.prev_hash, saved1.event_hash);

        let status = storage.verify_audit_chain().await.unwrap();
        assert!(status.valid);
        assert_eq!(status.total_events, 2);

        // Tamper with ev1 command in DB -> chain verification must fail!
        sqlx::query("UPDATE audit_logs SET command = 'rm -rf /' WHERE id = 'aud-1'")
            .execute(&storage.pool)
            .await
            .unwrap();
        let tampered = storage.verify_audit_chain().await.unwrap();
        assert!(!tampered.valid);
        assert_eq!(tampered.broken_at_id.as_deref(), Some("aud-1"));
    }
}
