use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROTOCOL: u32 = 1;
pub const PROMPT_LIMIT: u32 = 10;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("WEB_RESEARCH_HOME") {
        return Ok(PathBuf::from(path));
    }
    let dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Cannot locate user data directory"))?;
    Ok(dirs.data_local_dir().join("web-research-opencode"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub model: String,
    pub reasoning_preferences: Vec<String>,
    pub words_per_minute: u32,
    pub fixed_pause_seconds: u64,
    pub inactivity_hours: u64,
    pub search_timeout_seconds: u64,
    pub deep_research_timeout_seconds: u64,
    pub chrome_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chatgpt_project_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "default".into(),
            reasoning_preferences: vec!["Extra High".into(), "High".into()],
            words_per_minute: 40,
            fixed_pause_seconds: 15,
            inactivity_hours: 24,
            search_timeout_seconds: 900,
            deep_research_timeout_seconds: 3600,
            chrome_path: None,
            chatgpt_project_url: None,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if let Some(value) = &self.chatgpt_project_url {
            ensure!(
                valid_project_url(value),
                "chatgpt_project_url must be an HTTPS ChatGPT /g/<id>/project URL without query or fragment"
            );
        }
        ensure!(
            (1..=300).contains(&self.words_per_minute),
            "words_per_minute must be 1..300"
        );
        ensure!(
            self.fixed_pause_seconds <= 3600,
            "fixed_pause_seconds exceeds 3600"
        );
        ensure!(
            (1..=8760).contains(&self.inactivity_hours),
            "inactivity_hours must be 1..8760"
        );
        ensure!(!self.model.trim().is_empty(), "model is empty");
        ensure!(
            !self.reasoning_preferences.is_empty(),
            "reasoning_preferences is empty"
        );
        ensure!(
            (60..=86400).contains(&self.search_timeout_seconds),
            "Invalid Search timeout"
        );
        ensure!(
            (60..=86400).contains(&self.deep_research_timeout_seconds),
            "Invalid Deep Research timeout"
        );
        Ok(())
    }

    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let path = dir.join("config.json");
        let config = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    pub fn composition_seconds(&self, text: &str) -> u64 {
        (text.split_whitespace().count() as u64 * 60).div_ceil(self.words_per_minute as u64)
            + self.fixed_pause_seconds
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submit {
    pub project: String,
    pub session: String,
    pub key: String,
    pub thread_id: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub deep_research: bool,
}

impl Submit {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.prompt.trim().is_empty(), "Prompt is empty");
        ensure!(self.prompt.len() <= 32_000, "Prompt exceeds 32000 bytes");
        ensure!(
            !self.project.is_empty() && self.project.len() <= 4096,
            "Invalid project"
        );
        ensure!(
            !self.session.is_empty() && self.session.len() <= 256,
            "Invalid session"
        );
        ensure!(
            !self.key.is_empty() && self.key.len() <= 256,
            "Invalid idempotency key"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Thread {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_project_url: Option<String>,
    pub id: String,
    pub project: String,
    pub session: String,
    pub title: String,
    pub deep_research: bool,
    pub url: Option<String>,
    pub target: Option<String>,
    pub created_at: i64,
    pub active_at: i64,
    pub prompts: u32,
    pub state: String,
    pub cleanup_error: Option<String>,
    #[serde(default, skip_serializing_if = "zero_u32")]
    pub cleanup_attempts: u32,
    #[serde(default, skip_serializing_if = "zero_i64")]
    pub cleanup_retry_at: i64,
}

fn zero_u32(value: &u32) -> bool {
    *value == 0
}

pub fn valid_project_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        let parts: Vec<_> = u.path().split('/').collect();
        u.scheme() == "https"
            && u.host_str() == Some("chatgpt.com")
            && u.port().is_none()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && parts.len() == 4
            && parts[1] == "g"
            && parts[2].starts_with("g-p-")
            && parts[2].len() > 4
            && parts[3] == "project"
    })
}

#[test]
fn project_urls_require_exact_origin_and_project_route() {
    assert!(valid_project_url(
        "https://chatgpt.com/g/g-p-example/project"
    ));
    for bad in [
        "https://chatgpt.com/",
        "https://evil.org/g/g-p-example/project",
        "https://chatgpt.com/g/g-p-example/project?x=1",
        "https://chatgpt.com/g/g-example/project",
    ] {
        assert!(!valid_project_url(bad));
    }
}
fn zero_i64(value: &i64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub thread_id: String,
    pub key: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub new_thread: bool,
    pub prompt: String,
    pub config: Config,
    pub state: String,
    pub created_at: i64,
    pub send_after: Option<i64>,
    pub submitted_at: Option<i64>,
    pub baseline: Option<usize>,
    #[serde(default)]
    pub selection: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl Job {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "completed" | "failed" | "cancelled")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceDescriptor {
    pub protocol: u32,
    pub port: u16,
    pub token: String,
    pub instance: String,
    pub pid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pacing_counts_unicode_whitespace_and_rounds_up() {
        let c = Config::default();
        assert_eq!(c.composition_seconds(&vec!["word"; 80].join(" ")), 135);
        assert_eq!(c.composition_seconds("hi\u{2003}there\nfriend"), 20);
        assert!(
            Config {
                words_per_minute: 0,
                ..c
            }
            .validate()
            .is_err()
        );
    }
}
