//! Where and how "input blocked" notices are shown (Transfer Bar), plus their repeat throttle.

use std::collections::HashMap;
use std::sync::Mutex;

/// Minimum spacing between notices for the same reason + process.
pub const NOTICE_REPEAT_MS: u64 = 30_000;

/// Which PC's Transfer Bar shows the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeSurface {
    /// The Linked PC itself (the user's cursor is there).
    Local,
    /// The Main PC, via an `InputBlocked` packet (Linked PC shows a secure screen).
    Main,
}

pub fn surface_for(reason: &str) -> NoticeSurface {
    match reason {
        "secure_desktop" => NoticeSurface::Main,
        // elevated_foreground, sendinput_zero, unknown/legacy reasons
        _ => NoticeSurface::Local,
    }
}

pub fn message_for(reason: &str, proc_name: &str) -> String {
    if reason == "secure_desktop" {
        "Linked PC is showing a secure screen (UAC prompt / lock screen) — it can't be controlled remotely."
            .to_string()
    } else {
        format!(
            "Input blocked: {proc_name} is running as administrator. Run AnyLeap as administrator on this PC to control it."
        )
    }
}

/// At most one notice per `NOTICE_REPEAT_MS` for the same reason + process.
#[derive(Default)]
pub struct NoticeThrottle {
    last: HashMap<String, u64>,
}

impl NoticeThrottle {
    pub fn allow(&mut self, reason: &str, proc_name: &str, now_ms: u64) -> bool {
        let key = format!("{reason}|{proc_name}");
        match self.last.get(&key) {
            Some(prev) if now_ms.saturating_sub(*prev) < NOTICE_REPEAT_MS => false,
            _ => {
                self.last.insert(key, now_ms);
                true
            }
        }
    }
}

static THROTTLE: Mutex<Option<NoticeThrottle>> = Mutex::new(None);

/// Throttled check against the process-wide throttle.
pub fn should_show(reason: &str, proc_name: &str) -> bool {
    let now_ms = super::debug_log::now_us() / 1000;
    THROTTLE
        .lock()
        .map(|mut g| {
            g.get_or_insert_with(NoticeThrottle::default)
                .allow(reason, proc_name, now_ms)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_per_reason() {
        assert_eq!(surface_for("elevated_foreground"), NoticeSurface::Local);
        assert_eq!(surface_for("sendinput_zero"), NoticeSurface::Local);
        assert_eq!(surface_for("secure_desktop"), NoticeSurface::Main);
        assert_eq!(surface_for(""), NoticeSurface::Local);
    }

    #[test]
    fn messages() {
        assert_eq!(
            message_for("elevated_foreground", "taskmgr.exe"),
            "Input blocked: taskmgr.exe is running as administrator. Run AnyLeap as administrator on this PC to control it."
        );
        assert!(message_for("secure_desktop", "x").contains("secure screen"));
    }

    #[test]
    fn throttle_30s_per_key() {
        let mut t = NoticeThrottle::default();
        assert!(t.allow("a", "p", 1000));
        assert!(!t.allow("a", "p", 30_999));
        assert!(t.allow("a", "q", 2000));
        assert!(t.allow("b", "p", 2000));
        assert!(t.allow("a", "p", 31_000));
    }
}
