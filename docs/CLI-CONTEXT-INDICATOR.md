# rupoo CLI 上下文使用指示器 —— 技术设计文档

> 状态：设计阶段（未实现）。用户要求先出严谨设计文档，再决定实现。
> 原则：**表面极简（类 Claude Code 单栏聊天流），内里充满人文关怀**；上下文占用应"让用户随时看得见、并能主动管理"，但不增加视觉噪音。遵守质量红线——设计先行、必须有 TUI 快照测试守护、每步可回退、不行就停保持纯终端。

---

## 1. 设计原则

### 1.1 上下文透明度（新诉求）
- **可见**：用户应实时知道当前对话占用了多少上下文窗口，避免"聊着聊着 agent 变傻"却不知原因。
- **可管**：指示器不是终点，要引导用户用已有手段（`/clear`、`/memory`）主动管理上下文。
- **克制**：不抢占状态条焦点（模型名、阶段呼吸灯才是主角），指示器降亮、低闪烁。

### 1.2 质量红线（硬约束）
- 一次性做对，不"上线后反复修"；做不到稳定 + 有测试守护就不做。
- 指标必须基于**真实已发送/将发送数据**，不能拍脑袋估算。
- 不引入新依赖；复用 `ratatui` 现有渲染与 `ConversationHistory` 现有接口。

---

## 2. 现有基础盘点（关键 · 源码实证）

rupoo 的上下文管理分**两条独立路径**，且**只有软限制、无模型窗口硬上限**。

### 2.1 CLI 聊天路径（用户日常使用的 `bridge.rs` / `app.rs`）

| 维度 | 现状 | 位置 |
|---|---|---|
| 轮次上限 | `HISTORY_DEFAULT_MAX_TURNS = 10`（保留最近 20 条非 system 消息） | `src-agent/src/cli/mod.rs:45` |
| Token 预算 | `DEFAULT_TOKEN_BUDGET = 60000`（单位 = 字符数/2 估算的 token） | `src-agent/src/cli/mod.rs:39` |
| 自动裁剪入口 | `trim_to_limits()`：每次 push 后先裁轮次、再裁预算 | `src-agent/src/llm/history.rs:115` |
| 轮次裁剪 | `trim_by_turns()`：丢最旧 user/assistant 对，保留 system | `src-agent/src/llm/history.rs:124` |
| 预算裁剪 | `trim_by_token_budget()`：从最旧丢起；单条超预算则**原地截断该消息** | `src-agent/src/llm/history.rs:150` |
| 估算 token | `estimated_tokens()` = Σ字符数 / 2 | `src-agent/src/llm/history.rs:225` |
| 读预算 | `token_budget()` | `src-agent/src/llm/history.rs:220` |
| 清空指令 | `/clear` → `handle_clear()`（清空历史） | `src-agent/src/cli/bridge.rs:100` |

### 2.2 agent crate（ReAct 引擎）路径

| 维度 | 现状 | 位置 |
|---|---|---|
| 统一上下文 | `ConversationContext` + `TokenBudget`（environment/intent/memory/history 分区） | `src-agent/src/context.rs:223` |
| 预算档位 | default `total=4096` / compact `2048` / expanded `8192` | `src-agent/src/context.rs:39-78` |
| 访问/重置 | `context()` / `reset_context()` | `src-agent/src/agent.rs:370,378` |
| system 块 | `to_system_context_block()`（受各分区预算约束） | `src-agent/src/context.rs:309` |

### 2.3 工具输出压缩（减少喂给模型的量，已存在）

| 机制 | 行为 | 位置 |
|---|---|---|
| `compress_output` | 长输出仅保留"头 200 + 尾 200 字符 + 省略行数" | `src-agent/src/signal.rs:41` |
| `compress_file_content` | 带行号、按目标符号智能压缩大文件 | `src-agent/src/signal.rs:104` |
| `compress_plan_output` | 计划输出压成"前 200 + 后 200 + 步数" | `src-agent/src/loop_engine.rs:1398` |

### 2.4 关键结论（设计必须正视）

1. **没有模型上下文窗口硬上限**：`config.rs:234` 的 `max_tokens`（默认 2048）是**生成输出**上限（`llm/providers.rs:37` 传给 API），不是上下文窗口。代码从不读取"当前模型是 128k 还是 8k"，预算是写死常量，与模型无关。
2. **裁剪是机械式**：只丢/截**最旧**内容，**无语义压缩**（没有 Claude 那种"把旧轮次总结成一条摘要"）。超长单条 assistant 消息会被 `truncate` 截断（`:188-195`）。
3. **无运行时调整指令**：没有 `/context`、`/budget` 这类动态调整预算/优先级的命令。预算与轮次都是编译期常量。
4. **有效字符上限 ≈ 12 万字符**：`trim_by_token_budget` 把预算当 token 数、用 `字符/2` 估算，因此 `DEFAULT_TOKEN_BUDGET=60000` 对应约 120k 字符软上限。

---

## 3. 目标 / 非目标

### 3.1 目标（本期）
- 在现有 ratatui 状态条 / footer 提供**实时上下文占用百分比 + 迷你进度条**。
- 占用率分色（绿/琥珀/红），超阈值时给出柔和、不弹窗的管理提示。
- 指标完全基于 `ConversationHistory` 已提供的 `estimated_tokens()` / `token_budget()`，零新增估算逻辑。
- 配套快照测试守护。

### 3.2 非目标（本期不做，留作后续增强）
- 不改动裁剪/压缩算法本身（机械裁剪保持现状）。
- 不实现语义压缩（`/compact` 式总结）——属独立大改动，见 §6.2。
- 不绑定真实模型窗口（§6.1 列为"建议增强"，非必需）。
- 不新增运行时指令（如 `/context`）——可后续追加，见 §6.3。

---

## 4. 指示器设计

### 4.1 数据源与指标（已落地）
- 复用 `ConversationHistory::estimated_tokens()`（Σ字符/2）与 `token_budget()`（60000）。
- 占用率：`pct = token_budget>0 ? ((estimated_tokens*100)/token_budget) as u8 : 0`，clamp 0–100。
- 数据通路（解耦、符合现有 `AgentToTui` 事件架构）：`AgentUiBridge` 在每次"轮次结束"前通过封装方法 `send_idle()` 计算 pct 并发出 `AgentToTui::ContextUsage { pct }`；`tui_view::apply_event` 收到后写入 `ChatView.ctx_usage_pct: Option<u8>`。tui_run 在 `Idle` 分支调用 `push_token_footer` 时读取它。
- 不在 tui_run 直接读 `conversation_history`（该字段属 `AgentUiBridge`，对 `ReplSession` 不可见），严格走事件通道，保证单一数据源、不与 legacy 路径耦合。

### 4.2 渲染位置（已落地 = S2：并入 token 页脚）
- **最终选定 S2**：把上下文仪表追加到每条 assistant 回复后的 token 消耗提示栏（`⏱ {s}s · Σ{in} in / Σ{out} out`），**零新增布局行**，不挤占状态条/输入栏，最贴合"表面极简"约束。
- 实现：`StreamItem::TokenStat` 由 `String` 升级为结构 `TokenStat { stats: String, ctx_pct: Option<u8> }`；渲染时先画 dim 灰的 `stats`，再画 ` Cxt [⚠ ]▰▰▱▱▱ NN%`（按占用率上色）。
- 关于最初"固定不滚动"要求：S2 下仪表随聊天流滚动；但因 `follow` 默认钉底，回复后的页脚通常就在可视底部，常态可见。若需"永远不被滚走"，可回退 R1（底部状态条固定）。

### 4.3 颜色分区（贴合现有人文配色，已落地）
- `<60%`：柔绿 `Color::Rgb(150,170,150)`（sage）。
- `60–85%`：暖琥珀 `HINT_ACCENT_COLOR = Rgb(195,155,110)`。
- `>85%`：红 `Color::Red`，并加 `⚠ ` 预警前缀（渲染为 `⚠ Cxt ▰▰▰▰▱ 88%`）。
- 迷你条 5 格：每格 20%，`▰`=已用、`▱`=剩余，`filled = round(pct/20)` clamp 0–5。

### 4.4 超阈值联动（Phase B，未落地）
- **本期（S2）预警仅限仪表**：>85% 时仪表转红 + `⚠` 前缀，引导用户 `/clear` 或把关键信息存入 `/memory`。
- 更强联动（未做，属 Phase B）：占用率 >85% 时在聊天流内联推一条柔和 `StreamItem`（非模态、不破坏极简布局）：`「上下文接近上限，建议 /clear 或把关键信息存入 /memory」`。复用现有 `StreamItem` 内联块样式，不新增 UI 控件。

---

## 5. 测试策略（质量红线强制，已落地）

- **渲染快照测试**：`context_gauge_rendered_in_token_footer` —— 构造带 `ctx_pct=Some(88)` 的 `TokenStat` 经 `render_frame` 渲染到 `TestBackend`，断言 buffer 含 `Cxt` 与 `88%`（全角字符按记忆规则用 `contains` 子串，不逐 cell 整串匹配）。
- **reducer/携带单测**：`token_footer_carries_context_gauge`（普通 42%，验证 `TokenStat.ctx_pct` 正确携带）、`token_footer_context_over_limit_marks_red`（88% → `ctx_level` 返回红 + warn）、`ctx_gauge_and_level`（迷你条格数 0/20/40/60/80/100 + 三档配色边界 30/70/88）。
- 既有 3 个 token footer 测试已随 `TokenStat` 结构升级同步更新：`token_footer_pushed_after_reply` / `token_footer_skipped_when_no_reply` / `token_footer_cumulative_reflects_session_total`。
- 回归：本期零布局行新增，复用既有滚动/折行测试范式，未破坏 `↑/↓` 滚动。
- 状态：`cargo test --bin rupoo` 全绿（81）+ `cargo test -p rupoo --lib` 全绿（273）+ `cargo build --bin rupoo` 零警告。

---

## 6. 建议增强（非本期，设计阶段列出供决策）

### 6.1 预算绑定真实模型窗口（推荐，性价比高）
- 新增"从 provider/model 配置读取上下文窗口大小"的入口（目前缺失），指示器分母改为"真实窗口 − system 提示 − 输出上限"，比写死 60000 更有意义。
- 同时可作为 `trim_by_token_budget` 的真实上限，根除"小模型被预算撑爆"的隐患。

### 6.2 语义压缩 `/compact`（大改动，谨慎）
- 把最旧 N 轮用 LLM 总结成一条 system 摘要，替代机械丢/截，从根本上解决"关键信息被截断"。
- 风险高（需额外 LLM 调用、可能总结失真），应独立设计、独立测试，不在本期范围。

### 6.3 运行时管理指令（小改动）
- `/context`：打印分区明细（history 占用 / environment / memory / 轮次），让用户诊断为何快满。
- 可将 `DEFAULT_TOKEN_BUDGET` / `HISTORY_DEFAULT_MAX_TURNS` 改为可运行时覆盖（需从常量提升为 `App` 字段）。

---

## 7. 风险与开放问题

| 风险 / 问题 | 说明 | 处置 |
|---|---|---|
| 指标滞后 | `estimated_tokens` 基于字符/2 粗估，非真实 tokenizer 计数 | 可接受；仅作"趋势/占用"提示，不用于硬截断决策 |
| 分母失真 | 60000 非真实模型窗口 | §6.1 增强后修正；本期明确标注"相对预算"语义 |
| 状态条拥挤 | 模型名 + 阶段 + 提示轮播 + 新指示器可能挤 | 指示器用极简 `Cxt NN%`，优先放 footer；状态条仅超阈值才强化 |
| 双路径不一致 | CLI 用 `ConversationHistory`，agent 用 `ConversationContext` | 本期只覆盖 CLI 路径（用户可见面）；agent 路径若需可后续对称加 |

---

## 8. 实施阶段与状态（供决策）

- **Phase A = 已落地（2026-07-16，方案 S2）**：`ChatView.ctx_usage_pct: Option<u8>` + `AgentToTui::ContextUsage` 事件（bridge `send_idle()` 封装，Idle 前发出）+ `StreamItem::TokenStat` 结构化 + token 页脚内联 `Cxt ▰▰▱▱▱ NN%` 仪表 + 分色（绿/琥珀/红 + ⚠）+ 4 个新增单元测试 + 1 个渲染快照测试。纯展示、零行为变更、零新增布局行。
- **Phase B（已落地，2026-07-17）**：占用率 >85% 时在聊天流内联推一条柔和 `StreamItem::ContextHint`（`⚠ 上下文接近上限，建议 /clear 或把关键信息存入 /memory`），琥珀色降亮、非模态、复用内联块样式、零新增布局行。`push_token_footer` 在页脚后判断 `ctx_usage_pct > CONTEXT_WARN_THRESHOLD(85)` 触发；渲染臂加在 `to_lines`。新增 3 单测（超阈推送/低阈不推送/渲染可见）。
- **Phase C（已落地，2026-07-17）**：§6.3 `/context` 指令——`handle_context` 经 `build_context_report` 输出诊断（估算占用 vs 软预算 %、轮次消息数、模型名、模型窗口查表值、管理提示），单测 `build_context_report_contains_sections`。§6.1 模型窗口——新增 `model_context_window(label)` 静态查表（claude/gpt-4o/gemini/deepseek/qwen 等，大小写不敏感子串匹配，未知返回 None），单测 `model_context_window_known_and_unknown`；该值在 `/context` 中以「模型窗口(查表)」**信息性展示**。**关键决策**：为稳住 Phase A/B 仪表语义、不引入单位错配（估算=字符/2 vs 真实 token 窗口），**未把仪表分母改为真实窗口**——仪表仍相对软预算；真实窗口仅作 `/context` 参考。如需真正"分母=真实窗口"，属后续增强，需先统一估算口径。

> 每个 Phase 独立可验证、可回退；任一步若无法一次做对 + 测试守住，则停下保持现状（纯终端/现有渲染），不强行打补丁。
