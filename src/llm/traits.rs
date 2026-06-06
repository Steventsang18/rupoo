//! Trait abstractions for LLM providers to reduce coupling and improve testability.
//!
//! These traits allow:
//! - Mock implementations for unit testing
//! - Easy swapping of LLM providers
//! - Better separation of concerns
//! - Dependency injection patterns

use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;

use crate::error::{AgentError, AgentResult};
use crate::llm::{AgentEvent, ConversationHistory, TokenUsage};

/// Trait for LLM chat completion.
///
/// This trait abstracts the core LLM functionality, allowing different providers
/// (OpenAI, Anthropic, Ollama, etc.) to be swapped easily.
#[async_trait]
pub trait LlmAgent: Send + Sync {
    /// Generate a chat completion based on the conversation history.
    ///
    /// # Arguments
    ///
    /// * `history` - The conversation history containing messages.
    ///
    /// # Returns
    ///
    /// A result containing the response text and token usage.
    async fn chat(&self, history: &ConversationHistory) -> AgentResult<(String, TokenUsage)>;

    /// Generate a streaming chat completion.
    ///
    /// # Arguments
    ///
    /// * `history` - The conversation history containing messages.
    ///
    /// # Returns
    ///
    /// A result containing a pinned stream of AgentEvent.
    async fn chat_stream(
        &self,
        history: &ConversationHistory,
    ) -> AgentResult<std::pin::Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentError>> + Send + Sync>>>;

    /// Get the name of the provider.
    fn provider_name(&self) -> &str;

    /// Get the model name being used.
    fn model_name(&self) -> &str;
}

/// Trait for LLM gateway that manages multiple providers.
///
/// This trait abstracts the gateway functionality, allowing for:
/// - Multi-provider routing
/// - Fallback mechanisms
/// - Intent-driven model selection
#[async_trait]
pub trait LlmGatewayBackend: Send + Sync {
    /// Run a plan using the LLM.
    ///
    /// # Arguments
    ///
    /// * `plan` - The plan text to execute.
    /// * `history` - Optional conversation history for context.
    ///
    /// # Returns
    ///
    /// A result containing the executed plan text.
    async fn run_plan(&self, plan: &str, history: Option<&ConversationHistory>) -> AgentResult<String>;

    /// Generate a response based on context.
    ///
    /// # Arguments
    ///
    /// * `context` - The context text for generation.
    ///
    /// # Returns
    ///
    /// A result containing the generated text.
    async fn generate(&self, context: &str) -> AgentResult<String>;

    /// Get the current LLM configuration.
    fn config(&self) -> crate::llm::LlmConfig;

    /// Validate that a path is within the allowed jail root.
    fn validate_path(&self, path: &std::path::PathBuf) -> AgentResult<std::path::PathBuf>;
}

/// Helper type for Arc-wrapped LlmAgent.
pub type ArcLlmAgent = Arc<dyn LlmAgent>;

/// Helper type for Arc-wrapped LlmGatewayBackend.
pub type ArcLlmGateway = Arc<dyn LlmGatewayBackend>;

/// Mock implementation of LlmAgent for testing.
#[derive(Debug)]
pub struct MockLlmAgent {
    provider_name: String,
    model_name: String,
    responses: Vec<String>,
    response_index: Arc<std::sync::Mutex<usize>>,
}

impl MockLlmAgent {
    pub fn new(provider_name: &str, model_name: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            model_name: model_name.to_string(),
            responses: Vec::new(),
            response_index: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn with_response(mut self, response: &str) -> Self {
        self.responses.push(response.to_string());
        self
    }

    pub fn with_responses(mut self, responses: &[&str]) -> Self {
        self.responses.extend(responses.iter().map(|s| s.to_string()));
        self
    }
}

#[async_trait]
impl LlmAgent for MockLlmAgent {
    async fn chat(&self, _history: &ConversationHistory) -> AgentResult<(String, TokenUsage)> {
        let mut index = self.response_index.lock().unwrap();
        let response = if self.responses.is_empty() {
            "Mock response".to_string()
        } else {
            let response = self.responses[*index % self.responses.len()].clone();
            *index += 1;
            response
        };
        Ok((response, TokenUsage::default()))
    }

    async fn chat_stream(
        &self,
        _history: &ConversationHistory,
    ) -> AgentResult<std::pin::Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentError>> + Send + Sync>>> {
        let response = self.chat(_history).await?;
        let stream = futures::stream::once(async move { Ok(AgentEvent::TextDelta(response.0)) });
        Ok(Box::pin(stream))
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

/// Mock implementation of LlmGatewayBackend for testing.
#[derive(Debug)]
pub struct MockLlmGateway {
    config: crate::llm::LlmConfig,
    responses: Vec<String>,
    response_index: Arc<std::sync::Mutex<usize>>,
}

impl MockLlmGateway {
    pub fn new(config: crate::llm::LlmConfig) -> Self {
        Self {
            config,
            responses: Vec::new(),
            response_index: Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn with_response(mut self, response: &str) -> Self {
        self.responses.push(response.to_string());
        self
    }

    pub fn with_responses(mut self, responses: &[&str]) -> Self {
        self.responses.extend(responses.iter().map(|s| s.to_string()));
        self
    }
}

#[async_trait]
impl LlmGatewayBackend for MockLlmGateway {
    async fn run_plan(&self, _plan: &str, _history: Option<&ConversationHistory>) -> AgentResult<String> {
        let mut index = self.response_index.lock().unwrap();
        let response = if self.responses.is_empty() {
            "Mock plan response".to_string()
        } else {
            let response = self.responses[*index % self.responses.len()].clone();
            *index += 1;
            response
        };
        Ok(response)
    }

    async fn generate(&self, _context: &str) -> AgentResult<String> {
        let mut index = self.response_index.lock().unwrap();
        let response = if self.responses.is_empty() {
            "Mock generation response".to_string()
        } else {
            let response = self.responses[*index % self.responses.len()].clone();
            *index += 1;
            response
        };
        Ok(response)
    }

    fn config(&self) -> crate::llm::LlmConfig {
        self.config.clone()
    }

    fn validate_path(&self, path: &std::path::PathBuf) -> AgentResult<std::path::PathBuf> {
        Ok(path.clone())
    }
}
