# Rupoo 开发红线与人文关怀约束（精简版）

> 给 AI 协作者 / 新成员的「只守方向、不抠细节」速记。详细架构见代码与 `docs/`。

---

## 一、红线（铁律，违反即返工）

1. **渲染架构**：默认 ratatui；纯终端逃生舱（`RUPOO_TUI=0/false/off/no`）代码**一行不动**，两套路径完全隔离。禁止手写裸 ANSI / crossterm raw mode。
2. **状态/渲染分层**：所有状态变更**只走 `apply_event` 纯 reducer**，渲染**只走 `render_frame` 纯函数**。不在渲染里改业务状态。
3. **复验铁律**：改 TUI/CLI 后必须 `cargo install --path src-agent --bin rupoo --force` 重建实测——跑旧二进制会复现"修了没生效"的假象。
4. **质量红线**：CLI/UI 改动**一次性做对、稳定、有测试兜底**；先出方案再行为等价替换；风险不可控就停在纯终端，不搞半吊子。用户反感屎山代码与落地后反复修改。
5. **提交纪律**：不随意 commit；仅大版本级改动才提交并打 tag，日常增量留工作区。
6. **UTF-8 安全**：光标整字符移动、外部文本截断用 `floor_char_boundary`、不改 ratatui 私有 grapheme wrap。

---

## 二、人文关怀设计约束（大方向）

- **安静不喧闹**：底部用单一静态状态词（Idle / Thinking / Reading / Writing / Running / Generating / Reviewing）+ 慢心跳圆点；**禁止闪烁、跳动的过程细节**。
- **工具活动不入主聊天流噪音**：Idle 时折叠为一条 `✓ 完成 N 项任务` 摘要；需查看才用 `Shift+A` / `/activity` 唤起**静态浮层**（非模态、Esc 关）。
- **低饱和暖色**：品牌暖黄 `◉ Rupoo`、模型名降亮、状态词柔和色，避免过度刺眼。
- **可读性优先**：默认 comfortable 密度（轮次间留白、助手正文提亮），可用 `/ui density [compact|comfortable]` 切换。
- **陪伴感**：思考期安抚句、底部 `HINT_TIPS` 轮播、IM 风格布局（用户右对齐）。
- **顶部克制**：Banner 右侧仅留 `model_label`，不堆运行状态长句。
- **温柔提示**：上下文临近上限时用页脚温和提示（`/clear` 或存入 `/memory`），不弹窗、不打断。

---

## 三、验证闭环（一句话）

`cargo build`（零警告）→ 两套 `cargo test` 全绿 → `cargo clippy` 干净 → `cargo install --force` 重建 → `rupoo --version` 实测。
