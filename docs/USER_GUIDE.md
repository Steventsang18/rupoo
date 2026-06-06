# Rupoo 使用指南

---

## 目录

1. [项目概述](#1-项目概述)
2. [环境要求](#2-环境要求)
3. [安装步骤](#3-安装步骤)
   - [Windows](#31-windows)
   - [macOS](#32-macos)
   - [Linux](#33-linux)
4. [配置指南](#4-配置指南)
5. [基本操作](#5-基本操作)
   - [启动与退出](#51-启动与退出)
   - [REPL 命令](#52-repl-命令)
   - [键盘快捷键](#53-键盘快捷键)
6. [高级功能](#6-高级功能)
   - [计划模式](#61-计划模式)
   - [多会话管理](#62-多会话管理)
   - [技能系统](#63-技能系统)
   - [记忆系统](#64-记忆系统)
7. [CLI 命令参考](#7-cli-命令参考)
8. [常见问题解答 (FAQ)](#8-常见问题解答-faq)
9. [故障排除](#9-故障排除)
10. [安全注意事项](#10-安全注意事项)
11. [附录](#11-附录)

---

## 1. 项目概述

Rupoo 是一个基于终端的 AI 助手，采用原生 REPL（Read-Eval-Print Loop）交互界面，具备以下核心特性：

### 核心功能

| 功能 | 描述 |
|------|------|
| **原生 REPL** | 流畅滚动、终端自适应、无需帧缓冲 |
| **语法高亮** | 支持多种代码主题 |
| **Markdown 渲染** | 表格、代码块、链接等完整支持 |
| **主题系统** | 深色/浅色/Monokai 主题切换 |
| **双模式 Agent** | 聊天模式 + 计划模式 |
| **长期记忆** | FTS5 全文搜索，智能上下文管理 |
| **技能系统** | 可扩展的 AI 技能插件 |

### 支持的 LLM 提供商

- **Anthropic Claude** - 原生支持
- **OpenAI** - 兼容 API
- **DeepSeek** - 兼容 OpenAI API
- **Ollama** - 本地模型支持

### 安全特性

- 路径沙箱保护
- SSRF 防护
- 命令黑名单
- 环境变量清理

---

## 2. 环境要求

### 最低配置

| 项目 | 要求 |
|------|------|
| 操作系统 | Windows 10+, macOS 11+, Linux (任意主流发行版) |
| Rust 版本 | 1.75+ |
| 内存 | 2GB RAM |
| 存储 | 500MB 可用空间 |

### 推荐配置

| 项目 | 推荐 |
|------|------|
| 内存 | 8GB RAM |
| 网络 | 稳定的互联网连接（用于云端 LLM） |
| 存储 | 1GB+ 可用空间（用于缓存和日志） |

---

## 3. 安装步骤

### 3.1 Windows

#### 方法一：使用预编译二进制

1. 访问 [GitHub Releases](https://github.com/Steventsang18/rupoo/releases)
2. 下载最新版本的 `rupoo-windows-x86_64.zip`
3. 解压到任意目录
4. 将解压目录添加到系统 PATH 环境变量

#### 方法二：从源码编译

```powershell
# 安装 Rust (如果尚未安装)
# 访问 https://www.rust-lang.org/tools/install

# 克隆仓库
git clone https://github.com/Steventsang18/rupoo.git
cd rupoo

# 编译发布版本
cargo build --release

# 将二进制文件复制到系统路径
copy target\release\rupoo.exe C:\Windows\System32\
```

### 3.2 macOS

#### 方法一：使用 Homebrew（推荐）

```bash
# 添加 Tap (如果尚未添加)
brew tap Steventsang18/rupoo

# 安装
brew install rupoo
```

#### 方法二：使用预编译二进制

```bash
# 下载最新版本
curl -LO https://github.com/Steventsang18/rupoo/releases/latest/download/rupoo-macos-aarch64.tar.gz

# 解压
tar -xzf rupoo-macos-aarch64.tar.gz

# 移动到系统路径
sudo mv rupoo /usr/local/bin/
```

#### 方法三：从源码编译

```bash
# 安装 Rust (如果尚未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 克隆仓库
git clone https://github.com/Steventsang18/rupoo.git
cd rupoo

# 编译发布版本
cargo build --release

# 安装到系统路径
sudo cp target/release/rupoo /usr/local/bin/
```

### 3.3 Linux

#### 方法一：使用包管理器

**Debian/Ubuntu:**

```bash
# 添加仓库 (待实现)
# sudo add-apt-repository ppa:rupoo/stable
# sudo apt update
# sudo apt install rupoo
```

#### 方法二：使用预编译二进制

```bash
# 下载最新版本 (x86_64)
curl -LO https://github.com/Steventsang18/rupoo/releases/latest/download/rupoo-linux-x86_64.tar.gz

# 解压
tar -xzf rupoo-linux-x86_64.tar.gz

# 移动到系统路径
sudo mv rupoo /usr/local/bin/

# 赋予执行权限
sudo chmod +x /usr/local/bin/rupoo
```

#### 方法三：从源码编译

```bash
# 安装依赖
# Ubuntu/Debian
sudo apt update && sudo apt install -y build-essential libssl-dev pkg-config

# Fedora/CentOS
sudo dnf install -y gcc openssl-devel pkg-config

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 克隆仓库
git clone https://github.com/Steventsang18/rupoo.git
cd rupoo

# 编译发布版本
cargo build --release

# 安装
sudo cp target/release/rupoo /usr/local/bin/
```

---

## 4. 配置指南

### 4.1 配置文件位置

| 操作系统 | 配置文件路径 |
|----------|-------------|
| Windows | `%APPDATA%\rupoo\config.toml` |
| macOS | `~/Library/Application Support/rupoo/config.toml` |
| Linux | `~/.config/rupoo/config.toml` |

### 4.2 配置 LLM 提供商

#### Anthropic Claude（推荐）

```bash
# 设置 API 密钥
rupoo config set api_key.anthropic sk-ant-xxxxxxxxxxxxxxxxxxxx

# 设置模型
rupoo config set model.anthropic claude-sonnet-4-20250514

# 设置为活跃提供商
rupoo config set active_provider anthropic
```

#### OpenAI / DeepSeek

```bash
# 设置 API 密钥
rupoo config set api_key.openai sk-xxxxxxxxxxxxxxxxxxxx

# 设置模型
rupoo config set model.openai deepseek-chat

# 设置自定义 API 地址（适用于 DeepSeek 等兼容服务）
rupoo config set base_url.openai https://api.deepseek.com/v1

# 设置为活跃提供商
rupoo config set active_provider openai
```

#### Ollama（本地模型）

```bash
# 首先安装 Ollama: https://ollama.com/download

# 拉取模型
ollama pull llama3

# 设置为活跃提供商
rupoo config set active_provider ollama

# 设置模型名称
rupoo config set model.ollama llama3
```

### 4.3 查看配置

```bash
# 查看所有配置
rupoo config list

# 查看特定配置项
rupoo config get active_provider
rupoo config get model.anthropic
```

---

## 5. 基本操作

### 5.1 启动与退出

```bash
# 启动交互式 REPL
rupoo

# 退出
# 方法1: Ctrl+D (推荐)
# 方法2: /quit 或 /q 命令
# 方法3: /exit 命令
```

**注意**: 按 `Ctrl+C` 不会退出程序，只会显示提示信息。这是为了防止误操作导致意外退出。

### 5.2 REPL 命令

| 命令 | 别名 | 描述 |
|------|------|------|
| `/help` | `/?`, `/h` | 显示帮助信息 |
| `/new` | - | 新建对话会话 |
| `/model` | `/m` | 查看或切换当前模型 |
| `/theme <name>` | `/t` | 切换主题 (dark/light/monokai) |
| `/plan` | - | 进入计划模式 |
| `/tools` | `/ts` | 查看可用工具列表 |
| `/sessions` | `/ls` | 查看所有会话 |
| `/switch <name>` | `/s` | 切换到指定会话 |
| `/quit` | `/q`, `/exit` | 退出 Rupoo |
| `/clear` | `/cls` | 清空屏幕 |
| `/history` | - | 查看历史记录 |
| `/alias` | - | 查看命令别名 |

### 5.3 键盘快捷键

| 快捷键 | 功能 |
|--------|------|
| `Enter` | 发送消息 |
| `↑` / `↓` | 浏览历史消息 |
| `Ctrl+R` | 增量搜索历史 |
| `Ctrl+C` | 显示退出提示（不会退出） |
| `Ctrl+D` | 退出程序 |
| `Ctrl+L` | 清屏 |
| `Tab` | 自动补全命令 |
| `Ctrl+N` | 新建会话 |
| `Ctrl+S` | 保存会话 |
| `Alt+1-9` | 快速切换会话 |

---

## 6. 高级功能

### 6.1 计划模式

计划模式允许 AI 制定并执行复杂的多步骤计划。

```bash
# 进入计划模式
/plan

# 示例：让 AI 规划一个任务
/plan 帮我创建一个 Rust 项目的 CI/CD 流水线

# 退出计划模式
输入 /exit
```

#### 计划步骤类型

| 步骤类型 | 描述 |
|----------|------|
| `Think` | AI 思考步骤 |
| `ToolCall` | 调用工具 |
| `WaitForInput` | 等待用户输入 |
| `Exec` | 执行命令 |
| `HttpRequest` | 发送 HTTP 请求 |
| `BrowserAction` | 浏览器操作 |
| `Finish` | 完成计划 |

### 6.2 多会话管理

```bash
# 查看所有会话
/sessions

# 切换会话
/switch 会话名称

# 新建会话
/new

# 删除会话
# 在会话列表中按提示操作
```

### 6.3 技能系统

```bash
# 查看可用技能
rupoo skills list

# 查看技能详情
rupoo skills show <skill_name>

# 安装内置技能
rupoo skills install-builtin
```

### 6.4 记忆系统

Rupoo 会自动记录对话历史，并在需要时智能检索相关上下文。

```bash
# 搜索记忆
/history search <关键词>

# 查看最近历史
/history

# 查看记忆统计
/history stats
```

---

## 7. CLI 命令参考

### 7.1 全局命令

```bash
rupoo [OPTIONS] [COMMAND]
```

### 7.2 命令列表

| 命令 | 子命令 | 描述 |
|------|--------|------|
| `demo` | - | 运行内置演示 |
| `status` | - | 系统状态概览 |
| `model` | `show` | 显示当前模型 |
| | `list` | 列出可用模型 |
| | `set <provider>` | 设置活跃提供商 |
| `session` | `list` | 列出所有会话 |
| | `show <id>` | 显示会话详情 |
| | `resume <id>` | 恢复会话 |
| | `delete <id>` | 删除会话 |
| `skills` | `list` | 列出技能 |
| | `show <name>` | 显示技能详情 |
| | `install-builtin` | 安装内置技能 |
| `config` | `set <key> <value>` | 设置配置 |
| | `get <key>` | 获取配置 |
| | `list` | 列出所有配置 |
| `git` | `status` | Git 状态 |
| | `commit` | 智能提交 |
| | `pr` | PR 助手 |
| `doctor` | - | 诊断问题 |
| | `--fix` | 自动修复 |
| `logs` | - | 查看日志 |
| | `--follow` | 实时跟踪日志 |
| `mcp-server` | - | 启动 MCP 服务器 |

---

## 8. 常见问题解答 (FAQ)

### Q1: Ctrl+C 为什么不能退出程序？

**A:** 为了防止误操作导致意外退出，Rupoo 使用 `Ctrl+D` 作为退出快捷键。

```bash
# 正确的退出方式
Ctrl+D           # 直接退出
/quit            # 通过命令退出
/q               # 命令别名
/exit            # 命令别名
```

**提示**: 按 `Ctrl+C` 会显示 "Use Ctrl+D to quit" 提示信息。

### Q2: 如何设置代理？

**A:** 设置环境变量：

```bash
# Linux/macOS
export HTTP_PROXY=http://proxy:port
export HTTPS_PROXY=https://proxy:port

# Windows (PowerShell)
$env:HTTP_PROXY = "http://proxy:port"
$env:HTTPS_PROXY = "https://proxy:port"
```

### Q2: 如何更换模型？

**A:** 使用 `/model` 命令或 CLI：

```bash
# 在 REPL 中
/model

# 使用 CLI
rupoo model set anthropic
```

### Q3: 数据存储在哪里？

**A:** 

| 数据类型 | 路径 |
|----------|------|
| 配置 | `~/.config/rupoo/config.toml` |
| 数据库 | `~/.local/share/rupoo/rupoo.db` |
| 日志 | `~/.local/share/rupoo/logs/` |
| 历史 | `~/.local/share/rupoo/history.txt` |

### Q4: 如何清理缓存？

**A:** 

```bash
# 删除缓存目录
rm -rf ~/.local/share/rupoo/cache

# 或者使用 doctor 命令
rupoo doctor --fix
```

### Q5: 支持哪些本地模型？

**A:** 通过 Ollama 支持所有兼容的本地模型，包括：
- Llama 3
- Mistral
- Phi
- Qwen
- 等

### Q6: 如何提高响应速度？

**A:** 

1. 使用 Ollama 运行本地模型
2. 选择更快的网络连接
3. 减少上下文窗口大小
4. 使用更轻量的模型

---

## 9. 故障排除

### 9.1 常见错误

#### 错误：无法连接到 LLM

**原因:** 网络问题或 API 密钥错误

**解决:**

```bash
# 检查网络连接
ping api.anthropic.com

# 检查 API 密钥
rupoo config get api_key.anthropic

# 重新设置密钥
rupoo config set api_key.anthropic sk-ant-xxx
```

#### 错误：缺少 OpenSSL

**原因:** 系统缺少 OpenSSL 库

**解决:**

```bash
# Ubuntu/Debian
sudo apt install libssl-dev

# Fedora/CentOS
sudo dnf install openssl-devel

# macOS (使用 Homebrew)
brew install openssl
```

#### 错误：权限不足

**原因:** 没有写入配置目录的权限

**解决:**

```bash
# 创建配置目录并设置权限
mkdir -p ~/.config/rupoo
chmod 755 ~/.config/rupoo
```

#### 错误：Ollama 模型未找到

**原因:** 模型未下载或名称错误

**解决:**

```bash
# 列出已下载的模型
ollama list

# 拉取模型
ollama pull llama3

# 检查配置
rupoo config get model.ollama
```

### 9.2 日志分析

```bash
# 查看最近日志
rupoo logs

# 实时跟踪日志
rupoo logs --follow

# 查看特定日期的日志
cat ~/.local/share/rupoo/logs/2024-01-15.log
```

### 9.3 重置配置

```bash
# 备份当前配置
cp ~/.config/rupoo/config.toml ~/.config/rupoo/config.toml.bak

# 删除配置文件
rm ~/.config/rupoo/config.toml

# 重新配置
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set active_provider anthropic
```

---

## 10. 安全注意事项

### 10.1 安全特性

| 特性 | 说明 |
|------|------|
| **路径沙箱** | 限制文件操作在指定目录内 |
| **SSRF 防护** | 阻止访问本地网络和内部 IP |
| **命令黑名单** | 阻止危险命令执行 |
| **超时保护** | 命令执行最长 30 秒 |
| **输出截断** | 限制命令输出大小 |

### 10.2 最佳实践

1. **保护 API 密钥**: 不要在公共场合或版本控制中暴露密钥
2. **限制权限**: 以普通用户身份运行，避免使用 sudo
3. **定期更新**: 及时更新到最新版本
4. **审核日志**: 定期检查日志文件
5. **谨慎使用工具**: 对 AI 执行的命令保持警惕

---

## 11. 附录

### A. 配置文件示例

```toml
# ~/.config/rupoo/config.toml

[api_key]
anthropic = "sk-ant-xxxxxxxxxxxxxxxxxxxx"
openai = "sk-xxxxxxxxxxxxxxxxxxxx"

[model]
anthropic = "claude-sonnet-4-20250514"
openai = "gpt-4o"
ollama = "llama3"

[base_url]
openai = "https://api.openai.com/v1"

active_provider = "anthropic"

[settings]
theme = "dark"
max_history = 1000
auto_save = true
```

### B. 支持的主题

| 主题名称 | 描述 |
|----------|------|
| `dark` | 深色主题（默认） |
| `light` | 浅色主题 |
| `monokai` | Monokai 配色 |

### C. 工具列表

| 工具 | 描述 |
|------|------|
| `file_read` | 读取文件内容 |
| `file_write` | 写入文件 |
| `list_dir` | 列出目录内容 |
| `shell_exec` | 执行 shell 命令 |
| `web_search` | 网络搜索 |
| `browser` | 浏览器操作 |

---

## 支持与反馈

如果您遇到问题或有改进建议：

- **GitHub Issues**: https://github.com/Steventsang18/rupoo/issues
- **讨论区**: https://github.com/Steventsang18/rupoo/discussions

---

*版本: 0.3.1 | 最后更新: 2026年6月6日*

---

**许可证**: MIT License