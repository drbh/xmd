//! The one symbol vocabulary every label uses: inlays, badges, hovers, the
//! outline and the CLI. Only glyphs from blocks every monospace font covers
//! and that have no emoji form, so Zed, VS Code, the terminal, the browser and
//! the book all show the same thing at the same width.
pub const DONE: &str = "✓";
pub const FAIL: &str = "✗";
pub const ON: &str = "●";
pub const OFF: &str = "○";
pub const HALF: &str = "◐";
pub const PENDING: &str = "◌";
pub const FLAG: &str = "⚑";
pub const ALERT: &str = "!";
pub const BLOCKED: &str = "⊘";
pub const ARROW: &str = "→";
pub const REPEAT: &str = "↻";
pub const COUNTDOWN: &str = "◷";
pub const STOPWATCH: &str = "◴";
pub const RUNNING: &str = "▸";
pub const PAUSED: &str = "‖";
pub const CHOSEN: &str = "☑";
pub const UNCHOSEN: &str = "☐";

/// The glyph for a timer state name.
pub fn timer_state(state: &str) -> &'static str {
    match state {
        "running" => RUNNING,
        "paused" => PAUSED,
        "done" => DONE,
        _ => OFF,
    }
}
