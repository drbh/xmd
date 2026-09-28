//! The one symbol vocabulary every label uses: inlays, badges, hovers, the
//! outline and the CLI. Only glyphs from blocks every monospace font covers
//! and that have no emoji form, so Zed, VS Code, the terminal, the browser and
//! the book all show the same thing at the same width.
//!
//! Code lenses and row commands reuse the same set so a row of actions scans as
//! a row of symbols: the glyph leads, and at most one lowercase word follows
//! when the glyph alone would be ambiguous — "✓ done", "○ reopen", "↻ next",
//! "↗ open", "⟳ lookups", "▸ start", "‖ pause", "↺ reset", "⚑ today".
pub const DONE: &str = "✓";
pub const OFF: &str = "○";
pub const FLAG: &str = "⚑";
pub const REPEAT: &str = "↻";
pub const OPEN: &str = "↗";
pub const REFRESH: &str = "⟳";
