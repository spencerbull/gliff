//! Bounded physical keyboard capture. No platform APIs or shortcut reconstruction.
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

const CAPACITY: usize = 256;
const MODIFIERS: [u32; 8] = [29, 97, 42, 54, 56, 100, 125, 126];

/// Translate a Set-1 position (without its E0 prefix) to Linux evdev.
/// E1/Pause and unknown positions are deliberately unsupported.
pub fn evdev(scan: u8, extended: bool) -> Option<u32> {
    if !extended {
        return matches!(scan, 1..=0x53 | 0x56..=0x58).then_some(u32::from(scan));
    }
    Some(match scan {
        0x1c => 96,  // keypad Enter
        0x1d => 97,  // right Ctrl
        0x35 => 98,  // keypad slash
        0x37 => 99,  // Print Screen
        0x38 => 100, // right Alt
        0x47 => 102,
        0x48 => 103,
        0x49 => 104,
        0x4b => 105,
        0x4d => 106,
        0x4f => 107,
        0x50 => 108,
        0x51 => 109,
        0x52 => 110,
        0x53 => 111,
        0x5b => 125, // left Windows
        0x5c => 126, // right Windows
        0x5d => 127, // menu
        _ => return None,
    })
}

/// Extract a physical position from a Win32 key message. The fallback is lazy:
/// real scan codes must never be replaced by a layout-dependent VK lookup.
pub fn message_key(bits: usize, fallback: impl FnOnce() -> u32) -> Option<u32> {
    let mut scan = ((bits >> 16) & 0xff) as u8;
    let mut extended = bits & (1 << 24) != 0;
    if scan == 0 {
        let mapped = fallback();
        if mapped >> 8 == 0xe1 {
            return None;
        }
        scan = mapped as u8;
        extended |= mapped >> 8 == 0xe0;
    }
    evdev(scan, extended)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub code: u32,
    pub pressed: bool,
}

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Off,
    Candidate,
    Active,
}

#[derive(Default)]
struct State {
    epoch: u64,
    phase: Phase,
    queue: VecDeque<Transition>,
    down: BTreeSet<u32>,
}
impl State {
    fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.phase = Phase::Off;
        self.queue.clear();
        self.down.clear();
    }
    fn key(&mut self, code: u32, pressed: bool, repeat: bool) {
        if self.phase == Phase::Off || (pressed && repeat) {
            return;
        }
        if pressed && code == 1 && (self.down.contains(&42) || self.down.contains(&54)) {
            // Detect at event time, even if Shift is released before the UI frame.
            self.reset();
            return;
        }
        let changed = if pressed {
            self.down.insert(code)
        } else {
            self.down.remove(&code)
        };
        if changed {
            if self.queue.len() == CAPACITY {
                // Losing a release is unsafe. Invalidate the entire capture lease;
                // its owner must release remote input before accepting a new click.
                self.reset();
            } else {
                self.queue.push_back(Transition { code, pressed });
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Keyboard(Arc<Mutex<State>>);
impl Keyboard {
    /// Mark an own-window pointer-down boundary before egui processes the click.
    /// Only modifier state is sampled; pre-click ordinary typing is never retained.
    pub fn pointer_down(&self, held_modifiers: &[u32]) {
        let mut state = self.0.lock().unwrap();
        if state.phase == Phase::Active {
            return;
        }
        state.reset();
        state.phase = Phase::Candidate;
        for &code in held_modifiers {
            if MODIFIERS.contains(&code) {
                state.key(code, true, false);
            }
        }
    }
    pub fn interested(&self) -> bool {
        self.0.lock().unwrap().phase != Phase::Off
    }
    pub fn key(&self, code: u32, pressed: bool, repeat: bool) {
        self.0.lock().unwrap().key(code, pressed, repeat);
    }
    /// Accept only events following the most recent provisional click. Hook and
    /// UI run on the same thread, so this promotion cannot interleave a message.
    pub fn capture(&self) -> Option<Capture> {
        let mut state = self.0.lock().unwrap();
        if state.phase != Phase::Candidate {
            return None;
        }
        state.phase = Phase::Active;
        Some(Capture {
            keyboard: self.clone(),
            epoch: state.epoch,
        })
    }
    pub fn disable(&self) {
        self.0.lock().unwrap().reset();
    }
    pub fn discard_candidate(&self) {
        let mut state = self.0.lock().unwrap();
        if state.phase == Phase::Candidate {
            state.reset();
        }
    }
}

/// A per-tab lease. Dropping it releases adapter ownership, including on tab,
/// connection or focus changes. Stale leases cannot disable newer captures.
pub struct Capture {
    keyboard: Keyboard,
    epoch: u64,
}
impl Capture {
    /// None means fail closed: the caller must release all remote keys/buttons.
    pub fn drain(&self) -> Option<Vec<Transition>> {
        let mut state = self.keyboard.0.lock().unwrap();
        (state.epoch == self.epoch && state.phase == Phase::Active)
            .then(|| state.queue.drain(..).collect())
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let mut state = self.keyboard.0.lock().unwrap();
        if state.epoch == self.epoch {
            state.reset();
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // The sole Win32 boundary; the queue/session/UI remain safe Rust.
pub mod win32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Held;
    use gliff_proto::ClientMsg;

    fn press(keyboard: &Keyboard, scan: u8, extended: bool, pressed: bool) {
        keyboard.key(evdev(scan, extended).unwrap(), pressed, false);
    }
    fn forward(capture: &Capture, held: &mut Held) -> Option<Vec<(u32, bool)>> {
        Some(
            capture
                .drain()?
                .into_iter()
                .flat_map(|key| held.key(key.code, key.pressed))
                .map(|msg| match msg {
                    ClientMsg::Key { keycode, pressed } => (keycode, pressed),
                    _ => unreachable!(),
                })
                .collect(),
        )
    }
    fn active(modifiers: &[u32]) -> (Keyboard, Capture, Held) {
        let keyboard = Keyboard::default();
        keyboard.pointer_down(modifiers);
        let capture = keyboard.capture().unwrap();
        (keyboard, capture, Held::default())
    }

    #[test]
    fn vk_only_automation_falls_back_without_overriding_physical_positions() {
        assert_eq!(
            message_key(0x2f << 16, || panic!("must use physical scan")),
            Some(47)
        );
        assert_eq!(message_key((0x1d << 16) | (1 << 24), || 0), Some(97));
        assert_eq!(message_key(0, || 0x2f), Some(47));
        assert_eq!(message_key(0, || 0xe01d), Some(97));
        assert_eq!(message_key(1 << 24, || 0x1c), Some(96));
        assert_eq!(message_key(0, || 0), None);
        assert_eq!(message_key(0, || 0xe11d), None);
    }

    #[test]
    fn clipboard_chords_keep_event_time_order_for_both_modifier_release_orders() {
        for releases in [[42, 29], [29, 42]] {
            let (keyboard, capture, mut held) = active(&[]);
            press(&keyboard, 0x1d, false, true); // Ctrl
            press(&keyboard, 0x2a, false, true); // Shift
            press(&keyboard, 0x2f, false, true); // V (not an egui Paste)
            for code in releases {
                keyboard.key(code, false, false);
            }
            press(&keyboard, 0x2f, false, false);
            assert_eq!(
                forward(&capture, &mut held).unwrap(),
                [
                    (29, true),
                    (42, true),
                    (47, true),
                    (releases[0], false),
                    (releases[1], false),
                    (47, false),
                ]
            );
            assert!(held.release().is_empty());
        }
    }

    #[test]
    fn shift_release_then_ordinary_ctrl_v_has_no_shift_or_delayed_reconstruction() {
        let (keyboard, capture, mut held) = active(&[29, 42]);
        assert_eq!(
            forward(&capture, &mut held).unwrap(),
            [(29, true), (42, true)]
        );
        keyboard.key(42, false, false);
        press(&keyboard, 0x2f, false, true);
        assert_eq!(
            forward(&capture, &mut held).unwrap(),
            [(42, false), (47, true)]
        );
        press(&keyboard, 0x2f, false, false);
        keyboard.key(29, false, false);
        assert_eq!(
            forward(&capture, &mut held).unwrap(),
            [(47, false), (29, false)]
        );
        assert!(held.release().is_empty());
    }

    #[test]
    fn copy_cut_insert_and_delete_are_ordinary_physical_transitions() {
        let (keyboard, capture, mut held) = active(&[97]);
        assert_eq!(forward(&capture, &mut held).unwrap(), [(97, true)]);
        for (scan, extended, code) in [
            (0x2e, false, 46),
            (0x2d, false, 45),
            (0x52, true, 110),
            (0x53, true, 111),
        ] {
            press(&keyboard, scan, extended, true);
            assert_eq!(forward(&capture, &mut held).unwrap(), [(code, true)]);
            press(&keyboard, scan, extended, false);
            assert_eq!(forward(&capture, &mut held).unwrap(), [(code, false)]);
        }
    }

    #[test]
    fn extended_keys_preserve_right_sides_and_distinguish_keypad_navigation() {
        let (keyboard, capture, mut held) = active(&[]);
        for (scan, extended, expected) in [
            (0x1d, false, 29),
            (0x1d, true, 97),
            (0x38, false, 56),
            (0x38, true, 100),
            (0x36, false, 54),
            (0x1c, false, 28),
            (0x1c, true, 96),
            (0x35, false, 53),
            (0x35, true, 98),
            (0x47, false, 71),
            (0x47, true, 102),
            (0x48, true, 103),
            (0x49, true, 104),
            (0x4b, true, 105),
            (0x4d, true, 106),
            (0x4f, true, 107),
            (0x50, true, 108),
            (0x51, true, 109),
            (0x52, true, 110),
            (0x53, true, 111),
            (0x5b, true, 125),
            (0x5c, true, 126),
            (0x5d, true, 127),
            (0x37, true, 99),
            (0x3a, false, 58),
            (0x52, false, 82),
        ] {
            press(&keyboard, scan, extended, true);
            press(&keyboard, scan, extended, false);
            assert_eq!(
                forward(&capture, &mut held).unwrap(),
                [(expected, true), (expected, false)]
            );
        }
        assert_eq!(evdev(0, false), None);
        assert_eq!(evdev(0x2a, true), None); // PrintScreen fake Shift
        assert_eq!(evdev(0x7f, false), None);
    }

    #[test]
    fn activation_keeps_post_click_events_but_never_pre_click_gui_typing() {
        let keyboard = Keyboard::default();
        keyboard.key(30, true, false); // local typing: no provisional boundary
        assert!(!keyboard.interested());
        assert!(keyboard.capture().is_none());
        keyboard.pointer_down(&[97, 54, 30]); // only L/R modifiers may be sampled
        keyboard.key(47, true, false); // after click, before egui processes it
        keyboard.key(54, false, false);
        let capture = keyboard.capture().unwrap();
        assert_eq!(
            forward(&capture, &mut Held::default()).unwrap(),
            [(97, true), (54, true), (47, true), (54, false)]
        );
    }

    #[test]
    fn rejected_or_replaced_click_discards_candidate_events() {
        let keyboard = Keyboard::default();
        keyboard.pointer_down(&[]);
        keyboard.key(30, true, false);
        keyboard.discard_candidate(); // local text field was clicked
        assert!(keyboard.capture().is_none());
        keyboard.pointer_down(&[42]);
        keyboard.key(30, true, false);
        keyboard.pointer_down(&[97]); // newest click owns the provisional queue
        keyboard.key(47, true, false);
        let capture = keyboard.capture().unwrap();
        keyboard.discard_candidate(); // does not clear accepted capture
        assert_eq!(
            forward(&capture, &mut Held::default()).unwrap(),
            [(97, true), (47, true)]
        );
        keyboard.pointer_down(&[]); // clicking again while active preserves order
        keyboard.key(47, false, false);
        assert_eq!(
            capture.drain().unwrap(),
            [Transition {
                code: 47,
                pressed: false
            }]
        );
    }

    #[test]
    fn repeats_and_unmatched_releases_never_make_duplicate_or_phantom_downs() {
        let (keyboard, capture, mut held) = active(&[]);
        keyboard.key(30, true, true); // key held before capture, not synchronized
        keyboard.key(30, false, false);
        assert!(forward(&capture, &mut held).unwrap().is_empty());
        keyboard.key(47, true, false);
        keyboard.key(47, true, true);
        keyboard.key(47, true, false); // synthetic repeat with no previous-state bit
        keyboard.key(47, false, false);
        keyboard.key(47, false, false);
        assert_eq!(
            forward(&capture, &mut held).unwrap(),
            [(47, true), (47, false)]
        );
    }

    #[test]
    fn overflow_invalidates_lease_and_requires_release_before_another_click() {
        let (keyboard, capture, mut held) = active(&[97]);
        forward(&capture, &mut held).unwrap();
        for _ in 0..300 {
            keyboard.key(47, true, false);
            keyboard.key(47, false, false);
        }
        assert!(!keyboard.interested());
        assert!(forward(&capture, &mut held).is_none());
        assert!(keyboard.capture().is_none());
        assert!(matches!(
            held.release().as_slice(),
            [ClientMsg::Key {
                keycode: 97,
                pressed: false
            }]
        ));
        keyboard.key(30, true, false);
        keyboard.pointer_down(&[]);
        let next = keyboard.capture().unwrap();
        drop(capture); // a stale owner cannot disable a newer epoch
        assert!(next.drain().unwrap().is_empty());
    }

    #[test]
    fn focus_loss_and_capture_drop_clear_queue_and_invalidate_old_owners() {
        let (keyboard, capture, mut held) = active(&[29, 54]);
        forward(&capture, &mut held).unwrap();
        keyboard.key(47, true, false);
        keyboard.disable(); // focus loss, including loss/regain in one UI frame
        assert!(capture.drain().is_none());
        assert_eq!(held.release().len(), 2);
        keyboard.pointer_down(&[]);
        let next = keyboard.capture().unwrap();
        drop(capture);
        keyboard.key(30, true, false);
        assert_eq!(forward(&next, &mut held).unwrap(), [(30, true)]);
        drop(next); // same release path as tab switch/disconnect/close
        assert!(!keyboard.interested());
        assert!(keyboard.capture().is_none());
        keyboard.pointer_down(&[]);
        assert!(keyboard.capture().unwrap().drain().unwrap().is_empty());
    }

    #[test]
    fn shift_escape_releases_at_event_time_for_either_shift_side() {
        for shift in [42, 54] {
            let (keyboard, capture, mut held) = active(&[shift]);
            forward(&capture, &mut held).unwrap();
            keyboard.key(30, true, false);
            forward(&capture, &mut held).unwrap();
            keyboard.key(1, true, false);
            keyboard.key(shift, false, false);
            keyboard.key(1, false, false);
            assert!(capture.drain().is_none());
            assert!(!keyboard.interested());
            assert_eq!(held.release().len(), 2); // Shift and A; Escape stays local
        }
        let (keyboard, capture, mut held) = active(&[]);
        keyboard.key(1, true, false);
        keyboard.key(1, false, false);
        assert_eq!(
            forward(&capture, &mut held).unwrap(),
            [(1, true), (1, false)]
        );
    }
}
