/// 合规校验结果
#[derive(Debug, Clone)]
pub struct ComplianceResult {
    pub allowed: bool,
    pub reason: String,
}
