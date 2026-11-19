//! Core domain models, policy interfaces, and audit types for Sshield.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerAsset {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub jump_host_id: Option<String>,
    pub tags: Vec<String>,
}
