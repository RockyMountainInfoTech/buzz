//! Display-only machine name carried on member-visible events.
//!
//! The string is self-reported. Callers may show it. Nothing in this module,
//! and nothing that consumes it, may authorize or rank a turn on it.

/// Maximum machine-name length, matching desktop `MAX_MACHINE_NAME_LEN`.
pub const MAX_BODY_LABEL_LEN: usize = 64;

/// A machine name safe to put on a member-visible event.
///
/// Trims whitespace. Returns `None` for empty input, names longer than
/// [`MAX_BODY_LABEL_LEN`] characters, and names that contain a control
/// character. The same rules gate typing-indicator tags and `BUZZ_BODY_ID`.
pub fn publishable(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_BODY_LABEL_LEN {
        return None;
    }
    if name.chars().any(char::is_control) {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_trimmed_machine_name() {
        assert_eq!(
            publishable("  MacMiniM5Pro  ").as_deref(),
            Some("MacMiniM5Pro")
        );
    }

    #[test]
    fn rejects_empty_control_and_overlong_names() {
        assert_eq!(publishable(""), None);
        assert_eq!(publishable("   "), None);
        assert_eq!(publishable("bad\nname"), None);
        assert_eq!(
            publishable(&"x".repeat(MAX_BODY_LABEL_LEN)),
            Some("x".repeat(MAX_BODY_LABEL_LEN))
        );
        assert_eq!(publishable(&"x".repeat(MAX_BODY_LABEL_LEN + 1)), None);
    }
}
