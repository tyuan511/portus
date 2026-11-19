//! Output masking (Secret Redaction) to prevent `.env`, AKSK, PEM keys,
//! and tokens from leaking to external LLMs or audit previews.

use std::sync::LazyLock;

use regex::Regex;

static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----").unwrap(),
            "[REDACTED_PRIVATE_KEY]",
        ),
        (
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
            "[REDACTED_AWS_ACCESS_KEY]",
        ),
        (
            Regex::new(r"\bsk-(?:proj-|ant-|live-|test-)?[A-Za-z0-9_-]{20,}\b").unwrap(),
            "[REDACTED_API_KEY]",
        ),
        (
            Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{16,}").unwrap(),
            "${1}[REDACTED_TOKEN]",
        ),
        (
            Regex::new(r"(?im)^([A-Z0-9_]*(?:PASSWORD|PASSWD|SECRET|TOKEN|API_KEY|ACCESS_KEY|PRIVATE_KEY)[A-Z0-9_]*\s*=\s*)([^\s#]+)").unwrap(),
            "${1}[REDACTED]",
        ),
        (
            Regex::new(r"(?i)((?:postgres|postgresql|mysql|mongodb|redis)://[^:\s/]+:)([^@\s]+)(@)").unwrap(),
            "${1}[REDACTED]${3}",
        ),
    ]
});

/// Redact sensitive secrets from command stdout/stderr before returning to AI agents or audit logs.
pub fn mask_sensitive_output(input: &str) -> String {
    let mut out = input.to_string();
    for (re, replacement) in PATTERNS.iter() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_env_keys_and_connection_strings() {
        let raw = "DB_PASSWORD=supersecret123\nAWS_KEY=AKIAIOSFODNN7EXAMPLE\ntoken: sk-proj-abcdefghij1234567890\nDATABASE_URL=postgres://admin:s3cr3t@10.0.0.5:5432/prod";
        let masked = mask_sensitive_output(raw);
        assert!(!masked.contains("supersecret123"));
        assert!(!masked.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!masked.contains("sk-proj-abcdefghij1234567890"));
        assert!(!masked.contains("s3cr3t"));
        assert!(masked.contains("[REDACTED]"));
        assert!(masked.contains("[REDACTED_AWS_ACCESS_KEY]"));
        assert!(masked.contains("[REDACTED_API_KEY]"));
    }
}
