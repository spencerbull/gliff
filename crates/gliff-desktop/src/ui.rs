//! eframe/egui window adapter; no SSH, decode, or native clipboard I/O here.
use crate::{
    clipboard::NativeClipboard,
    config::{Options, Saved},
    geometry::Geometry,
    input::{button_code, keycode, Held},
    session::{Command, Session, MAX_TABS},
};
use eframe::egui::{
    self, Color32, Event, Key, MouseWheelUnit, Rect, Sense, TextureHandle, TextureOptions,
};
use gliff_proto::{Axis, ClientMsg};
use std::time::{Duration, Instant};

struct Tab {
    host: String,
    session: Option<Session>,
    texture: Option<TextureHandle>,
    picture_geometry: Option<Geometry>,
    cursor: Option<(u32, TextureHandle, [u32; 2], [i32; 2])>,
    held: Held,
    captured: bool,
    resize: Option<(u32, u32, u32)>,
    resized_at: Instant,
}
impl Tab {
    fn new(host: String) -> Self {
        Self {
            host,
            session: None,
            texture: None,
            picture_geometry: None,
            cursor: None,
            held: Held::default(),
            captured: false,
            resize: None,
            resized_at: Instant::now(),
        }
    }
    fn release(&mut self) {
        let active = !self.held.release().is_empty() || self.captured;
        self.captured = false;
        if active {
            if let Some(session) = &self.session {
                session.send(Command::Release);
            }
        }
    }
    fn input(&self, messages: Vec<ClientMsg>) {
        if !messages.is_empty() {
            if let Some(session) = &self.session {
                session.send(Command::Input(messages));
            }
        }
    }
}

pub struct Desktop {
    options: Options,
    address: String,
    tabs: Vec<Tab>,
    selected: usize,
    retired: Vec<Session>,
    clipboard: Option<NativeClipboard>,
    clipboard_text: Option<String>,
    clipboard_poll: Instant,
    saved: Saved,
    notice: String,
    remote_events: Option<Vec<Event>>,
}
impl Desktop {
    pub fn new(options: Options) -> Self {
        let saved = crate::config::state_path()
            .map(|p| Saved::load(&p))
            .unwrap_or_default();
        let (tabs, selected) = if options.connect.is_none() {
            restore_tabs(&saved)
        } else {
            (Vec::new(), 0)
        };
        let host = options.host.clone();
        let dev = options.connect.is_some();
        let mut result = Self {
            address: host.clone().unwrap_or_default(),
            tabs,
            selected,
            retired: Vec::new(),
            clipboard: (!options.no_clipboard).then(NativeClipboard::default),
            clipboard_text: None,
            clipboard_poll: Instant::now(),
            saved,
            notice: String::new(),
            remote_events: None,
            options,
        };
        if let Some(host) = host {
            result.add(host);
        } else if dev {
            result.add("Loopback development".into());
        }
        // Restored tabs are disconnected until explicitly reconnected.
        result
    }
    fn add(&mut self, host: String) {
        if self.tabs.len() >= MAX_TABS || self.worker_count() >= MAX_TABS {
            self.notice = "Maximum eight tabs; close a session first".into();
            return;
        }
        if self.options.connect.is_none() {
            if let Err(e) = crate::ssh::validate_host(&host) {
                self.notice = e.to_string();
                return;
            }
            self.saved.hosts.retain(|h| h != &host);
            self.saved.hosts.insert(0, host.clone());
            self.saved.hosts.truncate(16);
        }
        if let Some(tab) = self.tabs.get_mut(self.selected) {
            tab.release();
            if let Some(session) = &tab.session {
                session.set_clipboard_active(false);
            }
        }
        let mut tab = Tab::new(host.clone());
        tab.session = Some(Session::start(host, self.options.clone()));
        self.tabs.push(tab);
        self.selected = self.tabs.len() - 1;
        self.persist_tabs();
        self.clipboard_text = None;
        if let Some(clipboard) = &self.clipboard {
            clipboard.clear();
        }
    }
    fn worker_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|tab| tab.session.as_ref().is_some_and(|s| !s.finished()))
            .count()
            + self.retired.len()
    }
    fn persist_tabs(&mut self) {
        if self.options.connect.is_some() {
            return;
        }
        self.saved.tabs = self.tabs.iter().map(|tab| tab.host.clone()).collect();
        self.saved.selected = self.selected;
        if let Some(path) = crate::config::state_path() {
            if let Err(e) = self.saved.save(&path) {
                self.notice = format!("Cannot save tabs: {e}");
            }
        }
    }
    fn retire(&mut self, session: Session) {
        session.disconnect();
        self.retired.push(session);
    }
}
impl eframe::App for Desktop {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.remote_events = None;
        if raw.focused && self.tabs.get(self.selected).is_some_and(|tab| tab.captured) {
            self.remote_events = Some(raw.events.clone());
            // Keep egui from using Tab/Space/Enter or remote shortcuts to
            // navigate or activate local controls while the picture owns input.
            raw.events.retain(|event| {
                !matches!(
                    event,
                    Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)
                )
            });
        }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));
        self.retired.retain(|session| !session.finished());
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("SSH host");
                let edit = ui.add(egui::TextEdit::singleline(&mut self.address).hint_text("user@host or SSH alias"));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (ui.button("Connect in new tab").clicked() || enter) && self.options.connect.is_none() {
                    self.add(self.address.trim().to_owned());
                }
                egui::ComboBox::from_id_salt("recent").selected_text("Recent hosts").show_ui(ui, |ui| {
                    for host in &self.saved.hosts {
                        if ui.selectable_label(false, host).clicked() { self.address = host.clone(); }
                    }
                });
            });
            let mut switch = None;
            let mut close = None;
            ui.horizontal_wrapped(|ui| {
                for (index, tab) in self.tabs.iter().enumerate() {
                    if ui.selectable_label(index == self.selected, &tab.host).clicked() { switch = Some(index); }
                    if ui.small_button("×").on_hover_text("Close session").clicked() { close = Some(index); }
                }
            });
            if let Some(index) = switch {
                if let Some(tab) = self.tabs.get_mut(self.selected) {
                    tab.release();
                    if let Some(session) = &tab.session { session.set_clipboard_active(false); }
                }
                self.selected = index;
                self.persist_tabs();
                self.clipboard_text = None;
                if let Some(clipboard) = &self.clipboard { clipboard.clear(); }
            }
            if let Some(index) = close {
                if let Some(tab) = self.tabs.get_mut(self.selected) {
                    tab.release();
                    if let Some(session) = &tab.session { session.set_clipboard_active(false); }
                }
                self.tabs[index].release();
                let tab = self.tabs.remove(index);
                if let Some(session) = tab.session { self.retire(session); }
                self.selected = self.selected.saturating_sub(usize::from(index <= self.selected)).min(self.tabs.len().saturating_sub(1));
                self.persist_tabs();
                self.clipboard_text = None;
                if let Some(clipboard) = &self.clipboard { clipboard.clear(); }
            }
            if !self.notice.is_empty() { ui.colored_label(Color32::YELLOW, &self.notice); }
            ui.label("US physical keyboard · click picture to send input · Shift+Escape releases input · CPU video");
        });
        let replacement_capacity = self.worker_count() < MAX_TABS;
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(tab) = self.tabs.get_mut(self.selected) else {
                ui.centered_and_justified(|ui| { ui.label("Enter an SSH host to start a session."); }); return;
            };
            let Some(session) = &tab.session else {
                ui.label("Restored tab · disconnected");
                if ui.add_enabled(replacement_capacity, egui::Button::new("Reconnect")).clicked() {
                    tab.session = Some(Session::start(tab.host.clone(), self.options.clone()));
                }
                return;
            };
            if session.set_clipboard_active(ctx.input(|i| i.focused) && !self.options.no_clipboard) {
                self.clipboard_text = None;
                if let Some(clipboard) = &self.clipboard { clipboard.clear(); }
            }
            let (status, connected, headless, geometry, picture, cursor, cursor_pos, remote_text) = {
                let mut s = session.state.lock().unwrap();
                (s.status.clone(), s.connected, s.headless, s.geometry, s.picture.take(), s.cursor.take(), s.cursor_pos, s.clipboard.take())
            };
            ui.label(status);
            let mut reconnect = false;
            ui.horizontal(|ui| {
                if ui.button("Disconnect").clicked() { tab.release(); if let Some(s) = &tab.session { s.disconnect(); } }
                reconnect = ui.add_enabled(replacement_capacity || tab.session.as_ref().is_some_and(Session::finished), egui::Button::new("Reconnect")).clicked();
                ui.label(if tab.captured { "Remote input active" } else { "Local input active" });
            });
            if reconnect {
                tab.release();
                if let Some(session) = tab.session.take() {
                    session.disconnect(); self.retired.push(session);
                }
                // Clear stale screen and input state when replacing a connection.
                tab.texture = None; tab.picture_geometry = None; tab.cursor = None; tab.resize = None;
                tab.session = Some(Session::start(tab.host.clone(), self.options.clone()));
                self.clipboard_text = None;
                if let Some(clipboard) = &self.clipboard { clipboard.clear(); }
                return;
            }
            if !connected { tab.release(); return; }
            if let Some(picture) = picture {
                let image = bgra_image(picture.frame.width, picture.frame.height, &picture.frame.pixels);
                if let Some(texture) = &mut tab.texture { texture.set(image, TextureOptions::LINEAR); }
                else { tab.texture = Some(ctx.load_texture(format!("video-{}", self.selected), image, TextureOptions::LINEAR)); }
                tab.picture_geometry = Some(picture.geometry);
            }
            if let Some(cursor) = cursor {
                tab.cursor = if cursor.bgra.is_empty() { None } else {
                    Some((cursor.id, ctx.load_texture("remote cursor", bgra_image(cursor.size[0], cursor.size[1], &cursor.bgra), TextureOptions::LINEAR), cursor.size, cursor.hotspot))
                };
            }
            let available = ui.available_rect_before_wrap();
            let Some(geometry) = tab.picture_geometry.or(geometry) else { return; };
            let rect = geometry.fit(available);
            if let Some(texture) = &tab.texture {
                ui.painter().image(texture.id(), rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let response = ui.allocate_rect(available, Sense::click());
            if response.hovered() && ui.input(|i| i.pointer.any_pressed()) && ui.input(|i| i.pointer.interact_pos().is_some_and(|p| rect.contains(p))) {
                tab.captured = true; response.request_focus();
            }
            let focused = ctx.input(|i| i.focused);
            if !focused || (ctx.wants_keyboard_input() && !response.has_focus()) { tab.release(); }
            if tab.captured {
                response.request_focus();
                let events = self.remote_events.take().unwrap_or_else(|| ctx.input(|i| i.events.clone()));
                for event in events {
                    match event {
                        Event::Key { key, physical_key, pressed, modifiers, repeat, .. } => {
                            if key == Key::Escape && modifiers.shift && pressed { tab.release(); break; }
                            let modifiers = tab.held.modifiers(modifiers); tab.input(modifiers);
                            if !repeat {
                                if let Some(code) = physical_key.and_then(keycode) {
                                    let messages = if pressed { tab.held.key(code, true) } else { tab.held.key_up(code, ctx.input(|i| i.modifiers)) };
                                    tab.input(messages);
                                }
                            }
                        }
                        Event::Copy | Event::Cut | Event::Paste(_) => {
                            // egui swallowed the physical press. Remember its
                            // modifiers, then recover the actual US position on
                            // key-up (also distinguishes Ctrl+Insert from Ctrl+C).
                            let kind = match event { Event::Copy => 0, Event::Cut => 1, _ => 2 };
                            tab.held.shortcut(kind, ctx.input(|i| i.modifiers));
                        }
                        Event::PointerMoved(pos) => {
                            // Clamp drag coordinates to the image edge; bars never
                            // become remote desktop coordinates.
                            let pos = if ui.input(|i| i.pointer.any_down()) { rect.clamp(pos) } else { pos };
                            if let Some([x, y]) = geometry.remote(rect, pos) { tab.input(vec![ClientMsg::PointerMotion { x, y }]); }
                        }
                        Event::PointerButton { pos, button, pressed, modifiers } => {
                            let mods = tab.held.modifiers(modifiers); tab.input(mods);
                            if !pressed || rect.contains(pos) {
                                let messages = tab.held.button(button_code(button), pressed); tab.input(messages);
                            }
                        }
                        Event::MouseWheel { unit, delta, .. } if response.hovered() => {
                            let factor = match unit { MouseWheelUnit::Point => 1.0, MouseWheelUnit::Line => 15.0, MouseWheelUnit::Page => rect.height() };
                            for (axis, value) in [(Axis::Horizontal, delta.x), (Axis::Vertical, delta.y)] {
                                if value != 0.0 { tab.input(vec![ClientMsg::PointerAxis { axis, value: -(value * factor) as f64, discrete: if unit == MouseWheelUnit::Line { Some(-value.round() as i32) } else { None }, stop: false }]); }
                            }
                        }
                        Event::WindowFocused(false) => { tab.release(); break; }
                        _ => {}
                    }
                }
                if tab.captured { let mods = tab.held.modifiers(ctx.input(|i| i.modifiers)); tab.input(mods); }
            }
            if let Some((x, y, shape_id, true)) = cursor_pos {
                if let Some((id, texture, size, hotspot)) = &tab.cursor {
                    if *id == shape_id {
                        let factor = rect.width() / geometry.stream[0] as f32;
                        let point = geometry.local(rect, x, y) - egui::vec2(hotspot[0] as f32, hotspot[1] as f32) * factor;
                        let cursor_rect = Rect::from_min_size(point, egui::vec2(size[0] as f32, size[1] as f32) * factor);
                        ui.painter().with_clip_rect(rect).image(texture.id(), cursor_rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                }
            }
            if tab.captured && response.hovered() { ctx.set_cursor_icon(egui::CursorIcon::None); }
            if headless && tab.resized_at.elapsed() > Duration::from_millis(250) {
                let dpi = ctx.pixels_per_point();
                let resize = ((available.width() * dpi).clamp(320.0, 4096.0) as u32 & !1,
                    (available.height() * dpi).clamp(240.0, 4096.0) as u32 & !1, (dpi.clamp(0.5, 4.0) * 1000.0) as u32);
                if tab.resize != Some(resize) {
                    if let Some(session) = &tab.session { session.send(Command::Resize(resize.0, resize.1, resize.2 as f32 / 1000.0)); }
                    tab.resize = Some(resize); tab.resized_at = Instant::now();
                }
            }
            if focused {
                if let Some(clipboard) = &self.clipboard {
                    if let Some(text) = remote_text {
                        self.clipboard_text = Some(text.clone()); clipboard.set(text);
                    }
                    if self.clipboard_poll.elapsed() >= Duration::from_millis(500) {
                        clipboard.poll(); self.clipboard_poll = Instant::now();
                    }
                    if let Some(text) = clipboard.take() {
                        if self.clipboard_text.as_ref() != Some(&text) {
                            if let Some(session) = &tab.session { session.send(Command::Text(text.clone())); }
                            self.clipboard_text = Some(text);
                        }
                    }
                }
            }
        });
    }
}
fn restore_tabs(saved: &Saved) -> (Vec<Tab>, usize) {
    let tabs: Vec<_> = saved
        .tabs
        .iter()
        .filter(|host| crate::ssh::validate_host(host).is_ok())
        .take(MAX_TABS)
        .cloned()
        .map(Tab::new)
        .collect();
    let selected = saved.selected.min(tabs.len().saturating_sub(1));
    (tabs, selected)
}

fn bgra_image(width: u32, height: u32, bytes: &[u8]) -> egui::ColorImage {
    let pixels = bytes
        .chunks_exact(4)
        .map(|p| Color32::from_rgba_unmultiplied(p[2], p[1], p[0], p[3]))
        .collect();
    egui::ColorImage {
        size: [width as usize, height as usize],
        pixels,
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        for tab in &mut self.tabs {
            if let Some(session) = tab.session.take() {
                session.disconnect();
                self.retired.push(session);
            }
        }
        // The event loop has exited: wait for deterministic SSH/task teardown.
        for session in &mut self.retired {
            session.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restores_configured_tabs_and_selection_without_connecting() {
        let saved = Saved {
            tabs: vec!["host-one".into(), "user@host-two".into()],
            selected: 1,
            ..Default::default()
        };
        let (tabs, selected) = restore_tabs(&saved);
        assert_eq!(selected, 1);
        assert_eq!(
            tabs.iter().map(|t| t.host.as_str()).collect::<Vec<_>>(),
            ["host-one", "user@host-two"]
        );
        assert!(tabs.iter().all(|t| t.session.is_none() && !t.captured));
    }
}
