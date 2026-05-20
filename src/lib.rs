pub mod agent;
pub mod db;
pub mod error;
pub mod git;
pub mod llm;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod rig_tools;
pub mod shared;
pub mod skill;
pub mod task;

// Re-export shared types at crate root so cli/mod.rs can use rupoo::TuiToAgent etc.
pub use shared::{AgentToTui, ApprovalChoice, ChatMessage, MessageRole, PendingTool, TuiToAgent};

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "gui")]
pub mod tray;
