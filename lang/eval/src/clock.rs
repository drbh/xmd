//! The process clock every host reads a request's "now" from.
//!
//! Tests need that clock to be reproducible, so `WTF_NOW` freezes it: set it
//! to an RFC3339 timestamp (for example `2026-09-16T14:00:00-04:00`) and
//! [`now`] returns that instant for the life of the process instead of
//! reading the system clock. An unset or unparseable value is ignored and the
//! real clock is used.

/// The current time, or the instant pinned by the `WTF_NOW` environment
/// variable (read once, at startup).
pub fn now() -> chrono::DateTime<chrono::FixedOffset> {
    static FROZEN: std::sync::OnceLock<Option<chrono::DateTime<chrono::FixedOffset>>> =
        std::sync::OnceLock::new();
    (*FROZEN.get_or_init(|| {
        std::env::var("WTF_NOW")
            .ok()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text.trim()).ok())
    }))
    .unwrap_or_else(|| chrono::Local::now().fixed_offset())
}
