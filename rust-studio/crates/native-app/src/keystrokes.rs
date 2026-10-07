//! Short-lived screencast labels for the document viewport.
use iced::keyboard::{key::Named, Key, Modifiers};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const CHIP_TTL: Duration = Duration::from_millis(2_200);
const MAX_CHIPS: usize = 5;

#[derive(Debug)]
pub struct Chip {
    pub label: String,
    pub count: u32,
    expires: Instant,
}

#[derive(Debug, Default)]
pub struct Keystrokes {
    pub visible: bool,
    pub chips: VecDeque<Chip>,
    timer_running: bool,
}

impl Keystrokes {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.chips.clear();
    }

    /// Returns a delay only when a single expiry task needs to be started.
    pub fn record(&mut self, label: String, now: Instant) -> Option<Duration> {
        if !self.visible {
            return None;
        }
        if self.chips.back().is_some_and(|last| last.label == label) {
            let last = self.chips.back_mut().expect("checked above");
            last.count = last.count.saturating_add(1);
            last.expires = now + CHIP_TTL;
        } else {
            if self.chips.len() == MAX_CHIPS {
                self.chips.pop_front();
            }
            self.chips.push_back(Chip {
                label,
                count: 1,
                expires: now + CHIP_TTL,
            });
        }
        if self.timer_running {
            None
        } else {
            self.timer_running = true;
            Some(CHIP_TTL)
        }
    }

    /// Re-arm just one timer for the next expiry, including repeated keys.
    pub fn expire(&mut self, now: Instant) -> Option<Duration> {
        self.chips.retain(|chip| chip.expires > now);
        if let Some(first) = self.chips.front() {
            Some(first.expires.saturating_duration_since(now))
        } else {
            self.timer_running = false;
            None
        }
    }
}

pub fn label(key: &Key, modifiers: Modifiers, captured: bool, modal_open: bool) -> Option<String> {
    let shortcut = modifiers.control() || modifiers.alt() || modifiers.logo();
    // Iced marks keys consumed by text fields as Captured. Modal dialogs may
    // contain path/text inputs even when a widget does not consume a key.
    if (captured || modal_open) && !shortcut {
        return None;
    }
    let (name, special) = match key {
        Key::Named(Named::Control | Named::Shift | Named::Alt | Named::AltGraph)
        | Key::Named(Named::Meta | Named::Super | Named::Hyper | Named::Fn)
        | Key::Unidentified => return None,
        Key::Named(Named::Space) => ("Space".to_owned(), true),
        Key::Named(Named::Escape) => ("Esc".to_owned(), true),
        Key::Named(Named::Backspace) => ("⌫".to_owned(), true),
        Key::Named(Named::Delete) => ("Del".to_owned(), true),
        Key::Named(Named::ArrowUp) => ("↑".to_owned(), true),
        Key::Named(Named::ArrowDown) => ("↓".to_owned(), true),
        Key::Named(Named::ArrowLeft) => ("←".to_owned(), true),
        Key::Named(Named::ArrowRight) => ("→".to_owned(), true),
        Key::Named(named) => (format!("{named:?}"), true),
        Key::Character(value) => {
            if value.is_empty() || value.len() > 32 || value.chars().any(char::is_control) {
                return None;
            }
            (value.to_uppercase(), false)
        }
    };
    let mut parts = Vec::with_capacity(4);
    if modifiers.control() || modifiers.logo() {
        parts.push("Ctrl");
    }
    if modifiers.alt() {
        parts.push("Alt");
    }
    if modifiers.shift() && special {
        parts.push("Shift");
    }
    parts.push(&name);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_hides_plain_keys_but_shows_shortcuts() {
        let a = Key::Character("secret".into());
        assert_eq!(label(&a, Modifiers::default(), true, false), None);
        assert_eq!(label(&a, Modifiers::default(), false, true), None);
        assert_eq!(
            label(&Key::Character("s".into()), Modifiers::CTRL, true, true),
            Some("Ctrl+S".into())
        );
        assert_eq!(
            label(&Key::Named(Named::Control), Modifiers::CTRL, false, false),
            None
        );
    }

    #[test]
    fn repeats_bump_one_chip_and_expire_after_last_press() {
        let mut state = Keystrokes::default();
        state.toggle();
        let start = Instant::now();
        assert_eq!(state.record("G".into(), start), Some(CHIP_TTL));
        assert_eq!(
            state.record("G".into(), start + Duration::from_secs(1)),
            None
        );
        assert_eq!(state.chips[0].count, 2);
        assert!(state.expire(start + CHIP_TTL).is_some());
        assert_eq!(state.chips.len(), 1);
        assert_eq!(state.expire(start + Duration::from_secs(4)), None);
        assert!(state.chips.is_empty());
        assert_eq!(
            state.record("X".into(), start + Duration::from_secs(5)),
            Some(CHIP_TTL)
        );
        state.toggle();
        assert!(state.chips.is_empty());
    }

    #[test]
    fn caps_history_without_piling_up_timers() {
        let mut state = Keystrokes::default();
        state.toggle();
        let start = Instant::now();
        for i in 0..10 {
            let delay = state.record(format!("F{i}"), start + Duration::from_millis(i));
            assert_eq!(delay.is_some(), i == 0);
        }
        assert_eq!(state.chips.len(), MAX_CHIPS);
        assert_eq!(state.chips.front().unwrap().label, "F5");
    }
}
