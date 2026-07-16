//! 通道配置向导 — 交互式添加、查看、移除 IM 通道。
//!
//! 人文关怀优先：全程中文引导，提问式配置，立即验证，记住下一步。

use anyhow::{Context, Result};
use console::style;
use rupoo::channel::feishu::FeishuChannel;
use rupoo::config::{DingTalkConfig, FeishuConfig, RupooConfig};
use std::io::{self, Write};

/// 支持的通道类型列表
const SUPPORTED_CHANNELS: &[&str] = &["feishu", "dingtalk"];

// ── 公共入口 ────────────────────────────────────────────────────────────

pub async fn run(action: crate::main_cli::ChannelAction) -> Result<()> {
    match action {
        crate::main_cli::ChannelAction::Add { channel_type } => cmd_add(&channel_type).await,
        crate::main_cli::ChannelAction::List => cmd_list().await,
        crate::main_cli::ChannelAction::Remove { channel_type } => cmd_remove(&channel_type).await,
    }
}

// ── 添加通道 ────────────────────────────────────────────────────────────

async fn cmd_add(channel_type: &str) -> Result<()> {
    match channel_type {
        "feishu" => add_feishu_wizard().await,
        "dingtalk" => add_dingtalk_wizard().await,
        other => {
            anyhow::bail!(
                "暂不支持的通道类型：{other}。\n目前支持：{}",
                SUPPORTED_CHANNELS.join("、")
            );
        }
    }
}

/// 飞书通道交互式配置向导
async fn add_feishu_wizard() -> Result<()> {
    println!();
    println!("  {} 准备接入飞书！", style("🔌").bold());
    println!("  我来帮你完成配置，整个过程不到 1 分钟。");
    println!();

    let app_id = prompt_input("  → 请输入飞书 App ID（从开发者后台获取）");
    let app_secret = prompt_secret("  → 请输入飞书 App Secret");
    let lark_mode = prompt_yes_no("  → 是否使用国际版 Lark？（国内飞书选 n）");

    let feishu_cfg = FeishuConfig {
        app_id,
        app_secret,
        mention_only: true,
        approval_timeout_secs: 120,
        lark_mode,
    };

    // 验证
    print!("\n  {} 正在验证配置...", style("⏳").yellow());
    io::stdout().flush()?;

    let channel = FeishuChannel::new(feishu_cfg.clone()).context("创建飞书通道实例失败")?;

    match channel.validate_config().await {
        Ok(msg) => {
            println!("\r  {} 飞书连接验证通过！", style("✅").green());
            println!("     {}", style(msg).dim());
        }
        Err(e) => {
            println!("\r  {} 验证失败，请检查凭证是否正确。", style("❌").red());
            println!("    错误详情：{}", style(e).dim());
            println!("\n  💡 提示：确认 App ID 和 Secret 从飞书开发者后台复制，没有多余空格");
            return Ok(());
        }
    }

    // 写入配置
    save_feishu_config(feishu_cfg).await?;

    println!(
        "  {} 现在可以运行 {} 启动飞书机器人了！",
        style("🎉").bold(),
        style("rupoo serve").cyan()
    );
    println!("  有任何问题随时找我 :)");
    println!();

    Ok(())
}

/// 钉钉通道交互式配置向导
async fn add_dingtalk_wizard() -> Result<()> {
    println!();
    println!("  {} 准备接入钉钉！", style("🔌").bold());
    println!("  我来帮你完成配置，整个过程不到 1 分钟。");
    println!();

    let client_id = prompt_input("  → 请输入钉钉 Client ID (AppKey)");
    let client_secret = prompt_secret("  → 请输入钉钉 Client Secret (AppSecret)");

    let dd_cfg = DingTalkConfig {
        client_id,
        client_secret,
    };

    // 验证：尝试获取 token
    print!("\n  {} 正在验证配置...", style("⏳").yellow());
    io::stdout().flush()?;

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("创建 HTTP 客户端失败")?;

    let resp = http_client
        .post("https://api.dingtalk.com/v1.0/oauth2/accessToken")
        .json(&serde_json::json!({
            "appKey": dd_cfg.client_id,
            "appSecret": dd_cfg.client_secret,
        }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            println!("\r  {} 钉钉连接验证通过！", style("✅").green());
        }
        Ok(r) => {
            let status = r.status();
            println!("\r  {} 验证失败 (HTTP {status})", style("❌").red());
            println!("  💡 确认 Client ID 和 Secret 从钉钉开放平台复制");
            return Ok(());
        }
        Err(e) => {
            println!("\r  {} 网络错误：{e}", style("❌").red());
            println!("  💡 请检查网络连接");
            return Ok(());
        }
    }

    // 写入配置
    let config_path = rupoo::config::rupoo_home().join("config.toml");
    let mut config = RupooConfig::load().unwrap_or_default();
    config.channel.dingtalk = Some(dd_cfg);
    config.agents.insert(
        "dingtalk".into(),
        rupoo::config::AgentProfile {
            system_prompt: Some("你正在钉钉上与用户对话。用自然的中文回复，简洁口语化。".into()),
            label: Some("钉钉助手".into()),
            allowed_tools: None,
            excluded_tools: None,
        },
    );

    let content = toml::to_string_pretty(&config).context("序列化配置失败")?;
    std::fs::write(&config_path, content).context("写入配置文件失败")?;

    println!(
        "  {} 配置已保存到 {}",
        style("📁").cyan(),
        config_path.display()
    );
    println!();
    println!(
        "  {} 现在可以运行 {} 启动钉钉机器人了！",
        style("🎉").bold(),
        style("rupoo serve").cyan()
    );
    println!();

    Ok(())
}

// ── 保存配置 ──────────────────────────────────────────────────────────

async fn save_feishu_config(feishu_cfg: FeishuConfig) -> Result<()> {
    let config_path = rupoo::config::rupoo_home().join("config.toml");
    let mut config = RupooConfig::load().unwrap_or_default();
    config.channel.feishu = Some(feishu_cfg);

    config.agents.insert(
        "feishu".into(),
        rupoo::config::AgentProfile {
            system_prompt: Some(
                "\
你正在飞书 IM 上与用户对话。\
请注意：用户通过手机或电脑的聊天界面发送消息，\
你看不到对方的终端、文件系统或代码库。\
用自然的中文回复，简洁口语化，适合聊天场景。"
                    .into(),
            ),
            label: Some("飞书助手".into()),
            allowed_tools: None,
            excluded_tools: None,
        },
    );

    let content = toml::to_string_pretty(&config).context("序列化配置失败")?;
    std::fs::write(&config_path, content).context("写入配置文件失败")?;

    println!(
        "  {} 配置已保存到 {}",
        style("📁").cyan(),
        config_path.display()
    );
    Ok(())
}

// ── 查看通道列表 ────────────────────────────────────────────────────────

async fn cmd_list() -> Result<()> {
    let config = RupooConfig::load().unwrap_or_default();

    println!();
    println!("  {} 已配置的通道", style("📋").bold());
    println!();

    match &config.channel.feishu {
        Some(cfg) => println!(
            "  🔌  feishu    {}  ({})",
            style("✅ 已启用").green(),
            style(mask_key(&cfg.app_id, 12)).dim()
        ),
        None => println!(
            "  🔌  feishu    {}  (运行 {} 添加)",
            style("❌ 未配置").red(),
            style("rupoo channel add feishu").dim()
        ),
    }
    match &config.channel.dingtalk {
        Some(cfg) => println!(
            "  🔌  dingtalk  {}  ({})",
            style("✅ 已启用").green(),
            style(mask_key(&cfg.client_id, 8)).dim()
        ),
        None => println!(
            "  🔌  dingtalk  {}  (运行 {} 添加)",
            style("❌ 未配置").red(),
            style("rupoo channel add dingtalk").dim()
        ),
    }
    println!("  🔌  wecom     {}  (即将支持)", style("❌ 未配置").red());

    println!();
    println!(
        "  {} 运行 {} 添加新通道",
        style("💡").yellow(),
        style("rupoo channel add <名称>").dim()
    );
    println!();

    Ok(())
}

// ── 移除通道 ────────────────────────────────────────────────────────────

async fn cmd_remove(channel_type: &str) -> Result<()> {
    match channel_type {
        "feishu" => remove_channel("feishu", |c| c.channel.feishu = None, "飞书").await,
        "dingtalk" => remove_channel("dingtalk", |c| c.channel.dingtalk = None, "钉钉").await,
        other => anyhow::bail!(
            "暂不支持移除 {other} 通道。目前支持：{}",
            SUPPORTED_CHANNELS.join("、")
        ),
    }
}

async fn remove_channel(
    name: &str,
    clear_fn: impl FnOnce(&mut RupooConfig),
    label: &str,
) -> Result<()> {
    let confirm = prompt_yes_no(&format!(
        "  确定要移除{label}通道配置吗？{}",
        style("[y/N]").dim()
    ));
    if !confirm {
        println!("  已取消。");
        return Ok(());
    }

    let config_path = rupoo::config::rupoo_home().join("config.toml");
    let mut config = RupooConfig::load().unwrap_or_default();
    clear_fn(&mut config);
    config.agents.remove(name);

    let content = toml::to_string_pretty(&config).context("序列化配置失败")?;
    std::fs::write(&config_path, content).context("写入配置文件失败")?;

    println!("  {} 已移除{label}通道配置。", style("✅").green());
    println!(
        "  {} 需要时随时运行 {} 重新添加",
        style("💡").yellow(),
        style(format!("rupoo channel add {name}")).dim()
    );
    println!();
    Ok(())
}

// ── 辅助函数 ────────────────────────────────────────────────────────────

fn prompt_input(prompt: &str) -> String {
    println!("{}", prompt);
    print!("  > ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    input.trim().to_string()
}

fn prompt_secret(prompt: &str) -> String {
    println!("{}", prompt);
    print!("  > ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    println!("  {} 已输入 {} 个字符", style("✓").green(), trimmed.len());
    trimmed
}

fn prompt_yes_no(prompt: &str) -> bool {
    print!("{}\n  > ", prompt);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes" | "是")
}

fn mask_key(key: &str, show_chars: usize) -> String {
    if key.len() <= show_chars + 4 {
        return key.to_string();
    }
    let prefix: String = key.chars().take(show_chars).collect();
    let suffix: String = key.chars().skip(key.len().saturating_sub(4)).collect();
    format!("{}...{}", prefix, suffix)
}
