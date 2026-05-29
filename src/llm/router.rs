//! LLM Router — multi-provider routing with fallback, circuit breaker, and intent-driven selection.
//!
//! Core of the "积分消耗非核心化" strategy:
//! - IntentState drives model selection (precise → capable, vague → cheap)
//! - Local Ollama preferred when available (zero external API cost)
//! - Automatic fallback chain on failure
//! - Circuit breaker prevents hammering dead providers
//! - Exponential backoff on 429/503

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::config::RupooConfig;
use crate::error::{AgentError, AgentResult};
use crate::llm::{LlmConfig, LlmProvider, TokenUsage};
use crate::llm::gateway::LlmGateway;
use crate::llm::history::ConversationHistory;
use crate::llm::AgentEvent;
use crate::signal::IntentState;

// ---------------------------------------------------------------------------
// Provider health state (for circuit breaker)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ProviderHealth {
    /// Number of consecutive failures.
    consecutive_failures: u32,
    /// When the circuit breaker opened (if tripped).
    circuit_opened_at: Option<Instant>,
    /// Whether this provider is currently available.
    is_available: bool,
}

impl Default for ProviderHealth {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            circuit_opened_at: None,
            is_available: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Ollama health check
// ---------------------------------------------------------------------------

/// Check if Ollama is running and return available models.
pub async fn check_ollama_health(base_url: &str) -> OllamaStatus {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<OllamaTagsResponse>().await {
                Ok(tags) => {
                    let models: Vec<String> = tags.models.iter()
                        .map(|m| m.name.clone())
                        .collect();
                    OllamaStatus::Available { models }
                }
                Err(_) => OllamaStatus::Available { models: vec![] },
            }
        }
        Ok(resp) => {
            OllamaStatus::Unreachable(format!("HTTP {}", resp.status()))
        }
        Err(e) => {
            OllamaStatus::Unreachable(e.to_string())
        }
    }
}

#[derive(Debug, Clone)]
pub enum OllamaStatus {
    Available { models: Vec<String> },
    Unreachable(String),
}

impl OllamaStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, OllamaStatus::Available { .. })
    }
}

#[derive(serde::Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(serde::Deserialize)]
struct OllamaModel {
    name: String,
}

// ---------------------------------------------------------------------------
// LlmRouter
// ---------------------------------------------------------------------------

/// Circuit breaker threshold: trip after this many consecutive failures.
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;
/// Duration before retrying a tripped circuit breaker.
const CIRCUIT_BREAKER_RESET: Duration = Duration::from_secs(60);
/// Max retry attempts per request.
const MAX_RETRY_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff (doubles each attempt).
const BACKOFF_BASE: Duration = Duration::from_secs(1);

pub struct LlmRouter {
    config: RupooConfig,
    health: Arc<Mutex<HashMap<String, ProviderHealth>>>,
    jail_root: Option<std::path::PathBuf>,
}

impl LlmRouter {
    /// Create a new router from the loaded config.
    pub fn new(config: RupooConfig) -> Self {
        let mut health_map = HashMap::new();
        for name in config.llm.providers.keys() {
            health_map.insert(name.clone(), ProviderHealth::default());
        }
        Self {
            config,
            health: Arc::new(Mutex::new(health_map)),
            jail_root: None,
        }
    }

    /// Create with a jail root path.
    pub fn with_jail(config: RupooConfig, jail_root: std::path::PathBuf) -> Self {
        let mut router = Self::new(config);
        router.jail_root = Some(jail_root);
        router
    }

    /// Select the best provider based on intent and health state.
    ///
    /// Priority:
    /// 1. If intent is Precise + code-related → capable remote model
    /// 2. If intent is Moderate → balanced model
    /// 3. If intent is Vague → cheapest model (Ollama preferred)
    /// 4. Active provider from config
    /// 5. Any healthy provider
    pub fn select_provider(&self, intent: Option<&IntentState>) -> Option<String> {
        let active = &self.config.llm.active_provider;
        let fallback = self.config.llm.fallback_provider.as_deref();

        // Check if active provider is healthy
        let active_healthy = self.is_provider_healthy(active);

        if let Some(intent_state) = intent {
            match intent_state.precision() {
                crate::signal::IntentPrecision::Actionable | crate::signal::IntentPrecision::Structured => {
                    // Precise intent → use capable model (active or first remote)
                    if active_healthy {
                        return Some(active.clone());
                    }
                }
                crate::signal::IntentPrecision::Directional => {
                    // Moderate → prefer active, fallback to any healthy
                    if active_healthy {
                        return Some(active.clone());
                    }
                }
                crate::signal::IntentPrecision::Vague => {
                    // Vague → prefer local (cheapest), fallback to active
                    if self.is_provider_healthy("ollama") {
                        return Some("ollama".to_string());
                    }
                    if active_healthy {
                        return Some(active.clone());
                    }
                }
            }
        } else {
            // No intent info → use active provider
            if active_healthy {
                return Some(active.clone());
            }
        }

        // Active not available → try fallback
        if let Some(fb) = fallback {
            if self.is_provider_healthy(fb) {
                return Some(fb.to_string());
            }
        }

        // Last resort: any healthy provider
        for name in self.config.llm.providers.keys() {
            if self.is_provider_healthy(name) {
                return Some(name.clone());
            }
        }

        // Nothing healthy — return active anyway (will fail and trigger circuit breaker)
        Some(active.clone())
    }

    /// Get the ordered fallback chain for a given provider.
    fn fallback_chain(&self, start_provider: &str) -> Vec<String> {
        let mut chain = vec![start_provider.to_string()];

        // Add fallback provider next
        if let Some(fb) = &self.config.llm.fallback_provider {
            if fb != start_provider {
                chain.push(fb.clone());
            }
        }

        // Add all other providers in priority order
        let priority = ["ollama", "deepseek", "openai", "anthropic"];
        for p in priority {
            if p != start_provider && !chain.contains(&p.to_string()) {
                if self.config.llm.providers.contains_key(p) {
                    chain.push(p.to_string());
                }
            }
        }

        chain
    }

    /// Build an LlmConfig for a named provider.
    pub fn build_llm_config(&self, provider: &str) -> AgentResult<LlmConfig> {
        let pc = self.config.llm.providers.get(provider)
            .ok_or_else(|| AgentError::Config(format!("unknown provider: {provider}")))?;

        let llm_provider = match provider {
            "anthropic" => LlmProvider::Anthropic,
            "ollama" => LlmProvider::Ollama,
            _ => LlmProvider::OpenAI, // deepseek, openai, and any OpenAI-compatible
        };

        let mut cfg = LlmConfig::new(llm_provider, None);

        // Override model from config
        if let Some(ref model) = pc.model {
            cfg.model = model.clone();
        }

        // Override base_url from config
        if let Some(ref base_url) = pc.base_url {
            cfg.base_url = Some(base_url.clone());
        }

        cfg.max_tokens = pc.max_tokens;
        cfg.temperature = pc.temperature;

        Ok(cfg)
    }

    /// Send a chat request with automatic routing, fallback, and retry.
    pub async fn chat(
        &self,
        messages: &[crate::llm::history::LlmChatMessage],
        intent: Option<&IntentState>,
    ) -> AgentResult<(String, TokenUsage)> {
        let provider = self.select_provider(intent)
            .ok_or_else(|| AgentError::Config("no LLM provider available".into()))?;

        let chain = self.fallback_chain(&provider);

        let mut last_error = None;

        for provider_name in &chain {
            // Resolve API key
            let api_key = self.config.resolve_api_key(provider_name).await;

            // Skip providers that require API keys if none available
            let needs_key = !matches!(provider_name.as_str(), "ollama");
            if needs_key && api_key.is_none() {
                continue;
            }

            // Check circuit breaker
            if !self.is_provider_healthy(provider_name) {
                continue;
            }

            // Build config
            let mut llm_cfg = match self.build_llm_config(provider_name) {
                Ok(c) => c,
                Err(e) => {
                    warn!(provider = %provider_name, error = %e, "skipping provider");
                    continue;
                }
            };
            llm_cfg.api_key = api_key;

            // Build gateway
            let gateway = match &self.jail_root {
                Some(root) => LlmGateway::with_jail(llm_cfg, root.clone()),
                None => LlmGateway::new(llm_cfg),
            };

            // Retry with exponential backoff
            for attempt in 0..MAX_RETRY_ATTEMPTS {
                match gateway.chat(messages).await {
                    Ok(result) => {
                        self.record_success(provider_name);
                        info!(provider = %provider_name, "chat request succeeded");
                        return Ok(result);
                    }
                    Err(AgentError::Llm(ref msg)) if is_retryable_error(msg) => {
                        let delay = BACKOFF_BASE * 2u32.pow(attempt);
                        warn!(
                            provider = %provider_name,
                            attempt = attempt + 1,
                            delay_ms = delay.as_millis() as u64,
                            error = %msg,
                            "retryable error, backing off"
                        );
                        tokio::time::sleep(delay).await;
                    }
                    Err(e) => {
                        // Non-retryable error
                        self.record_failure(provider_name);
                        warn!(provider = %provider_name, error = %e, "non-retryable error");
                        last_error = Some(e);
                        break;
                    }
                }
            }

            // All retries exhausted for this provider
            self.record_failure(provider_name);
        }

        Err(last_error.unwrap_or_else(|| AgentError::Llm("all providers failed".into())))
    }

    /// Send a streaming chat request with automatic routing and fallback.
    ///
    /// Unlike `chat()`, this does NOT implement fallback across providers
    /// for streaming (the on_event callback is FnMut and can't be cloned).
    /// Instead, it selects the best provider and retries within that provider.
    pub async fn chat_agent_loop<F>(
        &self,
        user_message: &str,
        history: &ConversationHistory,
        max_turns: usize,
        safe_mode: bool,
        memory_context: Option<&str>,
        on_event: F,
        custom_preamble: Option<&str>,
        intent: Option<&IntentState>,
    ) -> AgentResult<(String, TokenUsage)>
    where
        F: FnMut(AgentEvent) + Send,
    {
        let provider = self.select_provider(intent)
            .ok_or_else(|| AgentError::Config("no LLM provider available".into()))?;

        let api_key = self.config.resolve_api_key(&provider).await;

        let mut llm_cfg = self.build_llm_config(&provider)?;
        llm_cfg.api_key = api_key;

        let gateway = match &self.jail_root {
            Some(root) => LlmGateway::with_jail(llm_cfg, root.clone()),
            None => LlmGateway::new(llm_cfg),
        };

        let params = crate::llm::gateway::ChatLoopParams {
            user_message,
            history,
            max_turns,
            safe_mode,
            memory_context,
            on_event,
            custom_preamble,
            intent,
        };

        match gateway.chat_agent_loop(params).await {
            Ok(result) => {
                self.record_success(&provider);
                Ok(result)
            }
            Err(e) => {
                self.record_failure(&provider);
                Err(e)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Circuit breaker state management
    // -----------------------------------------------------------------------

    fn is_provider_healthy(&self, name: &str) -> bool {
        let health = self.health.lock().unwrap_or_else(|p| p.into_inner());
        match health.get(name) {
            Some(h) => {
                if !h.is_available {
                    // Check if circuit breaker should be reset
                    if let Some(opened_at) = h.circuit_opened_at {
                        if opened_at.elapsed() > CIRCUIT_BREAKER_RESET {
                            return true; // Give it another chance
                        }
                    }
                    false
                } else {
                    true
                }
            }
            None => true, // Unknown provider, assume healthy
        }
    }

    fn record_success(&self, name: &str) {
        let mut health = self.health.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(h) = health.get_mut(name) {
            h.consecutive_failures = 0;
            h.is_available = true;
            h.circuit_opened_at = None;
        }
    }

    fn record_failure(&self, name: &str) {
        let mut health = self.health.lock().unwrap_or_else(|p| p.into_inner());
        let h = health.entry(name.to_string()).or_default();
        h.consecutive_failures += 1;
        if h.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
            h.is_available = false;
            h.circuit_opened_at = Some(Instant::now());
            warn!(
                provider = %name,
                failures = h.consecutive_failures,
                "circuit breaker tripped — provider marked unavailable"
            );
        }
    }

    /// Get current health status for all providers.
    pub fn health_status(&self) -> HashMap<String, bool> {
        let health = self.health.lock().unwrap_or_else(|p| p.into_inner());
        self.config.llm.providers.keys()
            .map(|name| {
                let healthy = match health.get(name) {
                    Some(h) => h.is_available,
                    None => true,
                };
                (name.clone(), healthy)
            })
            .collect()
    }

    /// Run Ollama health check and update state.
    pub async fn check_ollama(&self) -> OllamaStatus {
        let base_url = self.config.llm.providers.get("ollama")
            .and_then(|pc| pc.base_url.clone())
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        let status = check_ollama_health(&base_url).await;

        match &status {
            OllamaStatus::Available { models } => {
                self.record_success("ollama");
                info!(models = ?models, "Ollama health check passed");
            }
            OllamaStatus::Unreachable(reason) => {
                self.record_failure("ollama");
                warn!(reason = %reason, "Ollama unreachable");
            }
        }

        status
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if an LLM error is retryable (429 rate limit, 503 service unavailable, network timeout).
fn is_retryable_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("503")
        || lower.contains("service unavailable")
        || lower.contains("timeout")
        || lower.contains("connection reset")
        || lower.contains("unexpected eof")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_chain_order() {
        let config = RupooConfig::default();
        let router = LlmRouter::new(config);
        let chain = router.fallback_chain("deepseek");
        assert_eq!(chain[0], "deepseek");
        // Fallback should be ollama (from default config)
        assert!(chain.contains(&"ollama".to_string()));
    }

    #[test]
    fn test_is_retryable_error() {
        assert!(is_retryable_error("429 rate limit exceeded"));
        assert!(is_retryable_error("503 service unavailable"));
        assert!(is_retryable_error("request timeout after 30s"));
        assert!(is_retryable_error("connection reset by peer"));
        assert!(!is_retryable_error("invalid api key"));
        assert!(!is_retryable_error("model not found"));
    }

    #[test]
    fn test_circuit_breaker() {
        let config = RupooConfig::default();
        let router = LlmRouter::new(config);

        // Should start healthy
        assert!(router.is_provider_healthy("ollama"));

        // Record failures up to threshold
        for _ in 0..CIRCUIT_BREAKER_THRESHOLD {
            router.record_failure("ollama");
        }

        // Should now be unhealthy
        assert!(!router.is_provider_healthy("ollama"));

        // Record success should restore health
        router.record_success("ollama");
        assert!(router.is_provider_healthy("ollama"));
    }

    #[test]
    fn test_provider_selection_no_intent() {
        let config = RupooConfig::default();
        let router = LlmRouter::new(config);
        let selected = router.select_provider(None);
        assert!(selected.is_some());
        // Default active provider is "ollama"
        assert_eq!(selected.unwrap(), "ollama");
    }

    #[test]
    fn test_build_llm_config_deepseek() {
        let config = RupooConfig::default();
        let router = LlmRouter::new(config);
        let cfg = router.build_llm_config("deepseek").unwrap();
        assert_eq!(cfg.provider, LlmProvider::OpenAI); // DeepSeek uses OpenAI-compatible
        assert!(cfg.base_url.is_some());
    }

    #[test]
    fn test_build_llm_config_ollama() {
        let config = RupooConfig::default();
        let router = LlmRouter::new(config);
        let cfg = router.build_llm_config("ollama").unwrap();
        assert_eq!(cfg.provider, LlmProvider::Ollama);
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:11434"));
    }
}
