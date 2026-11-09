//! Window-independent session worker. Every queue and retained image is bounded.
use crate::{
    clipboard::{AbortTask, Bridge},
    config::Options,
    geometry::Geometry,
    input::Held,
};
use anyhow::{bail, ensure, Context, Result};
use bytes::Bytes;
use gliff_proto::{ChromaMode, ClientCaps, ClientMsg, Codec, ServerMsg, PROTOCOL_VERSION};
use gliff_sw::{BgraFrame, Decoder};
use gliff_transport::Framed;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    sync::{mpsc, watch},
};

const US_KEYMAP: &str = r#"xkb_keymap {
    xkb_keycodes { include "evdev+aliases(qwerty)" };
    xkb_types { include "complete" };
    xkb_compatibility { include "complete" };
    xkb_symbols { include "pc+us+inet(evdev)" };
};"#;

const IO_WAIT: Duration = Duration::from_secs(3);
const SETUP_WAIT: Duration = Duration::from_secs(15);
pub const MAX_PACKET: usize = 16 * 1024 * 1024;
pub const MAX_TABS: usize = 8;

pub struct Picture {
    pub frame: BgraFrame,
    pub geometry: Geometry,
}
pub struct Cursor {
    pub id: u32,
    pub size: [u32; 2],
    pub hotspot: [i32; 2],
    pub bgra: Vec<u8>,
}
#[derive(Default)]
pub struct Snapshot {
    pub status: String,
    pub connected: bool,
    pub headless: bool,
    pub geometry: Option<Geometry>,
    pub picture: Option<Picture>,
    pub cursor: Option<Cursor>,
    pub cursor_pos: Option<(f64, f64, u32, bool)>,
    pub clipboard: Option<String>,
    pub clipboard_active: bool,
}
pub type Shared = Arc<Mutex<Snapshot>>;

pub enum Command {
    Input(Vec<ClientMsg>),
    Release,
    Resize(u32, u32, f32),
    Text(String),
    ClipboardActive(bool),
}

pub struct Session {
    pub state: Shared,
    commands: mpsc::Sender<Command>,
    cancel: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Session {
    pub fn start(host: String, options: Options) -> Self {
        let state = Shared::default();
        let shared = state.clone();
        let (commands, mut rx) = mpsc::channel(64);
        let (cancel, mut cancelled) = watch::channel(false);
        let thread = std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    shared.lock().unwrap().status = format!("Cannot start session: {e}");
                    return;
                }
            };
            rt.block_on(async {
                let mut reconnecting = false;
                for attempt in 0..3 {
                    if *cancelled.borrow() {
                        break;
                    }
                    {
                        let mut s = shared.lock().unwrap();
                        s.status = format!("Connecting ({}/3)…", attempt + 1);
                        s.connected = false;
                        s.picture = None;
                        s.geometry = None;
                        s.cursor = None;
                        s.cursor_pos = None;
                        s.clipboard = None;
                    }
                    // A fresh LocalSet per connection cancels ALL engine/reader tasks on exit.
                    let local = tokio::task::LocalSet::new();
                    let result = local
                        .run_until(connect(&host, &options, &shared, &mut rx, &mut cancelled))
                        .await;
                    drop(local);
                    let was_connected = {
                        let mut s = shared.lock().unwrap();
                        let connected = s.connected;
                        s.connected = false;
                        connected
                    };
                    if *cancelled.borrow() || result.is_ok() {
                        break;
                    }
                    let error = result.unwrap_err();
                    reconnecting |= was_connected;
                    let retry =
                        reconnecting && error.downcast_ref::<Fatal>().is_none() && attempt < 2;
                    shared.lock().unwrap().status = format!(
                        "Disconnected: {error:#}{}",
                        if retry {
                            " — reconnecting shortly"
                        } else {
                            " — use Reconnect to try again"
                        }
                    );
                    if !retry {
                        return;
                    }
                    // Drop stale input before establishing a new remote input state.
                    while rx.try_recv().is_ok() {}
                    tokio::select! {
                        biased;
                        _ = cancelled.changed() => break,
                        _ = tokio::time::sleep(Duration::from_secs(1 << attempt)) => {}
                    }
                }
                let mut s = shared.lock().unwrap();
                s.connected = false;
                s.status = "Disconnected".into();
            });
        });
        Self {
            state,
            commands,
            cancel,
            thread: Some(thread),
        }
    }
    pub fn set_clipboard_active(&self, active: bool) -> bool {
        let changed = {
            let mut state = self.state.lock().unwrap();
            let changed = state.clipboard_active != active;
            state.clipboard_active = active;
            if !active {
                state.clipboard = None;
            }
            changed
        };
        if changed {
            self.send(Command::ClipboardActive(active));
        }
        changed
    }
    pub fn send(&self, command: Command) {
        if matches!(
            self.commands.try_send(command),
            Err(mpsc::error::TrySendError::Full(_))
        ) {
            // Losing a key-up is unsafe: terminate instead of silently dropping it.
            // A closed worker already cleaned up; preserve its useful error.
            self.disconnect();
        }
    }
    /// Called after window teardown, never during normal frame updates.
    pub fn shutdown(&mut self) {
        self.cancel.send_replace(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
    pub fn disconnect(&self) {
        self.cancel.send_replace(true);
        let mut snapshot = self.state.lock().unwrap();
        if snapshot.connected {
            snapshot.status = "Disconnecting…".into();
        }
        snapshot.connected = false;
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.disconnect();
        // No join on the GUI thread. The worker has bounded I/O deadlines and
        // owns kill_on_drop SSH; completed workers can be reclaimed immediately.
        if self.finished() {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[derive(Debug)]
struct Fatal(String);
impl std::fmt::Display for Fatal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Fatal {}
fn fatal(message: impl Into<String>) -> anyhow::Error {
    Fatal(message.into()).into()
}

async fn connect(
    host: &str,
    options: &Options,
    state: &Shared,
    commands: &mut mpsc::Receiver<Command>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    if let Some(addr) = options.connect {
        ensure!(addr.ip().is_loopback(), "Development TCP must use loopback");
        let stream = tokio::select! {
            biased;
            _ = cancel.changed() => return Ok(()),
            stream = tokio::time::timeout(SETUP_WAIT, tokio::net::TcpStream::connect(addr)) => stream.context("TCP connect timed out")??,
        };
        stream.set_nodelay(true)?;
        let (rd, wr) = tokio::io::split(stream);
        return run_stream(rd, wr, options, state, commands, cancel).await;
    }
    let child = crate::ssh::command(host, options)?.spawn().context(
        "Cannot launch OpenSSH (`ssh`); install Windows OpenSSH Client and add it to PATH",
    )?;
    run_child(child, options, state, commands, cancel).await
}

async fn run_child(
    mut child: tokio::process::Child,
    options: &Options,
    state: &Shared,
    commands: &mut mpsc::Receiver<Command>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    let stdout = child.stdout.take().context("SSH stdout unavailable")?;
    let stdin = child.stdin.take().context("SSH stdin unavailable")?;
    let mut stderr = child.stderr.take().context("SSH stderr unavailable")?;
    let diagnostics = Arc::new(Mutex::new(Vec::<u8>::new()));
    let captured = diagnostics.clone();
    let mut stderr_task = AbortTask(tokio::task::spawn_local(async move {
        let mut buffer = [0; 2048];
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            let mut text = captured.lock().unwrap();
            text.extend_from_slice(&buffer[..n]);
            if text.len() > 8192 {
                let excess = text.len() - 8192;
                text.drain(..excess);
            }
        }
    }));
    let result = run_stream(stdout, stdin, options, state, commands, cancel).await;
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    // Let the bounded stderr drain observe EOF before formatting diagnostics.
    let _ = tokio::time::timeout(Duration::from_millis(100), &mut stderr_task.0).await;
    match result {
        Err(e) => {
            let text = String::from_utf8_lossy(&diagnostics.lock().unwrap()).into_owned();
            if text.contains("Permission denied")
                || text.contains("Host key verification failed")
                || text.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
                || text.contains("not found")
            {
                Err(fatal(crate::ssh::diagnostic(&text)))
            } else {
                Err(e.context(crate::ssh::diagnostic(&text)))
            }
        }
        Ok(()) => Ok(()),
    }
}

/// Validate a packet's envelope before allocating its body/payload. Decode with
/// the shared framing implementation after imposing desktop-specific limits.
/// This runs in a dedicated task; partial reads are never cancelled/restarted.
pub async fn read_packet<R: AsyncRead + Unpin>(rd: &mut R) -> Result<(ServerMsg, Bytes)> {
    loop {
        let mut header = [0; 8];
        rd.read_exact(&mut header).await?;
        let body = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let payload = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if body > gliff_proto::frame::MAX_FRAME_BODY || payload > MAX_PACKET {
            return Err(fatal("Peer frame exceeds desktop memory limits"));
        }
        let mut packet = vec![0; 8 + body + payload];
        packet[..8].copy_from_slice(&header);
        rd.read_exact(&mut packet[8..]).await?;
        let mut framed = Framed::new(packet.as_slice());
        let msg: ServerMsg = match framed.read_msg().await {
            Ok(msg) => msg,
            // A complete unknown message was skipped by the shared codec.
            Err(gliff_transport::Error::Closed) => continue,
            Err(e) => return Err(fatal(format!("Invalid protocol frame: {e}"))),
        };
        let expected = match &msg {
            ServerMsg::VideoFrame {
                data_len, aux_len, ..
            } => u64::from(*data_len) + u64::from(*aux_len),
            ServerMsg::CursorShape { argb_len, .. } => u64::from(*argb_len),
            ServerMsg::ClipboardData { data_len, .. } => u64::from(*data_len),
            _ => 0,
        };
        if expected != payload as u64 {
            return Err(fatal("Payload length does not match message"));
        }
        if matches!(msg, ServerMsg::ClipboardData { data_len, .. } if data_len as usize > gliff_proto::clipboard::CHUNK)
        {
            return Err(fatal("Clipboard chunk exceeds flow-control limit"));
        }
        // Move the allocation into Bytes; no additional payload allocation.
        return Ok((msg, Bytes::from(packet).slice(8 + body..)));
    }
}

async fn write<W: AsyncWrite + Unpin>(
    wr: &mut Framed<W>,
    msg: &ClientMsg,
    payload: &[u8],
) -> Result<()> {
    tokio::time::timeout(IO_WAIT, wr.write_msg_with_payloads(msg, &[payload]))
        .await
        .context("Server write timed out")??;
    Ok(())
}
fn publish(state: &Shared, frame: BgraFrame, geometry: Geometry) {
    state.lock().unwrap().picture = Some(Picture { frame, geometry });
}

pub async fn run_stream<R: AsyncRead + Unpin + 'static, W: AsyncWrite + Unpin>(
    mut rd: R,
    wr: W,
    options: &Options,
    state: &Shared,
    commands: &mut mpsc::Receiver<Command>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<()> {
    let (packets, mut incoming) = mpsc::channel(1);
    let _reader = AbortTask(tokio::task::spawn_local(async move {
        loop {
            let packet = tokio::time::timeout(Duration::from_secs(30), read_packet(&mut rd))
                .await
                .context("Server read timed out")
                .and_then(|p| p);
            let done = packet.is_err();
            if packets.send(packet).await.is_err() || done {
                break;
            }
        }
    }));
    let mut wr = Framed::new(wr);
    let mut held = Held::default();
    let result = protocol(
        &mut wr,
        &mut incoming,
        options,
        state,
        commands,
        cancel,
        &mut held,
    )
    .await;
    // One total cleanup deadline, regardless of the number of held keys.
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        for msg in held.release() {
            wr.write_msg(&msg).await?;
        }
        wr.write_msg(&ClientMsg::Bye).await
    })
    .await;
    result
}

async fn protocol<W: AsyncWrite + Unpin>(
    wr: &mut Framed<W>,
    incoming: &mut mpsc::Receiver<Result<(ServerMsg, Bytes)>>,
    options: &Options,
    state: &Shared,
    commands: &mut mpsc::Receiver<Command>,
    cancel: &mut watch::Receiver<bool>,
    held: &mut Held,
) -> Result<()> {
    if *cancel.borrow() {
        return Ok(());
    }
    write(
        wr,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
            keymap: US_KEYMAP.into(),
            caps: ClientCaps {
                codecs: vec![Codec::H264],
                max_width: 4096,
                max_height: 4096,
                chroma: vec![ChromaMode::Dual420, ChromaMode::Single420],
                features: vec![],
            },
        },
        &[],
    )
    .await?;
    let setup = tokio::time::sleep(SETUP_WAIT);
    tokio::pin!(setup);
    let mut acknowledged = false;
    let mut headless = false;
    let mut config: Option<(Geometry, ChromaMode)> = None;
    let mut decoder: Option<Decoder> = None;
    let mut geometries = VecDeque::new();
    let (clip_out, mut clip_rx) = gliff_transport::clipboard::outbound_channel();
    let (clip_text, mut clip_text_rx) = watch::channel(None);
    let mut clipboard = (!options.no_clipboard).then(|| Bridge::new(clip_out, clip_text));
    if let Some(bridge) = clipboard.as_mut() {
        bridge.set_active(state.lock().unwrap().clipboard_active);
    }
    let mut timer = tokio::time::interval(Duration::from_millis(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut received_at = Instant::now();
    let mut ping_at = Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = cancel.changed() => return Ok(()),
            _ = &mut setup, if config.is_none() => return Err(fatal("Connection setup timed out; verify SSH key/agent, known_hosts and remote gliff-server")),
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()); };
                if let Command::ClipboardActive(active) = command {
                    // Drop queued old selection data before changing epochs.
                    // The explicit edge also preserves a rapid leave/return.
                    while clip_rx.try_recv().is_ok() {}
                    if let Some(bridge) = clipboard.as_mut() {
                        if let Some(msg) = bridge.set_active(active) {
                            if config.is_some() { write(wr, &msg.into(), &[]).await?; }
                        }
                    }
                    continue;
                }
                if config.is_none() { continue; }
                let clipboard_active = state.lock().unwrap().clipboard_active;
                match command {
                    Command::Input(messages) => for msg in messages {
                        if matches!(msg, ClientMsg::Key { .. } | ClientMsg::PointerMotion { .. } | ClientMsg::PointerButton { .. } | ClientMsg::PointerAxis { .. }) {
                            held.observe(&msg);
                            write(wr, &msg, &[]).await?;
                        }
                    },
                    Command::Release => for msg in held.release() { write(wr, &msg, &[]).await?; },
                    Command::Resize(width, height, scale) if headless && width > 0 && height > 0 && width <= 4096 && height <= 4096 && scale.is_finite() && (0.5..=4.0).contains(&scale) => {
                        write(wr, &ClientMsg::Resize { width, height, scale }, &[]).await?;
                    }
                    Command::Text(text) if clipboard_active => {
                        if let Some(bridge) = clipboard.as_mut() {
                            if let Some(offer) = bridge.offer(text) { write(wr, &offer.into(), &[]).await?; }
                        }
                    },
                    _ => {}
                }
            }
            Some((msg, data)) = clip_rx.recv(), if clipboard.is_some() => {
                let active = state.lock().unwrap().clipboard_active;
                if active || matches!(msg, gliff_proto::ClipboardMsg::Abort { .. }) {
                    write(wr, &msg.into(), &data).await?;
                }
            },
            Ok(()) = clip_text_rx.changed(), if clipboard.is_some() => {
                let text = clip_text_rx.borrow_and_update().clone();
                let mut snapshot = state.lock().unwrap();
                if snapshot.clipboard_active { snapshot.clipboard = text; }
            }
            _ = timer.tick() => {
                if received_at.elapsed() > Duration::from_secs(20) { bail!("Server stopped responding"); }
                if ping_at.elapsed() > Duration::from_secs(5) {
                    write(wr, &ClientMsg::Ping { t: now_ms() }, &[]).await?;
                    ping_at = Instant::now();
                }
                if let Some(decoder) = decoder.as_mut() {
                    if decoder.has_pending() {
                        if let Some(frame) = decoder.flush().map_err(|e| fatal(format!("Video drain failed: {e}")))? {
                            let geometry = geometries.pop_front().context("Missing decoded geometry")?;
                            publish(state, frame, geometry);
                        }
                    }
                }
            }
            packet = incoming.recv() => {
                let (msg, payload) = packet.context("Server closed the connection")??;
                received_at = Instant::now();
                match msg {
                    ServerMsg::HelloAck { version, session, .. } if !acknowledged => {
                        if version != PROTOCOL_VERSION { return Err(fatal(format!("Protocol version mismatch (client {PROTOCOL_VERSION}, server {version}); update the older side"))); }
                        acknowledged = true;
                        headless = session.headless;
                    }
                    ServerMsg::StreamConfig { codec, chroma, width, height, scale_milli, view_width, view_height, .. } if acknowledged => {
                        if codec != Codec::H264 || !matches!(chroma, ChromaMode::Single420 | ChromaMode::Dual420) { return Err(fatal("Unsupported video codec/chroma; desktop supports CPU H.264 Single420 and Dual420")); }
                        let geometry = Geometry { stream: [width, height], view: [view_width, view_height], scale_milli }.validate().map_err(|e| fatal(e.to_string()))?;
                        if config.is_none_or(|(old, mode)| old.stream != geometry.stream || mode != chroma) {
                            decoder = Some(Decoder::new(chroma == ChromaMode::Dual420, width, height)?);
                            geometries.clear();
                        }
                        config = Some((geometry, chroma));
                        let mut s = state.lock().unwrap();
                        s.connected = true; s.headless = headless; s.geometry = Some(geometry);
                        s.status = format!("Connected · CPU H.264 {chroma:?} · {width}×{height} · {}", if headless { "headless" } else { "mirror" });
                    }
                    ServerMsg::VideoFrame { frame_id, data_len, aux_len, .. } => {
                        let (geometry, chroma) = config.ok_or_else(|| fatal("Video arrived before configuration"))?;
                        if data_len == 0 || (chroma == ChromaMode::Dual420) != (aux_len > 0) { return Err(fatal("Invalid video stream payloads")); }
                        if geometries.len() >= 4 { return Err(fatal("Decoder retained too many pictures")); }
                        geometries.push_back(geometry);
                        let frame = decoder.as_mut().context("Missing decoder")?.decode(&payload[..data_len as usize], &payload[data_len as usize..])
                            .map_err(|e| fatal(format!("Video decode failed: {e}")))?;
                        if let Some(frame) = frame { publish(state, frame, geometries.pop_front().context("Missing geometry")?); }
                        write(wr, &ClientMsg::FrameAck { frame_id, decoded_at_ms: now_ms() }, &[]).await?;
                    }
                    ServerMsg::CursorShape { id, width, height, hot_x, hot_y, argb_len } if config.is_some() => {
                        if width == 0 && height == 0 && argb_len == 0 {
                            state.lock().unwrap().cursor = Some(Cursor { id, size: [0, 0], hotspot: [0, 0], bgra: vec![] });
                        } else if width > 0 && height > 0 && width <= 512 && height <= 512 && u64::from(width) * u64::from(height) * 4 == u64::from(argb_len)
                            && hot_x >= 0 && hot_y >= 0 && hot_x < width as i32 && hot_y < height as i32 {
                            state.lock().unwrap().cursor = Some(Cursor { id, size: [width, height], hotspot: [hot_x, hot_y], bgra: payload.to_vec() });
                        } else { return Err(fatal("Invalid remote cursor shape")); }
                    }
                    ServerMsg::CursorPos { x, y, shape_id, visible } if x.is_finite() && y.is_finite() && x.abs() <= 65536.0 && y.abs() <= 65536.0 => {
                        state.lock().unwrap().cursor_pos = Some((x, y, shape_id, visible));
                    }
                    ServerMsg::Ping { t } => write(wr, &ClientMsg::Pong { t }, &[]).await?,
                    ServerMsg::Pong { .. } => {}
                    ServerMsg::Error { code, message, .. } => return Err(fatal(format!("Remote error {code}: {message}"))),
                    other if config.is_some() => {
                        if let Ok(msg) = other.into_clipboard() {
                            if let Some(bridge) = clipboard.as_mut() {
                                if let Some(reply) = bridge.on_message(msg, payload) { write(wr, &reply.into(), &[]).await?; }
                            }
                        }
                    }
                    other => return Err(fatal(format!("Unexpected message during setup: {other:?}"))),
                }
            }
        }
    }
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use clap::Parser;
    use std::process::Stdio;

    #[tokio::test]
    async fn cancellation_kills_and_reaps_ssh_process() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let options = Options::parse_from(["gliff-desktop", "--no-clipboard"]);
                let mut cmd = tokio::process::Command::new("sleep");
                cmd.arg("60")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                let child = cmd.spawn().unwrap();
                let pid = child.id().unwrap();
                let state = Shared::default();
                let (_input, mut input_rx) = mpsc::channel(1);
                let (cancel, mut cancel_rx) = watch::channel(false);
                let cancel_task = tokio::task::spawn_local(async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    cancel.send_replace(true);
                });
                tokio::time::timeout(
                    Duration::from_secs(2),
                    run_child(child, &options, &state, &mut input_rx, &mut cancel_rx),
                )
                .await
                .unwrap()
                .unwrap();
                cancel_task.await.unwrap();
                assert!(
                    !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                    "SSH child must be reaped"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn host_key_and_authentication_failures_have_actionable_diagnostics() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for diagnostic in [
                    "Host key verification failed.",
                    "Permission denied (publickey).",
                    "gliff-server: command not found",
                ] {
                    let options = Options::parse_from(["gliff-desktop", "--no-clipboard"]);
                    let child = tokio::process::Command::new("sh")
                        .args(["-c", "printf '%s' \"$1\" >&2; exit 255", "test", diagnostic])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .spawn()
                        .unwrap();
                    let state = Shared::default();
                    let (_input, mut input_rx) = mpsc::channel(1);
                    let (_cancel, mut cancel_rx) = watch::channel(false);
                    let err = run_child(child, &options, &state, &mut input_rx, &mut cancel_rx)
                        .await
                        .unwrap_err();
                    let message = format!("{err:#}");
                    assert!(message.contains(diagnostic), "{message}");
                    assert!(message.contains("known_hosts"));
                    assert!(err.downcast_ref::<Fatal>().is_some());
                }
            })
            .await;
    }
}
