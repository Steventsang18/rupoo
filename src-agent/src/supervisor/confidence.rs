use crate::config::ConfidenceConfig;
use crate::error::{AgentError, AgentResult};
use crate::supervisor::ExecutionMeta;

/// 置信度拦截器——检查推理置信度是否达到阈值
#[derive(Debug, Clone)]
pub struct ConfidenceChecker {
    pub min_threshold: f64,
    pub pause_on_low_confidence: bool,
}

impl ConfidenceChecker {
    pub fn new(config: &ConfidenceConfig) -> Self {
        Self {
            min_threshold: config.min_threshold,
            pause_on_low_confidence: config.pause_on_low_confidence,
        }
    }

    /// 检查置信度
    /// 返回 Ok(()) 表示通过；Err 表示需要拦截/暂停
    pub fn check(&self, meta: &ExecutionMeta) -> AgentResult<()> {
        if let Some(confidence) = meta.confidence {
            if confidence < self.min_threshold && self.pause_on_low_confidence {
                return Err(AgentError::LowConfidence {
                    confidence,
                    threshold: self.min_threshold,
                });
            }
        }
        Ok(())
    }
}

impl Default for ConfidenceChecker {
    fn default() -> Self {
        Self {
            min_threshold: 0.7,
            pause_on_low_confidence: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfidenceConfig;

    #[test]
    fn test_high_confidence_passes() {
        let checker = ConfidenceChecker::default();
        let meta = ExecutionMeta::with_confidence(0.95);
        assert!(checker.check(&meta).is_ok());
    }

    #[test]
    fn test_low_confidence_blocked() {
        let checker = ConfidenceChecker::new(&ConfidenceConfig {
            min_threshold: 0.7,
            pause_on_low_confidence: true,
        });
        let meta = ExecutionMeta::with_confidence(0.3);
        let err = checker.check(&meta).unwrap_err();
        assert!(matches!(err, AgentError::LowConfidence { .. }));
    }

    #[test]
    fn test_low_confidence_no_pause_passes() {
        let checker = ConfidenceChecker::new(&ConfidenceConfig {
            min_threshold: 0.7,
            pause_on_low_confidence: false,
        });
        let meta = ExecutionMeta::with_confidence(0.3);
        assert!(checker.check(&meta).is_ok());
    }

    #[test]
    fn test_no_confidence_in_meta_passes() {
        let checker = ConfidenceChecker::default();
        let meta = ExecutionMeta::default();
        assert!(checker.check(&meta).is_ok());
    }
}
