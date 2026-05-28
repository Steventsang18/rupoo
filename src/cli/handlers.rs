//! Input handling — REPL mode uses rustyline directly.

use super::app::RupooApp;

/// No-op dispatch — kept for compatibility, REPL uses rustyline.
pub fn dispatch(_app: &mut RupooApp) -> bool {
    false
}
