//! L2 AI Semantic Review Strategy Engine.
//!
//! Provides a unified `SemanticReviewer` interface supporting:
//! - Standard LLM protocols (`OpenAiChat`, `OpenAiResponses`, `AnthropicMessages`)
//! - Ultra-fast dedicated decision model (`SystemOneFast`)
//!
//! Strictly follows Fail-Closed: any network error, timeout, or unparseable model
//! response escalates to `PolicyDecision::AskApproval` with full trace metadata.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use sshield_core::{
    L2Protocol, L2ProviderConfig, PolicyDecision, Result, SemanticReviewRequest,
    SemanticReviewVerdict, SemanticReviewer,
};
use sshield_vault::SealedVault;
use zeroize::Zeroizing;

const SYSTEM_PROMPT: &str = r#"You are Sshield L2 Security Reviewer.
Evaluate whether the shell command matches the stated AI intent and assess operational risk.
Respond strictly in JSON with keys:
- "decision": "allow" | "ask_approval" | "deny"
- "risk_score": integer 0..100
- "intent_aligned": boolean
- "reasoning": concise explanation"#;

#[derive(Debug, Deserialize)]
struct ModelJsonVerdict {
    decision: Option<String>,
    risk_score: Option<u8>,
    intent_aligned: Option<bool>,
    reasoning: Option<String>,
}

/// Unified L2 reviewer dispatching to LLM protocols or SystemOne fast-decision endpoints.
pub struct UnifiedL2Reviewer {
    http: Client,
    vault: Arc<SealedVault>,
    config: L2ProviderConfig,
}

impl UnifiedL2Reviewer {
    pub fn new(vault: Arc<SealedVault>, config: L2ProviderConfig) -> Self {
        Self {
            http: Client::new(),
            vault,
            config,
        }
    }

    async fn resolve_api_key(&self) -> Option<Zeroizing<String>> {
        let cred_id = self.config.api_key_credential_id.as_deref()?;
        if cred_id.trim().is_empty() {
            return None;
        }
        self.vault.get_secret(cred_id).await.ok()
    }

    async fn call_provider(&self, req: &SemanticReviewRequest) -> std::result::Result<ModelJsonVerdict, String> {
        let timeout = Duration::from_millis(self.config.timeout_ms.max(200));
        let api_key = self.resolve_api_key().await;
        let user_prompt = build_user_prompt(req);

        match self.config.protocol {
            L2Protocol::OpenAiChat => {
                let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": self.config.model,
                    "temperature": 0.0,
                    "response_format": { "type": "json_object" },
                    "messages": [
                        { "role": "system", "content": SYSTEM_PROMPT },
                        { "role": "user", "content": user_prompt }
                    ]
                });
                let mut builder = self.http.post(&url).timeout(timeout).json(&body);
                if let Some(ref key) = api_key {
                    builder = builder.bearer_auth(&**key);
                }
                let resp = builder.send().await.map_err(|e| format!("HTTP error: {e}"))?;
                let status = resp.status();
                let val: serde_json::Value = resp.json().await.map_err(|e| format!("JSON decode: {e}"))?;
                if !status.is_success() {
                    return Err(format!("OpenAiChat HTTP {status}: {val}"));
                }
                let text = val["choices"][0]["message"]["content"]
                    .as_str()
                    .ok_or_else(|| "Missing choices[0].message.content".to_string())?;
                parse_model_json(text)
            }
            L2Protocol::OpenAiResponses => {
                let url = format!("{}/responses", self.config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": self.config.model,
                    "instructions": SYSTEM_PROMPT,
                    "input": user_prompt,
                    "text": { "format": { "type": "json_object" } }
                });
                let mut builder = self.http.post(&url).timeout(timeout).json(&body);
                if let Some(ref key) = api_key {
                    builder = builder.bearer_auth(&**key);
                }
                let resp = builder.send().await.map_err(|e| format!("HTTP error: {e}"))?;
                let status = resp.status();
                let val: serde_json::Value = resp.json().await.map_err(|e| format!("JSON decode: {e}"))?;
                if !status.is_success() {
                    return Err(format!("OpenAiResponses HTTP {status}: {val}"));
                }
                let text = val["output_text"]
                    .as_str()
                    .or_else(|| val["output"][0]["content"][0]["text"].as_str())
                    .ok_or_else(|| "Missing output_text in Responses payload".to_string())?;
                parse_model_json(text)
            }
            L2Protocol::AnthropicMessages => {
                let url = format!("{}/messages", self.config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": self.config.model,
                    "max_tokens": 512,
                    "temperature": 0.0,
                    "system": SYSTEM_PROMPT,
                    "messages": [
                        { "role": "user", "content": user_prompt }
                    ]
                });
                let mut builder = self
                    .http
                    .post(&url)
                    .timeout(timeout)
                    .header("anthropic-version", "2023-06-01")
                    .json(&body);
                if let Some(ref key) = api_key {
                    builder = builder.header("x-api-key", &**key);
                }
                let resp = builder.send().await.map_err(|e| format!("HTTP error: {e}"))?;
                let status = resp.status();
                let val: serde_json::Value = resp.json().await.map_err(|e| format!("JSON decode: {e}"))?;
                if !status.is_success() {
                    return Err(format!("AnthropicMessages HTTP {status}: {val}"));
                }
                let text = val["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| "Missing content[0].text in Anthropic payload".to_string())?;
                parse_model_json(text)
            }
            L2Protocol::SystemOneFast => {
                // Dedicated low-latency SystemOne decision endpoint
                let body = serde_json::json!({
                    "model": self.config.model,
                    "server_name": req.server_name,
                    "server_tags": req.server_tags,
                    "command": req.command,
                    "intent": req.intent,
                    "l1_decision": req.l1_verdict.decision,
                    "ast_programs": req.l1_verdict.parsed_commands.iter().map(|c| &c.program).collect::<Vec<_>>(),
                });
                let mut builder = self.http.post(&self.config.base_url).timeout(timeout).json(&body);
                if let Some(ref key) = api_key {
                    builder = builder.bearer_auth(&**key);
                }
                let resp = builder.send().await.map_err(|e| format!("SystemOne HTTP error: {e}"))?;
                let status = resp.status();
                let val: serde_json::Value = resp.json().await.map_err(|e| format!("JSON decode: {e}"))?;
                if !status.is_success() {
                    return Err(format!("SystemOne HTTP {status}: {val}"));
                }
                serde_json::from_value::<ModelJsonVerdict>(val)
                    .map_err(|e| format!("Invalid SystemOne verdict JSON: {e}"))
            }
        }
    }

    fn apply_thresholds(&self, raw: ModelJsonVerdict, latency_ms: u64) -> SemanticReviewVerdict {
        let risk_score = raw.risk_score.unwrap_or(50).min(100);
        let intent_aligned = raw.intent_aligned.unwrap_or(false);
        let parsed_decision = match raw.decision.as_deref() {
            Some("allow") => PolicyDecision::Allow,
            Some("deny") => PolicyDecision::Deny,
            _ => PolicyDecision::AskApproval,
        };

        let mut final_decision = parsed_decision;
        if risk_score >= self.config.risk_threshold_deny {
            final_decision = PolicyDecision::Deny;
        } else if (!intent_aligned || risk_score >= self.config.risk_threshold_ask)
            && final_decision == PolicyDecision::Allow
        {
            final_decision = PolicyDecision::AskApproval;
        }

        SemanticReviewVerdict {
            decision: final_decision,
            risk_score,
            intent_aligned,
            reasoning: raw
                .reasoning
                .unwrap_or_else(|| "Evaluated by L2 semantic reviewer".to_string()),
            provider_name: self.config.name.clone(),
            protocol: self.config.protocol,
            model: self.config.model.clone(),
            latency_ms,
            fallback_triggered: false,
        }
    }
}

#[async_trait]
impl SemanticReviewer for UnifiedL2Reviewer {
    async fn review(&self, req: &SemanticReviewRequest) -> Result<SemanticReviewVerdict> {
        let started = Instant::now();
        match self.call_provider(req).await {
            Ok(parsed) => Ok(self.apply_thresholds(parsed, started.elapsed().as_millis() as u64)),
            Err(err) => {
                tracing::warn!(
                    provider = %self.config.name,
                    protocol = ?self.config.protocol,
                    error = %err,
                    "L2 semantic review failed; failing closed to AskApproval"
                );
                Ok(SemanticReviewVerdict {
                    decision: PolicyDecision::AskApproval,
                    risk_score: 75,
                    intent_aligned: false,
                    reasoning: format!("L2 fail-closed fallback triggered: {err}"),
                    provider_name: self.config.name.clone(),
                    protocol: self.config.protocol,
                    model: self.config.model.clone(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    fallback_triggered: true,
                })
            }
        }
    }
}

fn build_user_prompt(req: &SemanticReviewRequest) -> String {
    let ast_summary: Vec<String> = req
        .l1_verdict
        .parsed_commands
        .iter()
        .map(|c| format!("{} {} [{}]", c.program, c.args.join(" "), c.context))
        .collect();

    format!(
        "Target Server: {} (tags: {:?})\nStated Intent: {}\nCommand: {}\nL1 AST Decision: {}\nParsed Subcommands: {:?}",
        req.server_name, req.server_tags, req.intent, req.command, req.l1_verdict.decision, ast_summary
    )
}

fn parse_model_json(text: &str) -> std::result::Result<ModelJsonVerdict, String> {
    let trimmed = text.trim();
    let json_slice = if let Some(start) = trimmed.find('{')
        && let Some(end) = trimmed.rfind('}')
    {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    serde_json::from_str::<ModelJsonVerdict>(json_slice)
        .map_err(|e| format!("Failed to parse L2 JSON `{trimmed}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshield_core::CommandVerdict;

    #[test]
    fn thresholds_escalate_misaligned_intent_or_high_risk() {
        let vault = Arc::new(SealedVault::new("/tmp/unused-l2-test.enc", 600));
        let cfg = L2ProviderConfig {
            enabled: true,
            risk_threshold_ask: 40,
            risk_threshold_deny: 85,
            ..Default::default()
        };
        let reviewer = UnifiedL2Reviewer::new(vault, cfg);

        // High risk escalates to Deny even if model suggested allow
        let v1 = reviewer.apply_thresholds(
            ModelJsonVerdict {
                decision: Some("allow".into()),
                risk_score: Some(90),
                intent_aligned: Some(true),
                reasoning: Some("Risky".into()),
            },
            12,
        );
        assert_eq!(v1.decision, PolicyDecision::Deny);

        // Misaligned intent escalates Allow -> AskApproval
        let v2 = reviewer.apply_thresholds(
            ModelJsonVerdict {
                decision: Some("allow".into()),
                risk_score: Some(15),
                intent_aligned: Some(false),
                reasoning: Some("Mismatch".into()),
            },
            10,
        );
        assert_eq!(v2.decision, PolicyDecision::AskApproval);
    }

    #[tokio::test]
    async fn fail_closed_when_provider_unreachable() {
        let vault = Arc::new(SealedVault::new("/tmp/unused-l2-test2.enc", 600));
        let cfg = L2ProviderConfig {
            enabled: true,
            protocol: L2Protocol::SystemOneFast,
            base_url: "http://127.0.0.1:59999/unreachable".into(),
            timeout_ms: 250,
            ..Default::default()
        };
        let reviewer = UnifiedL2Reviewer::new(vault, cfg);
        let req = SemanticReviewRequest {
            server_name: "prod-db".into(),
            server_tags: vec!["prod".into()],
            command: "ls -la".into(),
            intent: "list files".into(),
            l1_verdict: CommandVerdict {
                decision: PolicyDecision::Allow,
                parse_ok: true,
                parsed_commands: vec![],
                matched_rules: vec![],
                reason: "ok".into(),
            },
        };
        let verdict = reviewer.review(&req).await.unwrap();
        assert!(verdict.fallback_triggered);
        assert_eq!(verdict.decision, PolicyDecision::AskApproval);
    }
}
