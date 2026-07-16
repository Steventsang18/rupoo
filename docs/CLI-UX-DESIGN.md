# rupoo CLI 人文关怀体验 —— 技术设计文档

> 状态：Step 0 启动中（设计文档 + ratatui 单栏渲染引擎 + 快照测试已落地）。
> 原则：**表面极简（类 Claude Code 单栏聊天流），内里充满人文关怀**；同时遵守质量红线——绝不屎山、绝不"上线后反复修"，做不到一次做对就保持纯终端。

---

## 1. 设计原则

### 1.1 人文（内里）
- **透明**：让用户看见 agent 在想什么（思考块）、在做什么（工具行）。
- **陪伴**：阶段感（理解→规划→行动→验证）、可打断、可插话、取消后温柔总结。
- **尊重**：柔和配色、低闪烁、克制动画、无障碍主题、错误三段式、写操作显式确认。

### 1.2 质量红线（硬约束）
- 设计先行、行为等价替换优先、必须有 TUI 快照测试守护、每步可回退、优先成熟库（ratatui）、不行就停保持纯终端。

---

## 2. 现有基础盘点（关键）

| 能力 | 现状 | 位置 |
|---|---|---|
| 思考事件 | `AgentToTui::Thinking` / `ThinkingSummary` | `shared.rs:156` |
| 工具状态 | `AgentToTui::ToolStatus { tool_name, phase }` / `RequestApproval` | `shared.rs` |
| 阶段进度 | `AgentToTui::PhaseProgress` / `StepProgress` / `LlmStatus` | `shared.rs` |
| 工具渲染函数 | `tool_call_start/end`、`tool_result` | `output.rs` |
| 思考渲染函数 | `thinking_spinner`、`thinking_summary`、`clear_thinking_summary` | `output.rs` |
| 阶段渲染函数 | `phase_progress` | `output.rs` |
| 输入/主循环 | `ReplSession::run` / `run_loop` / `handle_input` / `handle_agent_event` | `mod.rs` |

**结论**：人文**内容**已基本具备，瓶颈在**渲染层**——当前是裸 ANSI + 逐行追加（raw mode，无 alternate screen），脆弱且无法做精致呈现。迁移到 ratatui 单栏即可释放这些现成事件的价值。

---

## 3. 架构总览

```
backend (tokio task)
   │  AgentToTui 事件（已有，扩展少量变体）
   ▼
tokio::sync::mpsc::Sender<AgentEvent>
   │
   ▼
UI 主循环：select! { rx.recv() , event::read() }
   │  更新 ChatView（唯一可变状态）
   ▼
render_frame(&mut Frame, &ChatView)   ← ratatui 保留模式，每帧从 buffer 重绘
```

- **单栏布局**：顶部一行状态条 + 中部向下生长的聊天流 + 底部一行输入。思考/工具/阶段都是流内联元素，不是独立面板。
- **ratatui 提供**：保留模式 + 每帧重绘 + 绝对布局 → 等于标准"全屏重绘"，且免费获得多行/滚动/无障碍，从架构上杜绝裸 ANSI 的屏幕错乱。

---

## 4. 数据模型

```rust
enum StreamItem {
    User(String),
    Assistant(String),
    Thinking { id, text, collapsed },
    Tool { id, name, args, state: ToolState },
    Phase(Phase),
    System(String),
    Error(String),
}
enum ToolState { Running, Done(f64), Failed }
enum Phase { Idle, Understanding, Planning, Acting, Verifying }
struct ChatView { items: Vec<StreamItem>, input: String, phase: Phase, follow: bool }
```

### 4.1 `AgentToTui` → `StreamItem` 映射

| AgentToTui 变体 | StreamItem |
|---|---|
| `Message(User)` | `User` |
| `Message(Assistant)` / `StreamChunk` | `Assistant`（流式追加） |
| `Thinking` / `ThinkingSummary` | `Thinking` |
| `ToolStatus { tool_name, phase }` | `Tool`（Running/Done/Failed） |
| `PhaseProgress` / `StepProgress` | `Phase` 提示行 + 顶部状态条 |
| `Message(System)` | `System` |
| `Message(Error)` / 错误 | `Error` |

---

## 5. 渲染设计（各元素视觉）

- **User**：`> 文本`（青色加粗）。
- **Assistant**：普通文本（后续接 termimad 做 Markdown 精致渲染）。
- **Thinking**：`✶ 文本` 首行 + `│ 续行`，斜体 + 低饱和灰；长思考可折叠为 `✶ thinking…`。
- **Tool**：`⏺ name(args) …`（黄，进行中）→ `⏺ name(args) ✓ done (0.3s)`（绿，完成）→ `✗ name failed`（红）。
- **Phase**：顶部状态条 `rupoo ● understanding`（青）；流内轻量 `— verifying —` 提示行。
- **System/Error**：蓝 / 红，分别呈现。
- **输入行**：`> {input}`，流式时仍可输入插话。

---

## 6. 并发与 Rust 特性

- 后端 tokio task 经 `mpsc::Sender<AgentEvent>` 推事件；UI `rx.recv().await` 消费 → 所有权转移 + 消息传递，无共享可变状态，天然线程安全。
- `select!` 同时 `rx.recv()` 与 `event::read()` → 非阻塞 UI、可插话/取消。
- `enum` 状态机强类型（`Phase`/`ToolState`/`StreamItem`）驱动视觉，编译期防错。
- ratatui `Style` 类型安全 ANSI → 杜绝裸转义。

---

## 7. 分阶段实施

- **Step 0（进行中）**：设计文档 + `src/cli/tui_view.rs` 渲染引擎（ChatView/StreamItem/render_frame）+ 快照测试。本轮已完成。下一步：接入主循环，等价替换裸 ANSI。
- **Step 1**：把 `handle_agent_event` 的 `StreamChunk`/`Thinking`/`ToolStatus`/`PhaseProgress` 分支桥接到 `ChatView`，每帧 `render_frame`。
- **Step 2**：输入行接 `reedline`（历史/补全/多行/插话），Esc 取消 → `Interrupt` → 温柔总结。
- **Step 3**：Markdown 精致渲染（termimad 或 ratatui Paragraph + markdown.rs 解析）。
- **Step 4**：主题/无障碍/动画打磨、权限确认面板。

每步独立可验证、可回退；每步带测试。

---

## 8. 测试策略（回归守护）

- ratatui `TestBackend` + 快照断言：`render_frame` 在固定尺寸下，buffer 内容应包含关键文本（用户/思考/工具/助手/错误）。
- follow-to-bottom：长流时最新行可见、最旧行滚出。
- 每个 Step 后跑 `cargo test --lib tui_view`，确保渲染不回归。
- 历史坑（裸 `\r\x1b[2K` 相对擦行导致列表错乱/闪退）由 ratatui 保留模式从架构上消除，无需手工守卫。

---

## 9. 风险与回退

- 若任一 Step 风险不可控或无法一次做对 + 测试守住 → **停下，保持当前稳定的纯终端逐行输出**（已回退到该安全状态），不强行打补丁。
- ratatui 引入为纯新增依赖，不改动现有逻辑直至 Step 1 接入；接入前引擎与测试独立存在、零回归。

---

## 10. 实施进度（Implementation Log）

### Step 0 ✅ 已完成（2026-07-14，第一轮）
- `docs/CLI-UX-DESIGN.md` 设计文档。
- `Cargo.toml` 新增 `ratatui = "0.29"`。
- `src-agent/src/cli/tui_view.rs`：`ChatView` / `StreamItem` / `ToolState` / `Phase` / `render_frame`（纯函数，3 行布局，follow 自动滚底）。**全程 ratatui 保留模式 buffer，无裸 ANSI**。
- `mod.rs` 声明 `pub mod tui_view;`。
- 2 个 `TestBackend` 快照测试通过：`renders_core_stream_items`、`follow_scrolls_to_newest`。
- 运行时路径**零改动**，默认纯终端行为完全不变。

### Step 1 ✅ 已完成（2026-07-14，第二轮）
- 在 `tui_view.rs` 新增**纯函数 reducer** `apply_event(&mut ChatView, &AgentToTui) -> ApplyOutcome`：
  - `StreamChunk` → 累积到 `pending_assistant`，`Message(Assistant)`/`Idle` 时 finalize 成 `Assistant` 项。
  - `Thinking` / `ThinkingSummary` → 内联斜体思考块 + 状态条 `Understanding`。
  - `ToolStatus(Calling/Completed)` → 内联工具行 `Running→Done`，靠 `open_tool_id` 就地升级同一行。
  - `PhaseProgress` → 状态条 `Acting` + `phase_detail`（如 "refactor 62%"）。
  - `Message(User/System/Error)` → 用户行 / 系统行 / 错误行；工具噪声（🔧/✅/⠋）抑制。
  - `Idle` → finalize + 复位 + 返回 `GenerationComplete`。
- `ChatView` 扩展：`phase_detail` / `pending_assistant` / `open_tool_id` / `next_id`。
- 渲染细节：工具行空 args 不显示 `()`；`Done(0.0)` 显示纯 "✓ done"；状态条展示 `phase_detail`。
- **9 个测试全绿**，含端到端 `full_turn_produces_humanistic_stream`（喂入真实事件序列，断言精确流：User→Thinking→Tool(Done)→Assistant）。
- 运行时路径**仍零改动**。reducer 已是 Step 2 配线的「大脑」，可直接被主循环调用。

### Step 2 ✅ 已完成（2026-07-15）
- 新增 `src-agent/src/cli/tui_run.rs`：`run_ratatui`（alternate screen + raw mode，退出必还原）/ `run_ratatui_terminal<B>`（泛型 Backend，核心循环）/ `pump_agent_event`（单事件→reducer+重绘，可独立测试）/ `on_agent_event_tui`（事件→`apply_event` + 必要状态副作用：history/persist/thinking/idle/token/tool/审批提示）/ `handle_key_tui`（内联输入编辑器：Enter/Ctrl+C/D/Backspace/方向/Home/End/上下历史/Tab 补全/字符）/ `handle_approval_tui`（ratatui 内联审批 y/n/a）。
- **门控（历史）**：初版 `run_tui_with_agent` 仅在 `RUPOO_TUI=1` 时切 `render_mode=Ratatui`（默认纯终端零改动）；**2026-07-15 已切默认**（见 Step 3.2），现 `RUPOO_TUI=0/false/off/no` 退回纯终端。
- 渲染全走 `apply_event`+`render_frame`；`ChatView` 加 `cursor` 字段并在 `render_frame` 用 `set_cursor_position` 定位输入光标。
- 默认路径最小改动：`RupooApp` 加 `RenderMode`（默认 Terminal）；`ReplSession` 加 `chat_view` 字段；用户回声在 tui 模式改为推 `StreamItem::User`（4 处 gated）；`complete_input` 去 `&self`（避借用冲突）；bridge 回显的 `Message(User)` 在 `on_agent_event_tui` 跳过避免重复。
- **非交互冒烟测试**（`TestBackend` 驱动 `pump_agent_event` 真实终端类型）：`ratatui_smoke_renders_full_turn` + `ratatui_user_message_not_duplicated`。`cargo test --bin rupoo` 全绿（40 测试），默认路径无回归。
- **上线前提（已满足）**：live 终端循环本环境无法交互验证，需人工 `RUPOO_TUI=1` 实跑确认无闪烁/光标/resize 问题——**2026-07-15 已实跑通过**（滚动/折行/resize + 中断/取消 + 对话式审批兼容），已切默认（Step 3.2）。

### Step 3 ✅ 已完成（2026-07-15）
- 目标：slash 命令输出在 ratatui 模式下**内联且不闪烁**（`emit` 已在 Step 2 把命令输出灌入 `chat_view`，但引入时**编译器未过**——`emit(&mut self)` 在仍持有 `&self.app` 不可变借用的循环里被调用，造成 5 处 `E0502` 借用冲突；本步修复）。
- `tui_view.rs`：新增 `StreamItem::Command(String)` 变体，渲染为**中性朴素文本**（区别于蓝色 agent `System` 行 / 红色 `Error` 行），命令输出（/help、/tools、/sessions 等）读起来更像原生终端文本、又不与系统/错误混淆。
- `mod.rs`：
  - `ReplSession::emit` 在 ratatui 模式改为推 `StreamItem::Command`（`strip_ansi` 后）。
  - 修复 `show_history` / `list_sessions` 的借用冲突——先把待输出行收集进**自有 `Vec<String>`**，释放对 `self.app` 的不可变借用，再逐行 `emit`。这是「命令输出内联」真正可编译、可运行的关键修复。
  - `handle_command` / `show_available_tools` / `list_sessions` / `show_history` / `handle_alias` / `handle_memory` / theme / model / plan 等所有命令输出均经 `emit` 单一出口，ratatui 模式零裸 `println!`。
- **回归测试**（TUI 快照守护质量红线）：新增 `ratatui_command_output_inline_no_flicker`——用真实命令路径（`process_input("/help")` → `handle_command` → `emit`）驱动 ratatui 管线，断言 (a) `/help` 的 `Commands:` / `Quick Actions:` / `/tools` 出现在 buffer 内联；(b) **二次重绘后仍在**（证明活在保留模式 `chat_view`，非裸 stdout 瞬写 → 不闪烁）；(c) `/tools` 的 `Available Tools:` / `file_read` 同样内联。该测试锁死「任何人把命令输出改回 `println!` 就会闪烁/丢失」的回归。
- 关于「reedline 输入」：本步评估后**不引入 reedline**。`handle_key_tui` 已实现 reedline 等价能力（历史/补全/多行光标/中断），且无新增依赖风险；强行换库属无谓返工，违背质量红线「一次做对、不先上再修」。保留自研内联编辑器。
- 现状：`cargo test --bin rupoo` 全绿（40）+ `cargo test --lib` 全绿（259），纯终端回退路径零回归。ratatui 仍门控 `RUPOO_TUI=1`（切默认见 Step 3.2）。

### Step 3.1 ✅ 实跑反馈修复（2026-07-15）
- 人工 `RUPOO_TUI=1` 实跑反馈两批真实 bug，按质量红线「一次做对」修复。
- **第一批**（窗口满后不能翻看 + resize 不自适应）：
  1. 根因 `render_frame` 的 `offset` 永远算成 `total-height`（强制贴底），`ChatView` 无滚动状态、也无滚动按键。`ChatView` 新增 `scroll: u16` + `height: u16`；`follow` 默认改 `true`；`render_frame` 改「follow 贴底 / 否则 `scroll` 偏移」，贴底同步 `scroll=max_scroll`、手动滚到底恢复 `follow`；`handle_key_tui` 加 `PageUp`/`PageDown` 整页翻。
  2. `Resize` 分支为空导致不重绘 → 改为立即 `draw_frame`。
- **第二批**（仍不能回看历史 + 长回复不折行）：
  1. 滚动按键易用性：用户更直觉用 `↑`/`↓` 滚动，但彼时被历史召回占用且 `PageUp/Down` 在其终端未生效。改为 **`↑`/`↓` = 滚动流（细粒度，1/3 页）**，**`PageUp`/`PageDown` = 整页**；历史召回移到 **`Ctrl+P` / `Ctrl+N`**（readline 惯例，无新增依赖）。
  2. 长回复不折行：根因 `Paragraph` 默认不 `wrap`（超宽截断）。修复：启用 `Paragraph::wrap(Wrap { trim: true })`。折行后一行文字占多行，滚动偏移须按**视觉行**计（`wrap_rows()` 按 ratatui 的 `Wrap{trim:true}` 词折行逐字宽统计视觉行数，贴底时 `scroll` 同步为 `max_scroll`），否则滚动错位/底部留白。
- **回归测试**：`tui_view` 单测 `manual_scroll_pauses_follow_and_shows_older_lines` + `long_line_wraps_without_truncation`（折行不截断，尾部可见）；bin 测试 `pageup_scrolls_up_and_pauses_follow` + `resize_repaints_and_keeps_newest`。`follow_scrolls_to_newest` 仍守护贴底。`long_line_wraps_without_truncation` 锁死「折行被悄悄关掉/截断」的回归。
- 现状：`cargo test --bin rupoo` 全绿（44，+4）+ `cargo test --lib` 全绿（259）。仍门控 `RUPOO_TUI=1`（切默认见 Step 3.2）。
- 剩余人工验收（已全部通过 2026-07-15）：live 终端实跑——滚动 `↑/↓`+`PageUp/Down` 回看 ✅、`resize` 自适应 ✅、长回复折行 ✅；第 4 步清单「中断/取消不崩」✅（Ctrl-C 温柔取消 / Esc 补绑温柔取消 / Ctrl-D 退出）、「审批提示交互」✅（对话式 1/2/3 在 ratatui 下兼容；写文件走 LLM 对话式确认而非 y/n/a 弹窗，用户决策**保持对话式不统一**）。已切默认（Step 3.2）。

### Step 3.2 ✅ 切默认（2026-07-15）
- 决策：用户确认对话式审批保持现状 + 中断/取消 live 通过 → 将 ratatui 提升为**默认 REPL 渲染**（A 路径：先验后切默认）。
- `app.rs`：`RenderMode` 默认由 `Terminal` 改为 `Ratatui`（仅翻转 `#[default]`，枚举/逻辑零改动）。
- `mod.rs`：`run_tui_with_agent` 翻转分发——默认 `render_mode=Ratatui` 调 `run_ratatui()`；仅当 `RUPOO_TUI=0/false/off/no` 退回 `session.run()`（纯终端）。**纯终端路径完整保留为逃生舱**，隔离不变，绝不删回退。
- `tui_run.rs`：模块注释由「strictly opt-in」改为「now the default」，并说明 `RUPOO_TUI=0/false/off/no` 退回纯终端。
- 质量红线守护：两条路径完全隔离；纯终端代码一行未动，回归测试（bin 44 + lib 259）仍全绿，纯终端回退路径无回归风险。
- 行为变化：直接 `cargo run --bin rupoo`（无需 `RUPOO_TUI=1`）即进入 ratatui 人文陪伴界面；遇终端不兼容可 `RUPOO_TUI=0 rupoo` 退回老界面。

### Step 4 ✅ 状态条 tagline + token 页脚 + 首次启动使用指南（2026-07-15）
- 用户三项需求：①状态条加简短 Tagline；②AI 输出完成后附带 token 消耗统计；③首次启动弹「使用指南」窗口（含 `/` 命令列表、模型增删切换、飞书等渠道添加），提供「关闭使用指南提醒」一键关闭后续弹出。
- 状态条（`render_frame`）：1 行横向分块——左 `rupoo · <TAGLINE>`（暗灰，`TAGLINE="Your Trusted Sidekick"`），右 `model_label`(青色粗体)+phase(青/暗灰) 右对齐（`Constraint::Length(right.width())`）。`LlmStatus` 同步 `chat_view.model_label`。
- token 页脚：`StreamItem::TokenStat` 变体（暗灰）；`ChatView` 加 `assistant_emitted`（`finalize_assistant` 置位、`Thinking` 重置），`Idle` 时 `push_token_footer(token_in, token_out)` 仅当本轮有 assistant 输出才追加 `⏱ X in · Y out`（`fmt_tokens` 紧凑格式化）。
- 首次启动指南：`ChatView.guide: Option<GuideOverlay>`；`run_ratatui_terminal` 启动前读 `guide_dismissed`（settings KV，新增键），未设置则弹居中模态，含三大板块（`guide_content()`，文案取自真实代码：`/model <prov> [model]`、`rupoo config set api_key.<p>`、`rupoo feishu`/`rupoo channel add feishu`、`rupoo serve`）；footer 固定底行始终可见，`D` 切换「不再显示」、`Enter/Esc` 关闭（`close_guide` 勾选则持久化 `guide_dismissed=true`）。窄终端 footer 固定、body 可滚。
- 纯终端路径零改动；`cargo test --bin rupoo` 48 + `cargo test -p rupoo --lib` 259 全绿。
- 经验：ratatui buffer 全角 CJK 占 2 cell（次格空格占位），快照测试断言前 `replace(' ','')`；本版本 `Title` 不在 `ratatui::widgets`（用 `Line` + `.alignment` 居中）。

### Step 4.1 ✅ banner 模型名修复 + 累计/每轮 token + 每轮耗时（2026-07-15）
- 用户三项 UX 优化：①累计 tokens 统计与显示；②修复 banner 模型名显示异常（错误 "no model" → 实际模型）；③每轮对话结束显示耗时。
- **Bug 2 根因**：`ReplSession::new` 只给 `app.model_label` 赋了真实模型，`chat_view.model_label` 始终为空 → banner 走空分支显示 "no model"。修复：构造时同步 `chat_view.model_label = model_label`（model_label 来自 `run_tui_with_agent` 解析的 `provider/model`，启动时即正确）；`LlmStatus` 仍会在 `/model` 切换时更新它。
- **Feature 1 累计 token**：`ChatView` 新增 `token_in_total/out_total`（在 `apply_event` 的 `TokenUpdate` 分支累加，display 侧镜像 `app` 计数器）+ `turn_in_start/out_start` 基线（`Thinking` 分支捕获）。footer 格式：`⏱ {s}s · +{本轮in} in / +{本轮out} out · Σ{累计in} / Σ{累计out}`（暗灰 `TokenStat`），同时呈现本轮增量与累计。
- **Feature 3 每轮耗时**：ratatui 路径 `gen_start` 此前从未被设为 `Some`（只在 Idle 置 None）→ 时长未跟踪。修复：`handle_key_tui` 的 Enter 提交分支设 `gen_start = Some(Instant::now())`；`Idle` 时 `duration = gen_start.elapsed()` 传入 `push_token_footer`（纯终端路径本就显示 `⏱ {s}s`，未受影响）。
- 样式统一：footer 沿用暗灰 `TokenStat`；banner 模型名青色粗体（同 Step 4）；状态条/token 页脚/指南三处视觉一致。
- 测试：`push_token_footer` 签名改为 `(duration_secs: f64)`，更新 `token_footer_pushed_after_reply` + `token_footer_skipped_when_no_reply`、新增 `token_footer_shows_per_turn_delta`（验证 +700 增量与 Σ 累计）。`cargo test --bin rupoo` 49 + `cargo test -p rupoo --lib` 259 全绿。
