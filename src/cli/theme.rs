//! Terminal color theme — runtime-switchable color palette.
//!
//! Three built-in themes:
//!   - Dark (default): GitHub Dark Dimmed + Catppuccin Mocha
//!   - Light: Solarized Light
//!   - Monokai: base16-mocha inspired

use owo_colors::Rgb;
use std::sync::OnceLock;

// ═══════════════════════════════════════════════════════════════════════════
// Theme struct
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: &'static str,
    /// User message text (right-aligned)
    pub user_bright: Rgb,
    /// User separators / accent
    pub user_med: Rgb,
    /// User dim lines
    pub user_dim: Rgb,
    /// AI header label
    pub ai_header: Rgb,
    /// AI accent (list bullets, inline code)
    pub ai_accent: Rgb,
    /// Tool card borders/name
    pub tool_accent: Rgb,
    /// Tool result text
    pub tool_dim: Rgb,
    /// Thinking spinner/text
    pub think: Rgb,
    /// Error text
    pub error: Rgb,
    /// Dimmed text (line numbers, footer)
    pub dim: Rgb,
    /// Border / separator lines
    pub border: Rgb,
    /// Prompt symbol
    #[allow(dead_code)]
    pub prompt: Rgb,
    /// Cursor color
    pub cursor: Rgb,
    /// Syntect theme name for code highlighting
    pub code_theme: &'static str,
}

// ═══════════════════════════════════════════════════════════════════════════
// Built-in themes
// ═══════════════════════════════════════════════════════════════════════════

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "dark",
            user_bright: Rgb(0x7E, 0xE7, 0x87), // #7ee787
            user_med: Rgb(0x3F, 0xB9, 0x50),    // #3fb950
            user_dim: Rgb(0x23, 0x86, 0x36),    // #238636
            ai_header: Rgb(0x58, 0xA6, 0xFF),   // #58a6ff
            ai_accent: Rgb(0x79, 0xC0, 0xFF),   // #79c0ff
            tool_accent: Rgb(0xD2, 0xA8, 0xFF), // #d2a8ff
            tool_dim: Rgb(0x8B, 0x95, 0x9E),    // #8b949e
            think: Rgb(0xE3, 0xB3, 0x41),       // #e3b341
            error: Rgb(0xF8, 0x51, 0x49),       // #f85149
            dim: Rgb(0x48, 0x4F, 0x58),         // #484f58
            border: Rgb(0x30, 0x36, 0x3D),      // #30363d
            prompt: Rgb(0x3F, 0xB9, 0x50),      // #3fb950
            cursor: Rgb(0x3F, 0xB9, 0x50),      // #3fb950
            code_theme: "base16-ocean.dark",
        }
    }

    pub fn light() -> Self {
        Self {
            name: "light",
            user_bright: Rgb(0x1A, 0x7F, 0x37), // #1a7f37
            user_med: Rgb(0x23, 0x86, 0x36),    // #238636
            user_dim: Rgb(0x2E, 0xA0, 0x43),    // #2ea043
            ai_header: Rgb(0x05, 0x50, 0xAE),   // #0550ae
            ai_accent: Rgb(0x09, 0x69, 0xDA),   // #0969da
            tool_accent: Rgb(0x82, 0x5D, 0xB8), // #825db8
            tool_dim: Rgb(0x65, 0x6D, 0x76),    // #656d76
            think: Rgb(0x9A, 0x67, 0x00),       // #9a6700
            error: Rgb(0xCF, 0x22, 0x2E),       // #cf222e
            dim: Rgb(0x8B, 0x94, 0x9E),         // #8b949e
            border: Rgb(0xD0, 0xD7, 0xDE),      // #d0d7de
            prompt: Rgb(0x1A, 0x7F, 0x37),      // #1a7f37
            cursor: Rgb(0x1A, 0x7F, 0x37),      // #1a7f37
            code_theme: "InspiredGitHub",
        }
    }

    pub fn monokai() -> Self {
        Self {
            name: "monokai",
            user_bright: Rgb(0xA6, 0xE2, 0x2E), // #a6e22e
            user_med: Rgb(0x84, 0xB6, 0x21),    // #84b621
            user_dim: Rgb(0x5E, 0x8A, 0x17),    // #5e8a17
            ai_header: Rgb(0x66, 0xD9, 0xEF),   // #66d9ef
            ai_accent: Rgb(0x78, 0xD9, 0xEC),   // #78d9ec
            tool_accent: Rgb(0xAE, 0x81, 0xFF), // #ae81ff
            tool_dim: Rgb(0x75, 0x71, 0x5E),    // #75715e
            think: Rgb(0xE6, 0xDB, 0x74),       // #e6db74
            error: Rgb(0xF9, 0x26, 0x72),       // #f92672
            dim: Rgb(0x75, 0x71, 0x5E),         // #75715e
            border: Rgb(0x49, 0x48, 0x3E),      // #49483e
            prompt: Rgb(0xA6, 0xE2, 0x2E),      // #a6e22e
            cursor: Rgb(0xA6, 0xE2, 0x2E),      // #a6e22e
            code_theme: "base16-mocha.dark",
        }
    }

    pub fn all_names() -> &'static [&'static str] {
        &["dark", "light", "monokai"]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            "monokai" => Some(Self::monokai()),
            _ => None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Global theme state
// ═══════════════════════════════════════════════════════════════════════════

use std::sync::RwLock;

static CURRENT_THEME: OnceLock<RwLock<Theme>> = OnceLock::new();

fn theme_lock() -> &'static RwLock<Theme> {
    CURRENT_THEME.get_or_init(|| RwLock::new(Theme::dark()))
}

/// Get the current theme (read lock).
pub fn current() -> Theme {
    theme_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Set the active theme.
pub fn set(theme: Theme) {
    let mut guard = theme_lock().write().unwrap_or_else(|e| e.into_inner());
    *guard = theme;
}

/// Get current theme name.
pub fn current_name() -> &'static str {
    current().name
}
