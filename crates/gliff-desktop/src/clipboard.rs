//! Text-only adapter around the shared flow-controlled clipboard engine.
use bytes::Bytes;
use gliff_proto::{
    clipboard::{TEXT_MIME, TEXT_MIMES},
    ClipboardItem, ClipboardMsg,
};
use gliff_transport::clipboard::{ChunkSink, Event, ReadSource, Side, Transfers};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

pub const TEXT_CAP: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RemoteText {
    pub generation: u64,
    pub text: String,
}

pub struct AbortTask(pub JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone)]
struct Sink(Rc<RefCell<Vec<u8>>>);
impl ChunkSink for Sink {
    async fn write(&mut self, bytes: Bytes) -> std::io::Result<()> {
        self.0.borrow_mut().extend_from_slice(&bytes);
        Ok(())
    }
    async fn finish(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct Bridge {
    engine: Transfers,
    active: bool,
    serial: u32,
    text: String,
    fetch: Option<AbortTask>,
    received: watch::Sender<Option<RemoteText>>,
    generation: Arc<AtomicU64>,
}
impl Bridge {
    pub fn new(
        out: gliff_transport::clipboard::Outbound,
        received: watch::Sender<Option<RemoteText>>,
    ) -> Self {
        Self::with_generation(out, received, Arc::default())
    }
    pub fn with_generation(
        out: gliff_transport::clipboard::Outbound,
        received: watch::Sender<Option<RemoteText>>,
        generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            engine: Transfers::new(out, Side::Client),
            active: false,
            serial: 0,
            text: String::new(),
            fetch: None,
            received,
            generation,
        }
    }
    fn invalidate_fetch(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.fetch = None;
        self.received.send_replace(None);
    }
    /// Withdraw selection and cancel all transfer work when a tab loses ownership.
    pub fn set_active(&mut self, active: bool) -> Option<ClipboardMsg> {
        if self.active == active {
            return None;
        }
        self.active = active;
        self.engine.abort_serving();
        self.invalidate_fetch();
        self.text.clear();
        self.serial = self.serial.wrapping_add(1);
        (!active).then(|| ClipboardMsg::Offer {
            serial: self.serial,
            mime_types: vec![],
            files: vec![],
        })
    }
    pub fn offer(&mut self, text: String) -> Option<ClipboardMsg> {
        if !self.active || text.len() > TEXT_CAP {
            return None;
        }
        self.invalidate_fetch();
        self.engine.abort_serving();
        self.text = text;
        self.serial = self.serial.wrapping_add(1);
        Some(ClipboardMsg::Offer {
            serial: self.serial,
            mime_types: vec![TEXT_MIME.into()],
            files: vec![],
        })
    }
    pub fn on_message(&mut self, msg: ClipboardMsg, bytes: Bytes) -> Option<ClipboardMsg> {
        if !self.active {
            return match msg {
                ClipboardMsg::Request { id, .. } => Some(ClipboardMsg::Abort { id }),
                _ => None,
            };
        }
        // Do not create per-request tasks for unsupported/file/stale requests.
        if let ClipboardMsg::Request {
            id,
            serial,
            ref item,
        } = msg
        {
            if id % 2 != 0
                || serial != self.serial
                || !matches!(item, ClipboardItem::Mime(m) if m == TEXT_MIME)
            {
                return Some(ClipboardMsg::Abort { id });
            }
            // At most one outgoing and one incoming transfer per tab.
            self.engine.abort_serving();
            self.engine.serve(
                id,
                ReadSource(std::io::Cursor::new(self.text.clone().into_bytes())),
            );
            return None;
        }
        if let Some(Event::Offer {
            serial, mime_types, ..
        }) = self.engine.on_msg(msg, bytes)
        {
            self.invalidate_fetch();
            if let Some(mime) = TEXT_MIMES
                .iter()
                .find(|mime| mime_types.iter().any(|m| m == **mime))
            {
                let engine = self.engine.clone();
                let received = self.received.clone();
                let mime = mime.to_string();
                let generation = self.generation.clone();
                let started = generation.load(Ordering::SeqCst);
                self.fetch = Some(AbortTask(tokio::task::spawn_local(async move {
                    let data = Rc::new(RefCell::new(Vec::new()));
                    if engine
                        .fetch(
                            ClipboardItem::Mime(mime),
                            serial,
                            Sink(data.clone()),
                            Some(TEXT_CAP as u64),
                        )
                        .await
                        .is_ok()
                    {
                        if let Ok(text) = String::from_utf8(std::mem::take(&mut *data.borrow_mut()))
                        {
                            if generation.load(Ordering::SeqCst) == started {
                                received.send_replace(Some(RemoteText {
                                    generation: started,
                                    text,
                                }));
                            }
                        }
                    }
                })));
            }
        }
        None
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.engine.abort_serving();
    }
}

/// Native calls have a replaceable write slot, poll slot and result slot.
/// Polling cannot overwrite a pending remote write. Epochs prevent stale poll
/// results from being offered to a newly selected session.
#[derive(Default)]
struct NativeState {
    set: Option<(u64, String)>,
    poll: Option<u64>,
    text: Option<(u64, String)>,
}

pub struct NativeClipboard {
    state: Arc<Mutex<NativeState>>,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}
impl Default for NativeClipboard {
    fn default() -> Self {
        let state = Arc::new(Mutex::new(NativeState::default()));
        let worker = state.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            let Ok(mut clipboard) = arboard::Clipboard::new() else {
                return;
            };
            while !stopped.load(Ordering::Relaxed) {
                let (set, poll) = {
                    let mut state = worker.lock().unwrap();
                    (state.set.take(), state.poll.take())
                };
                if let Some((generation, value)) = set {
                    if generation == current.load(Ordering::Relaxed) {
                        let _ = clipboard.set_text(value);
                    }
                }
                if let Some(generation) = poll {
                    if generation == current.load(Ordering::Relaxed) {
                        if let Ok(value) = clipboard.get_text() {
                            if value.len() <= TEXT_CAP {
                                worker.lock().unwrap().text = Some((generation, value));
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Self {
            state,
            generation,
            stop,
        }
    }
}
impl NativeClipboard {
    pub fn poll(&self) {
        self.state.lock().unwrap().poll = Some(self.generation.load(Ordering::Relaxed));
    }
    pub fn set(&self, text: String) {
        self.state.lock().unwrap().set = Some((self.generation.load(Ordering::Relaxed), text));
    }
    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        *self.state.lock().unwrap() = NativeState::default();
    }
    pub fn take(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .text
            .take()
            .and_then(|(generation, text)| {
                (generation == self.generation.load(Ordering::Relaxed)).then_some(text)
            })
    }
}
impl Drop for NativeClipboard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn newer_local_selection_cancels_and_invalidates_remote_fetch() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (out, mut messages) = gliff_transport::clipboard::outbound_channel();
                let (received, rx) = watch::channel(None);
                let revision = Arc::new(AtomicU64::new(0));
                let mut bridge = Bridge::with_generation(out, received, revision.clone());
                bridge.set_active(true);
                bridge.on_message(
                    ClipboardMsg::Offer {
                        serial: 9,
                        mime_types: vec![TEXT_MIME.into()],
                        files: vec![],
                    },
                    Bytes::new(),
                );
                let (ClipboardMsg::Request { id, .. }, _) = messages.recv().await.unwrap() else {
                    panic!("fetch request")
                };
                assert!(bridge.offer("newer local selection".into()).is_some());
                let old = b"older remote selection";
                bridge.on_message(
                    ClipboardMsg::Data {
                        id,
                        offset: 0,
                        data_len: old.len() as u32,
                        done: true,
                    },
                    Bytes::from_static(old),
                );
                tokio::task::yield_now().await;
                assert!(rx.borrow().is_none());

                bridge.on_message(
                    ClipboardMsg::Offer {
                        serial: 10,
                        mime_types: vec![TEXT_MIME.into()],
                        files: vec![],
                    },
                    Bytes::new(),
                );
                let id = loop {
                    if let (ClipboardMsg::Request { id, .. }, _) = messages.recv().await.unwrap() {
                        break id;
                    }
                };
                // GUI observed a new local copy before its command was drained.
                revision.fetch_add(1, Ordering::SeqCst);
                bridge.on_message(
                    ClipboardMsg::Data {
                        id,
                        offset: 0,
                        data_len: old.len() as u32,
                        done: true,
                    },
                    Bytes::from_static(old),
                );
                tokio::task::yield_now().await;
                assert!(rx.borrow().is_none());
            })
            .await;
    }
    #[test]
    fn clipboard_epochs_discard_previous_tabs_and_poll_does_not_replace_write() {
        // Exercise slots without opening a real OS clipboard.
        let clipboard = NativeClipboard {
            state: Arc::new(Mutex::new(NativeState::default())),
            generation: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        clipboard.set("remote text".into());
        clipboard.poll();
        assert_eq!(
            clipboard.state.lock().unwrap().set.as_ref().unwrap().1,
            "remote text"
        );
        clipboard.state.lock().unwrap().text = Some((0, "previous tab".into()));
        clipboard.clear();
        assert!(clipboard.take().is_none());
        clipboard.state.lock().unwrap().text = Some((0, "late poll".into()));
        assert!(clipboard.take().is_none());
    }
    #[tokio::test]
    async fn deactivation_cancels_inflight_fetch_and_serving() {
        tokio::task::LocalSet::new().run_until(async {
            let (out, mut messages) = gliff_transport::clipboard::outbound_channel();
            let (received, _rx) = watch::channel(None);
            let mut bridge = Bridge::new(out, received);
            bridge.set_active(true);
            let offer = bridge.offer("x".repeat(TEXT_CAP)).unwrap();
            let ClipboardMsg::Offer { serial, .. } = offer else { unreachable!() };
            assert!(bridge.on_message(ClipboardMsg::Request { id: 2, serial, item: ClipboardItem::Mime(TEXT_MIME.into()) }, Bytes::new()).is_none());
            bridge.on_message(ClipboardMsg::Offer { serial: 9, mime_types: vec![TEXT_MIME.into()], files: vec![] }, Bytes::new());
            let mut fetch_id = None;
            let mut serving = false;
            while fetch_id.is_none() || !serving {
                let (msg, _) = messages.recv().await.unwrap();
                match msg {
                    ClipboardMsg::Request { id, .. } => fetch_id = Some(id),
                    ClipboardMsg::Data { id: 2, .. } => serving = true,
                    _ => {}
                }
            }
            assert!(matches!(bridge.set_active(false), Some(ClipboardMsg::Offer { mime_types, .. }) if mime_types.is_empty()));
            let mut aborted = Vec::new();
            tokio::time::timeout(Duration::from_secs(1), async {
                while !aborted.contains(&2) || !aborted.contains(&fetch_id.unwrap()) {
                    let (msg, _) = messages.recv().await.unwrap();
                    if let ClipboardMsg::Abort { id } = msg { aborted.push(id); }
                }
            }).await.unwrap();
            assert!(bridge.offer("inactive text".into()).is_none());
        }).await;
    }
}
