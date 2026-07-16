//! Ratatui-based single-column "companion" chat view.
//!
//! The rendering engine for the human-friendly CLI experience. Surface is
//! deliberately simple — one downward-growing chat stream plus a thin status
//! bar and an input line — while the warmth lives *inside*: thinking blocks,
//! tool rows and phase hints are rendered inline as part of the same stream,
//! never as separate panels.
//!
//! Design notes (quality red line):
//! - No raw ANSI, no `\r\x1b[2K` cursor hacks. Everything goes through
//!   ratatui's retained-mode buffer, so the screen can never desync.
//! - `ChatView` is the single source of truth; `render_frame` is a pure
//!   function of `&ChatView` and the terminal size.
//!
//! Step 0 lands this engine (state model + pure renderer + snapshot tests)
//! before it is wired into the live event loop, so several variants/fields are
//! constructed only in later steps. `allow(dead_code)` documents that intent
//! and keeps the build warning-free until Step 2 wiring lands.
#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use rupoo::{AgentToTui, MessageRole, ToolPhase};
use std::collections::VecDeque;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Short tagline shown in the top status bar (next to the brand).
pub const TAGLINE: &str = "Your Trusted Sidekick";

/// Brand wordmark casing + a small monochrome glyph that evokes a car steering
/// wheel (the product's "sidekick / driving" motif). Rendered in a single cell
/// with a calm accent so it reads consistently across terminals and never
/// competes with the conversation for attention. (A real steering-wheel emoji
/// would risk 2-cell width + color inconsistency, so we keep it ASCII-safe and
/// easy to swap via this one constant.)
pub const BRAND_GLYPH: &str = "◉";

/// Frame interval (ms) for the status pulse + hint rotation. Driven by a single
/// loop tick so `render_frame` stays a pure function of `&ChatView`.
pub const ANIM_MS: u64 = 120;
/// How often the bottom hint tip rotates to a new suggestion (ms).
pub const HINT_ROTATE_MS: u64 = 6000;

/// Pool of rotating tips / example prompts shown in the bottom hint bar.
pub const HINT_TIPS: &[&str] = &[
    "Tip: type /help to list all commands",
    "Tip: /model switches the LLM mid-session",
    "Tip: connect Feishu / DingTalk via /channel",
    "Try: \"refactor this module for clarity\"",
    "Try: \"explain how this function works\"",
];

/// Soft, low-saturation foreground colors for the user's own text (the live
/// input line and the user's sent messages in the stream). Deliberately calm so
/// the interface never feels loud — pick a scheme via `INPUT_COLOR_INDEX`.
pub const INPUT_COLOR_SCHEMES: [Color; 4] = [
    Color::Rgb(150, 165, 180), // "Slate"  — cool blue-grey
    Color::Rgb(150, 170, 150), // "Sage"   — soft green-grey
    Color::Rgb(180, 165, 145), // "Sand"   — warm sand
    Color::Rgb(165, 155, 180), // "Lilac"  — soft lavender
];
const INPUT_COLOR_INDEX: usize = 0;
/// The active input color scheme (the user's text color).
pub const INPUT_COLOR: Color = INPUT_COLOR_SCHEMES[INPUT_COLOR_INDEX];

/// A short, prominent colored marker placed at the right edge of the user's
/// own messages in the stream. It lets the eye tell "me" from "the assistant"
/// at a glance when scrolling back through history — the chat-bubble
/// equivalent of a sender tag. Kept to a single calm-but-visible accent so it
/// reads as a role cue, not as decoration.
pub const USER_MARKER: &str = "›";
pub const USER_MARKER_COLOR: Color = Color::Rgb(160, 195, 220);

/// Warm accent used for the single word that opens each rotating bottom-tip
/// ("Tip:" / "Try:"). Coloring just that label — instead of the whole line —
/// is the small "detail" that keeps the otherwise-dim bar from feeling cold,
/// without adding any extra instructional text.
pub const HINT_ACCENT_COLOR: Color = Color::Rgb(195, 155, 110);

/// Max entries kept in the bottom status panel's live-activity ring. Bounds
/// memory and the expanded mini-log height regardless of how many tools run.
pub const STATUS_RING_CAP: usize = 50;

/// Human-readable verb for a tool name, shown in the bottom status panel
/// (collapsed summary + expanded mini-log). Local + zero-cost — no paths, no
/// external data. Unknown tools fall back to their raw name.
pub const KIND_LABEL: &[(&str, &str)] = &[
    ("read_file", "读取文件"),
    ("glob", "检索文件"),
    ("grep", "搜索内容"),
    ("web_search", "网络搜索"),
    ("bash", "执行命令"),
    ("write_file", "写入文件"),
    ("edit_file", "编辑文件"),
];

/// Map a tool name to its localized verb (see `KIND_LABEL`).
pub fn kind_label(name: &str) -> String {
    KIND_LABEL
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Breathing-pulse glyphs cycled while the agent is working, to convey live
/// activity without a hard spinner.
const PULSE: &[&str] = &["●", "◍", "○", "◍"];

/// State for the first-launch "使用指南" (getting-started) overlay.
#[derive(Debug, Clone)]
pub struct GuideOverlay {
    /// When true, dismissing the guide also suppresses future auto-popups.
    pub dismiss_checked: bool,
    /// Scroll offset (visual rows) for the guide body.
    pub scroll: u16,
}

/// Lifecycle of a tool invocation shown inline in the stream.
///
/// `Done(secs)` carries the wall-clock duration when known; a value of `0.0`
/// means "unknown" and the renderer prints a plain "✓ done" without seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolState {
    Running,
    Done(f64),
    Failed,
}

/// High-level agent phase, drives the top status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Understanding,
    Planning,
    Acting,
    Verifying,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Understanding => "understanding",
            Phase::Planning => "planning",
            Phase::Acting => "acting",
            Phase::Verifying => "verifying",
        }
    }

    /// Friendly, English status sentence shown next to the pulse while the
    /// agent is busy — reassures the user about what is happening right now
    /// (replaces the terse `label()` in the status bar).
    fn status_text(self) -> &'static str {
        match self {
            Phase::Idle => "",
            Phase::Understanding => "Organizing response…",
            Phase::Planning => "Planning the approach…",
            Phase::Acting => "Searching for relevant information…",
            Phase::Verifying => "Reviewing the answer…",
        }
    }
}

/// One inline element of the conversation stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamItem {
    User(String),
    Assistant(String),
    /// Soft, italic thinking block (collapsible when long).
    Thinking {
        id: usize,
        text: String,
        collapsed: bool,
    },
    /// Inline tool row: `⏺ name(args) … / ✓ done (0.3s)`.
    Tool {
        id: usize,
        name: String,
        args: String,
        state: ToolState,
    },
    /// Lightweight phase hint line.
    Phase(Phase),
    /// Command / info output from slash commands (e.g. `/help`, `/tools`).
    /// Rendered calmly and distinct from agent `System`/`Error` lines so the
    /// humanistic surface stays readable.
    Command(String),
    System(String),
    Error(String),
    /// Subtle per-turn token-usage footer shown after an assistant reply.
    TokenStat(String),
    /// Compact one-line summary of the tool tasks performed in a turn, replacing
    /// the individual inline `⏺ tool … ✓ done` rows so the stream stays focused
    /// on the actual conversation (decision 2026-07-16: filter/collapse redundant
    /// "done" entries). "核心结果或状态摘要" — keeps only the status summary.
    Summary(String),
}

/// One entry in the bottom status panel's live-activity ring. Carries only
/// *what* is running and *its state* — deliberately path-free (per the
/// 2026-07-16 design): the panel stays lightweight and never records file
/// paths or tool content.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusEvent {
    pub name: String,
    pub state: ToolState,
}

/// Whole UI state — the only mutable thing the event loop touches.
#[derive(Debug, Clone)]
pub struct ChatView {
    pub items: Vec<StreamItem>,
    pub input: String,
    pub phase: Phase,
    /// Optional free-form detail shown next to the phase in the status bar
    /// (e.g. "refactor error handling 62%").
    pub phase_detail: Option<String>,
    /// Accumulator for streamed assistant text not yet finalized into an item.
    pub pending_assistant: String,
    /// Id of the tool row currently in `Running` state, if any. Lets a later
    /// `Completed` event upgrade the exact same row instead of appending.
    pub open_tool_id: Option<usize>,
    /// Monotonic id source for inline elements that need a stable key.
    pub next_id: usize,
    /// Auto-scroll to newest content. When true the view pins to the bottom
    /// (newest). Any manual scroll-up clears it; reaching the bottom restores
    /// it. Defaults to `true` so a fresh view always shows the latest line.
    pub follow: bool,
    /// Vertical scroll offset in lines — how many top lines are hidden.
    /// `0` = top of the stream visible; `max_scroll` = newest content visible.
    /// `render_frame` reconciles this with `follow` on every paint.
    pub scroll: u16,
    /// Last painted height of the chat stream area (in lines). Lets
    /// PageUp/PageDown step a full page without re-deriving the layout.
    pub height: u16,
    /// Current model label (e.g. "claude-sonnet-4"), shown in the status bar.
    pub model_label: String,
    /// True once an assistant reply has been produced this turn (used to
    /// decide whether to append a token-usage footer on `Idle`).
    pub assistant_emitted: bool,
    /// First-launch getting-started overlay, if it should be shown.
    pub guide: Option<GuideOverlay>,
    /// Cumulative session token usage (display mirror of the app counters),
    /// surfaced in the footer as the "Σ total".
    pub token_in_total: u64,
    pub token_out_total: u64,
    /// Cursor column within `input` (byte index), for the inline input editor.
    pub cursor: usize,
    /// Monotonic animation frame counter, advanced by the event loop to drive
    /// the status-bar pulse and hint rotation. Reading it keeps `render_frame`
    /// a pure function of `&ChatView`.
    pub anim_frame: u64,
    /// Index into `HINT_TIPS` for the currently shown bottom-bar tip.
    pub hint_index: usize,
    /// Live running-activity ring for the bottom status panel. Holds recent
    /// tool calls (name + state) only — no paths. Capped at `STATUS_RING_CAP`.
    pub status_ring: VecDeque<StatusEvent>,
    /// Whether the bottom status panel is expanded (mini-log) vs collapsed
    /// (single-line summary). Defaults to collapsed.
    pub status_expanded: bool,
    /// Whether the expanded mini-log auto-follows the newest activity (live
    /// scroll). When a workflow run ends (`Idle`) this is cleared so the panel
    /// freezes at the last item instead of keeping scroll-jumping — the user
    /// then scrolls it manually. Any manual ↑/↓ scroll also clears it.
    pub status_follow: bool,
    /// Scroll offset (lines up from the newest) for the expanded mini-log.
    /// `0` = pinned to the newest (bottom) line.
    pub status_scroll: u16,
    /// Cached rect of the panel's right (activity) cell, set each paint; the
    /// mouse handler uses it to detect clicks on the panel.
    pub status_panel_rect: Rect,
}

impl Default for ChatView {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            input: String::new(),
            phase: Phase::Idle,
            phase_detail: None,
            pending_assistant: String::new(),
            open_tool_id: None,
            next_id: 0,
            follow: true,
            scroll: 0,
            height: 0,
            model_label: String::new(),
            assistant_emitted: false,
            guide: None,
            token_in_total: 0,
            token_out_total: 0,
            cursor: 0,
            anim_frame: 0,
            hint_index: 0,
            status_ring: VecDeque::with_capacity(STATUS_RING_CAP),
            status_expanded: false,
            status_follow: true,
            status_scroll: 0,
            status_panel_rect: Rect::default(),
        }
    }
}

impl ChatView {
    /// Record one tool activity into the bottom-panel ring (capped at
    /// `STATUS_RING_CAP`). The ring feeds both the collapsed summary and the
    /// expanded mini-log; it holds no path/content by design.
    pub fn push_status(&mut self, name: String, state: ToolState) {
        if self.status_ring.len() >= STATUS_RING_CAP {
            self.status_ring.pop_front();
        }
        self.status_ring.push_back(StatusEvent { name, state });
    }

    /// Append an assistant message (helper for backfill/tests).
    pub fn push_assistant(&mut self, text: String) {
        self.items.push(StreamItem::Assistant(text));
    }

    /// Allocate the next stable inline id.
    fn next_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Finalize any in-progress streamed assistant text into a concrete item.
    /// Safe to call at any time; a no-op when nothing is pending.
    pub fn finalize_assistant(&mut self) {
        if !self.pending_assistant.is_empty() {
            let text = std::mem::take(&mut self.pending_assistant);
            self.items.push(StreamItem::Assistant(text));
            self.assistant_emitted = true;
        }
    }

    /// Append a usage footer after an assistant reply: the turn wall-clock
    /// `duration_secs` plus the **cumulative session token totals** (Σ = the
    /// running sum across the whole session). The per-turn delta is omitted to
    /// keep the line concise — `Σ in` is total prompt tokens, `Σ out` total
    /// completion tokens. Called by the runtime on `Idle`; no-op when no
    /// assistant reply was produced this turn.
    pub fn push_token_footer(&mut self, duration_secs: f64) {
        if self.assistant_emitted {
            self.items.push(StreamItem::TokenStat(format!(
                "⏱ {:.1}s · Σ{} in / Σ{} out",
                duration_secs,
                fmt_tokens(self.token_in_total),
                fmt_tokens(self.token_out_total),
            )));
        }
    }

    /// At the end of a turn (`Idle`), collapse the inline `⏺ tool … ✓ done`
    /// rows produced during the turn into a single compact status summary
    /// (e.g. `✓ 完成 2 项任务：读取文件 1 · 网络搜索 1`). This keeps the chat
    /// stream focused on the actual conversation instead of accumulating
    /// redundant per-tool "done" noise. Tool rows from earlier turns were
    /// already collapsed on their own `Idle`, so this only touches the rows the
    /// current turn appended (idempotent across turns).
    ///
    /// No-op when the turn performed no tools (nothing to collapse).
    pub fn collapse_tool_rows(&mut self) {
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut any = false;
        for item in &self.items {
            if let StreamItem::Tool { name, .. } = item {
                *counts.entry(name.clone()).or_insert(0) += 1;
                any = true;
            }
        }
        if !any {
            return;
        }
        let total: usize = counts.values().sum();
        // Sort by tool name so the summary text is deterministic across runs
        // (HashMap iteration order is not stable).
        let mut entries: Vec<(&String, &usize)> = counts.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let parts: Vec<String> = entries
            .iter()
            .map(|(n, c)| format!("{} {}", kind_label(n), c))
            .collect();
        let summary = format!("✓ 完成 {} 项任务：{}", total, parts.join(" · "));
        // Drop every inline tool row (previous turns' rows are already gone).
        self.items.retain(|i| !matches!(i, StreamItem::Tool { .. }));
        // Append the summary last so the flow reads: … → answer → "what I did".
        self.items.push(StreamItem::Summary(summary));
    }

    /// Render the full set of items into terminal `Line`s (pre-wrapping).
    fn to_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for item in &self.items {
            match item {
                StreamItem::User(text) => {
                    // Chat-bubble style (WeChat/Feishu): the user's own messages
                    // sit on the right in a soft, low-saturation color. Each
                    // logical line becomes its own right-aligned row so multi-line
                    // input wraps cleanly to the right edge. A prominent colored
                    // `›` at the far right edge tags the line as "you", so the
                    // role is unmistakable when scrolling back through history.
                    for l in text.lines() {
                        let mut line = Line::from(vec![
                            Span::styled(l.to_string(), Style::default().fg(INPUT_COLOR)),
                            Span::raw(" "),
                            Span::styled(
                                USER_MARKER,
                                Style::default()
                                    .fg(USER_MARKER_COLOR)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]);
                        line.alignment = Some(Alignment::Right);
                        lines.push(line);
                    }
                }
                StreamItem::Assistant(text) => {
                    for l in text.lines() {
                        lines.push(Line::from(Span::raw(l.to_string())));
                    }
                }
                StreamItem::Thinking {
                    text, collapsed, ..
                } => {
                    if *collapsed {
                        lines.push(Line::from(Span::styled(
                            "✶ thinking…",
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        )));
                    } else {
                        for (i, l) in text.lines().enumerate() {
                            let prefix = if i == 0 { "✶ " } else { "│ " };
                            lines.push(Line::from(Span::styled(
                                format!("{prefix}{l}"),
                                Style::default()
                                    .fg(Color::DarkGray)
                                    .add_modifier(Modifier::ITALIC),
                            )));
                        }
                    }
                }
                StreamItem::Tool {
                    name, args, state, ..
                } => {
                    let (glyph, style, extra) = match state {
                        ToolState::Running => {
                            ("⏺", Style::default().fg(Color::Yellow), " …".to_string())
                        }
                        ToolState::Done(secs) => (
                            "⏺",
                            Style::default().fg(Color::Green),
                            if *secs > 0.0 {
                                format!(" ✓ done ({secs:.1}s)")
                            } else {
                                " ✓ done".to_string()
                            },
                        ),
                        ToolState::Failed => {
                            ("✗", Style::default().fg(Color::Red), " failed".to_string())
                        }
                    };
                    let args_disp = if args.is_empty() {
                        String::new()
                    } else {
                        format!("({args})")
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{glyph} {name}"), style),
                        Span::raw(format!("{args_disp}{extra}")),
                    ]));
                }
                StreamItem::Phase(p) => {
                    lines.push(Line::from(Span::styled(
                        format!("— {} —", p.label()),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                StreamItem::System(text) => {
                    for l in text.lines() {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default().fg(Color::Blue),
                        )));
                    }
                }
                StreamItem::Command(text) => {
                    // Calm, neutral rendering — command output reads like plain
                    // terminal text, distinct from blue agent notes / red errors.
                    for l in text.lines() {
                        lines.push(Line::from(Span::raw(l.to_string())));
                    }
                }
                StreamItem::Error(text) => {
                    for l in text.lines() {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default().fg(Color::Red),
                        )));
                    }
                }
                StreamItem::TokenStat(text) => {
                    for l in text.lines() {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
                StreamItem::Summary(text) => {
                    // Compact end-of-turn status summary that replaces the
                    // individual inline `⏺ tool … ✓ done` rows. Rendered calm and
                    // distinct (dim + a soft green check) so the stream stays
                    // focused on the conversation, not the tooling noise.
                    for l in text.lines() {
                        lines.push(Line::from(Span::styled(
                            l.to_string(),
                            Style::default()
                                .fg(Color::Rgb(120, 150, 120))
                                .add_modifier(Modifier::DIM),
                        )));
                    }
                }
            }
        }
        lines
    }
}

/// Remove ANSI escape sequences (CSI/SGR) from `s`.
///
/// The legacy printers emit colored text via owo_colors (ANSI escapes). When
/// that text is shown as a plain `StreamItem::System` line in the ratatui
/// surface, the raw escapes must be stripped or they would leak into the
/// rendered buffer. Terminal mode never calls this (it prints escapes as-is).
pub fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip a CSI sequence: ESC [ … final byte in 0x40..=0x7E.
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // consume the final byte
            }
        } else {
            let len = utf8_len(bytes[i]);
            if i + len <= bytes.len() {
                out.push_str(&s[i..i + len]);
                i += len;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Length in bytes of the UTF-8 char starting with byte `b`.
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Plain text of a `Line` (styles stripped) — used to measure display width
/// for wrapping/scroll math.
fn line_plain(line: &Line) -> String {
    let mut out = String::new();
    for s in &line.spans {
        out.push_str(s.content.as_ref());
    }
    out
}

/// Wrap `text` into visual rows each no wider than `width`, mirroring ratatui's
/// `Wrap { trim: true }` closely enough for our own input line. We do the
/// wrapping ourselves (instead of relying on `Paragraph::wrap`) so the block
/// cursor can be placed by exact math — ratatui's wrap is grapheme-based and not
/// exposed for cursor math, and a mismatch would put the caret in the wrong
/// column. The returned rows are rendered verbatim (no further wrapping).
///
/// Behaviour: split on whitespace into words; a word that does not fit on the
/// current row moves to a fresh row; a word wider than `width` is hard-broken
/// into `width`-wide chunks (matching ratatui's long-word handling). Leading /
/// trailing / collapsed whitespace is dropped, like `trim: true`.
/// Wrap `text` into visual rows, returning the **byte ranges** of each row
/// within the original `text` (not owned copies). Rendering slices the original
/// so the displayed text is byte-for-byte identical to `text`; the ranges are
/// also used to map the input cursor (a byte offset) onto an exact (row, col)
/// so the block caret can never desync from what is drawn.
///
/// Algorithm (mirrors ratatui's `Wrap { trim: true }` closely enough for our
/// input line): split each logical line on spaces into words; a word that fits
/// on the current row is appended (with a separating space), otherwise it
/// starts a fresh row; a word wider than `width` is hard-broken into `width`-wide
/// chunks. Leading / trailing / collapsed whitespace is dropped (trim). The
/// returned ranges are contiguous, gap-free byte spans of `text` (a trailing
/// space that triggered a wrap simply ends the range one byte early — the next
/// row starts at the next word), which is exactly why the cursor can be mapped
/// by a plain `byte <= range.end` test below.
fn wrap_to_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    if width == 0 {
        return vec![(0, 0)];
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for logical in text.split('\n') {
        let bytes = logical.as_bytes();
        // Tokenize into (start, end) byte ranges of non-space words.
        let mut words: Vec<(usize, usize)> = Vec::new();
        let mut in_word = false;
        let mut ws = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b' ' {
                if in_word {
                    words.push((ws, i));
                    in_word = false;
                }
            } else if !in_word {
                ws = i;
                in_word = true;
            }
        }
        if in_word {
            words.push((ws, bytes.len()));
        }

        let mut row_start: Option<usize> = None;
        let mut row_end: usize = 0;
        let mut row_w = 0usize;

        for (ws_off, we_off) in words {
            let word = &logical[ws_off..we_off];
            let ww = UnicodeWidthStr::width(word);
            if ww > width {
                // Close any open row, then hard-break this over-wide word.
                if let Some(s) = row_start.take() {
                    ranges.push((s, row_end));
                }
                let mut rem = ws_off;
                while rem < we_off {
                    let take = take_width(&logical[rem..we_off], width);
                    let end = rem + take;
                    ranges.push((rem, end));
                    rem = end;
                }
                row_w = 0;
                continue;
            }
            match row_start {
                None => {
                    row_start = Some(ws_off);
                    row_end = we_off;
                    row_w = ww;
                }
                Some(_) if row_w + 1 + ww <= width => {
                    row_end = we_off;
                    row_w += 1 + ww;
                }
                Some(_) => {
                    ranges.push((row_start.take().unwrap(), row_end));
                    row_start = Some(ws_off);
                    row_end = we_off;
                    row_w = ww;
                }
            }
        }
        if let Some(s) = row_start.take() {
            ranges.push((s, row_end));
        }
    }
    if ranges.is_empty() {
        ranges.push((0, 0));
    }
    ranges
}

/// Byte length of the prefix of `s` whose display width is `<= width`.
fn take_width(s: &str, width: usize) -> usize {
    let mut w = 0usize;
    for (i, c) in s.char_indices() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width {
            return i;
        }
        w += cw;
    }
    s.len()
}

/// Wrap `s` to `width` columns (mirrors ratatui `Wrap { trim: true }`) and
/// return `(total_rows, cols_in_last_row)`. The last-row column is needed to
/// place the block cursor inside a (possibly multi-line) wrapped input line.
fn wrap_metrics(s: &str, width: usize) -> (usize, usize) {
    if width == 0 {
        let n = s.split('\n').count().max(1);
        return (n, 0);
    }
    let mut rows = 0usize;
    let mut last = 0usize; // cols occupied in the final row
    for logical in s.split('\n') {
        let mut row_started = false;
        let mut col = 0usize;
        for word in logical.split(' ') {
            let ww = UnicodeWidthStr::width(word);
            if ww == 0 {
                continue; // collapsed whitespace — no row cost
            }
            if !row_started {
                row_started = true;
                col = ww;
                if ww > width {
                    let r = ww.div_ceil(width); // ceil
                    col = ww - (r - 1) * width;
                    rows += r;
                } else {
                    rows += 1;
                }
            } else if col + 1 + ww <= width {
                col += 1 + ww;
            } else {
                row_started = true;
                col = ww;
                if ww > width {
                    let r = ww.div_ceil(width); // ceil
                    col = ww - (r - 1) * width;
                    rows += r;
                } else {
                    rows += 1;
                }
            }
        }
        if !row_started {
            rows += 1; // an empty logical line still occupies one row
            col = 0;
        }
        last = col;
    }
    (rows.max(1), last)
}

/// Draw the entire frame from `view`. Pure function of state + size.
pub fn render_frame(f: &mut Frame, view: &mut ChatView) {
    let area = f.area();
    // The live input prompt wraps when long, so it can occupy more than one
    // row. We wrap it ourselves into byte ranges of the prompt (see
    // `wrap_to_ranges`) and render those slices verbatim (no ratatui `Wrap`) so
    // the block cursor can be mapped onto the *exact* ranges we draw — ratatui's
    // grapheme-based wrap is not exposed for cursor math, and any mismatch would
    // put the caret in the wrong column. The wrapped row count reserves the
    // right number of layout rows up front.
    let input_text = format!("> {}", view.input);
    let input_width = area.width as usize;
    let input_ranges = wrap_to_ranges(&input_text, input_width);
    let input_rows = input_ranges.len();
    // Reserve 1 row each for the status + hint bars; the stream keeps at least 1.
    // Cap the rows the input may reserve (Issue 1): a very long typed message
    // must never grow to fill the whole screen and cover the chat above it — past
    // this height it scrolls internally instead. The chat therefore always keeps
    // a usable share of the terminal.
    let hard_cap = (area.height / 2).max(3);
    let max_input_rows = (area.height.saturating_sub(3)).max(1).min(hard_cap) as usize;
    let input_block_rows = input_rows.min(max_input_rows);

    // Bottom status panel: collapsed = 1 line, expanded = up to 3 lines. Never
    // steal the chat's 1-line floor or the input block (chat stays Min(1)).
    let avail_for_hint = (area.height as i32 - 2 - input_block_rows as i32).clamp(1, 3);
    let hint_rows: u16 = if view.status_expanded {
        avail_for_hint as u16
    } else {
        1
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                       // status bar
            Constraint::Min(1),                          // chat stream
            Constraint::Length(input_block_rows as u16), // input (wraps)
            Constraint::Length(hint_rows),               // status panel
        ])
        .split(area);

    // Top status bar: brand + tagline (left) | model + phase (right).
    let phase_span = match view.phase {
        Phase::Idle => Span::styled("●", Style::default().fg(Color::DarkGray)),
        p => {
            // Breathing pulse + a reassuring English status sentence so the
            // user never stares at a dead "waiting" state.
            let pulse = PULSE[view.anim_frame as usize % PULSE.len()];
            let status = p.status_text();
            Span::styled(
                format!("{pulse} {status}"),
                Style::default().fg(Color::Cyan),
            )
        }
    };
    let model_span = if view.model_label.is_empty() {
        Span::styled("no model", Style::default().fg(Color::DarkGray))
    } else {
        // Low-contrast neutral: the model name is informational, not a focal
        // point, so it stays legible but calm (no bold, dim grey).
        Span::styled(
            view.model_label.clone(),
            Style::default().fg(Color::DarkGray),
        )
    };
    let right = Line::from(vec![model_span, Span::raw(" "), phase_span]);
    let left = Line::from(vec![
        Span::styled(
            BRAND_GLYPH,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("Rupoo", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" · "),
        Span::styled(TAGLINE, Style::default().fg(Color::DarkGray)),
    ]);
    let bar = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right.width() as u16)])
        .split(chunks[0]);
    f.render_widget(Paragraph::new(left), bar[0]);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), bar[1]);

    // Chat stream: follow-to-bottom by default, with manual scroll (↑/↓ and
    // PageUp/PageDown). Long lines are word-wrapped so nothing is truncated.
    // `scroll`/`max_scroll` are expressed in *visual* rows because wrapping can
    // make one logical line occupy several rows.
    let lines = view.to_lines();
    let width = chunks[1].width as usize;
    let height = chunks[1].height as usize;
    view.height = height as u16; // remembered for full-page scroll steps
    let total_rows: usize = lines
        .iter()
        .map(|l| wrap_metrics(&line_plain(l), width).0)
        .sum();
    let max_scroll = (total_rows.saturating_sub(height)) as u16;
    let offset = if view.follow {
        max_scroll
    } else {
        view.scroll.min(max_scroll)
    };
    if view.follow {
        // Keep `scroll` in sync with the bottom while pinned, so the first
        // scroll-up moves exactly one page up from the newest line.
        view.scroll = max_scroll;
    } else if offset >= max_scroll {
        // Manual scroll reached the bottom — resume following.
        view.follow = true;
    }
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .scroll((offset, 0));
    f.render_widget(para, chunks[1]);

    // Input prompt: render the pre-wrapped ranges verbatim (no ratatui `Wrap`,
    // so the cursor math below stays exact) and keep the cursor in view. The
    // block can grow to several rows for long prompts; when it would exceed the
    // reserved space we scroll the input to the cursor line so the user always
    // sees what they are typing.
    //
    // Cursor mapping: the caret byte (`2 + view.cursor`, to skip the "> "
    // prompt) is floored to a char boundary, then located inside the very same
    // `input_ranges` we render. Because the ranges ARE the drawn rows, the
    // (row, col) is always correct — including after spaces and across wrap
    // boundaries, where a prefix-rewrap would otherwise desync the caret. If the
    // caret lands in trailing whitespace / past the last glyph, it sits at the
    // end of the final row.
    let prompt_cursor = input_text.floor_char_boundary((2 + view.cursor).min(input_text.len()));
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut matched = false;
    for (i, &(s, e)) in input_ranges.iter().enumerate() {
        if prompt_cursor <= e {
            cursor_row = i;
            cursor_col = UnicodeWidthStr::width(&input_text[s..prompt_cursor]);
            matched = true;
            break;
        }
    }
    if !matched {
        if let Some(&(s, e)) = input_ranges.last() {
            cursor_row = input_ranges.len() - 1;
            cursor_col = UnicodeWidthStr::width(&input_text[s..e]);
        }
    }
    let input_scroll = cursor_row.saturating_sub(input_block_rows.saturating_sub(1));
    let input_lines: Vec<Line> = input_ranges
        .iter()
        .map(|&(s, e)| {
            Line::from(Span::styled(
                input_text[s..e].to_string(),
                Style::default().fg(INPUT_COLOR),
            ))
        })
        .collect();
    let input_para = Paragraph::new(input_lines).scroll((input_scroll as u16, 0));
    f.render_widget(input_para, chunks[2]);

    // Bottom bar: left = quiet rotating tip; right = live status panel. The bar
    // splits horizontally, mirroring the top status bar's left/right layout.
    let panel_w = (area.width / 2).max(1);
    let hbar = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(panel_w)])
        .split(chunks[3]);
    let tip = HINT_TIPS[view.hint_index % HINT_TIPS.len()];
    let (label, rest) = tip.split_once(": ").unwrap_or((tip, ""));
    let mut hint_spans = vec![Span::styled(
        format!("{label}: "),
        Style::default().fg(HINT_ACCENT_COLOR),
    )];
    if !rest.is_empty() {
        hint_spans.push(Span::styled(
            rest.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(hint_spans)), hbar[0]);
    // Record the panel's right cell for the mouse click hit-test, then paint it.
    view.status_panel_rect = hbar[1];
    render_status_panel(f, &*view, hbar[1]);

    // First-launch getting-started overlay (drawn on top of everything).
    if let Some(guide) = &view.guide {
        render_guide(f, guide);
    }

    // Place the block cursor inside the (wrapped) input. `cursor_col` is the
    // column within the cursor's own row; the row follows the input scroll.
    let cur_y = chunks[2].y + (cursor_row.saturating_sub(input_scroll)) as u16;
    f.set_cursor_position((chunks[2].x.saturating_add(cursor_col as u16), cur_y));
}

/// Render the bottom status panel's right cell: a collapsed one-line summary of
/// running activities, or (when expanded) a `▾` header plus a recent-activity
/// mini-log. Path-free by design — only the tool name + state are shown.
fn render_status_panel(f: &mut Frame, view: &ChatView, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if !view.status_expanded {
        // Collapsed: count currently-running activities by tool name.
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for e in &view.status_ring {
            if matches!(e.state, ToolState::Running) {
                *counts.entry(e.name.clone()).or_insert(0) += 1;
            }
        }
        if counts.is_empty() {
            lines.push(Line::from(Span::styled(
                "✓ 就绪",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let summary = counts
                .iter()
                .map(|(n, c)| format!("{} {}", kind_label(n), c))
                .collect::<Vec<_>>()
                .join(" · ");
            lines.push(Line::from(Span::styled(
                format!("⏺ {summary}"),
                Style::default().fg(Color::Cyan),
            )));
        }
    } else {
        // Expanded: header + the recent activity log (newest at the bottom).
        lines.push(Line::from(Span::styled(
            format!("▾ 运行活动 ({})", view.status_ring.len()),
            Style::default().fg(Color::Cyan),
        )));
        for e in &view.status_ring {
            let (glyph, color) = match e.state {
                ToolState::Running => ("⏺", Color::Yellow),
                ToolState::Done(_) => ("✓", Color::Green),
                ToolState::Failed => ("✗", Color::Red),
            };
            lines.push(Line::from(Span::styled(
                format!("{} {}", glyph, kind_label(&e.name)),
                Style::default().fg(color),
            )));
        }
    }
    let total = lines.len();
    let visible = area.height as usize;
    // Expanded log: while `status_follow` is set the panel live-follows the
    // newest line (auto-scroll). Once the workflow ends (`Idle`) or the user
    // scrolls manually, `status_follow` is cleared and the panel freezes at the
    // user's scroll position (or the last item when `status_scroll == 0`) — no
    // more auto-scrolling churn.
    let offset = if view.status_expanded {
        let bottom = total.saturating_sub(visible) as u16;
        if view.status_follow {
            bottom
        } else {
            bottom.saturating_sub(view.status_scroll)
        }
    } else {
        0
    };
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), area);
}

/// Outcome of applying one agent event to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// More events expected; keep streaming.
    Continue,
    /// Generation finished (`Idle` received). The runtime should stop the
    /// streaming loop and return to the input prompt.
    GenerationComplete,
}

/// Pure reducer — the heart of the humanistic UI.
///
/// Maps a single `AgentToTui` event onto `view`, producing zero side effects
/// (no I/O, no clock, no allocation beyond the state itself). Every visible
/// element of the companion stream is produced here, so the mapping is fully
/// unit-testable and the runtime only has to drive `render_frame`.
///
/// Events that are not part of the core humanistic stream (plan-mode task
/// lists, layout-mode hints, token counters, approval requests, file-change
/// diffs) are intentionally ignored here; the runtime keeps handling them on
/// the legacy path until Step 2 wires a unified loop.
pub fn apply_event(view: &mut ChatView, msg: &AgentToTui) -> ApplyOutcome {
    match msg {
        AgentToTui::StreamChunk { text } => {
            view.pending_assistant.push_str(text);
            ApplyOutcome::Continue
        }
        AgentToTui::Thinking => {
            view.phase = Phase::Understanding;
            view.assistant_emitted = false;
            ApplyOutcome::Continue
        }
        AgentToTui::ThinkingSummary { text } => {
            view.phase = Phase::Understanding;
            let id = view.next_id();
            view.items.push(StreamItem::Thinking {
                id,
                text: text.clone(),
                collapsed: false,
            });
            ApplyOutcome::Continue
        }
        AgentToTui::ToolStatus { tool_name, phase } => {
            view.phase = Phase::Acting;
            match phase {
                ToolPhase::Calling => {
                    let id = view.next_id();
                    view.open_tool_id = Some(id);
                    view.items.push(StreamItem::Tool {
                        id,
                        name: tool_name.clone(),
                        args: String::new(),
                        state: ToolState::Running,
                    });
                }
                ToolPhase::Completed => {
                    let target = view.open_tool_id.take();
                    let mut placed = false;
                    if let Some(open_id) = target {
                        for item in view.items.iter_mut().rev() {
                            if let StreamItem::Tool { id, state, .. } = item {
                                if *id == open_id {
                                    *state = ToolState::Done(0.0);
                                    placed = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !placed {
                        let id = view.next_id();
                        view.items.push(StreamItem::Tool {
                            id,
                            name: tool_name.clone(),
                            args: String::new(),
                            state: ToolState::Done(0.0),
                        });
                    }
                }
            }
            // Mirror the activity into the bottom-panel ring (no path/content).
            let st = match phase {
                ToolPhase::Calling => ToolState::Running,
                ToolPhase::Completed => ToolState::Done(0.0),
            };
            view.push_status(tool_name.clone(), st);
            // While a tool is running the expanded panel live-follows the newest
            // activity (Issue 3): any in-flight activity cancels a frozen state.
            view.status_follow = true;
            view.status_scroll = 0;
            ApplyOutcome::Continue
        }
        AgentToTui::PhaseProgress {
            phase_name,
            percentage,
        } => {
            view.phase = Phase::Acting;
            view.phase_detail = Some(format!("{phase_name} {percentage}%"));
            ApplyOutcome::Continue
        }
        AgentToTui::Message(m) => {
            match m.role {
                MessageRole::User => {
                    view.items.push(StreamItem::User(m.content.clone()));
                }
                MessageRole::Assistant => {
                    // Prefer streamed chunks; fall back to the full message
                    // content when no chunks arrived (legacy fallback).
                    if view.pending_assistant.is_empty() && !m.content.is_empty() {
                        view.pending_assistant.push_str(&m.content);
                    }
                    view.finalize_assistant();
                    view.phase = Phase::Verifying;
                }
                MessageRole::System => {
                    let c = m.content.trim();
                    if c.starts_with("🔧") || c.starts_with("✅") || c.starts_with("⠋") {
                        // Tool-call noise — suppressed in the humanistic stream.
                    } else if c.contains("Error") {
                        view.items.push(StreamItem::Error(c.to_string()));
                    } else if !c.is_empty() {
                        view.items.push(StreamItem::System(c.to_string()));
                    }
                }
                _ => {
                    if !m.content.is_empty() {
                        view.items.push(StreamItem::System(m.content.clone()));
                    }
                }
            }
            ApplyOutcome::Continue
        }
        AgentToTui::Idle => {
            view.finalize_assistant();
            view.phase = Phase::Idle;
            view.phase_detail = None;
            view.open_tool_id = None;
            // Issue 2: fold this turn's inline tool "done" rows into one summary
            // so the stream keeps only the core result / status, not the noise.
            view.collapse_tool_rows();
            // Issue 3: the workflow has ended — freeze the bottom status panel at
            // the last item (stop auto-scrolling) so it no longer churns; the user
            // scrolls it manually via ↑/↓. We intentionally do NOT auto-collapse
            // it, so the final activity stays visible for review.
            view.status_follow = false;
            ApplyOutcome::GenerationComplete
        }
        AgentToTui::TokenUpdate {
            in_count,
            out_count,
        } => {
            view.token_in_total = view.token_in_total.saturating_add(*in_count);
            view.token_out_total = view.token_out_total.saturating_add(*out_count);
            ApplyOutcome::Continue
        }
        // Mode/plan/data events: not part of the core humanistic stream.
        _ => ApplyOutcome::Continue,
    }
}

/// Compact token-count formatting (e.g. 1234 -> "1.2k", 12345 -> "12.3k").
pub fn fmt_tokens(n: u64) -> String {
    if n >= 10_000 {
        format!("{}.{}k", n / 1000, (n % 1000) / 100)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Build the getting-started guide body (3 core sections).
fn guide_content() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let head = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let key = |s: &'static str| Span::styled(s, Style::default().fg(Color::Yellow));
    let dim = |s: &'static str| Span::styled(s, Style::default().fg(Color::DarkGray));

    lines.push(Line::from(Span::styled(
        " 欢迎使用 Rupoo —— 你的 AI 编程搭子",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(dim(
        " 直接输入问题即可开始；下面三件事帮你快速上手。",
    )));
    lines.push(Line::from(""));

    lines.push(head(" ① 常用命令（输入 / 唤起）"));
    lines.push(Line::from(vec![
        key(" /help"),
        dim("          查看全部命令"),
    ]));
    lines.push(Line::from(vec![key(" /new"), dim("           新建会话")]));
    lines.push(Line::from(vec![
        key(" /sessions"),
        dim("       列出会话 · /switch <n> 切换"),
    ]));
    lines.push(Line::from(vec![
        key(" /model"),
        dim("          查看当前模型"),
    ]));
    lines.push(Line::from(vec![
        key(" /tools"),
        dim("          查看可用工具"),
    ]));
    lines.push(Line::from(vec![
        key(" /plan <需求>"),
        dim("    进入计划模式"),
    ]));
    lines.push(Line::from(vec![
        key(" /clear"),
        dim("          清屏 · /quit 退出"),
    ]));
    lines.push(Line::from(vec![
        key(" /read /cmd /search"),
        dim("  读文件 / 跑命令 / 联网搜索"),
    ]));
    lines.push(Line::from(""));

    lines.push(head(" ② 模型添加与切换"));
    lines.push(Line::from(vec![
        dim(" 配置密钥："),
        key("rupoo config set api_key.<provider> <key>"),
    ]));
    lines.push(Line::from(vec![
        dim(" 切换模型："),
        key("/model <provider> [model]"),
    ]));
    lines.push(Line::from(vec![
        dim("   例："),
        key("/model anthropic claude-sonnet-4"),
        dim(" · "),
        key("/model set deepseek"),
    ]));
    lines.push(Line::from(vec![
        dim(" 或全局："),
        key("rupoo config set active_provider <provider>"),
    ]));
    lines.push(Line::from(vec![dim(" 查看当前："), key("/model")]));
    lines.push(Line::from(""));

    lines.push(head(" ③ 飞书等外部渠道"));
    lines.push(Line::from(vec![
        dim(" 接入飞书："),
        key("rupoo feishu"),
        dim("（向导：App ID / App Secret / 国内或 Lark）"),
    ]));
    lines.push(Line::from(vec![
        dim(" 或："),
        key("rupoo channel add feishu"),
        dim(" · "),
        key("channel list"),
        dim(" · "),
        key("channel remove feishu"),
    ]));
    lines.push(Line::from(vec![dim(" 钉钉："), key("rupoo dingtalk")]));
    lines.push(Line::from(vec![
        dim(" 启动机器人："),
        key("rupoo serve"),
        dim("（或 "),
        key("rupoo serve --daemon"),
        dim(" 后台）"),
    ]));
    lines
}

/// Render the getting-started overlay as a centered modal on top of the UI.
fn render_guide(f: &mut Frame, guide: &GuideOverlay) {
    let area = f.area();
    let w = area.width.saturating_sub(4).min(78);
    let h = area.height.saturating_sub(2).min(28);
    if w < 24 || h < 8 {
        return;
    }
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(" 使用指南 / Getting Started ").alignment(Alignment::Center))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    // Footer (dismiss option + close hint) is pinned to the bottom line so it
    // stays visible regardless of how far the body is scrolled.
    let body_h = inner.height.saturating_sub(1);
    let checkbox = if guide.dismiss_checked { "[x]" } else { "[ ]" };
    let footer = Line::from(vec![
        Span::styled(
            format!(" {checkbox} 不再显示使用指南（按 D 切换）"),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("     "),
        Span::styled("Enter / Esc 关闭", Style::default().fg(Color::DarkGray)),
    ]);
    if body_h == 0 {
        f.render_widget(Paragraph::new(footer), inner);
        return;
    }
    let body_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: body_h,
    };
    let footer_rect = Rect {
        x: inner.x,
        y: inner.y + body_h,
        width: inner.width,
        height: 1,
    };

    let lines = guide_content();
    let body = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .scroll((guide.scroll, 0));
    f.render_widget(body, body_rect);
    f.render_widget(Paragraph::new(footer), footer_rect);
}

#[cfg(test)]
mod tests {
    // Tests intentionally build a ChatView via Default and then tweak a few
    // fields to model a specific state; the initializer form would be far
    // noisier here than the targeted reassignments.
    #![allow(clippy::field_reassign_with_default)]
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_text(term: &Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn renders_core_stream_items() {
        let mut view = ChatView::default();
        view.items.push(StreamItem::User("hi".into()));
        view.items.push(StreamItem::Thinking {
            id: 1,
            text: "let me think".into(),
            collapsed: false,
        });
        view.items.push(StreamItem::Tool {
            id: 1,
            name: "read_file".into(),
            args: "main.rs".into(),
            state: ToolState::Done(0.3),
        });
        view.items
            .push(StreamItem::Assistant("here is the answer".into()));
        view.items.push(StreamItem::Error("oops".into()));

        let backend = TestBackend::new(80, 12);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();

        let s = buffer_text(&term);
        assert!(s.contains("hi"), "user line missing");
        assert!(s.contains("let me think"), "thinking missing");
        assert!(s.contains("read_file"), "tool name missing");
        assert!(s.contains("done"), "tool done state missing");
        assert!(s.contains("here is the answer"), "assistant missing");
        assert!(s.contains("oops"), "error missing");
    }

    #[test]
    fn follow_scrolls_to_newest() {
        let mut view = ChatView::default();
        for i in 0..100 {
            view.items.push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 8);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(s.contains("line 99"), "newest line should be visible");
        assert!(!s.contains("line 0"), "oldest line should be scrolled off");
    }

    #[test]
    fn manual_scroll_pauses_follow_and_shows_older_lines() {
        let mut view = ChatView::default();
        for i in 0..100 {
            view.items.push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 8);
        let mut term = Terminal::new(backend).unwrap();

        // Default is follow → newest visible.
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        assert!(view.follow, "fresh view should follow to bottom");
        assert!(buffer_text(&term).contains("line 99"));

        // Manual scroll to the top pauses follow and reveals older lines.
        view.follow = false;
        view.scroll = 0;
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(!view.follow, "manual scroll must pause follow");
        assert!(
            s.contains("line 0"),
            "oldest line should be visible when scrolled up"
        );
        assert!(
            !s.contains("line 99"),
            "newest line should be scrolled off when at top"
        );
    }

    /// IM-style contract (WeChat/Feishu habits):
    ///  - new content auto-scrolls to the bottom, so the AI reply's tail is
    ///    always visible (follow mode; the view is never anchored at the user
    ///    message — it always pins to the newest line);
    ///  - scrolling up pauses follow and reveals the user's own earlier turn;
    ///  - scrolling back to the bottom resumes following (jump to latest);
    ///  - submitting a new message resets follow so the fresh turn stays in view.
    #[test]
    fn im_style_follows_bottom_and_scroll_up_reveals_user() {
        let mut view = ChatView::default();
        // User submits a turn.
        view.items.push(StreamItem::User("my question".into()));
        // A long assistant reply is committed (as finalize_assistant would).
        let long: String = (0..120).map(|i| format!("reply {i}\n")).collect();
        apply_event(&mut view, &msg(MessageRole::Assistant, &long));

        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();

        // 1) Following: newest AI line visible, user turn scrolled off the top.
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        assert!(
            view.follow,
            "should keep following the newest line (no anchor at user message)"
        );
        let s = buffer_text(&term);
        assert!(
            s.contains("reply 119"),
            "newest AI line (tail) must be visible"
        );
        assert!(
            !s.contains("my question"),
            "user turn scrolls off the top while following the bottom"
        );

        // 2) Scroll up to read history: reveals the user's own message.
        view.follow = false;
        view.scroll = 0;
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s2 = buffer_text(&term);
        assert!(!view.follow, "scroll-up pauses follow");
        assert!(
            s2.contains("my question"),
            "scrolling up reveals the user's message"
        );
        assert!(
            !s2.contains("reply 119"),
            "newest line scrolls off when reading history"
        );

        // 3) Scrolling back to the bottom resumes following (jump to latest).
        view.scroll = u16::MAX;
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        assert!(view.follow, "reaching the bottom resumes following");
    }

    /// Submitting a new message must reset `follow` so the fresh turn (and the
    /// incoming reply) stays in view even after the user scrolled up to read
    /// history — the WeChat/Feishu "send → jump to bottom" habit.
    #[test]
    fn submit_resets_follow_to_bottom() {
        let mut view = ChatView::default();
        // Simulate having scrolled up to read older history.
        view.follow = false;
        view.scroll = 0;
        // A new turn is committed (what submit_message does in tui mode).
        view.items.push(StreamItem::User("new question".into()));
        view.follow = true; // submit_message now re-pins to the bottom.
        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        assert!(view.follow, "submit must re-enable following");
        assert!(
            buffer_text(&term).contains("new question"),
            "the just-sent user message must be visible after submit"
        );
    }

    #[test]
    fn long_line_wraps_without_truncation() {
        let mut view = ChatView::default();
        // A line far wider than the 20-col terminal must wrap, not be clipped.
        let long = "abcdefghij".repeat(6); // 60 chars, no spaces
        view.items.push(StreamItem::Assistant(long.clone()));
        let backend = TestBackend::new(20, 10);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();

        // With wrapping the full text spans 3 rows; without it, only the first
        // 20 chars would be drawn and the tail would be lost. Catch that.
        let s = buffer_text(&term);
        assert!(
            s.contains("abcdefghij"),
            "wrapped content must not be truncated"
        );
        // The tail (rows 2-3) must also be reachable/present.
        assert!(
            s.contains(&long[40..50]),
            "later portion must survive wrapping"
        );
        assert!(view.follow, "short overflow still follows the bottom");
    }

    /// A long user message must word-wrap (so it is never clipped at the
    /// terminal width) and still carry its colored role marker at the right
    /// edge — so a long "me" turn stays identifiable when scrolling back.
    #[test]
    fn user_long_text_wraps_and_tags_role() {
        let mut view = ChatView::default();
        let long = "abcdefghij".repeat(6); // 60 chars, no spaces → wraps
        view.items.push(StreamItem::User(long.clone()));
        let backend = TestBackend::new(20, 14);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        // Marker identifies the user's own message even after wrapping.
        assert!(s.contains(USER_MARKER), "user role marker missing: {s}");
        // Long content must wrap, not be clipped at the terminal width.
        assert!(s.contains("abcdefghij"), "wrapped content truncated");
        assert!(s.contains(&long[40..50]), "later portion lost on wrap: {s}");
    }

    /// The live input prompt must wrap a long typed line instead of letting it
    /// run off the terminal edge; the whole prompt stays readable without
    /// resizing the window, and the cursor follows the wrapped text.
    #[test]
    fn input_long_text_wraps_without_truncation() {
        let mut view = ChatView::default();
        let long = "abcdefghij".repeat(6); // 60 chars
        view.input = long.clone();
        view.cursor = long.len(); // caret at the end
        let backend = TestBackend::new(20, 14);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        // The prompt marker and the start of the typed text are both present...
        assert!(s.contains("> "), "input prompt clipped: {s}");
        assert!(s.contains("abcdefghij"), "input head clipped: {s}");
        // ...and so is the tail — proof the long line wrapped, not truncated.
        assert!(s.contains(&long[50..60]), "input tail clipped: {s}");
    }

    /// Bug #2 regression: a cursor byte that is NOT on a char boundary (what the
    /// old byte-wise `Left`/`Right` produced for multibyte input) must never make
    /// the renderer panic on `&input[..cursor]`. `render_frame` now floors the
    /// caret to a char boundary, so even a corrupted cursor stays safe.
    #[test]
    fn input_multibyte_cursor_off_boundary_does_not_panic() {
        let mut view = ChatView::default();
        let s = "你好世界"; // 4 CJK chars, each 3 bytes
        view.input = s.to_string();
        view.cursor = 1; // deliberately inside the first multibyte char
        let backend = TestBackend::new(30, 10);
        let mut term = Terminal::new(backend).unwrap();
        // Must not panic despite the off-boundary cursor.
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        // Double-width CJK chars each occupy two cells, so the TestBackend
        // buffer interleaves a space placeholder between them (e.g. "你 好 世
        // 界"). Assert the individual glyphs rendered — the real point is that an
        // off-boundary cursor must not panic.
        assert!(s.contains('你'), "multibyte input must render: {s}");
        assert!(s.contains('界'), "multibyte input must render: {s}");
    }

    /// Bug #1 regression: when the caret sits right after a space at a wrap
    /// boundary (exactly the case after pressing `Left`), both wrapped rows must
    /// still render and the frame must not panic. The caret is mapped onto the
    /// same ranges that are drawn, so it cannot desync to the wrong line.
    #[test]
    fn input_cursor_at_wrap_boundary_renders() {
        let mut view = ChatView::default();
        view.input = "aaa bbb".to_string();
        view.cursor = 3; // right after "aaa" (the wrap point, past the space)
        let backend = TestBackend::new(6, 10);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(s.contains("aaa"), "first wrapped row must render: {s}");
        assert!(s.contains("bbb"), "second wrapped row must render: {s}");
    }

    #[test]
    fn wrap_to_ranges_hard_breaks_long_words() {
        let ranges = wrap_to_ranges("abcdefghijklmnopqrst", 10);
        assert_eq!(ranges, vec![(0, 10), (10, 20)]);
    }

    #[test]
    fn wrap_to_ranges_word_wrap_respects_spaces() {
        // "aa bb cc" in width 5: "aa bb" (bytes 0..5) fits row0, "cc" (6..8) row1.
        let ranges = wrap_to_ranges("aa bb cc", 5);
        assert_eq!(ranges, vec![(0, 5), (6, 8)]);
    }

    #[test]
    fn wrap_to_ranges_drops_trailing_space() {
        let ranges = wrap_to_ranges("aa ", 10);
        assert_eq!(ranges, vec![(0, 2)]);
    }

    // ───────────────────────── reducer tests ─────────────────────────

    fn msg(role: MessageRole, content: &str) -> AgentToTui {
        AgentToTui::Message(rupoo::ChatMessage {
            role,
            content: content.to_string(),
            is_command_output: false,
            timestamp: None,
        })
    }

    #[test]
    fn stream_chunks_accumulate_then_finalize() {
        let mut view = ChatView::default();
        assert_eq!(
            apply_event(
                &mut view,
                &AgentToTui::StreamChunk {
                    text: "Hello ".into()
                }
            ),
            ApplyOutcome::Continue
        );
        assert_eq!(
            apply_event(
                &mut view,
                &AgentToTui::StreamChunk {
                    text: "world".into()
                }
            ),
            ApplyOutcome::Continue
        );
        // Not yet finalized — stays in the pending accumulator.
        assert!(
            view.items.is_empty(),
            "nothing should be committed before finalize"
        );
        assert_eq!(view.pending_assistant, "Hello world");

        // Finalizing via an empty assistant message flushes the accumulator.
        assert_eq!(
            apply_event(&mut view, &msg(MessageRole::Assistant, "")),
            ApplyOutcome::Continue
        );
        assert_eq!(view.items.len(), 1);
        assert_eq!(view.items[0], StreamItem::Assistant("Hello world".into()));
        assert!(view.pending_assistant.is_empty());
    }

    #[test]
    fn thinking_summary_pushes_inline_block() {
        let mut view = ChatView::default();
        apply_event(
            &mut view,
            &AgentToTui::ThinkingSummary {
                text: "analyzing error.rs".into(),
            },
        );
        assert_eq!(view.phase, Phase::Understanding);
        assert_eq!(view.items.len(), 1);
        assert!(matches!(
            &view.items[0],
            StreamItem::Thinking { text, .. } if text == "analyzing error.rs"
        ));
    }

    #[test]
    fn tool_calling_then_completed_upgrades_same_row() {
        let mut view = ChatView::default();
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "search".into(),
                phase: ToolPhase::Calling,
            },
        );
        assert_eq!(view.phase, Phase::Acting);
        assert_eq!(view.items.len(), 1);
        assert!(matches!(
            view.items[0],
            StreamItem::Tool {
                state: ToolState::Running,
                ..
            }
        ));

        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "search".into(),
                phase: ToolPhase::Completed,
            },
        );
        // Same row upgraded in place — still exactly one tool item.
        assert_eq!(view.items.len(), 1);
        assert!(matches!(
            view.items[0],
            StreamItem::Tool {
                state: ToolState::Done(_),
                ..
            }
        ));
    }

    #[test]
    fn phase_progress_sets_status_detail() {
        let mut view = ChatView::default();
        apply_event(
            &mut view,
            &AgentToTui::PhaseProgress {
                phase_name: "refactor".into(),
                percentage: 62,
            },
        );
        assert_eq!(view.phase, Phase::Acting);
        assert_eq!(view.phase_detail.as_deref(), Some("refactor 62%"));
    }

    #[test]
    fn message_user_system_and_noise() {
        let mut view = ChatView::default();
        apply_event(&mut view, &msg(MessageRole::User, "hi"));
        assert_eq!(view.items.last(), Some(&StreamItem::User("hi".into())));

        // Tool-call noise is suppressed in the humanistic stream.
        let before = view.items.len();
        apply_event(&mut view, &msg(MessageRole::System, "🔧 tool(x)"));
        assert_eq!(view.items.len(), before, "tool noise must be suppressed");

        apply_event(&mut view, &msg(MessageRole::System, "note for you"));
        assert_eq!(
            view.items.last(),
            Some(&StreamItem::System("note for you".into()))
        );

        apply_event(&mut view, &msg(MessageRole::System, "Boom Error happened"));
        assert_eq!(
            view.items.last(),
            Some(&StreamItem::Error("Boom Error happened".into()))
        );
    }

    #[test]
    fn idle_completes_and_resets_state() {
        let mut view = ChatView::default();
        apply_event(&mut view, &AgentToTui::Thinking);
        apply_event(
            &mut view,
            &AgentToTui::PhaseProgress {
                phase_name: "x".into(),
                percentage: 10,
            },
        );
        assert_eq!(view.phase, Phase::Acting);

        let outcome = apply_event(&mut view, &AgentToTui::Idle);
        assert_eq!(outcome, ApplyOutcome::GenerationComplete);
        assert_eq!(view.phase, Phase::Idle);
        assert!(view.phase_detail.is_none());
        assert!(view.open_tool_id.is_none());
    }

    #[test]
    fn full_turn_produces_humanistic_stream() {
        // Simulate exactly the event sequence the bridge emits for one turn.
        let mut view = ChatView::default();
        apply_event(&mut view, &msg(MessageRole::User, "fix the bug"));
        apply_event(&mut view, &AgentToTui::Thinking);
        apply_event(
            &mut view,
            &AgentToTui::ThinkingSummary {
                text: "looking at main.rs".into(),
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Completed,
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::StreamChunk {
                text: "The fix is ".into(),
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::StreamChunk {
                text: "to add a guard.".into(),
            },
        );
        apply_event(&mut view, &msg(MessageRole::Assistant, ""));
        let outcome = apply_event(&mut view, &AgentToTui::Idle);
        assert_eq!(outcome, ApplyOutcome::GenerationComplete);

        // Exactly the humanistic surface: user → thinking → answer → a single
        // collapsed status summary (Issue 2 folds the inline tool "done" rows).
        assert_eq!(
            view.items,
            vec![
                StreamItem::User("fix the bug".into()),
                StreamItem::Thinking {
                    id: 0,
                    text: "looking at main.rs".into(),
                    collapsed: false
                },
                StreamItem::Assistant("The fix is to add a guard.".into()),
                StreamItem::Summary("✓ 完成 1 项任务：读取文件 1".into()),
            ]
        );
        assert_eq!(view.phase, Phase::Idle);
        assert!(view.pending_assistant.is_empty());
    }

    /// Issue 2: at turn end the inline tool "done" rows are folded into one
    /// compact summary, in tool-count order, and the stream keeps only that.
    #[test]
    fn tool_rows_collapse_into_one_summary_on_idle() {
        let mut view = ChatView::default();
        apply_event(&mut view, &msg(MessageRole::User, "do the thing"));
        apply_event(&mut view, &AgentToTui::Thinking);
        apply_event(
            &mut view,
            &AgentToTui::ThinkingSummary {
                text: "planning".into(),
            },
        );
        // Two tools: each call upgrades the same row in place → one row each.
        for name in ["read_file", "web_search"] {
            apply_event(
                &mut view,
                &AgentToTui::ToolStatus {
                    tool_name: name.into(),
                    phase: ToolPhase::Calling,
                },
            );
            apply_event(
                &mut view,
                &AgentToTui::ToolStatus {
                    tool_name: name.into(),
                    phase: ToolPhase::Completed,
                },
            );
        }
        apply_event(
            &mut view,
            &AgentToTui::StreamChunk {
                text: "done".into(),
            },
        );
        apply_event(&mut view, &msg(MessageRole::Assistant, ""));
        assert!(
            matches!(view.items.last(), Some(StreamItem::Assistant(_))),
            "assistant committed"
        );
        assert_eq!(
            view.items
                .iter()
                .filter(|i| matches!(i, StreamItem::Tool { .. }))
                .count(),
            2,
            "two tool rows before Idle"
        );

        apply_event(&mut view, &AgentToTui::Idle);

        // Every inline tool row is gone; exactly one summary remains.
        assert!(
            view.items
                .iter()
                .all(|i| !matches!(i, StreamItem::Tool { .. })),
            "tool rows must be folded away"
        );
        let summaries: Vec<&String> = view
            .items
            .iter()
            .filter_map(|i| match i {
                StreamItem::Summary(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(summaries.len(), 1, "exactly one summary expected");
        assert_eq!(
            summaries[0], "✓ 完成 2 项任务：读取文件 1 · 网络搜索 1",
            "summary text wrong: {}",
            summaries[0]
        );
        // The summary sits after the assistant answer.
        let pos_a = view
            .items
            .iter()
            .position(|i| matches!(i, StreamItem::Assistant(_)))
            .unwrap();
        let pos_s = view
            .items
            .iter()
            .position(|i| matches!(i, StreamItem::Summary(_)))
            .unwrap();
        assert!(pos_s > pos_a, "summary must come after the answer");
    }

    /// Issue 2 regression: a turn with no tools produces no summary line.
    #[test]
    fn no_tools_means_no_summary() {
        let mut view = ChatView::default();
        apply_event(&mut view, &msg(MessageRole::User, "hi"));
        apply_event(
            &mut view,
            &AgentToTui::StreamChunk {
                text: "hello".into(),
            },
        );
        apply_event(&mut view, &msg(MessageRole::Assistant, ""));
        apply_event(&mut view, &AgentToTui::Idle);
        assert!(
            !view
                .items
                .iter()
                .any(|i| matches!(i, StreamItem::Summary(_))),
            "no summary without tools"
        );
    }

    /// Issue 3: while a tool runs the expanded panel live-follows the newest
    /// activity; once the workflow ends (`Idle`) it freezes (status_follow off)
    /// and stays expanded so the user can scroll it manually.
    #[test]
    fn status_panel_freezes_at_last_item_after_idle() {
        let mut view = ChatView::default();
        view.status_expanded = true;
        assert!(view.status_follow, "fresh panel follows");
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        assert!(view.status_follow, "running activity keeps following");
        apply_event(&mut view, &AgentToTui::Idle);
        assert!(
            !view.status_follow,
            "panel must freeze after the workflow ends"
        );
        assert!(
            view.status_expanded,
            "panel stays expanded, frozen at the last item"
        );
    }

    /// Issue 1: capping the input's reserved rows keeps the chat above it usable
    /// even when the typed message is very long — the input never covers the
    /// whole screen. The newest chat line stays visible (bottom-anchored).
    #[test]
    fn long_input_keeps_chat_visible_not_covered() {
        let mut view = ChatView::default();
        for i in 0..200 {
            view.items.push(StreamItem::Assistant(format!("line {i}")));
        }
        // A very long (200-row) typed message must not consume the whole screen.
        view.input = "x\n".repeat(200);
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        // The chat stays bottom-anchored: the newest line is visible (not hidden
        // behind the input), because the input height is capped.
        assert!(view.follow, "chat stays following the bottom");
        assert!(
            buffer_text(&term).contains("line 199"),
            "newest chat line must be visible above the input"
        );
    }

    #[test]
    fn status_bar_shows_tagline_and_model() {
        let mut view = ChatView::default();
        view.model_label = "claude-sonnet-4".into();
        let backend = TestBackend::new(90, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(s.contains("Rupoo"), "brand casing wrong");
        assert!(s.contains(TAGLINE), "tagline missing");
        assert!(s.contains("claude-sonnet-4"), "model label missing");
        // Model name is dimmed (no bold) — still present, just not a focal point.
        assert!(!s.contains("RUPOO"), "brand must be cased 'Rupoo'");
    }

    /// During an active phase the status bar must show a pulsing indicator plus
    /// a reassuring English status sentence, not a dead "waiting" state.
    #[test]
    fn status_bar_shows_pulsing_status_during_active_phase() {
        let mut view = ChatView::default();
        view.model_label = "claude-sonnet-4".into();
        view.phase = Phase::Understanding;
        view.anim_frame = 1; // pick a non-trivial pulse frame
        let backend = TestBackend::new(100, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(
            s.contains("Organizing response"),
            "active status sentence missing: {s}"
        );
        assert!(s.contains("Rupoo"), "brand missing: {s}");
        assert!(s.contains("claude-sonnet-4"), "model missing: {s}");
    }

    /// The bottom hint bar stays a single quiet line: it rotates through the
    /// tip pool but must NOT advertise any scrolling / keybinding reminders.
    #[test]
    fn hint_bar_rotates_tips_without_scroll_hint() {
        let mut view = ChatView::default();
        view.hint_index = (view.hint_index + 1) % HINT_TIPS.len();
        let backend = TestBackend::new(100, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let s = buffer_text(&term);
        assert!(
            s.contains(HINT_TIPS[1]),
            "rotated tip missing: {s} (expected '{}')",
            HINT_TIPS[1]
        );
        // The redundant scroll / mouse-wheel reminder must be gone.
        assert!(
            !s.contains("mouse wheel"),
            "redundant scroll hint must be removed: {s}"
        );
        assert!(
            !s.contains("scroll"),
            "no scroll reminder text in the hint bar: {s}"
        );
    }

    /// The bottom status panel collapses by default to a one-line summary of
    /// currently-running activities (path-free), driven by the status ring.
    #[test]
    fn status_panel_collapsed_shows_running_summary() {
        let mut view = ChatView::default();
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "web_search".into(),
                phase: ToolPhase::Calling,
            },
        );
        let backend = TestBackend::new(100, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        // CJK glyphs occupy two cells (second cell is a space placeholder), so
        // strip whitespace before substring checks.
        let compact: String = buffer_text(&term)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(compact.contains('⏺'), "running marker missing: {compact}");
        assert!(
            compact.contains("读取文件2"),
            "read_file x2 not summarized: {compact}"
        );
        assert!(
            compact.contains("网络搜索1"),
            "web_search x1 not summarized: {compact}"
        );
    }

    /// Expanded, the panel lists recent activities (newest at the bottom) with a
    /// header — still path-free. The panel caps at 3 rows, so two events keep
    /// the header on screen.
    #[test]
    fn status_panel_expanded_lists_activities() {
        let mut view = ChatView::default();
        view.status_expanded = true;
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Completed,
            },
        );
        let backend = TestBackend::new(100, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let compact: String = buffer_text(&term)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(
            compact.contains("运行活动(2)"),
            "header+count missing: {compact}"
        );
        assert!(
            compact.contains("读取文件"),
            "read_file not listed: {compact}"
        );
        assert!(
            compact.contains('✓'),
            "completed tool should show done glyph: {compact}"
        );
    }

    /// Unknown tool names fall back to the raw name (no panic, no blank label).
    #[test]
    fn kind_label_falls_back_to_raw_name() {
        assert_eq!(kind_label("mystery_tool"), "mystery_tool");
        assert_eq!(kind_label("read_file"), "读取文件");
    }

    /// Expanding on a tiny terminal must not panic and still paints the panel
    /// (chat + input floors are preserved).
    #[test]
    fn status_panel_expanded_on_tiny_terminal_no_panic() {
        let mut view = ChatView::default();
        view.status_expanded = true;
        view.input = "hello".into();
        apply_event(
            &mut view,
            &AgentToTui::ToolStatus {
                tool_name: "read_file".into(),
                phase: ToolPhase::Calling,
            },
        );
        let backend = TestBackend::new(40, 6);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let compact: String = buffer_text(&term)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(
            compact.contains("运行活动"),
            "panel still renders expanded: {compact}"
        );
    }

    /// User messages render right-aligned (IM / chat-bubble style), so the
    /// user's own text hugs the right edge like WeChat / Feishu. A colored `›`
    /// marker sits at the far right to tag the role when reading history.
    #[test]
    fn user_message_right_aligned_like_im() {
        let mut view = ChatView::default();
        view.items.push(StreamItem::User("hello".into()));
        let backend = TestBackend::new(20, 5);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        // Chat stream is chunks[1]; the status bar occupies row 0, so the user
        // line is row 1 (cells 20..40). "hello ›" (7 cols) in a 20-wide area
        // right-aligns to columns 13..20, so "hello" starts at column 13.
        let row: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .skip(20)
            .take(20)
            .map(|c| c.symbol())
            .collect();
        let idx = row.find("hello").expect("user text must render");
        assert!(
            idx >= 12,
            "user message should hug the right edge, got col {idx}: '{row}'"
        );
        // The role marker is present at the right edge.
        assert!(
            row.contains(USER_MARKER),
            "user role marker missing: '{row}'"
        );
    }

    /// Assistant messages stay left-aligned (the counterpart to the right-aligned
    /// user messages), keeping the classic chat layout.
    #[test]
    fn assistant_message_left_aligned() {
        let mut view = ChatView::default();
        view.items.push(StreamItem::Assistant("reply".into()));
        let backend = TestBackend::new(20, 5);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        let row: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .skip(20)
            .take(20)
            .map(|c| c.symbol())
            .collect();
        let idx = row.find("reply").expect("assistant text must render");
        assert_eq!(
            idx, 0,
            "assistant message should hug the left edge: '{row}'"
        );
    }

    #[test]
    fn guide_overlay_renders_three_sections() {
        let mut view = ChatView::default();
        view.guide = Some(GuideOverlay {
            dismiss_checked: false,
            scroll: 0,
        });
        let backend = TestBackend::new(90, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render_frame(f, &mut view)).unwrap();
        // Fullwidth CJK glyphs occupy two buffer cells (the second is a space
        // placeholder), so strip spaces before substring checks.
        let s = buffer_text(&term).replace(' ', "");
        assert!(s.contains("使用指南"), "guide title missing");
        assert!(s.contains("/model"), "model section missing");
        assert!(s.contains("飞书"), "channel section missing");
        assert!(s.contains("不再显示"), "dismiss option missing");
    }

    #[test]
    fn token_footer_pushed_after_reply() {
        let mut view = ChatView::default();
        view.pending_assistant = "hello".into();
        view.finalize_assistant();
        // Simulate 1200 in / 800 out of cumulative usage this session.
        view.token_in_total = 1200;
        view.token_out_total = 800;
        view.push_token_footer(3.4);
        let footer = view
            .items
            .iter()
            .find_map(|i| match i {
                StreamItem::TokenStat(s) => Some(s.clone()),
                _ => None,
            })
            .expect("token footer should be pushed");
        assert!(footer.contains("3.4s"), "duration missing: {footer}");
        assert!(footer.contains("Σ1.2k in"), "cumulative in wrong: {footer}");
        assert!(
            footer.contains("Σ800 out"),
            "cumulative out wrong: {footer}"
        );
        assert!(
            !footer.contains("+"),
            "per-turn delta should be omitted from the concise footer: {footer}"
        );
    }

    #[test]
    fn token_footer_skipped_when_no_reply() {
        let mut view = ChatView::default();
        view.push_token_footer(0.0);
        assert!(
            !view
                .items
                .iter()
                .any(|i| matches!(i, StreamItem::TokenStat(_))),
            "no footer without an assistant reply"
        );
    }

    #[test]
    fn token_footer_cumulative_reflects_session_total() {
        let mut view = ChatView::default();
        view.pending_assistant = "hi".into();
        view.finalize_assistant();
        // 500 in before this turn, then 700 more -> 1200 cumulative; out 300.
        view.token_in_total = 1200;
        view.token_out_total = 300;
        view.push_token_footer(1.0);
        let footer = view
            .items
            .iter()
            .find_map(|i| match i {
                StreamItem::TokenStat(s) => Some(s.clone()),
                _ => None,
            })
            .expect("token footer should be pushed");
        assert!(footer.contains("Σ1.2k in"), "cumulative in wrong: {footer}");
        assert!(
            footer.contains("Σ300 out"),
            "cumulative out wrong: {footer}"
        );
    }
}
