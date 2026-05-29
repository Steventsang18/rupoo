pub mod agent;
pub mod approval;
pub mod config;
pub mod db;
pub mod error;
pub mod git;
pub mod llm;
pub mod mcp;
pub mod mcp_client;
pub mod mcp_server;
pub mod memory;
pub mod plan_pool;
pub mod rig_tools;
pub mod safety;
pub mod shared;
pub mod signal;
pub mod skill;
pub mod task;
pub mod tools;

// Re-export shared types at crate root so cli/mod.rs can use rupoo::TuiToAgent etc.
pub use shared::{AgentToTui, ApprovalChoice, ChatMessage, MessageRole, PendingTool, ToolPhase, TuiToAgent};

// Re-export LLM types for CLI bridge
pub use llm::{AgentEvent, ConversationHistory, LlmConfig, LlmGateway, LlmProvider, StepSpec, TokenUsage};
