pub mod agent;
pub mod bench;
pub mod budget_tracker;
pub mod build_engine;
pub mod channel;
pub mod cognitive;
pub mod config;
pub mod config_watch;
pub mod context;
pub mod credentials;
pub mod cron;
pub mod db;
pub mod embedding;
pub mod error;
pub mod execution;
pub mod git;
pub mod http_client;
pub mod llm;
pub mod loop_engine;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod memory_cache;
pub mod ops_server;
pub mod orchestrator;
pub mod plan_cache;
pub mod planning;
pub mod retry;
pub mod rig_tools;
pub mod safety;
pub mod secret_manager;
pub mod shared;
pub mod signal;
pub mod skill;
pub mod strings;
pub mod supervisor;
pub mod task;
pub mod telemetry;
pub mod tool_selector;
pub mod tools;
pub mod tracing_setup;
pub mod updater;
pub mod vector_store;

// Re-export shared types at crate root so cli/mod.rs can use rupoo::TuiToAgent etc.
pub use config::rupoo_home;
pub use shared::{
    AgentToTui, ApprovalChoice, ChatMessage, Density, FileAction, FileChangeInfo, LayoutMode,
    MessageRole, PendingTool, ToolPhase, TuiControlAction, TuiToAgent,
};

// Re-export LLM types for CLI bridge
pub use llm::{
    AgentEvent, ConversationHistory, LlmConfig, LlmGateway, LlmProvider, StepSpec, TokenUsage,
};

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "gui")]
pub mod tray;
