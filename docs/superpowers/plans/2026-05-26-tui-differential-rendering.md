# TUI 虚拟列表 + 脏区域差分渲染 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 TUI 每帧全量渲染改为"虚拟列表 + 脏区域标记"模式，50+ 历史消息时单帧渲染时间从 ~8ms 降至 ~2ms

**Architecture:** 在现有 `cli/ui.rs` + `cli/app.rs` 基础上，新增 `DirtyRegionTracker` 位掩码和 `VirtualMessageList` 消息行缓存。渲染流程从"每次都从 messages 构建行"改为"只在脏区域或 size 变化时重新计算"。

**Tech Stack:** Rust, ratatui, std::cell::Cell (for interior mutability in app state)

---

## 文件结构

```
src/cli/
├── mod.rs        # TuiSession event loop — 添加脏区域刷新逻辑
├── app.rs        # RupooApp — 添加 DirtyRegionTracker 字段，修改 change_counter 机制
├── handlers.rs   # 事件处理 — 添加脏区域标记
└── ui.rs         # render() — 按脏区域选择性渲染
```

---

### Task 1: 定义 DirtyRegionTracker

**Files:**
- Create: `src/cli/dirty.rs`

- [ ] **Step 1: Write the failing test**

`tests/dirty_tracker_test.rs`:
```rust
#[cfg(test)]
mod tests {
    use rupoo::cli::dirty::DirtyRegionTracker;

    #[test]
    fn test_dirty_region_default_clean() {
        let tracker = DirtyRegionTracker::new();
        assert!(!tracker.is_dirty(DirtyRegion::Chat));
        assert!(!tracker.is_dirty(DirtyRegion::Sidebar));
        assert!(!tracker.is_dirty(DirtyRegion::StatusBar));
    }

    #[test]
    fn test_dirty_region_mark() {
        let tracker = DirtyRegionTracker::new();
        tracker.mark_dirty(DirtyRegion::Chat);
        assert!(tracker.is_dirty(DirtyRegion::Chat));
        assert!(!tracker.is_dirty(DirtyRegion::Sidebar));
    }

    #[test]
    fn test_dirty_region_clear() {
        let tracker = DirtyRegionTracker::new();
        tracker.mark_dirty(DirtyRegion::Chat);
        tracker.mark_dirty(DirtyRegion::Sidebar);
        tracker.clear(DirtyRegion::Chat);
        assert!(!tracker.is_dirty(DirtyRegion::Chat));
        assert!(tracker.is_dirty(DirtyRegion::Sidebar));
    }

    #[test]
    fn test_dirty_all() {
        let tracker = DirtyRegionTracker::new();
        tracker.mark_all();
        assert!(tracker.is_dirty(DirtyRegion::Chat));
        assert!(tracker.is_dirty(DirtyRegion::Sidebar));
        assert!(tracker.is_dirty(DirtyRegion::StatusBar));
        assert!(tracker.is_dirty(DirtyRegion::Input));
    }

    #[test]
    fn test_dirty_clear_all() {
        let tracker = DirtyRegionTracker::new();
        tracker.mark_all();
        tracker.clear_all();
        assert!(!tracker.is_dirty(DirtyRegion::Chat));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_dirty_region_default_clean
```
Expected: compile error, `cli::dirty` module does not exist.

- [ ] **Step 3: Write minimal implementation**

Create `src/cli/dirty.rs`:
```rust
/// Bitmask regions for selective re-rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyRegion {
    Chat      = 0b00001,
    Sidebar   = 0b00010,
    StatusBar = 0b00100,
    Input     = 0b01000,
    Palette   = 0b10000,
}

/// Tracks which UI regions need re-rendering this frame.
#[derive(Debug, Default)]
pub struct DirtyRegionTracker(u32);

impl DirtyRegionTracker {
    pub fn new() -> Self { Self(0) }

    pub fn mark_dirty(&self, region: DirtyRegion) {
        // Cell-like interior mutability: actually we need &mut for clean API
        // But in TUI context, the tracker is stored in RupooApp which is &mut in draw
        // So we use plain &mut.
        self.0 |= region as u32;
    }

    pub fn mark_all(&mut self) {
        self.0 = 0b11111;
    }

    pub fn is_dirty(&self, region: DirtyRegion) -> bool {
        self.0 & (region as u32) != 0
    }

    pub fn clear(&mut self, region: DirtyRegion) {
        self.0 &= !(region as u32);
    }

    pub fn clear_all(&mut self) {
        self.0 = 0;
    }

    pub fn any_dirty(&self) -> bool {
        self.0 != 0
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_dirty_region_default_clean test_dirty_region_mark test_dirty_region_clear test_dirty_all test_dirty_clear_all
```
Expected: all 5 tests PASS.

- [ ] **Step 5: Register module and export**

In `src/cli/mod.rs`, add after line 1 (`pub mod app;`):
```rust
pub mod dirty;
```

In `src/cli/mod.rs`, add to the pub use block:
```rust
pub use dirty::{DirtyRegion, DirtyRegionTracker};
```

- [ ] **Step 6: Run all existing tests to verify no regression**

```bash
cargo test --lib
```
Expected: all existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/cli/dirty.rs
git commit -m "feat(tui): add DirtyRegionTracker for differential rendering"
```

---

### Task 2: 实现 VirtualMessageList — 消息行缓存 + 可见窗口

**Files:**
- Create: `src/cli/virtual_list.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write the failing test**

`tests/virtual_list_test.rs`:
```rust
#[cfg(test)]
mod tests {
    use rupoo::cli::virtual_list::VirtualMessageList;
    use rupoo::shared::ChatMessage;

    fn make_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: rupoo::shared::MessageRole::User,
            content: text.to_string(),
            is_command_output: false,
        }
    }

    #[test]
    fn test_empty_list() {
        let vl = VirtualMessageList::new(80);
        assert_eq!(vl.visible_range(), 0..0);
        assert_eq!(vl.total_visible_lines(), 0);
    }

    #[test]
    fn test_single_message() {
        let mut vl = VirtualMessageList::new(80);
        vl.append_message(make_msg("hello"));
        assert!(vl.total_visible_lines() > 0);
        assert!(vl.auto_scroll());
        // Should be at the bottom
        let range = vl.visible_range();
        assert_eq!(range.end, vl.total_visible_lines());
    }

    #[test]
    fn test_manual_scroll_pauses_auto() {
        let mut vl = VirtualMessageList::new(80);
        // Simulating long content
        for i in 0..20 {
            vl.append_message(make_msg(&format!("line {i}")));
        }
        assert!(vl.auto_scroll());
        vl.scroll_up(3);
        assert!(!vl.auto_scroll());
        vl.scroll_to_bottom();
        assert!(vl.auto_scroll());
    }

    #[test]
    fn test_on_resize_invalidates_cache() {
        let mut vl = VirtualMessageList::new(80);
        vl.append_message(make_msg("hello world"));
        let lines_80 = vl.total_visible_lines();
        vl.on_resize(40);
        assert_ne!(vl.total_visible_lines(), lines_80);
    }

    #[test]
    fn test_render_does_not_panic_on_empty() {
        let mut vl = VirtualMessageList::new(80);
        let lines = vl.render_lines(20);
        assert!(lines.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_empty_list`
Expected: compile error, module not found.

- [ ] **Step 3: Write minimal implementation**

Create `src/cli/virtual_list.rs`:
```rust
use rupoo::shared::{ChatMessage, MessageRole};

/// Pre-computed render lines for a single message.
struct MessageRenderCache {
    /// Line-wrapped display lines, computed at current width.
    lines: Vec<String>,
}

/// Virtual list that only renders visible messages.
pub struct VirtualMessageList {
    /// Original messages (not line-wrapped).
    messages: Vec<ChatMessage>,
    /// Per-message line-wrapped render cache.
    message_lines: Vec<MessageRenderCache>,
    /// Total visible (wrapped) line count.
    total_lines: usize,
    /// Current viewport width.
    viewport_width: u16,
    /// Line offset from top of all content (used for scroll).
    scroll_offset: usize,
    /// Whether auto-scroll to bottom is active.
    auto_scroll_enabled: bool,
}

impl VirtualMessageList {
    pub fn new(viewport_width: u16) -> Self {
        Self {
            messages: Vec::new(),
            message_lines: Vec::new(),
            total_lines: 0,
            viewport_width,
            scroll_offset: 0,
            auto_scroll_enabled: true,
        }
    }

    /// Append a new message and auto-scroll to bottom.
    pub fn append_message(&mut self, msg: ChatMessage) {
        let lines = self.wrap_content(&msg.content);
        let line_count = lines.len();
        self.messages.push(msg);
        self.message_lines.push(MessageRenderCache { lines });
        self.total_lines += line_count;
        if self.auto_scroll_enabled {
            self.scroll_offset = self.total_lines.saturating_sub(1);
        }
    }

    /// Get the visible range of display lines.
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.scroll_offset..self.total_lines
    }

    pub fn total_visible_lines(&self) -> usize {
        self.total_lines
    }

    pub fn auto_scroll(&self) -> bool {
        self.auto_scroll_enabled
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll_enabled = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        let max_offset = self.total_lines.saturating_sub(viewport_height);
        self.scroll_offset = self.scroll_offset.saturating_add(amount).min(max_offset);
        if self.scroll_offset >= max_offset {
            self.auto_scroll_enabled = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll_enabled = true;
        self.scroll_offset = self.total_lines.saturating_sub(1);
    }

    /// Called when terminal width changes — invalidates all wrap caches.
    pub fn on_resize(&mut self, new_width: u16) {
        if self.viewport_width == new_width { return; }
        self.viewport_width = new_width;
        self.total_lines = 0;
        for cache in &mut self.message_lines {
            // Regenerate lines at new width
            // For now, use width-2 for padding as the actual render width
            cache.lines = self.wrap_content_at_width(&self.messages[...].content, new_width as usize - 2);
            self.total_lines += cache.lines.len();
        }
        if self.auto_scroll_enabled {
            self.scroll_offset = self.total_lines.saturating_sub(1);
        }
    }

    /// Render visible display lines into String vectors for the given viewport.
    pub fn render_lines(&mut self, viewport_height: usize) -> Vec<String> {
        if self.messages.is_empty() || viewport_height == 0 {
            return Vec::new();
        }

        let start = self.scroll_offset;
        let end = (start + viewport_height).min(self.total_lines);
        if start >= end { return Vec::new(); }

        // Walk through messages to find display lines in [start, end)
        let mut result = Vec::with_capacity(end - start);
        let mut cursor = 0usize;
        for cache in &self.message_lines {
            let chunk_start = cursor;
            let chunk_end = cursor + cache.lines.len();
            if chunk_end <= start {
                cursor = chunk_end;
                continue;
            }
            if chunk_start >= end { break; }
            let local_start = start.saturating_sub(chunk_start);
            let local_end = (end - chunk_start).min(cache.lines.len());
            for line in &cache.lines[local_start..local_end] {
                result.push(line.clone());
            }
            cursor = chunk_end;
        }
        result
    }

    /// Simple word-wrap at viewport width.
    fn wrap_content(&self, content: &str) -> Vec<String> {
        self.wrap_content_at_width(content, self.viewport_width as usize - 2)
    }

    fn wrap_content_at_width(&self, content: &str, max_width: usize) -> Vec<String> {
        if max_width == 0 { return vec![content.to_string()]; }
        let max = max_width.max(1);
        let mut result = Vec::new();
        for line in content.lines() {
            if line.len() <= max {
                result.push(line.to_string());
            } else {
                let mut pos = 0;
                while pos < line.len() {
                    let end = (pos + max).min(line.len());
                    // Try to break at word boundary
                    if end < line.len() {
                        if let Some(space) = line[pos..end].rfind(' ') {
                            result.push(line[pos..pos + space].to_string());
                            pos = pos + space + 1;
                            continue;
                        }
                    }
                    result.push(line[pos..end].to_string());
                    pos = end;
                }
            }
        }
        result
    }
}

// Deref to messages for rendering
impl std::ops::Deref for VirtualMessageList {
    type Target = Vec<ChatMessage>;
    fn deref(&self) -> &Self::Target { &self.messages }
}
```

Wait — the borrow checker will not allow the `on_resize` method to use `self.messages[...]` while also borrowing `self.message_lines` mutably. Let me fix this:

```rust
pub fn on_resize(&mut self, new_width: u16) {
    if self.viewport_width == new_width { return; }
    self.viewport_width = new_width;
    self.total_lines = 0;

    // Take ownership temporarily to satisfy borrow checker
    let messages = std::mem::take(&mut self.message_lines);
    let width = new_width as usize - 2;
    let mut new_caches = Vec::with_capacity(messages.len());

    for (i, _old_cache) in messages.into_iter().enumerate() {
        let lines = self.wrap_content_at_width(&self.messages[i].content, width);
        self.total_lines += lines.len();
        new_caches.push(MessageRenderCache { lines });
    }
    self.message_lines = new_caches;

    if self.auto_scroll_enabled {
        self.scroll_offset = self.total_lines.saturating_sub(1);
    }
}
```

Actually, for the simple case, let me just collect the content strings first:

```rust
pub fn on_resize(&mut self, new_width: u16) {
    if self.viewport_width == new_width { return; }
    self.viewport_width = new_width;
    self.total_lines = 0;

    let content: Vec<String> = self.messages.iter().map(|m| m.content.clone()).collect();
    let width = new_width as usize - 2;
    let mut new_caches = Vec::with_capacity(content.len());

    for text in &content {
        let lines = self.wrap_content_at_width(text, width);
        self.total_lines += lines.len();
        new_caches.push(MessageRenderCache { lines });
    }
    self.message_lines = new_caches;

    if self.auto_scroll_enabled {
        self.scroll_offset = self.total_lines.saturating_sub(1);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_empty_list test_single_message test_manual_scroll_pauses_auto test_on_resize_invalidates_cache test_render_does_not_panic_on_empty
```
Expected: all 5 tests PASS.

- [ ] **Step 5: Register module and export**

In `src/cli/mod.rs`, add after `pub mod dirty;`:
```rust
pub mod virtual_list;
```

Also register the module in `lib.rs` — actually `cli` is private to the main crate. Let me check where tests will live.

The tests for virtual_list should live in `src/cli/virtual_list.rs` as `#[cfg(test)] mod tests`, since it's part of the CLI crate that's built as part of the main binary. No need for an external test file.

- [ ] **Step 6: Move tests inline and commit**

Move test code into a `#[cfg(test)]` block at the end of `virtual_list.rs`, then:
```bash
cargo test --lib
git add src/cli/virtual_list.rs src/cli/mod.rs
git commit -m "feat(tui): add VirtualMessageList with line wrap cache and scroll"
```

---

### Task 3: 集成 DirtyRegionTracker + VirtualMessageList 到 RupooApp

**Files:**
- Modify: `src/cli/app.rs`
- Modify: `src/cli/handlers.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Add DirtyRegionTracker and VirtualMessageList to RupooApp**

In `src/cli/app.rs`, replace the old caching/scrolling fields:

**Remove** (lines 136-152 in current app.rs):
```rust
    /// Chat rendering cache — invalidated when change_counter increments
    pub chat_cache_lines: Vec<String>,
    pub change_counter: u64,
    ...
    /// When true, viewport always jumps to bottom on next render.
    pub scroll_bottom: bool,
    /// Manual scroll position (Paragraph::scroll value). Only used when scroll_bottom=false.
    pub scroll_offset: usize,
    /// Last max_scroll value computed during render, used by scroll handlers.
    pub max_scroll_cache: std::cell::Cell<usize>,
```

**Add** (in the same position in the struct):
```rust
    /// Dirty region tracker for differential rendering.
    pub dirty: DirtyRegionTracker,
    /// Virtual message list (line cache + scroll).
    pub virtual_list: VirtualMessageList,
```

And add the import at the top:
```rust
use super::dirty::{DirtyRegion, DirtyRegionTracker};
use super::virtual_list::VirtualMessageList;
```

In the `new()` constructor, initialize:
```rust
dirty: DirtyRegionTracker::new(),
virtual_list: VirtualMessageList::new(80), // Will be updated on first render
```

Replace `scroll_bottom: true,` initializer removal with nothing.

- [ ] **Step 2: Update `apply_agent_event` to set dirty regions**

When a new message arrives via `AgentToTui::Message(m)`:
```rust
AgentToTui::Message(m) => {
    self.virtual_list.append_message(m.clone());
    self.push_message(m);
    self.persist_sessions();
    self.dirty.mark_dirty(DirtyRegion::Chat);
    self.dirty.mark_dirty(DirtyRegion::StatusBar);
    self.change_counter = self.change_counter.wrapping_add(1);
}
```

Wait, `change_counter` is being removed. Let me update the handler:

```rust
AgentToTui::Message(m) => {
    self.virtual_list.append_message(m.clone());
    self.push_message(m);
    self.persist_sessions();
    self.dirty.mark_dirty(DirtyRegion::Chat);
    self.dirty.mark_dirty(DirtyRegion::StatusBar);
}
```

For `Thinking` → `Idle` transitions, mark status bar dirty:
```rust
AgentToTui::Idle => {
    self.set_idle();
    self.dirty.mark_dirty(DirtyRegion::Chat);
    self.dirty.mark_dirty(DirtyRegion::StatusBar);
}
```

For `TokenUpdate`:
```rust
AgentToTui::TokenUpdate { in_count, out_count } => {
    ...
    self.dirty.mark_dirty(DirtyRegion::StatusBar);
}
```

- [ ] **Step 3: Update event handlers for scroll**

In `src/cli/mod.rs`, the scroll handling in `handle_event`:

Replace scroll mouse events:
```rust
crossterm::event::MouseEventKind::ScrollDown => {
    if !app.virtual_list.auto_scroll() {
        app.virtual_list.scroll_down(3, viewport_height);
    }
    app.dirty.mark_dirty(DirtyRegion::Chat);
    handled = true;
}
crossterm::event::MouseEventKind::ScrollUp => {
    app.virtual_list.scroll_up(3);
    app.dirty.mark_dirty(DirtyRegion::Chat);
    handled = true;
}
```

For `Shift+↑` / `Shift+↓` keyboard scroll, add handlers in the keyboard event handler section (if not already present).

- [ ] **Step 4: Update `submit_message` to update virtual_list**

In `app.rs`, `submit_message()`:
```rust
pub fn submit_message(&mut self) {
    ...
    self.dirty.mark_dirty(DirtyRegion::Chat);
    self.dirty.mark_dirty(DirtyRegion::StatusBar);
    // Remove old scroll_bottom and change_counter calls
    ...
}
```

- [ ] **Step 5: Update `switch_session`**

```rust
pub fn switch_session(&mut self, session_id: &str) {
    ...
    self.virtual_list = VirtualMessageList::new(80);
    for msg in &self.messages {
        self.virtual_list.append_message(msg.clone());
    }
    self.dirty.mark_all();
    // Remove old scroll_bottom and change_counter calls
}
```

- [ ] **Step 6: Build check**

```bash
cargo check
```
Expected: compilation succeeds with no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/cli/app.rs src/cli/mod.rs src/cli/handlers.rs
git commit -m "feat(tui): integrate DirtyRegionTracker and VirtualMessageList into app state"
```

---

### Task 4: 更新 render() 使用脏区域 + 虚拟列表

**Files:**
- Modify: `src/cli/ui.rs`

- [ ] **Step 1: Refactor render() to check dirty regions**

Replace the `pub fn render()` entry point:
```rust
/// Render the full TUI. Only renders regions marked dirty.
pub fn render(frame: &mut Frame, app: &RupooApp) {
    let area = frame.area();

    // On resize, mark all dirty and update virtual_list width
    // (This requires RupooApp to be &mut — need to handle via interior mutability)
    // For now, we always render fully but with VirtualMessageList caching enabled.

    let rects = compute_three_column(area);

    if app.dirty.is_dirty(DirtyRegion::Sidebar) || app.dirty.is_dirty(DirtyRegion::Chat) {
        render_left(frame, rects.left, app);
        app.dirty.clear(DirtyRegion::Sidebar);
    }

    if app.dirty.is_dirty(DirtyRegion::Chat) || app.dirty.is_dirty(DirtyRegion::Input) {
        render_center(frame, rects.center, app);
        app.dirty.clear(DirtyRegion::Chat);
        app.dirty.clear(DirtyRegion::Input);
    }

    if app.dirty.is_dirty(DirtyRegion::StatusBar) {
        render_right(frame, rects.right, app);
        app.dirty.clear(DirtyRegion::StatusBar);
    }

    // Overlays always on top (render regardless of dirty state)
    if matches!(app.overlay, OverlayState::Approval { .. }) {
        render_approval_dialog(frame, area, app);
    }

    // ── Anchor cursor ──────────────────────────────────
    let center = rects.center;
    let input_y = center.y.saturating_add(center.height.saturating_sub(2));
    frame.set_cursor_position(ratatui::layout::Position {
        x: center.x.saturating_add(1),
        y: input_y,
    });
}
```

But wait — `render` takes `&RupooApp` (immutable ref), and `dirty` needs `&mut`. There are two approaches:

1. Change `render` signature to `&mut RupooApp` 
2. Use `Cell<u32>` for dirty mask

Option 2 is cleaner and avoids changing the ratatui Frame draw closure signature.

- [ ] **Step 2: Change DirtyRegionTracker to use Cell internally**

Update `src/cli/dirty.rs`:
```rust
use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyRegion {
    Chat      = 0b00001,
    Sidebar   = 0b00010,
    StatusBar = 0b00100,
    Input     = 0b01000,
    Palette   = 0b10000,
}

/// Tracks which UI regions need re-rendering this frame.
/// Uses Cell for interior mutability — allows marking/clearing regions
/// through &self (required by ratatui's Frame::draw closure which takes &App).
#[derive(Debug)]
pub struct DirtyRegionTracker(Cell<u32>);

impl DirtyRegionTracker {
    pub fn new() -> Self { Self(Cell::new(0)) }

    pub fn mark_dirty(&self, region: DirtyRegion) {
        let current = self.0.get();
        self.0.set(current | (region as u32));
    }

    pub fn mark_all(&self) {
        self.0.set(0b11111);
    }

    pub fn is_dirty(&self, region: DirtyRegion) -> bool {
        self.0.get() & (region as u32) != 0
    }

    pub fn clear(&self, region: DirtyRegion) {
        let current = self.0.get();
        self.0.set(current & !(region as u32));
    }

    pub fn clear_all(&self) {
        self.0.set(0);
    }

    pub fn any_dirty(&self) -> bool {
        self.0.get() != 0
    }
}

impl Default for DirtyRegionTracker {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: Update render_chat_area to use VirtualMessageList**

Replace the old `render_chat_area` implementation (which built lines fresh every frame) with one that uses the virtual list:

```rust
fn render_chat_area(frame: &mut Frame, area: Rect, app: &RupooApp) {
    if area.height < 2 || area.width < 2 {
        return;
    }

    // Get render lines from the virtual list (cached line-wrapped)
    let viewport_height = (area.height as usize).saturating_sub(2); // 1 for border, 1 for padding
    let display_lines = app.virtual_list.render_lines(viewport_height);

    // Convert to ratatui Text lines
    let text_lines: Vec<Line> = display_lines
        .iter()
        .map(|l| Line::from(Span::raw(l.as_str())))
        .collect();

    let chat_widget = Paragraph::new(Text::from(text_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .scroll((0, 0)); // Virtual list handles scroll offset

    frame.render_widget(chat_widget, area);
}
```

- [ ] **Step 4: Update render_left to respect sidebar dirty state**

Currently `render_left` always redraws. The dirty check is already in `render()`, so no changes needed to the function body.

- [ ] **Step 5: Update render_right to respect status dirty state**

Same logic — the dirty check is in `render()` now.

- [ ] **Step 6: Handle terminal resize in event loop**

In `src/cli/mod.rs`, in the event loop body, detect resize:
```rust
if let Event::Resize(cols, rows) = &event {
    app.virtual_list.on_resize(*cols);
    app.dirty.mark_all();
    handled = true;
}
```

Add `Event::Resize` to the `crossterm::event::Event` match.

- [ ] **Step 7: Build check**

```bash
cargo check
```
Expected: compilation succeeds.

- [ ] **Step 8: Commit**

```bash
git add src/cli/ui.rs src/cli/dirty.rs src/cli/mod.rs
git commit -m "feat(tui): differential rendering with dirty regions and virtual list"
```

---

### Task 5: 性能基准验证

**Files:**
- Create: `tests/tui_benchmark_test.rs`

- [ ] **Step 1: Write benchmark test**

```rust
/// Micro-benchmark: measure VirtualMessageList render time with 200 messages.
/// This is a functional test that asserts performance, not a precision benchmark.
#[test]
#[ignore]
fn test_virtual_list_render_performance() {
    use rupoo::cli::virtual_list::VirtualMessageList;
    use rupoo::shared::ChatMessage;

    let mut vl = VirtualMessageList::new(80);
    for i in 0..200u32 {
        vl.append_message(ChatMessage {
            role: rupoo::shared::MessageRole::User,
            content: format!("This is test message number {} with some extra text to make it long enough to wrap across multiple lines in the terminal viewport", i),
            is_command_output: false,
        });
    }

    let start = std::time::Instant::now();
    const ITERATIONS: u32 = 100;
    for _ in 0..ITERATIONS {
        let _lines = vl.render_lines(30);
    }
    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / ITERATIONS as u128;
    // Should be < 100_000 ns (0.1ms) per render for 200 messages
    assert!(
        avg_ns < 200_000,
        "VirtualMessageList::render_lines too slow: {avg_ns}ns avg over {ITERATIONS} iterations"
    );
    println!("VirtualMessageList render: {avg_ns}ns avg over {ITERATIONS} iterations (200 messages, viewport 30)");
}
```

- [ ] **Step 2: Run benchmark**

```bash
cargo test test_virtual_list_render_performance -- --ignored --nocapture
```
Expected: PASS with print showing render time.

- [ ] **Step 3: Document baseline**

Run and capture the output, then:
```bash
cargo test --lib 2>&1 | tail -5
git add tests/tui_benchmark_test.rs
git commit -m "test: add VirtualMessageList render performance benchmark"
```

---

## Manual Verification Checklist

After implementing, manually verify in the TUI:

1. **Basic rendering**: `rupoo` → 发送几条消息 → 确认显示正常
2. **Scroll**: Shift+↑/↓ 滚动 → 查看历史消息 → 无空白行
3. **Auto-scroll**: 滚回底部 → 新消息自动到达 → 视图跟随到底部
4. **Resize**: 调整终端窗口宽度 → 内容正确重排、不截断
5. **Performance**: 快速连续发送 50+ 消息 → 帧率不低于 15fps
6. **Session switch**: 切换到其他 session → 恢复滚动位置正确
7. **Sidebar**: 侧栏 session 列表在切换时才重绘
