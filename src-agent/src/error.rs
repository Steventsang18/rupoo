use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
pub enum AgentError {
    #[error("Database error: {0}")]
    #[diagnostic(code(rupoo::E001), help("检查数据库文件权限和磁盘空间"))]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    #[diagnostic(code(rupoo::E002), help("数据结构格式异常，检查输入 JSON/YAML 格式"))]
    Serialization(#[from] serde_json::Error),

    #[error("Plan not found: {0}")]
    #[diagnostic(
        code(rupoo::E003),
        help("执行计划可能已过期或被删除，使用 /session list 查看")
    )]
    PlanNotFound(String),

    #[error("Invalid step index: {0}")]
    #[diagnostic(code(rupoo::E004), help("内部步骤索引错误，请重启任务"))]
    InvalidStepIndex(usize),

    #[error("Agent already running for plan: {0}")]
    #[diagnostic(code(rupoo::E005), help("等待当前任务完成，或使用 /session kill 终止"))]
    AlreadyRunning(String),

    #[error("MCP error: {0}")]
    #[diagnostic(code(rupoo::E006), help("检查 MCP 服务端状态和配置"))]
    Mcp(String),

    #[error("IO error: {0}")]
    #[diagnostic(code(rupoo::E007), help("检查文件路径、权限和磁盘空间"))]
    Io(#[from] std::io::Error),

    #[error("Join error: {0}")]
    #[diagnostic(code(rupoo::E008), help("异步任务被取消或 panic，检查日志详情"))]
    Join(String),

    // --- LLM-related errors ---
    #[error("LLM error: {0}")]
    #[diagnostic(code(rupoo::E009), help("AI 服务返回错误，检查 API 配额和网络连接"))]
    Llm(String),

    #[error("LLM request failed")]
    #[diagnostic(code(rupoo::E010), help("AI 请求失败，检查 provider 配置和网络"))]
    LlmRequest {
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        provider: String,
    },

    #[error("LLM rate limited")]
    #[diagnostic(code(rupoo::E011), help("请求过于频繁，等待限制解除后重试"))]
    LlmRateLimited {
        retry_after: Option<u64>,
        provider: String,
    },

    #[error("LLM model not found: {model}")]
    #[diagnostic(
        code(rupoo::E012),
        help("检查模型名称拼写，或确认 provider 支持该模型")
    )]
    LlmModelNotFound { model: String, provider: String },

    #[error("LLM authentication failed: {provider}")]
    #[diagnostic(
        code(rupoo::E013),
        help("检查 API 密钥是否正确/有效，使用 /config set 更新")
    )]
    LlmAuthFailed {
        provider: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Configuration errors ---
    #[error("Config error: {0}")]
    #[diagnostic(code(rupoo::E014), help("配置文件格式错误，检查 config.toml 语法"))]
    Config(String),

    #[error("Missing required config: {key}")]
    #[diagnostic(code(rupoo::E015), help("在 config.toml 中添加缺失的配置项"))]
    MissingConfig {
        key: String,
        section: Option<String>,
    },

    #[error("Invalid config value: {key} = {value}")]
    #[diagnostic(code(rupoo::E016), help("根据错误提示修正配置项的值"))]
    InvalidConfig {
        key: String,
        value: String,
        reason: String,
    },

    // --- Git errors ---
    #[error("Git error: {0}")]
    #[diagnostic(code(rupoo::E017), help("Git 操作失败，检查仓库状态和权限"))]
    Git(String),

    #[error("Not a git repository: {path}")]
    #[diagnostic(
        code(rupoo::E018),
        help("在 Git 仓库目录内运行，或使用 git init 初始化")
    )]
    NotGitRepository { path: String },

    // --- Browser errors ---
    #[error("Browser error: {0}")]
    #[diagnostic(
        code(rupoo::E019),
        help("浏览器操作失败，检查 Chrome/Firefox 是否可用")
    )]
    Browser(String),

    #[error("Browser not found")]
    #[diagnostic(code(rupoo::E020), help("安装 Chrome 或 Firefox 浏览器"))]
    BrowserNotFound,

    // --- Network errors ---
    #[error("Network error: {0}")]
    #[diagnostic(code(rupoo::E021), help("检查网络连接和代理设置"))]
    Network(String),

    #[error("Connection timeout")]
    #[diagnostic(code(rupoo::E022), help("网络超时，稍后重试或检查目标服务状态"))]
    ConnectionTimeout,

    #[error("DNS resolution failed: {host}")]
    #[diagnostic(code(rupoo::E023), help("检查 DNS 配置，或尝试更换 DNS 服务器"))]
    DnsResolutionFailed {
        host: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Safety errors ---
    #[error("Safety error: {0}")]
    #[diagnostic(code(rupoo::E024), help("操作违反安全策略，如有需要请联系管理员"))]
    Safety(String),

    #[error("Path traversal detected: {path} is outside jail root {jail_root}")]
    #[diagnostic(code(rupoo::E025), help("禁止访问沙箱外的路径"))]
    PathTraversal { path: String, jail_root: String },

    #[error("Forbidden command: {command}")]
    #[diagnostic(code(rupoo::E026), help("该命令已被安全策略禁止执行"))]
    ForbiddenCommand {
        command: String,
        reason: Option<String>,
    },

    #[error("SSRF blocked: {host} resolves to private IP")]
    #[diagnostic(code(rupoo::E027), help("网络请求被 SSRF 防护拦截，禁止访问内网地址"))]
    SsrfBlocked { host: String, ip: String },

    // --- Skill errors ---
    #[error("Skill error: {0}")]
    #[diagnostic(code(rupoo::E028), help("skill 执行异常，检查 skill 定义和依赖"))]
    Skill(String),

    #[error("Skill not found: {name}")]
    #[diagnostic(
        code(rupoo::E029),
        help("确认 skill 名称正确，使用 /skills list 查看已安装")
    )]
    SkillNotFound { name: String },

    #[error("Skill loading failed: {name}")]
    #[diagnostic(code(rupoo::E030), help("skill 文件损坏或格式错误，尝试重新安装"))]
    SkillLoadFailed {
        name: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // --- Tool errors ---
    #[error("Tool error: {0}")]
    #[diagnostic(code(rupoo::E031), help("工具执行异常，检查工具配置和环境"))]
    Tool(String),

    #[error("Tool not found: {name}")]
    #[diagnostic(
        code(rupoo::E032),
        help("确认工具名称正确，使用 /tools list 查看已安装")
    )]
    ToolNotFound { name: String },

    #[error("Tool execution failed: {name}")]
    #[diagnostic(code(rupoo::E033), help("工具执行出错，检查输入参数和目标环境"))]
    ToolExecutionFailed {
        name: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Tool timeout: {name}")]
    #[diagnostic(code(rupoo::E034), help("工具执行超时，增加超时时间或简化任务"))]
    ToolTimeout { name: String, timeout_secs: u64 },

    #[error("Tool requires approval: {name}")]
    #[diagnostic(code(rupoo::E035), help("该工具需要用户授权后执行"))]
    ToolRequiresApproval {
        name: String,
        params: serde_json::Value,
    },

    // --- Tray errors ---
    #[error("Tray error: {0}")]
    #[diagnostic(code(rupoo::E036), help("系统托盘功能异常，检查桌面环境支持"))]
    Tray(String),

    // --- Memory errors ---
    #[error("Memory feature is disabled")]
    #[diagnostic(code(rupoo::E037), help("使用 /memory on 命令启用记忆功能"))]
    MemoryDisabled,

    #[error("Memory error: {0}")]
    #[diagnostic(code(rupoo::E038), help("记忆系统操作失败，检查数据库状态"))]
    Memory(String),

    // --- Secret errors ---
    #[error("Keyring error: {0}")]
    #[diagnostic(code(rupoo::E039), help("密钥环操作失败，检查系统密钥管理服务"))]
    Keyring(String),

    // --- Supervisor errors ---
    #[error("Low confidence: {confidence} (threshold: {threshold})")]
    #[diagnostic(code(rupoo::E040), help("AI 推理置信度过低，请提供更明确的指令"))]
    LowConfidence { confidence: f64, threshold: f64 },

    // --- Circuit breaker ---
    #[error("Circuit breaker is open: {reason}")]
    #[diagnostic(code(rupoo::E041), help("系统熔断保护已触发，等待冷却后自动恢复"))]
    CircuitBreakerOpen {
        reason: String,
        retry_after_secs: u64,
    },

    // --- Other errors ---
    #[error("{0}")]
    #[diagnostic(code(rupoo::E042), help("未知错误，查看日志获取详细信息"))]
    Other(String),
}

pub type AgentResult<T> = Result<T, AgentError>;

// ============================================================================
// Error Handling Extensions - User-friendly error messages and solutions
// ============================================================================

impl AgentError {
    /// Returns a user-friendly error message that explains the error in simple terms.
    pub fn user_friendly_message(&self) -> String {
        match self {
            AgentError::Database(e) => format!("数据库操作失败: {}", e),
            AgentError::Serialization(e) => format!("数据解析失败: {}", e),
            AgentError::PlanNotFound(id) => format!("未找到执行计划: {}", id),
            AgentError::InvalidStepIndex(idx) => format!("无效的步骤索引: {}", idx),
            AgentError::AlreadyRunning(id) => format!("计划正在执行中: {}", id),
            AgentError::Mcp(e) => format!("MCP协议错误: {}", e),
            AgentError::Io(e) => format!("文件操作失败: {}", e),
            AgentError::Join(e) => format!("异步任务执行失败: {}", e),
            AgentError::Llm(e) => format!("AI服务错误: {}", e),
            AgentError::LlmRequest { provider, .. } => {
                format!("AI服务请求失败 (提供商: {})", provider)
            }
            AgentError::LlmRateLimited {
                provider,
                retry_after,
            } => {
                let wait = retry_after
                    .map(|s| format!("{}秒", s))
                    .unwrap_or_else(|| "未知".to_string());
                format!(
                    "AI服务请求过于频繁，请等待{}后重试 (提供商: {})",
                    wait, provider
                )
            }
            AgentError::LlmModelNotFound { model, provider } => format!(
                "未找到AI模型 '{}'，请检查配置 (提供商: {})",
                model, provider
            ),
            AgentError::LlmAuthFailed { provider, .. } => {
                format!("AI服务认证失败，请检查API密钥配置 (提供商: {})", provider)
            }
            AgentError::Config(e) => format!("配置错误: {}", e),
            AgentError::MissingConfig { key, section } => {
                let location = section
                    .as_ref()
                    .map(|s| format!("[{}.{}]", s, key))
                    .unwrap_or_else(|| key.clone());
                format!("缺少必需的配置项: {}", location)
            }
            AgentError::InvalidConfig { key, value, reason } => {
                format!("配置项 '{}' 的值 '{}' 无效: {}", key, value, reason)
            }
            AgentError::Git(e) => format!("Git操作失败: {}", e),
            AgentError::NotGitRepository { path } => format!("目录 '{}' 不是Git仓库", path),
            AgentError::Browser(e) => format!("浏览器操作失败: {}", e),
            AgentError::BrowserNotFound => "未找到浏览器，请安装Chrome或Firefox".to_string(),
            AgentError::Network(e) => format!("网络连接失败: {}", e),
            AgentError::ConnectionTimeout => "连接超时，请检查网络后重试".to_string(),
            AgentError::DnsResolutionFailed { host, .. } => {
                format!("无法解析域名 '{}'，请检查网络连接", host)
            }
            AgentError::Safety(e) => format!("安全检查失败: {}", e),
            AgentError::PathTraversal { path, .. } => format!("访问路径 '{}' 被安全策略阻止", path),
            AgentError::ForbiddenCommand { command, reason } => {
                let r = reason.as_deref().unwrap_or("命令被安全策略禁止");
                format!("命令 '{}' 被禁止: {}", command, r)
            }
            AgentError::SsrfBlocked { host, ip } => {
                format!("网络请求被阻止: '{}' 解析到内部IP '{}'", host, ip)
            }
            AgentError::Skill(e) => format!("技能执行失败: {}", e),
            AgentError::SkillNotFound { name } => format!("未找到技能: {}", name),
            AgentError::SkillLoadFailed { name, .. } => format!("技能 '{}' 加载失败", name),
            AgentError::Tool(e) => format!("工具执行失败: {}", e),
            AgentError::ToolNotFound { name } => format!("未找到工具: {}", name),
            AgentError::ToolExecutionFailed { name, .. } => format!("工具 '{}' 执行失败", name),
            AgentError::ToolTimeout { name, timeout_secs } => {
                format!("工具 '{}' 执行超时 ({}秒)", name, timeout_secs)
            }
            AgentError::ToolRequiresApproval { name, .. } => {
                format!("工具 '{}' 需要您授权后才能执行", name)
            }
            AgentError::Tray(e) => format!("系统托盘错误: {}", e),
            AgentError::MemoryDisabled => "记忆功能已禁用，请使用 /memory on 启用".to_string(),
            AgentError::Memory(e) => format!("记忆操作失败: {}", e),
            AgentError::Keyring(e) => format!("密钥环操作失败: {}", e),
            AgentError::LowConfidence {
                confidence,
                threshold,
            } => {
                format!(
                    "推理置信度过低 ({:.1}%)，低于最低要求 ({:.1}%)，已暂停执行",
                    confidence * 100.0,
                    threshold * 100.0,
                )
            }
            AgentError::CircuitBreakerOpen {
                reason,
                retry_after_secs,
            } => {
                format!(
                    "系统熔断器已触发：{}，请等待 {} 秒后重试",
                    reason, retry_after_secs
                )
            }
            AgentError::Other(e) => e.clone(),
        }
    }

    /// Returns possible causes for this error.
    pub fn possible_causes(&self) -> Vec<String> {
        match self {
            AgentError::Network(_) => vec![
                "网络连接不稳定".to_string(),
                "防火墙阻止了连接".to_string(),
                "代理服务器配置错误".to_string(),
            ],
            AgentError::ConnectionTimeout => vec![
                "服务器响应过慢".to_string(),
                "网络不稳定".to_string(),
                "服务器负载过高".to_string(),
            ],
            AgentError::LlmRateLimited { .. } => vec![
                "API请求频率超出限制".to_string(),
                "账户配额已用尽".to_string(),
                "短时间内请求过多".to_string(),
            ],
            AgentError::LlmAuthFailed { .. } => vec![
                "API密钥无效或已过期".to_string(),
                "API密钥权限不足".to_string(),
                "账户已被禁用".to_string(),
            ],
            AgentError::ToolTimeout { .. } => vec![
                "命令执行时间过长".to_string(),
                "目标服务无响应".to_string(),
                "系统资源不足".to_string(),
            ],
            AgentError::PathTraversal { .. } => vec![
                "尝试访问允许范围之外的文件".to_string(),
                "路径包含可疑字符".to_string(),
            ],
            AgentError::MemoryDisabled => vec![
                "记忆功能未启用".to_string(),
                "记忆系统初始化失败".to_string(),
            ],
            _ => vec!["可能是临时故障".to_string(), "请稍后重试".to_string()],
        }
    }

    /// Returns suggested solutions for this error.
    pub fn solutions(&self) -> Vec<String> {
        match self {
            AgentError::Network(_) => vec![
                "检查网络连接是否正常".to_string(),
                "尝试重新连接WiFi或有线网络".to_string(),
                "如果是企业网络，请联系网络管理员".to_string(),
            ],
            AgentError::ConnectionTimeout => vec![
                "稍后重试操作".to_string(),
                "检查目标服务器状态".to_string(),
                "增加超时时间配置".to_string(),
            ],
            AgentError::LlmRateLimited { retry_after, .. } => {
                let mut solutions =
                    vec!["等待一段时间后再试".to_string(), "降低请求频率".to_string()];
                if let Some(seconds) = retry_after {
                    solutions.insert(0, format!("等待约 {} 秒后重试", seconds));
                }
                solutions
            }
            AgentError::LlmAuthFailed { .. } => vec![
                "检查配置文件中的API密钥是否正确".to_string(),
                "确认API密钥还有效且有足够配额".to_string(),
                "查看API服务提供商的账户状态".to_string(),
            ],
            AgentError::ToolTimeout { name, .. } => vec![
                format!("检查 '{}' 命令是否正确", name),
                "增加工具执行超时时间".to_string(),
                "检查目标资源是否可用".to_string(),
            ],
            AgentError::MemoryDisabled => vec![
                "运行 /memory on 命令启用记忆功能".to_string(),
                "检查记忆系统是否正常初始化".to_string(),
                "运行 /doctor 检查系统状态".to_string(),
            ],
            _ => vec![
                "稍后重试操作".to_string(),
                "如果问题持续存在，请查看日志获取详细信息".to_string(),
            ],
        }
    }

    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AgentError::Network(_)
                | AgentError::ConnectionTimeout
                | AgentError::LlmRateLimited { .. }
                | AgentError::LlmRequest { .. }
                | AgentError::DnsResolutionFailed { .. }
                | AgentError::ToolTimeout { .. }
                | AgentError::CircuitBreakerOpen { .. }
        )
    }

    /// Check if this error requires user approval to retry.
    pub fn requires_approval(&self) -> bool {
        matches!(self, AgentError::ToolRequiresApproval { .. })
    }
}
