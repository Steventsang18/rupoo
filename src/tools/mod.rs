//! Rupoo system tool modules.
//!
//! Each tool provides a single `async fn execute_*` entry point that
//! returns `AgentResult<String>`. All tools integrate with SafetyContext
//! for access control and timeout protection.

pub mod browser;
pub mod network;
pub mod search;
pub mod terminal;
