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
use tracing::{error, info, warn};

use crate::credentials::CredentialVault;
use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// Config directory
// ---------------------------------------------------------------------------

/// Validation warning produced by `RupooConfig::validate()`.
#[derive(Debug, Clone)]
pub struct ConfigWarning {
    pub section: String,
    pub key: String,
    pub message: String,
    pub severity: WarningSeverity,
}

/// Severity of a config validation warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

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
    /// Ops server (health / metrics endpoint for serve mode).
    #[serde(default)]
    pub server: ServerSection,
    /// Runtime logging — `logging.level` is hot-reloaded in serve mode.
    #[serde(default)]
    pub logging: LoggingSection,
    /// Agent identity profiles keyed by role name.
    /// e.g. `[agents.feishu]` / `[agents.cli]`
    #[serde(default)]
    pub agents: HashMap<String, AgentProfile>,
    /// Absolute path this config was loaded from.
    ///
    /// Runtime bookkeeping for the hot-reload watcher; never serialized.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

/// Agent identity profile — defines system prompt, tool scope per role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// System prompt that defines this agent's identity.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Human-readable label (e.g. "终端助手", "飞书助手").
    #[serde(default)]
    pub label: Option<String>,
    /// Only allow these tools. None = all tools allowed.
    /// Example: ["web_search", "memory_query"]
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Exclude these tools (applied on top of allowed_tools).
    /// Example: ["shell", "file_write"]
    #[serde(default)]
    pub excluded_tools: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Server section
// ---------------------------------------------------------------------------

/// Ops server configuration — a tiny loopback HTTP endpoint serving
/// `/healthz` (liveness) and `/metrics` (Prometheus) in serve mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSection {
    /// Whether the ops server starts with the serve daemon.
    #[serde(default = "default_server_enabled")]
    pub enabled: bool,
    /// Bind address; loopback only unless remote monitoring is intended.
    #[serde(default = "default_server_listen")]
    pub listen: String,
    /// Upper bound on concurrent requests (cheap DoS guard).
    #[serde(default = "default_server_max_concurrency")]
    pub max_concurrency: usize,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            enabled: default_server_enabled(),
            listen: default_server_listen(),
            max_concurrency: default_server_max_concurrency(),
        }
    }
}

fn default_server_enabled() -> bool {
    true
}

fn default_server_listen() -> String {
    "127.0.0.1:8899".to_string()
}

fn default_server_max_concurrency() -> usize {
    64
}

// ---------------------------------------------------------------------------
// Logging section
// ---------------------------------------------------------------------------

/// Accepted values for `[logging] level`.
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Runtime logging configuration.
///
/// `level` is the only hot-reloadable field today: in serve mode a config
/// file change is applied live, everything else needs a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    /// Log level: trace / debug / info / warn / error.
    #[serde(default = "default_logging_level")]
    pub level: String,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: default_logging_level(),
        }
    }
}

fn default_logging_level() -> String {
    "info".to_string()
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
        providers.insert(
            "gemini".into(),
            ProviderConfig {
                model: Some("gemini-2.5-flash".into()),
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
        let mut config: RupooConfig = toml::from_str(&content)
            .map_err(|e| AgentError::Config(format!("parse config: {e}")))?;
        config.source_path = Some(path.to_path_buf());
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

    /// Validate the configuration for completeness and correctness.
    /// Returns a list of validation warnings; errors indicate hard blocks.
    pub fn validate(&self) -> AgentResult<Vec<ConfigWarning>> {
        let mut warnings = Vec::new();

        // ── LLM section ──────────────────────────────────────────────

        // 1. Active provider must be defined
        if !self.llm.providers.contains_key(&self.llm.active_provider) {
            return Err(AgentError::MissingConfig {
                key: format!("llm.providers.{}", self.llm.active_provider),
                section: Some("llm".into()),
            });
        }

        // 2. Fallback provider must exist if set
        if let Some(ref fallback) = self.llm.fallback_provider {
            if !fallback.is_empty() && !self.llm.providers.contains_key(fallback) {
                warnings.push(ConfigWarning {
                    section: "llm".into(),
                    key: "fallback_provider".into(),
                    message: format!("fallback provider '{}' is not configured", fallback),
                    severity: WarningSeverity::Medium,
                });
            }
        }

        // 3. Validate each provider config
        for (name, pc) in &self.llm.providers {
            // Model must be set
            if pc.model.as_deref().unwrap_or("").is_empty() {
                return Err(AgentError::InvalidConfig {
                    key: format!("llm.providers.{}.model", name),
                    value: "(empty)".into(),
                    reason: "model name is required".into(),
                });
            }

            // Max tokens must be > 0
            if pc.max_tokens == 0 {
                warnings.push(ConfigWarning {
                    section: "llm".into(),
                    key: format!("providers.{}.max_tokens", name),
                    message: "max_tokens is 0, LLM responses may be empty".into(),
                    severity: WarningSeverity::High,
                });
            }

            // Temperature range check
            if !(0.0..=2.0).contains(&pc.temperature) {
                warnings.push(ConfigWarning {
                    section: "llm".into(),
                    key: format!("providers.{}.temperature", name),
                    message: format!(
                        "temperature {} is outside valid range [0.0, 2.0]",
                        pc.temperature
                    ),
                    severity: WarningSeverity::Medium,
                });
            }

            // Non-local providers should have an API key configured
            let is_local = name == "ollama"
                || pc
                    .base_url
                    .as_deref()
                    .map(|u| u.contains("localhost") || u.contains("127.0.0.1"))
                    .unwrap_or(false);

            if !is_local {
                let has_key = pc.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false);
                if !has_key {
                    warnings.push(ConfigWarning {
                        section: "llm".into(),
                        key: format!("providers.{}.api_key", name),
                        message: format!(
                            "no API key configured for '{}' — set via credentials.toml or {}_API_KEY env var",
                            name,
                            name.to_uppercase()
                        ),
                        severity: WarningSeverity::High,
                    });
                }
            }
        }

        // ── Safety section ───────────────────────────────────────────

        if self.safety.approval_policy.is_empty() {
            warnings.push(ConfigWarning {
                section: "safety".into(),
                key: "approval_policy".into(),
                message: "approval_policy is empty, using default 'dangerous_only'".into(),
                severity: WarningSeverity::Low,
            });
        }

        // ── Confidence section ───────────────────────────────────────

        if !(0.0..=1.0).contains(&self.confidence.min_threshold) {
            return Err(AgentError::InvalidConfig {
                key: "confidence.min_threshold".into(),
                value: self.confidence.min_threshold.to_string(),
                reason: "must be between 0.0 and 1.0".into(),
            });
        }

        // ── Shell section ────────────────────────────────────────────

        if self.shell.default_timeout == 0 {
            warnings.push(ConfigWarning {
                section: "shell".into(),
                key: "default_timeout".into(),
                message: "default_timeout is 0, shell commands will never timeout".into(),
                severity: WarningSeverity::Medium,
            });
        }

        if self.shell.max_output_bytes == 0 {
            warnings.push(ConfigWarning {
                section: "shell".into(),
                key: "max_output_bytes".into(),
                message: "max_output_bytes is 0, shell output will be truncated".into(),
                severity: WarningSeverity::High,
            });
        }

        // ── Memory section ───────────────────────────────────────────

        if self.memory.ttl_days == 0 {
            warnings.push(ConfigWarning {
                section: "memory".into(),
                key: "ttl_days".into(),
                message: "memory TTL is 0, stored memories will expire immediately".into(),
                severity: WarningSeverity::High,
            });
        }

        // ── Server section ─────────────────────────────────────────

        // 10. Ops server listen address must be host:port with a valid port
        match self.server.listen.rsplit_once(':') {
            Some((_, port)) if port.parse::<u16>().is_ok() => {}
            _ => {
                return Err(AgentError::InvalidConfig {
                    key: "server.listen".into(),
                    value: self.server.listen.clone(),
                    reason: "must be host:port with a numeric port".into(),
                });
            }
        }

        // ── Logging section ────────────────────────────────────────

        // 11. Log level must be one of the supported tracing levels
        if !LOG_LEVELS.contains(&self.logging.level.as_str()) {
            return Err(AgentError::InvalidConfig {
                key: "logging.level".into(),
                value: self.logging.level.clone(),
                reason: "must be one of trace/debug/info/warn/error".into(),
            });
        }

        Ok(warnings)
    }
}

impl CredentialsFile {
    /// Load credentials from `RUPOO_HOME/credentials.toml`.
    ///
    /// Values stored in the `enc:v1:` format are decrypted transparently so
    /// callers only ever see plaintext. A value that cannot be decrypted is
    /// dropped (cleared) with an error log — never passed through as-is,
    /// which would leak a ciphertext into an API call.
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

        let mut creds: CredentialsFile = toml::from_str(&content)
            .map_err(|e| AgentError::Config(format!("parse credentials: {e}")))?;

        let vault = CredentialVault::try_load();
        for (provider, value) in creds.api_keys.iter_mut() {
            if value.starts_with(crate::credentials::ENC_PREFIX) {
                match vault.decrypt(value) {
                    Some(plain) => *value = plain,
                    None => {
                        error!(
                            provider = %provider,
                            "encrypted credential cannot be decrypted (missing RUPOO_MASTER_KEY or corrupted value), ignoring"
                        );
                        value.clear();
                    }
                }
            }
        }
        Ok(creds)
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

    /// Encrypt all plaintext keys in place with the given vault.
    ///
    /// Values already in `enc:v1:` form are left untouched, making this
    /// safe to run repeatedly. Fails on the first key when the vault has
    /// no master key.
    pub fn encrypt_all(&mut self, vault: &CredentialVault) -> AgentResult<()> {
        for value in self.api_keys.values_mut() {
            if value.starts_with(crate::credentials::ENC_PREFIX) {
                continue;
            }
            *value = vault.encrypt(value)?;
        }
        Ok(())
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
        // Encrypt the migrated keys when a master key is available;
        // otherwise keep plaintext (existing behaviour).
        let vault = CredentialVault::try_load();
        if let Err(e) = creds.encrypt_all(&vault) {
            warn!(error = %e, "credentials will be stored in plaintext (no master key)");
        }
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
    fn test_server_section_defaults() {
        let section = ServerSection::default();
        assert!(section.enabled);
        assert_eq!(section.listen, "127.0.0.1:8899");
        assert_eq!(section.max_concurrency, 64);

        // Serialize/parse round-trip
        let config = RupooConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[server]"));
        let parsed: RupooConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.server.enabled);
    }

    #[test]
    fn test_logging_section_defaults() {
        let section = LoggingSection::default();
        assert_eq!(section.level, "info");

        // Serialize/parse round-trip
        let config = RupooConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[logging]"));
        let parsed: RupooConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.logging.level, "info");

        // Absent section falls back to defaults
        let parsed: RupooConfig = toml::from_str("[llm]\nactive_provider=\"ollama\"").unwrap();
        assert_eq!(parsed.logging.level, "info");
    }

    #[test]
    fn test_validate_log_level() {
        let mut config = RupooConfig::default();

        for valid in ["trace", "debug", "info", "warn", "error"] {
            config.logging.level = valid.into();
            assert!(config.validate().is_ok(), "level {valid} should be valid");
        }

        config.logging.level = "verbose".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_load_from_records_source_path() {
        with_temp_home(|_| {
            let path = crate::config::rupoo_home().join("config.toml");
            RupooConfig::default().save_to(&path).unwrap();

            let loaded = RupooConfig::load_from(&path).unwrap();
            assert_eq!(loaded.source_path.as_deref(), Some(path.as_path()));
        });
    }

    #[test]
    fn test_validate_server_listen() {
        let mut config = RupooConfig::default();

        config.server.listen = "127.0.0.1:8899".into();
        assert!(config.validate().is_ok());

        config.server.listen = "no-port-here".into();
        assert!(config.validate().is_err());

        config.server.listen = "127.0.0.1:not-a-port".into();
        assert!(config.validate().is_err());
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

    #[test]
    fn test_validate_default_config_is_ok() {
        let config = RupooConfig::default();
        let result = config.validate();
        assert!(result.is_ok());
        // Default config should have some warnings (e.g., no API keys for cloud providers)
        let warnings = result.unwrap();
        assert!(
            !warnings.is_empty(),
            "default config should warn about missing API keys"
        );
    }

    #[test]
    fn test_validate_active_provider_must_exist() {
        let mut config = RupooConfig::default();
        config.llm.active_provider = "nonexistent".into();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_missing_model_is_error() {
        let mut config = RupooConfig::default();
        config.llm.providers.get_mut("ollama").unwrap().model = None;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_shell_timeout_zero_warns() {
        let mut config = RupooConfig::default();
        config.shell.default_timeout = 0;
        let warnings = config.validate().unwrap();
        assert!(warnings.iter().any(|w| w.key == "default_timeout"));
    }

    #[test]
    fn test_validate_memory_ttl_zero_warns() {
        let mut config = RupooConfig::default();
        config.memory.ttl_days = 0;
        let warnings = config.validate().unwrap();
        assert!(warnings.iter().any(|w| w.key == "ttl_days"));
    }

    #[test]
    fn test_validate_confidence_threshold_out_of_range_is_error() {
        let mut config = RupooConfig::default();
        config.confidence.min_threshold = 1.5;
        assert!(config.validate().is_err());

        config.confidence.min_threshold = -0.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_temperature_oor_warns() {
        let mut config = RupooConfig::default();
        config.llm.providers.get_mut("ollama").unwrap().temperature = 3.0;
        let warnings = config.validate().unwrap();
        assert!(warnings.iter().any(|w| w.key.contains("temperature")));
    }

    #[test]
    fn test_validate_max_tokens_zero_warns() {
        let mut config = RupooConfig::default();
        config.llm.providers.get_mut("ollama").unwrap().max_tokens = 0;
        let warnings = config.validate().unwrap();
        assert!(warnings.iter().any(|w| w.key.contains("max_tokens")));
    }

    /// Write a credentials.toml in a temp RUPOO_HOME and read it back.
    ///
    /// RUPOO_HOME is process-global, so these tests must not run in
    /// parallel — otherwise temp dirs leak into each other.
    fn with_temp_home(test: impl FnOnce(PathBuf)) {
        static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = HOME_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        std::env::set_var("RUPOO_HOME", &home);
        test(home);
        std::env::remove_var("RUPOO_HOME");
    }

    #[test]
    fn test_load_decrypts_encrypted_credentials() {
        with_temp_home(|_| {
            std::env::set_var("RUPOO_MASTER_KEY", "a".repeat(64));

            let vault = CredentialVault::try_load();
            assert!(vault.available());

            let mut creds = CredentialsFile::default();
            creds.set_key("deepseek", "sk-secret-1");
            creds.encrypt_all(&vault).unwrap();
            assert!(creds.get_key("deepseek").unwrap().starts_with("enc:v1:"));
            creds.save().unwrap();

            // A fresh load must return the plaintext.
            let loaded = CredentialsFile::load().unwrap();
            assert_eq!(loaded.get_key("deepseek"), Some("sk-secret-1"));

            std::env::remove_var("RUPOO_MASTER_KEY");
        });
    }

    #[test]
    fn test_load_plaintext_credentials_unchanged() {
        with_temp_home(|_| {
            let mut creds = CredentialsFile::default();
            creds.set_key("openai", "sk-plain");
            creds.save().unwrap();

            let loaded = CredentialsFile::load().unwrap();
            assert_eq!(loaded.get_key("openai"), Some("sk-plain"));
        });
    }

    #[test]
    fn test_load_drops_undecryptable_value() {
        with_temp_home(|_| {
            // No master key set: encrypted value must be dropped, not leaked.
            std::env::remove_var("RUPOO_MASTER_KEY");

            let mut creds = CredentialsFile::default();
            creds.set_key("deepseek", "enc:v1:deadbeef");
            creds.save().unwrap();

            let loaded = CredentialsFile::load().unwrap();
            assert_eq!(loaded.get_key("deepseek"), Some(""));
        });
    }
}
