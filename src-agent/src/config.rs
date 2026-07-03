//! Configuration file system for Rupoo.
//!
//! Manages `RUPOO_HOME/config.toml` (user preferences) and
//! `RUPOO_HOME/credentials.toml` (API keys, chmod 600).
//!
//! Migrated from DB-only settings to file-based config for:
//! - Portability (copy `RUPOO_HOME/` to another machine)
//! - Version control (config.toml can be tracked)
//! - Security (credentials.toml is separate, 0600)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// Config directory
// ---------------------------------------------------------------------------

/// Return the Rupoo data directory.
///
/// Priority:
/// 1. `$RUPOO_HOME` environment variable (if set)
/// 2. `~/.rupoo` (default)
pub fn rupoo_home() -> PathBuf {
    if let Ok(home) = std::env::var("RUPOO_HOME") {
        return PathBuf::from(home);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".rupoo")
}

// ---------------------------------------------------------------------------
// Top-level config schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RupooConfig {
    #[serde(default)]
    pub llm: LlmSection,
    #[serde(default)]
    pub safety: SafetySection,
    #[serde(default)]
    pub shell: ShellSection,
    #[serde(default)]
    pub memory: MemorySection,
    #[serde(default)]
    pub mcp: McpSection,
    #[serde(default)]
    pub confidence: ConfidenceConfig,
    /// Channel integrations (Feishu, DingTalk, WeCom, etc.)
    #[serde(default)]
    pub channel: ChannelSection,
    /// Agent identity profiles keyed by role name.
    /// e.g. `[agents.feishu]` / `[agents.cli]`
    #[serde(default)]
    pub agents: HashMap<String, AgentProfile>,
}

/// Agent identity profile — defines system prompt + tool scope per role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// System prompt that defines this agent's identity.
    /// When empty, the default compiled-in prompt is used.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Human-readable label (e.g. "终端助手", "飞书助手").
    #[serde(default)]
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Channel section
// ---------------------------------------------------------------------------

/// Channel (IM bot) integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelSection {
    /// Feishu / Lark bot configuration.
    #[serde(default)]
    pub feishu: Option<FeishuConfig>,
    /// DingTalk (钉钉) bot configuration.
    #[serde(default)]
    pub dingtalk: Option<DingTalkConfig>,
    /// Database path for the agent (default: $RUPOO_HOME/agent.db).
    #[serde(default)]
    pub db_path: Option<String>,
}



/// DingTalk (钉钉) application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkConfig {
    /// DingTalk app Client ID (AppKey).
    pub client_id: String,
    /// DingTalk app Client Secret (AppSecret).
    pub client_secret: String,
}

/// Feishu (飞书) application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    /// Feishu app ID (from Feishu Developer Console).
    pub app_id: String,
    /// Feishu app secret.
    pub app_secret: String,
    /// Whether to only reply when the bot is @mentioned.
    /// In group chats, set to `true` to avoid replying to every message.
    #[serde(default = "default_feishu_mention_only")]
    pub mention_only: bool,
    /// Seconds to wait for user approval via card button before auto-denying.
    #[serde(default = "default_feishu_approval_timeout")]
    pub approval_timeout_secs: u64,
    /// Whether to use the international Lark API (vs Feishu CN).
    #[serde(default)]
    pub lark_mode: bool,
}

fn default_feishu_mention_only() -> bool {
    true
}
fn default_feishu_approval_timeout() -> u64 {
    120
}

// ---------------------------------------------------------------------------
// LLM section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSection {
    /// Active provider name (e.g. "deepseek", "ollama", "openai", "anthropic").
    #[serde(default = "default_active_provider")]
    pub active_provider: String,
    /// Fallback provider when active is unavailable.
    #[serde(default)]
    pub fallback_provider: Option<String>,
    /// Named provider configs.
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

fn default_active_provider() -> String {
    "ollama".to_string()
}

impl Default for LlmSection {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "ollama".into(),
            ProviderConfig {
                base_url: Some("http://localhost:11434".into()),
                model: Some("qwen2.5-coder:7b".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "deepseek".into(),
            ProviderConfig {
                base_url: Some("https://api.deepseek.com".into()),
                model: Some("deepseek-chat".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "qwen".into(),
            ProviderConfig {
                base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".into()),
                model: Some("qwen-max".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "glm".into(),
            ProviderConfig {
                base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
                model: Some("glm-4".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "moonshot".into(),
            ProviderConfig {
                base_url: Some("https://api.moonshot.cn/v1".into()),
                model: Some("moonshot-v1-auto".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "openai".into(),
            ProviderConfig {
                model: Some("gpt-4o".into()),
                ..ProviderConfig::default()
            },
        );
        providers.insert(
            "anthropic".into(),
            ProviderConfig {
                model: Some("claude-sonnet-4-20250514".into()),
                ..ProviderConfig::default()
            },
        );
        Self {
            active_provider: default_active_provider(),
            fallback_provider: Some("ollama".into()),
            providers,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// API key — read from credentials.toml, not stored in config.toml.
    /// This field is for deserialization only; use `resolve_api_key()`.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Base URL override (for proxies or local servers).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Model name.
    #[serde(default)]
    pub model: Option<String>,
    /// Max tokens for this provider.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Temperature for this provider.
    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            model: None,
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
        }
    }
}

fn default_max_tokens() -> u32 {
    2048
}
fn default_temperature() -> f64 {
    0.7
}

// ---------------------------------------------------------------------------
// Safety section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySection {
    #[serde(default = "default_jail_root")]
    pub jail_root: String,
    #[serde(default = "default_approval_policy")]
    pub approval_policy: String,
    #[serde(default = "default_forbidden_commands")]
    pub forbidden_commands: Vec<String>,
    #[serde(default = "default_auto_approve_tools")]
    pub auto_approve_tools: Vec<String>,
}

impl Default for SafetySection {
    fn default() -> Self {
        Self {
            jail_root: default_jail_root(),
            approval_policy: default_approval_policy(),
            forbidden_commands: default_forbidden_commands(),
            auto_approve_tools: default_auto_approve_tools(),
        }
    }
}

fn default_jail_root() -> String {
    ".".into()
}
fn default_approval_policy() -> String {
    "dangerous_only".into()
}
fn default_forbidden_commands() -> Vec<String> {
    vec![]
}
fn default_auto_approve_tools() -> Vec<String> {
    vec![]
}

// ---------------------------------------------------------------------------
// Confidence section
// ---------------------------------------------------------------------------

/// 置信度拦截配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceConfig {
    /// 最低置信阈值（0.0-1.0），低于此值的推理被暂停
    #[serde(default = "default_confidence_threshold")]
    pub min_threshold: f64,
    /// 是否在低置信时暂停（true）或直接放行（false）
    #[serde(default = "default_pause_on_low_confidence")]
    pub pause_on_low_confidence: bool,
}

fn default_confidence_threshold() -> f64 {
    0.7
}
fn default_pause_on_low_confidence() -> bool {
    true
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            min_threshold: default_confidence_threshold(),
            pause_on_low_confidence: default_pause_on_low_confidence(),
        }
    }
}

// ---------------------------------------------------------------------------
// Shell section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSection {
    #[serde(default = "default_timeout")]
    pub default_timeout: u64,
    #[serde(default = "default_max_output_bytes")]
    pub max_output_bytes: usize,
}

impl Default for ShellSection {
    fn default() -> Self {
        Self {
            default_timeout: default_timeout(),
            max_output_bytes: default_max_output_bytes(),
        }
    }
}

fn default_timeout() -> u64 {
    30
}
fn default_max_output_bytes() -> usize {
    10240
}

// ---------------------------------------------------------------------------
// Memory section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySection {
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    #[serde(default = "default_ttl_days")]
    pub ttl_days: u64,
}

impl Default for MemorySection {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            ttl_days: default_ttl_days(),
        }
    }
}

fn default_max_entries() -> usize {
    10000
}
fn default_ttl_days() -> u64 {
    90
}

// ---------------------------------------------------------------------------
// MCP section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpSection {
    /// External MCP servers to connect as client.
    /// Key: server name, Value: command to start the server.
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Command to start the MCP server (e.g. "npx", "python").
    pub command: String,
    /// Arguments for the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Credentials file (separate for security)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialsFile {
    /// API keys per provider: { "deepseek": "sk-xxx", "openai": "sk-yyy" }
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Load / Save
// ---------------------------------------------------------------------------

impl RupooConfig {
    /// Load config from `RUPOO_HOME/config.toml`, creating defaults if missing.
    pub fn load() -> AgentResult<Self> {
        let path = rupoo_home().join("config.toml");
        Self::load_from(&path)
    }

    /// Load config from a specific path.
    pub fn load_from(path: &Path) -> AgentResult<Self> {
        if !path.exists() {
            info!("config not found at {}, using defaults", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| AgentError::Config(format!("read config: {e}")))?;
        let config: RupooConfig = toml::from_str(&content)
            .map_err(|e| AgentError::Config(format!("parse config: {e}")))?;
        Ok(config)
    }

    /// Save config to `RUPOO_HOME/config.toml`.
    pub fn save(&self) -> AgentResult<()> {
        let dir = rupoo_home();
        std::fs::create_dir_all(&dir)
            .map_err(|e| AgentError::Config(format!("create config dir: {e}")))?;
        let path = dir.join("config.toml");
        self.save_to(&path)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &Path) -> AgentResult<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| AgentError::Config(format!("serialize config: {e}")))?;
        std::fs::write(path, content)
            .map_err(|e| AgentError::Config(format!("write config: {e}")))?;
        Ok(())
    }

    /// Resolve the API key for a provider:
    /// 1. From credentials.toml
    /// 2. From environment variable (e.g. DEEPSEEK_API_KEY)
    /// 3. From provider config (legacy)
    /// 4. From DB settings (legacy migration)
    pub async fn resolve_api_key(&self, provider: &str) -> Option<String> {
        // 1. credentials.toml
        if let Ok(creds) = CredentialsFile::load() {
            if let Some(key) = creds.api_keys.get(provider) {
                return Some(key.clone());
            }
        }

        // 2. Environment variable
        let env_var = format!("{}_API_KEY", provider.to_uppercase());
        if let Ok(key) = std::env::var(&env_var) {
            return Some(key);
        }

        // 3. Provider config (legacy field)
        if let Some(pc) = self.llm.providers.get(provider) {
            if let Some(ref key) = pc.api_key {
                return Some(key.clone());
            }
        }

        None
    }

    /// Get the active provider config.
    pub fn active_provider_config(&self) -> Option<(&String, &ProviderConfig)> {
        self.llm.providers.get_key_value(&self.llm.active_provider)
    }

    /// Generate a default config.toml content for new installations.
    pub fn generate_default_toml() -> String {
        let config = Self::default();
        toml::to_string_pretty(&config).unwrap_or_default()
    }
}

impl CredentialsFile {
    /// Load credentials from `RUPOO_HOME/credentials.toml`.
    pub fn load() -> AgentResult<Self> {
        let path = rupoo_home().join("credentials.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| AgentError::Config(format!("read credentials: {e}")))?;

        // Restrict permissions on read
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&path, perms);
        }

        toml::from_str(&content).map_err(|e| AgentError::Config(format!("parse credentials: {e}")))
    }

    /// Save credentials to `RUPOO_HOME/credentials.toml` with 0600 permissions.
    pub fn save(&self) -> AgentResult<()> {
        let dir = rupoo_home();
        std::fs::create_dir_all(&dir)
            .map_err(|e| AgentError::Config(format!("create config dir: {e}")))?;

        let path = dir.join("credentials.toml");
        let content = toml::to_string_pretty(self)
            .map_err(|e| AgentError::Config(format!("serialize credentials: {e}")))?;

        // Write with restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = std::fs::OpenOptions::new();
            let mut file = opts
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| AgentError::Config(format!("create credentials file: {e}")))?;
            use std::io::Write;
            file.write_all(content.as_bytes())
                .map_err(|e| AgentError::Config(format!("write credentials: {e}")))?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&path, content)
                .map_err(|e| AgentError::Config(format!("write credentials: {e}")))?;
        }

        Ok(())
    }

    /// Set an API key for a provider.
    pub fn set_key(&mut self, provider: &str, key: &str) {
        self.api_keys.insert(provider.to_string(), key.to_string());
    }

    /// Get an API key for a provider.
    pub fn get_key(&self, provider: &str) -> Option<&str> {
        self.api_keys.get(provider).map(|s| s.as_str())
    }

    /// Remove an API key for a provider.
    pub fn remove_key(&mut self, provider: &str) -> Option<String> {
        self.api_keys.remove(provider)
    }
}

// ---------------------------------------------------------------------------
// Config init command
// ---------------------------------------------------------------------------

/// Initialize the config directory and generate default files.
pub fn init_config() -> AgentResult<PathBuf> {
    let dir = rupoo_home();
    std::fs::create_dir_all(&dir)
        .map_err(|e| AgentError::Config(format!("create RUPOO_HOME: {e}")))?;
    for sub in &["db", "logs", "skills"] {
        let sub_dir = dir.join(sub);
        if !sub_dir.exists() {
            std::fs::create_dir_all(&sub_dir)
                .map_err(|e| AgentError::Config(format!("create RUPOO_HOME/{sub}: {e}")))?;
        }
    }

    // Generate default config.toml if not present
    let config_path = dir.join("config.toml");
    if !config_path.exists() {
        let default_config = RupooConfig::generate_default_toml();
        std::fs::write(&config_path, &default_config)
            .map_err(|e| AgentError::Config(format!("write config.toml: {e}")))?;
        info!("created default config at {}", config_path.display());
    }

    // Generate empty credentials.toml with 0600 if not present
    let creds_path = dir.join("credentials.toml");
    if !creds_path.exists() {
        let empty = CredentialsFile::default();
        empty.save()?;
        info!("created credentials.toml (chmod 600)");
    }

    Ok(dir)
}

// ---------------------------------------------------------------------------
// Migration: DB settings → config.toml
// ---------------------------------------------------------------------------

/// Migrate API keys and settings from DB to config files.
pub async fn migrate_from_db(repo: &crate::db::TaskRepo) -> AgentResult<()> {
    let mut config = RupooConfig::load().unwrap_or_default();
    let mut creds = CredentialsFile::load().unwrap_or_default();
    let mut migrated = false;

    // Active provider
    if let Ok(Some(provider)) = repo.get_setting("active_provider").await {
        config.llm.active_provider = provider;
        migrated = true;
    }

    // API keys → credentials.toml
    const KNOWN_PROVIDERS: &[&str] = &[
        "anthropic",
        "openai",
        "deepseek",
        "qwen",
        "glm",
        "moonshot",
        "yi",
        "baichuan",
        "minimax",
        "spark",
        "ollama",
    ];

    // API keys → credentials.toml
    for provider in KNOWN_PROVIDERS {
        let key_name = format!("api_key.{}", provider);
        if let Ok(Some(key)) = repo.get_setting(&key_name).await {
            if !key.is_empty() && creds.get_key(provider).is_none() {
                creds.set_key(provider, &key);
                migrated = true;
            }
        }
    }

    // Model per provider
    for provider in KNOWN_PROVIDERS {
        let key_name = format!("model.{}", provider);
        if let Ok(Some(model)) = repo.get_setting(&key_name).await {
            let pc = config
                .llm
                .providers
                .entry(provider.to_string())
                .or_default();
            pc.model = Some(model);
            migrated = true;
        }
    }

    // Base URL per provider
    for provider in KNOWN_PROVIDERS {
        let key_name = format!("base_url.{}", provider);
        if let Ok(Some(base_url)) = repo.get_setting(&key_name).await {
            let pc = config
                .llm
                .providers
                .entry(provider.to_string())
                .or_default();
            pc.base_url = Some(base_url);
            migrated = true;
        }
    }

    if migrated {
        config.save()?;
        creds.save()?;
        info!("migrated settings from DB to config files");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serialization() {
        let config = RupooConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[llm]"));
        assert!(toml_str.contains("active_provider"));
        assert!(toml_str.contains("[safety]"));
        assert!(toml_str.contains("[shell]"));

        // Round-trip
        let parsed: RupooConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.llm.active_provider, "ollama");
    }

    #[test]
    fn test_provider_config_defaults() {
        let pc = ProviderConfig::default();
        assert_eq!(pc.max_tokens, 2048);
        assert!((pc.temperature - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_credentials_round_trip() {
        let mut creds = CredentialsFile::default();
        creds.set_key("deepseek", "sk-test-123");
        creds.set_key("openai", "sk-openai-456");

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        let parsed: CredentialsFile = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.get_key("deepseek"), Some("sk-test-123"));
        assert_eq!(parsed.get_key("openai"), Some("sk-openai-456"));
        assert_eq!(parsed.get_key("anthropic"), None);
    }

    #[test]
    fn test_safety_defaults() {
        let safety = SafetySection::default();
        assert_eq!(safety.jail_root, ".");
        assert_eq!(safety.approval_policy, "dangerous_only");
        // Default lists are now empty — real defaults live in SafetyContext
        assert!(safety.forbidden_commands.is_empty());
        assert!(safety.auto_approve_tools.is_empty());
    }

    #[test]
    fn test_resolve_api_key_env() {
        let config = RupooConfig::default();
        // Set env var for the test
        std::env::set_var("TESTPROVIDER_API_KEY", "sk-from-env");
        // This won't match any configured provider, but shows the pattern
        let rt = tokio::runtime::Runtime::new().unwrap();
        let key = rt.block_on(config.resolve_api_key("testprovider"));
        assert_eq!(key, Some("sk-from-env".to_string()));
        std::env::remove_var("TESTPROVIDER_API_KEY");
    }
}
