use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Plan not found: {0}")]
    PlanNotFound(String),

    #[error("Invalid step index: {0}")]
    InvalidStepIndex(usize),

    #[error("Agent already running for plan: {0}")]
    AlreadyRunning(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Join error: {0}")]
    Join(String),

    // --- LLM-related errors ---

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("LLM request failed")]
    LlmRequest {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        provider: String,
    },

    #[error("LLM rate limited")]
    LlmRateLimited {
        retry_after: Option<u64>,
        provider: String,
    },

    #[error("LLM model not found: {model}")]
    LlmModelNotFound {
        model: String,
        provider: String,
    },

    #[error("LLM authentication failed: {provider}")]
    LlmAuthFailed {
        provider: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Configuration errors ---

    #[error("Config error: {0}")]
    Config(String),

    #[error("Missing required config: {key}")]
    MissingConfig {
        key: String,
        section: Option<String>,
    },

    #[error("Invalid config value: {key} = {value}")]
    InvalidConfig {
        key: String,
        value: String,
        reason: String,
    },

    // --- Git errors ---

    #[error("Git error: {0}")]
    Git(String),

    #[error("Not a git repository: {path}")]
    NotGitRepository {
        path: String,
    },

    // --- Browser errors ---

    #[error("Browser error: {0}")]
    Browser(String),

    #[error("Browser not found")]
    BrowserNotFound,

    // --- Network errors ---

    #[error("Network error: {0}")]
    Network(String),

    #[error("Connection timeout")]
    ConnectionTimeout,

    #[error("DNS resolution failed: {host}")]
    DnsResolutionFailed {
        host: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Safety errors ---

    #[error("Safety error: {0}")]
    Safety(String),

    #[error("Path traversal detected: {path} is outside jail root {jail_root}")]
    PathTraversal {
        path: String,
        jail_root: String,
    },

    #[error("Forbidden command: {command}")]
    ForbiddenCommand {
        command: String,
        reason: Option<String>,
    },

    #[error("SSRF blocked: {host} resolves to private IP")]
    SsrfBlocked {
        host: String,
        ip: String,
    },

    // --- Skill errors ---

    #[error("Skill error: {0}")]
    Skill(String),

    #[error("Skill not found: {name}")]
    SkillNotFound {
        name: String,
    },

    #[error("Skill loading failed: {name}")]
    SkillLoadFailed {
        name: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Tool errors ---

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Tool not found: {name}")]
    ToolNotFound {
        name: String,
    },

    #[error("Tool execution failed: {name}")]
    ToolExecutionFailed {
        name: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Tool timeout: {name}")]
    ToolTimeout {
        name: String,
        timeout_secs: u64,
    },

    #[error("Tool requires approval: {name}")]
    ToolRequiresApproval {
        name: String,
        params: serde_json::Value,
    },

    // --- Tray errors ---

    #[error("Tray error: {0}")]
    Tray(String),

    // --- Memory errors ---

    #[error("Memory feature is disabled")]
    MemoryDisabled,

    #[error("Memory error: {0}")]
    Memory(String),

    // --- Other errors ---

    #[error("{0}")]
    Other(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
