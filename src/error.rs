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

    // --- New typed variants (replacing generic Other) ---

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Browser error: {0}")]
    Browser(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Safety error: {0}")]
    Safety(String),

    #[error("Skill error: {0}")]
    Skill(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Tray error: {0}")]
    Tray(String),

    #[error("{0}")]
    Other(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
