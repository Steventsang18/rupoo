pub mod agent;
pub mod db;
pub mod error;
pub mod git;
pub mod llm;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod rig_tools;
pub mod skill;
pub mod task;

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "gui")]
pub mod tray;
