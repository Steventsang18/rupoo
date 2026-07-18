# Rupoo 深度剖析报告
**——使用体验流畅度 · 本地代码开发/操作能力 · 同类对比 · 测试验证建议**
（基于 `src-agent` 源码剖析 + 2026 年中公开资料，评估时点 2026-07-16，对应版本 v0.6.1）

---

## 一、执行摘要

| 维度 | 结论 |
|------|------|
| **使用体验流畅度** | **中上**。ratatui 单栏 TUI 渲染稳固、IM 风格滚动、可中断、有快照测试兜底；但**本地代码编辑/检索的"丝滑度"依赖整文件覆盖写 + shell，缺乏局部编辑与代码检索工具**，流畅度弱于 Claude Code 等。 |
| **本地代码开发/操作能力** | **安全闭环完整、精细编辑薄弱**。"执行→评估→纠正→重复"的 Loop 模式 + 本地 `cargo build/test` 自检 + path_jail/命令黑名单/SSRF/审计 齐备；但缺 `edit/str-replace/patch`、`grep/ripgrep`、diff 预览审批流、后台命令、文件监听、多文件编辑、用户侧 undo。 |
| **同类定位** | 开源、provider 无关、强安全、偏"受控环境自治"；与 Goose/Aider/OpenCode 同类，但与 Claude Code 在"本地开发人因工程"上差距明显。 |
| **测试验证** | **自身工程测试强（lib 259 + bin 77，clippy 零警告），但缺 agent 能力评测（无 SWE-bench/Terminal-Bench 等对接）**。 |

**一句话**：rupoo 在"安全 + 自治闭环 + 终端 UI"上已成形且可交付；短板集中在"作为日常本地编码伙伴的精细操作能力与可量化能力评测"两块。

---

## 二、使用体验流畅度剖析

### 2.1 做得好的部分（流畅度支撑）
- **渲染稳定**：ratatui 保留模式全屏重绘，逐帧纯函数 `render_frame`，不再有旧纯终端的"光标/ANSI 序列损坏偶发闪退"（见 `cli/tui_view.rs`）。
- **IM 风格滚动**：`follow` 钉底、↑/↓ 细粒度、PageUp/Down 整页、鼠标滚轮、resize 即时重绘、长回复视觉行折行——浏览大输出不卡顿。
- **可中断 + 对话式审批**：`Esc`/`Ctrl-C` 温柔取消；写文件是 LLM 生成的 1/2/3 确认而非硬编码菜单，体验自然。
- **人文感知**：品牌字符、思考期呼吸灯 + 安抚句、底部技巧轮播、token 页脚——降低"输入→傻等"的焦虑感。
- **测试兜底**：TUI 快照测试（`assert_buffer_eq!`）保证渲染不被回归破坏。

### 2.2 流畅度的真实瓶颈
- **编辑不是"丝滑"的**：`file_write` 只能**整文件覆盖写**（`rig_tools.rs:231-294`，`tokio::fs::write`）。改一行要重写整个文件 → 大文件时 token 成本高、易误覆盖、无 diff 预览。这是与 Claude Code（`edit`/`apply_patch` 局部编辑 + 预览）最大的体验落差。
- **无本地代码检索**：全仓**没有 grep/ripgrep/代码搜索工具**，只有 `web_search`（DuckDuckGo）。Agent 在本地"找定义/找引用"只能靠 LLM 读目录或跑 `shell_exec grep`——不可靠且慢。
- **无后台/长任务流式**：所有命令同步 + 超时（`terminal.rs` 默认 30s，`wait_with_output`），无 `&`/job/实时 tail，跑长构建时用户只能干等。
- **无文件监听/多文件编辑/用户侧 undo**：只有步骤级 checkpoint 重放（`agent.rs:973`、`db/plans.db` 规划表），没有 git stash 式用户撤销。

---

## 三、本地代码开发与操作能力评估

### 3.1 现有工具集（事实）
| 能力 | 工具 | 位置 | 备注 |
|------|------|------|------|
| 读文件 | `file_read` | `rig_tools.rs:145` | 经 path_jail 沙箱 |
| 写文件 | `file_write` | `rig_tools.rs:231` | **整文件覆盖**，无 append/insert/edit |
| 列目录 | `list_directory` | `rig_tools.rs:318` | — |
| 终端执行 | `shell_exec` | `rig_tools.rs:494` | `sh -c`，30s 超时，1万字符上限，黑名单拦截 sudo/rm 等 |
| 跑测试 | `run_tests` | `tools/verify.rs:49` | 自动探测 Cargo/npm/go/pytest，120s |
| 自检输出 | `check_output` | `tools/verify.rs:198` | — |
| diff 查看 | `diff_check` | `tools/verify.rs:334` | `git diff`，但**无"预览→确认"工作流** |
| 网络 | `web_http` | `tools/network.rs` | SSRF 防护 |
| 浏览器 | `browser` | `tools/browser.rs` | 无头 Chrome，无 JS 执行 |
| 搜索 | `web_search` | `tools/search.rs` | **仅网络**，无本地代码搜索 |

### 3.2 执行 / 构建 / 测试闭环（强项）
- **Loop 模式**（`loop_engine.rs:713 run_loop`）是真正的"执行→评估→纠正→重复"：
  1. `execute_plan` → `evaluate`（LLM 严格对比 goal 与输出，verdict ∈ {done,continue,decompose,impossible}，`loop_engine.rs:1190`）；
  2. `generate_correction_plan` 针对未满足项重跑（`loop_engine.rs:1297`）；
  3. 含**震荡/停滞检测**（`detect_oscillation`/`detect_stall`）、token/时间预算、迭代上限 10、自治等级 L1–L5。
- **本地自修复闭环成立**：Agent 可在 Loop 中跑 `cargo test` → 据 `unmet` 改代码 → 再测，具备"本地构建测试并据结果自愈"的能力。

### 3.3 安全模型（强项，也是约束）
- **path_jail**（`safety.rs:257`）防 `../`、symlink、绝对路径注入；文件工具均经 jail。
- **命令黑名单穿透校验**（`safety.rs:180`）能识破 `env rm`、`/usr/bin/rm`、相对路径绕过。
- **SSRF 防护**（`safety.rs:276`）+ DNS 重绑定检测；`forward_safe_env`（`safety.rs:462`）清空 env 只转发白名单，防密钥泄漏。
- **Supervisor 三闸门**（合规→置信度→熔断，`supervisor/mod.rs:122`）+ SQLite 审计日志。
- ⚠️ 代价：`rm`/`chmod`/`chown` 被禁 → Agent **自身无法直接删文件/改权限**（安全但限制了"重命名/清理"等常规开发动作）。

### 3.4 已知薄弱点（对比 Claude Code 等）
| 能力 | rupoo | 说明 |
|------|-------|------|
| 局部编辑 edit/patch | ❌ | 仅整文件覆盖 |
| 本地代码 grep | ❌ | 仅网络搜索 |
| diff 预览审批流 | ⚠️ | 有 `diff_check` 文本，无"编辑前预览确认" |
| 交互式权限审批（非 TUI） | ⚠️ | `--run` 非 TUI 下 `RequiresApproval` 会直接中止（`executor.rs:87`） |
| 后台命令/长任务流式 | ❌ | 全同步+超时 |
| 文件监听/多文件编辑 | ❌ | 无 fsnotify、无 batch edit |
| 用户侧 undo | ⚠️ | 仅步骤级 checkpoint，无 git 级回滚 |
| 多方案规划择优 | ❌ | `planning/planner.rs`、`scorer.rs` 为**空桩** |
| 执行层重规划 | ❌ | `execution/replanner.rs` 空桩，`validator.rs` 恒返回 passed |

> **核心判断**：rupoo 的工程骨架（安全 + Loop 自治 + 测试闭环 + 终端 UI）已"足够优秀"到可投入受控编码任务；但作为"日常本地开发伙伴"，在**精细编辑、代码检索、审批人因、长任务流式**四个点上明显落后于头部产品。规划/重规划模块仍是桩，是后续最大的能力杠杆点。

---

## 四、同类项目对比（2026 年中）

### 4.1 能力矩阵（精选）
| 项目 | 开源 | 模型无关 | 局部编辑 | 代码检索 | 自治 Loop | 安全沙箱 | 测试自检 | 免费起步 |
|------|------|----------|----------|----------|-----------|----------|----------|----------|
| **Rupoo** | ✅ | ✅ | ❌(整文件) | ❌ | ✅(L1–L5) | ✅强 | ✅ | ✅ |
| **Claude Code** | ❌ | ❌(Anthropic) | ✅ | ✅ | ✅ | ✅ | ✅ | 按 token |
| **Codex CLI** | ❌ | ❌(OpenAI) | ✅ | ✅ | ✅ | ✅ | ✅ | ChatGPT 订阅 |
| **Aider** | ✅ | ✅ | ✅(patch) | ✅(repo map) | 中 | 中 | ✅(自动 commit) | ✅ |
| **Goose** | ✅(Apache2) | ✅ | 中 | 中 | ✅ | MCP 扩展 | 中 | ✅ |
| **OpenCode** | ✅ | ✅(75+家) | ✅ | ✅(LSP) | ✅ | 中 | 中 | ✅ |
| **Gemini CLI** | ✅ | ❌(Google) | ✅ | ✅ | 中 | 中 | 中 | ✅(最慷慨免费) |
| **Cline** | ✅(扩展) | ✅ | ✅ | ✅ | 人审为主 | ✅(逐审批) | 中 | ✅ |

### 4.2 定位与哲学差异
- **rupoo ≈ Goose / Aider 路线**：开源、provider 无关、强安全、偏自治。差异化在**五层流水线 + Supervisor 三闸门 + 中文 IM 渠道（飞书/钉钉）**。
- **与 Claude Code 的差距**集中在"本地开发人因工程"：局部编辑、代码检索、diff 预览、权限审批流——这些恰恰是"流畅度"的关键。
- **能力评测（Terminal-Bench，2026 年中公开榜）**：头部 CLI agent 在 60–83% 区间（Codex CLI/GPT-5.5 居首约 80%+，Claude Code/Opus 约 79%，Factory Droid 约 59%）。**rupoo 目前没有任何公开/私有的 Terminal-Bench 成绩**，无法横向定位能力水位。

---

## 五、Runtime Agent 测试平台 / 工具推荐（供 rupoo 验证）

rupoo 自身有**单元测试强**（lib 259 + bin 77，clippy 零警告），但**缺"agent 端到端能力评测"**。建议按"由易到难"引入：

### 5.1 外部基准（能力对标）
| 基准 | 测什么 | 对 rupoo 价值 | 接入难度 |
|------|--------|---------------|----------|
| **Terminal-Bench** | 真实终端任务（命令/shell/文件操作） | 最直接对标 rupoo 的"本地操作能力"；与上面头部产品同台 | 中（需把 rupoo 包成 agent harness） |
| **SWE-bench / SWE-bench Verified** | 真实 GitHub issue→PR 修复 | 测"代码开发"端到端（检索+改+测闭环） | 中高（需 git/测试执行） |
| **AgentBench / OSWorld** | 跨应用/OS 级任务 | 测泛化，对 rupoo 偏重，优先级低 | 高 |
| **τ-bench (tau-bench)** | 工具调用 + 用户交互（含打断/澄清） | 测 rupoo 的"对话式审批 + 人际协作"强项 | 中 |
| **RE-Bench (METR)** | 长时自主研究/编码任务 | 测 Loop 长程自治与预算控制 | 高 |
| **ToolEmu / 安全评估** | 工具调用安全性（越权/注入） | 验证 path_jail/黑名单/SSRF 是否真挡住攻击 | 低（高价值） |

### 5.2 推荐落地路径
1. **先接 Terminal-Bench（最高性价比）**：rupoo 的本地执行/文件/命令能力正对其考点。把 rupoo 的 `--run` 非交互模式 + `shell_exec`/`file_write`/`run_tests` 包成 Agent-Bench-style harness，跑官方 task 集，得到首个可对标数字。
2. **接 ToolEmu 做安全回归**：用已知越权/注入 prompt 验证 Supervisor 三闸门 + path_jail 是否拦截，作为 CI 门禁（补"安全测试"缺口）。
3. **自建"内部 smoke harness"**：用 rupoo 自己的 `live_chat_sequence` 思路，构造"修复一个会编译失败的 Rust 函数→跑 `cargo test` 通过"的最小闭环，纳入 `cargo test --bin rupoo`，每天跑——零外部依赖即可持续验证"编辑→构建→测试"主干。
4. **后续接 SWE-bench Verified**：当作"能力里程碑"而非日常 CI（成本高）。

### 5.3 给 rupoo 的"补能力 + 可验证"建议（优先级）
| 优先级 | 动作 | 收益 |
|--------|------|------|
| P0 | 加 `edit`/`str_replace`/`apply_patch` 工具（替代整文件覆盖）+ diff 预览审批 | 直接补齐最大体验短板，Terminal-Bench/SWE-bench 分数跃升 |
| P0 | 加本地代码检索工具（ripgrep wrapper） | 补"找定义/引用"，Loop 评估更准 |
| P1 | 把 Terminal-Bench harness 接入 CI（每周跑） | 获得可量化能力水位 + 防回归 |
| P1 | 实现 `planning/planner.rs` + `execution/replanner.rs`（当前为空桩） | 解锁"多方案择优 + 执行层重规划"，能力杠杆最大 |
| P2 | 后台命令 / 流式 tail / 文件监听 | 流畅度（长任务不干等） |
| P2 | 用户侧 undo（git stash 式） | 信任感 |

---

## 六、结论

rupoo 已是一个**架构完整、安全扎实、终端体验现代、自治闭环可用**的开源 runtime agent，适合"受控环境下的自治式编码任务"。它的**流畅度瓶颈不在渲染，而在本地编码的精细操作层**（编辑/检索/审批/长任务）；它的**最大验证盲区是没有 agent 能力评测**——自身单测强，但对标 Terminal-Bench/SWE-bench 的能力水位仍是空白。

**最该做的三件事**：① 补局部编辑 + 代码检索工具（体验与分数双赢）；② 把 `planner`/`replanner` 空桩落地（能力杠杆）；③ 接 Terminal-Bench + ToolEmu 建立可量化、可防回归的能力/安全评测。

---

*报告生成：2026-07-16 · 版本 v0.6.1 · 评估基于 `src-agent` 源码剖析与 2026 年中公开资料。*
