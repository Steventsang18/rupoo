pub mod agent;
pub mod cognitive;
pub mod planning;
pub mod config;
pub mod context;
pub mod db;
pub mod embedding;
pub mod error;
pub mod git;
pub mod http_client;
pub mod llm;
pub mod loop_engine;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod memory_cache;
pub mod retry;
pub mod rig_tools;
pub mod safety;
pub mod secret_manager;
pub mod shared;
pub mod supervisor;
pub mod signal;
pub mod skill;
pub mod strings;
pub mod task;
pub mod tool_selector;
pub mod tools;
pub mod vector_store;

// Re-export shared types at crate root so cli/mod.rs can use rupoo::TuiToAgent etc.
pub use config::rupoo_home;
pub use shared::{
    AgentToTui, ApprovalChoice, ChatMessage, MessageRole, PendingTool, ToolPhase, TuiToAgent,
};

// Re-export LLM types for CLI bridge
pub use llm::{
    AgentEvent, ConversationHistory, LlmConfig, LlmGateway, LlmProvider, StepSpec, TokenUsage,
};

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "gui")]
pub mod tray;
