//! US physical key positions mapped to Linux evdev codes (no XKB +8 offset).
use eframe::egui::{Key, Modifiers, PointerButton};
use gliff_proto::ClientMsg;
use std::collections::BTreeSet;

pub fn keycode(key: Key) -> Option<u32> {
    use Key::*;
    Some(match key {
        Escape => 1,
        Num1 => 2,
        Num2 => 3,
        Num3 => 4,
        Num4 => 5,
        Num5 => 6,
        Num6 => 7,
        Num7 => 8,
        Num8 => 9,
        Num9 => 10,
        Num0 => 11,
        Minus => 12,
        Equals => 13,
        Backspace => 14,
        Tab => 15,
        Q => 16,
        W => 17,
        E => 18,
        R => 19,
        T => 20,
        Y => 21,
        U => 22,
        I => 23,
        O => 24,
        P => 25,
        OpenBracket => 26,
        CloseBracket => 27,
        Enter => 28,
        A => 30,
        S => 31,
        D => 32,
        F => 33,
        G => 34,
        H => 35,
        J => 36,
        K => 37,
        L => 38,
        Semicolon => 39,
        Quote => 40,
        Backtick => 41,
        Backslash => 43,
        Z => 44,
        X => 45,
        C => 46,
        V => 47,
        B => 48,
        N => 49,
        M => 50,
        Comma => 51,
        Period => 52,
        Slash => 53,
        Space => 57,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        F11 => 87,
        F12 => 88,
        Home => 102,
        ArrowUp => 103,
        PageUp => 104,
        ArrowLeft => 105,
        ArrowRight => 106,
        End => 107,
        ArrowDown => 108,
        PageDown => 109,
        Insert => 110,
        Delete => 111,
        _ => return None,
    })
}

pub fn button_code(button: PointerButton) -> u32 {
    match button {
        PointerButton::Primary => 272,
        PointerButton::Secondary => 273,
        PointerButton::Middle => 274,
        PointerButton::Extra1 => 275,
        PointerButton::Extra2 => 276,
    }
}

#[derive(Default)]
pub struct Held {
    keys: BTreeSet<u32>,
    buttons: BTreeSet<u32>,
    shortcuts: Vec<(u8, Modifiers)>,
}

impl Held {
    pub fn key(&mut self, code: u32, pressed: bool) -> Vec<ClientMsg> {
        let changed = if pressed {
            self.keys.insert(code)
        } else {
            self.keys.remove(&code)
        };
        if changed {
            vec![ClientMsg::Key {
                keycode: code,
                pressed,
            }]
        } else {
            vec![]
        }
    }
    /// egui replaces some physical presses with Copy/Cut/Paste events.
    /// Retain modifiers until the corresponding physical key-up arrives.
    pub fn shortcut(&mut self, kind: u8, modifiers: Modifiers) {
        if self.shortcuts.len() < 8 {
            self.shortcuts.push((kind, modifiers));
        }
    }
    pub fn key_up(&mut self, code: u32, current: Modifiers) -> Vec<ClientMsg> {
        if self.keys.contains(&code) {
            return self.key(code, false);
        }
        let index = self.shortcuts.iter().position(|(kind, _)| match kind {
            0 => matches!(code, 46 | 110), // C or Insert
            1 => matches!(code, 45 | 111), // X or Delete
            _ => matches!(code, 47 | 110), // V or Insert
        });
        let modifiers = if let Some(index) = index {
            self.shortcuts.remove(index).1
        } else if (current.ctrl && matches!(code, 45..=47 | 110))
            || (current.shift && matches!(code, 110 | 111))
        {
            current
        } else {
            return vec![];
        };
        let mut messages = self.modifiers(modifiers);
        messages.extend(self.key(code, true));
        messages.extend(self.key(code, false));
        messages.extend(self.modifiers(current));
        messages
    }
    pub fn button(&mut self, code: u32, pressed: bool) -> Vec<ClientMsg> {
        let changed = if pressed {
            self.buttons.insert(code)
        } else {
            self.buttons.remove(&code)
        };
        if changed {
            vec![ClientMsg::PointerButton {
                button: code,
                pressed,
            }]
        } else {
            vec![]
        }
    }
    pub fn modifiers(&mut self, modifiers: Modifiers) -> Vec<ClientMsg> {
        [
            (42, modifiers.shift),
            (29, modifiers.ctrl),
            (56, modifiers.alt),
            (125, modifiers.mac_cmd),
        ]
        .into_iter()
        .flat_map(|(code, pressed)| self.key(code, pressed))
        .collect()
    }
    pub fn observe(&mut self, msg: &ClientMsg) {
        match *msg {
            ClientMsg::Key { keycode, pressed } => {
                self.key(keycode, pressed);
            }
            ClientMsg::PointerButton { button, pressed } => {
                self.button(button, pressed);
            }
            _ => {}
        }
    }
    pub fn release(&mut self) -> Vec<ClientMsg> {
        self.shortcuts.clear();
        let keys = std::mem::take(&mut self.keys)
            .into_iter()
            .map(|keycode| ClientMsg::Key {
                keycode,
                pressed: false,
            });
        let buttons =
            std::mem::take(&mut self.buttons)
                .into_iter()
                .map(|button| ClientMsg::PointerButton {
                    button,
                    pressed: false,
                });
        keys.chain(buttons).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn us_physical_mapping_is_evdev() {
        assert_eq!(keycode(Key::A), Some(30));
        assert_eq!(keycode(Key::Q), Some(16));
        assert_eq!(keycode(Key::Backslash), Some(43));
        assert_eq!(keycode(Key::ArrowLeft), Some(105));
        assert_eq!(keycode(Key::F12), Some(88));
        assert_eq!(keycode(Key::F20), None);
    }
    #[test]
    fn modifiers_and_releases_are_complete_and_idempotent() {
        let mut held = Held::default();
        assert_eq!(
            held.modifiers(Modifiers {
                shift: true,
                ctrl: true,
                ..Modifiers::NONE
            })
            .len(),
            2
        );
        assert!(held.key(42, true).is_empty());
        held.key(30, true);
        held.button(272, true);
        let release = held.release();
        assert_eq!(release.len(), 4);
        assert!(release.iter().all(|m| matches!(
            m,
            ClientMsg::Key { pressed: false, .. } | ClientMsg::PointerButton { pressed: false, .. }
        )));
        assert!(held.release().is_empty());
    }
    #[test]
    fn swallowed_shortcuts_use_physical_position_and_never_stick() {
        let mut held = Held::default();
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        held.shortcut(0, ctrl);
        let messages = held.key_up(110, Modifiers::NONE); // Ctrl+Insert, Ctrl released first
        assert!(messages.contains(&ClientMsg::Key {
            keycode: 110,
            pressed: true
        }));
        assert!(!messages.contains(&ClientMsg::Key {
            keycode: 46,
            pressed: true
        }));
        assert!(held.release().is_empty());
        held.shortcut(
            1,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        );
        let messages = held.key_up(111, Modifiers::NONE);
        assert!(messages.contains(&ClientMsg::Key {
            keycode: 111,
            pressed: false
        }));
        assert!(held.release().is_empty());
    }
}
