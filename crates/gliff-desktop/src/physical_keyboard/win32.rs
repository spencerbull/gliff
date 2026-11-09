//! Own-thread, own-HWND message observation. Never installs a global hook and
//! never consumes a message: winit/DefWindowProc retain all OS shortcut behavior.
use super::{Capture, InputEvent, Keyboard};
use std::{
    sync::{Arc, Mutex},
    thread::{self, ThreadId},
};
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::KeyboardAndMouse::{
            GetFocus, GetKeyState, MapVirtualKeyW, MAPVK_VK_TO_VSC_EX, VK_LCONTROL, VK_LMENU,
            VK_LSHIFT, VK_LWIN, VK_PAUSE, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
        },
        WindowsAndMessaging::{
            MSG, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
            WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};
use winit::{
    platform::windows::EventLoopBuilderExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};

type Target = Arc<Mutex<Option<(ThreadId, isize)>>>;

#[derive(Clone, Default)]
pub struct NativeKeyboard {
    input: Keyboard,
    target: Target,
}
impl NativeKeyboard {
    pub fn install(mut options: eframe::NativeOptions) -> (eframe::NativeOptions, Self) {
        let keyboard = Self::default();
        let observer = keyboard.clone();
        options.event_loop_builder = Some(Box::new(move |builder| {
            builder.with_msg_hook(move |message| {
                // SAFETY: winit 0.30 calls this synchronously with a live, aligned
                // Win32 MSG pointer from PeekMessageW. It is read only here and
                // neither the pointer nor a reference escapes this callback.
                let message = unsafe { &*message.cast::<MSG>() };
                observer.message(message);
                false // Never swallow system/local input or bypass winit dispatch.
            });
        }));
        (options, keyboard)
    }
    pub fn bind(
        &self,
        context: &eframe::CreationContext<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let RawWindowHandle::Win32(window) = context.window_handle()?.as_raw() else {
            return Err("Gliff keyboard capture requires a Win32 window".into());
        };
        *self.target.lock().unwrap() = Some((thread::current().id(), window.hwnd.get()));
        Ok(())
    }
    pub fn capture(&self, owns: impl Fn([i32; 2]) -> bool) -> Option<Capture> {
        self.input.capture_picture(owns)
    }
    pub fn disable(&self) {
        self.input.disable();
    }
    pub fn discard_candidate(&self) {
        self.input.discard_candidate();
    }
    fn message(&self, message: &MSG) {
        let Some((owner, hwnd)) = *self.target.lock().unwrap() else {
            return;
        };
        if thread::current().id() != owner || message.hwnd as isize != hwnd {
            return;
        }
        // SAFETY: GetFocus has no pointer arguments and reads this UI thread's
        // focus only. Never attach thread input or query global async key state.
        if unsafe { GetFocus() } != message.hwnd {
            self.input.disable();
            return;
        }
        match message.message {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_LBUTTONUP
            | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                let pressed = matches!(
                    message.message,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                );
                if pressed {
                    let mut held = Vec::with_capacity(8);
                    for (vk, code) in [
                        (VK_LCONTROL, 29),
                        (VK_RCONTROL, 97),
                        (VK_LSHIFT, 42),
                        (VK_RSHIFT, 54),
                        (VK_LMENU, 56),
                        (VK_RMENU, 100),
                        (VK_LWIN, 125),
                        (VK_RWIN, 126),
                    ] {
                        // SAFETY: valid modifier VKs, sampled from this thread's
                        // message-queue state at the provisional activation boundary.
                        if unsafe { GetKeyState(i32::from(vk)) } < 0 {
                            held.push(code);
                        }
                    }
                    self.input.pointer_down(&held);
                }
                let code = match message.message {
                    WM_LBUTTONDOWN | WM_LBUTTONUP => 272,
                    WM_RBUTTONDOWN | WM_RBUTTONUP => 273,
                    WM_MBUTTONDOWN | WM_MBUTTONUP => 274,
                    _ => match (message.wParam >> 16) as u16 {
                        1 => 275,
                        2 => 276,
                        _ => return,
                    },
                };
                self.input.pointer(InputEvent::Button {
                    position: position(message.lParam),
                    code,
                    pressed,
                });
            }
            WM_MOUSEMOVE => self
                .input
                .pointer(InputEvent::Motion(position(message.lParam))),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let [x, y] = position(message.lParam);
                let mut point = POINT { x, y };
                // SAFETY: own live HWND and writable stack POINT, neither retained.
                // Wheel LPARAM is in screen pixels; every other pointer message is client-local.
                if unsafe { ScreenToClient(message.hwnd, &mut point) } == 0 {
                    self.input.disable();
                    return;
                }
                self.input.pointer(InputEvent::Wheel {
                    position: [point.x, point.y],
                    horizontal: message.message == WM_MOUSEHWHEEL,
                    delta: (message.wParam >> 16) as i16,
                });
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP if self.input.interested() => {
                if message.wParam == usize::from(VK_PAUSE) {
                    return;
                } // E1 sequence
                let bits = message.lParam as usize;
                let code = super::message_key(bits, || {
                    // SAFETY: value-only Win32 lookup, no pointers. Automation may
                    // post VK-only messages; physical messages always win here.
                    unsafe { MapVirtualKeyW(message.wParam as u32, MAPVK_VK_TO_VSC_EX) }
                });
                if let Some(code) = code {
                    let pressed = matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN);
                    self.input.key(code, pressed, bits & (1 << 30) != 0);
                }
            }
            _ => {}
        }
    }
}

fn position(bits: isize) -> [i32; 2] {
    [i32::from(bits as i16), i32::from((bits >> 16) as i16)]
}
