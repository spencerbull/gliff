//! Real protocol sessions and actual OpenH264 encoding/decoding, without a GUI.
use clap::Parser;
use gliff_desktop::{
    config::Options,
    session::{self, Command, Shared, Snapshot},
};
use gliff_proto::{ChromaMode, ClientMsg, Codec, ServerMsg, SessionInfo, PROTOCOL_VERSION};
use gliff_transport::Framed;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf},
    sync::{mpsc, watch},
};

const WAIT: Duration = Duration::from_secs(10);
struct Peer {
    rd: Framed<ReadHalf<DuplexStream>>,
    wr: Framed<WriteHalf<DuplexStream>>,
    input: mpsc::Sender<Command>,
    cancel: watch::Sender<bool>,
    state: Shared,
}
impl Peer {
    async fn handshake(&mut self, dual: bool, headless: bool) {
        let hello = self.rd.read_msg::<ClientMsg>().await.unwrap();
        let ClientMsg::Hello {
            version,
            caps,
            keymap,
        } = hello
        else {
            panic!("missing hello")
        };
        assert_eq!(version, PROTOCOL_VERSION);
        assert_eq!(caps.chroma, [ChromaMode::Dual420, ChromaMode::Single420]);
        assert!(keymap.contains("pc+us+inet(evdev)"));
        self.wr
            .write_msg(&ServerMsg::HelloAck {
                version,
                session: SessionInfo {
                    headless,
                    output: "mock".into(),
                },
                outputs: vec![],
                features: vec![],
            })
            .await
            .unwrap();
        self.wr
            .write_msg(&ServerMsg::Ping { t: 123 })
            .await
            .unwrap();
        self.wr.write_msg(&config(dual)).await.unwrap();
        assert_eq!(self.next().await, ClientMsg::Pong { t: 123 });
        // A barrier guarantees the worker processed StreamConfig.
        self.barrier(124).await;
    }
    async fn next(&mut self) -> ClientMsg {
        self.rd.read_msg().await.unwrap()
    }
    async fn barrier(&mut self, t: u64) {
        self.wr.write_msg(&ServerMsg::Ping { t }).await.unwrap();
        loop {
            if self.next().await == (ClientMsg::Pong { t }) {
                break;
            }
        }
    }
}
fn config(dual: bool) -> ServerMsg {
    ServerMsg::StreamConfig {
        codec: Codec::H264,
        chroma: if dual {
            ChromaMode::Dual420
        } else {
            ChromaMode::Single420
        },
        pipeline: None,
        width: 64,
        height: 64,
        scale_milli: 1000,
        extradata: vec![],
        aux_extradata: None,
        view_width: 64,
        view_height: 64,
        fps_cap: 30,
    }
}
fn run<F, Fut>(no_clipboard: bool, case: F) -> anyhow::Result<()>
where
    F: FnOnce(Peer) -> Fut,
    Fut: Future<Output = ()>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&rt, async {
        let options = Options::parse_from(if no_clipboard {
            vec!["gliff-desktop", "--no-clipboard"]
        } else {
            vec!["gliff-desktop"]
        });
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let (input, mut input_rx) = mpsc::channel(64);
        let (cancel, mut cancel_rx) = watch::channel(false);
        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (crd, cwr) = tokio::io::split(client);
        let (srd, swr) = tokio::io::split(server);
        let shared = state.clone();
        let mut worker = tokio::task::spawn_local(async move {
            session::run_stream(crd, cwr, &options, &shared, &mut input_rx, &mut cancel_rx).await
        });
        let peer = case(Peer {
            rd: Framed::new(srd),
            wr: Framed::new(swr),
            input,
            cancel,
            state,
        });
        tokio::time::timeout(WAIT, async {
            tokio::select! {
                result = &mut worker => result.unwrap(),
                _ = peer => worker.await.unwrap(),
            }
        })
        .await
        .expect("bounded session completion")
    })
}
#[test]
fn actual_single_and_dual420_session_decodes_latest_frame() {
    for dual in [false, true] {
        let mut encoder = gliff_sw::Encoder::new(
            gliff_sw::EncoderSettings {
                width: 64,
                height: 64,
                bitrate: 500_000,
                framerate: 30,
            },
            dual,
        )
        .unwrap();
        let packets: Vec<_> = [20u8, 80, 160]
            .into_iter()
            .enumerate()
            .map(|(index, red)| {
                let pixels: Vec<_> = (0..64 * 64).flat_map(|_| [40u8, 80, red, 255]).collect();
                encoder.encode_bgra(&pixels, index == 0).unwrap()
            })
            .collect();
        run(true, |mut peer| async move {
            peer.handshake(dual, false).await;
            for (index, packet) in packets.into_iter().enumerate() {
                let frame_id = 99 + index as u64;
                peer.wr.write_msg_with_payloads(&ServerMsg::VideoFrame {
                    frame_id, pts_us: 0, keyframe: index == 0, damage: vec![],
                    data_len: packet.main.len() as u32,
                    aux_len: packet.aux.as_ref().map_or(0, |p| p.len() as u32),
                }, &[&packet.main, packet.aux.as_deref().unwrap_or_default()]).await.unwrap();
                assert!(matches!(peer.next().await, ClientMsg::FrameAck { frame_id: id, .. } if id == frame_id));
            }
            // Deliberately leave the picture slot unconsumed until the final
            // image arrives: a stopped UI must retain only the latest image.
            loop {
                if peer.state.lock().unwrap().picture.as_ref().is_some_and(|p| p.frame.pixels[2] > 140) { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let picture = peer.state.lock().unwrap().picture.take().unwrap();
            assert_eq!((picture.frame.width, picture.frame.height), (64, 64));
            assert_eq!(picture.frame.pixels.len(), 64 * 64 * 4);
            let mean: f64 = picture
                .frame
                .pixels
                .chunks_exact(4)
                .map(|p| p[2] as f64)
                .sum::<f64>()
                / 4096.0;
            assert!(
                (mean - 160.0).abs() < 20.0,
                "decoded real image content: {mean}"
            );
            peer.cancel.send_replace(true);
            assert_eq!(peer.next().await, ClientMsg::Bye);
        })
        .unwrap();
    }
}
#[test]
fn cancellation_releases_keys_buttons_and_sends_bye() {
    run(true, |mut peer| async move {
        peer.handshake(false, false).await;
        peer.input
            .send(Command::Input(vec![
                ClientMsg::Key {
                    keycode: 42,
                    pressed: true,
                },
                ClientMsg::Key {
                    keycode: 30,
                    pressed: true,
                },
                ClientMsg::PointerButton {
                    button: 272,
                    pressed: true,
                },
            ]))
            .await
            .unwrap();
        for _ in 0..3 {
            peer.next().await;
        }
        peer.cancel.send_replace(true);
        let mut releases = Vec::new();
        loop {
            let msg = peer.next().await;
            if msg == ClientMsg::Bye {
                break;
            }
            releases.push(msg);
        }
        assert_eq!(releases.len(), 3);
        assert!(releases.contains(&ClientMsg::Key {
            keycode: 42,
            pressed: false
        }));
        assert!(releases.contains(&ClientMsg::Key {
            keycode: 30,
            pressed: false
        }));
        assert!(releases.contains(&ClientMsg::PointerButton {
            button: 272,
            pressed: false
        }));
    })
    .unwrap();
}
#[test]
fn release_command_cleans_state_and_mirror_resize_is_gated() {
    run(true, |mut peer| async move {
        peer.handshake(false, false).await;
        peer.input
            .send(Command::Resize(1280, 720, 1.5))
            .await
            .unwrap();
        peer.input
            .send(Command::Input(vec![ClientMsg::Key {
                keycode: 29,
                pressed: true,
            }]))
            .await
            .unwrap();
        assert_eq!(
            peer.next().await,
            ClientMsg::Key {
                keycode: 29,
                pressed: true
            }
        );
        peer.input.send(Command::Release).await.unwrap();
        assert_eq!(
            peer.next().await,
            ClientMsg::Key {
                keycode: 29,
                pressed: false
            }
        );
        peer.cancel.send_replace(true);
        assert_eq!(peer.next().await, ClientMsg::Bye);
    })
    .unwrap();
}
#[test]
fn resize_is_sent_only_for_acknowledged_headless_session() {
    run(true, |mut peer| async move {
        peer.handshake(false, true).await;
        peer.input
            .send(Command::Resize(1280, 720, 1.5))
            .await
            .unwrap();
        assert_eq!(
            peer.next().await,
            ClientMsg::Resize {
                width: 1280,
                height: 720,
                scale: 1.5
            }
        );
        peer.cancel.send_replace(true);
        assert_eq!(peer.next().await, ClientMsg::Bye);
    })
    .unwrap();
}
#[test]
fn version_and_hostile_geometry_fail_without_allocating_decoder() {
    let err = run(true, |mut peer| async move {
        peer.next().await;
        peer.wr
            .write_msg(&ServerMsg::HelloAck {
                version: 99,
                session: SessionInfo {
                    headless: false,
                    output: "test".into(),
                },
                outputs: vec![],
                features: vec![],
            })
            .await
            .unwrap();
        std::future::pending::<()>().await;
    })
    .unwrap_err();
    assert!(err.to_string().contains("Protocol version mismatch"));
    let err = run(true, |mut peer| async move {
        peer.handshake(false, false).await;
        let mut bad = config(false);
        if let ServerMsg::StreamConfig { width, .. } = &mut bad {
            *width = u32::MAX;
        }
        peer.wr.write_msg(&bad).await.unwrap();
        std::future::pending::<()>().await;
    })
    .unwrap_err();
    assert!(err.to_string().contains("Invalid stream geometry"));
}
#[test]
fn clipboard_disabled_ignores_remote_offer_and_local_text() {
    run(true, |mut peer| async move {
        peer.handshake(false, false).await;
        peer.input
            .send(Command::Text("private text".into()))
            .await
            .unwrap();
        peer.wr
            .write_msg(&ServerMsg::ClipboardOffer {
                serial: 1,
                mime_types: vec!["text/plain".into()],
                files: vec![],
            })
            .await
            .unwrap();
        peer.wr
            .write_msg(&ServerMsg::Ping { t: 999 })
            .await
            .unwrap();
        assert_eq!(peer.next().await, ClientMsg::Pong { t: 999 });
        assert!(peer.state.lock().unwrap().clipboard.is_none());
        peer.cancel.send_replace(true);
        assert_eq!(peer.next().await, ClientMsg::Bye);
    })
    .unwrap();
}
#[tokio::test]
async fn framing_rejects_huge_envelopes_truncation_and_payload_mismatch() {
    for header in [[2u32, (session::MAX_PACKET + 1) as u32], [1_048_577, 0]] {
        let bytes: Vec<u8> = header.into_iter().flat_map(u32::to_le_bytes).collect();
        let err = session::read_packet(&mut bytes.as_slice())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("memory limits"));
    }
    let mut framed = Framed::new(Vec::new());
    framed
        .write_msg_with_payloads(&ServerMsg::Ping { t: 1 }, &[b"extra"])
        .await
        .unwrap();
    let bytes = framed.into_inner();
    assert!(session::read_packet(&mut bytes.as_slice())
        .await
        .unwrap_err()
        .to_string()
        .contains("Payload length"));
    assert!(session::read_packet(&mut &bytes[..bytes.len() - 1])
        .await
        .is_err());
}
#[tokio::test]
async fn framing_accepts_fragmented_stream_and_skips_unknown_variant() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0x82, 0x18, 99, 0x80]);
    bytes.extend_from_slice(b"abc");
    let mut wr = Framed::new(Vec::new());
    wr.write_msg(&ServerMsg::Ping { t: 12345 }).await.unwrap();
    bytes.extend_from_slice(&wr.into_inner());
    let (mut write, mut read) = tokio::io::duplex(1);
    let sender = tokio::spawn(async move {
        for byte in bytes {
            write.write_all(&[byte]).await.unwrap();
        }
    });
    let (msg, payload) = session::read_packet(&mut read).await.unwrap();
    assert_eq!(msg, ServerMsg::Ping { t: 12345 });
    assert!(payload.is_empty());
    sender.await.unwrap();
}

#[test]
fn unicode_text_clipboard_round_trip_and_inactive_tab_withdrawal() {
    run(false, |mut peer| async move {
        peer.state.lock().unwrap().clipboard_active = true;
        peer.input.send(Command::ClipboardActive(true)).await.unwrap();
        peer.handshake(false, false).await;
        let text = "Gliff 🦋 Καλημέρα 日本語";
        peer.input.send(Command::Text(text.into())).await.unwrap();
        let ClientMsg::ClipboardOffer { serial, .. } = peer.next().await else { panic!("offer") };
        peer.wr.write_msg(&ServerMsg::ClipboardRequest { id: 2, serial, item: gliff_proto::ClipboardItem::Mime(gliff_proto::clipboard::TEXT_MIME.into()) }).await.unwrap();
        let mut received = Vec::new();
        loop {
            let msg = peer.next().await;
            let ClientMsg::ClipboardData { id, data_len, done, .. } = msg else { panic!("expected data: {msg:?}") };
            received.extend_from_slice(&peer.rd.read_payload(data_len).await.unwrap());
            if done { break; }
            peer.wr.write_msg(&ServerMsg::ClipboardAck { id, received: received.len() as u64 }).await.unwrap();
        }
        assert_eq!(received, text.as_bytes());
        peer.wr.write_msg(&ServerMsg::ClipboardOffer { serial: 50, mime_types: vec!["text/plain".into()], files: vec![] }).await.unwrap();
        let ClientMsg::ClipboardRequest { id, serial: 50, .. } = peer.next().await else { panic!("fetch") };
        peer.wr.write_msg_with_payloads(&ServerMsg::ClipboardData { id, offset: 0, data_len: text.len() as u32, done: true }, &[text.as_bytes()]).await.unwrap();
        loop {
            if peer.state.lock().unwrap().clipboard.as_deref() == Some(text) { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // Leave this tab. The core, not just the GUI, withdraws and forgets it.
        peer.state.lock().unwrap().clipboard_active = false;
        peer.input.send(Command::ClipboardActive(false)).await.unwrap();
        assert!(matches!(peer.next().await, ClientMsg::ClipboardOffer { ref mime_types, ref files, .. } if mime_types.is_empty() && files.is_empty()));
        peer.wr.write_msg(&ServerMsg::ClipboardRequest { id: 4, serial, item: gliff_proto::ClipboardItem::Mime(gliff_proto::clipboard::TEXT_MIME.into()) }).await.unwrap();
        assert_eq!(peer.next().await, ClientMsg::ClipboardAbort { id: 4 });
        peer.wr.write_msg(&ServerMsg::ClipboardOffer { serial: 51, mime_types: vec!["text/plain".into()], files: vec![] }).await.unwrap();
        peer.input.send(Command::Text("must not be offered".into())).await.unwrap();
        peer.wr.write_msg(&ServerMsg::Ping { t: 987 }).await.unwrap();
        assert_eq!(peer.next().await, ClientMsg::Pong { t: 987 });
        // Returning cannot serve the old serial until a fresh local offer.
        peer.state.lock().unwrap().clipboard_active = true;
        peer.input.send(Command::ClipboardActive(true)).await.unwrap();
        peer.wr.write_msg(&ServerMsg::ClipboardRequest { id: 6, serial, item: gliff_proto::ClipboardItem::Mime(gliff_proto::clipboard::TEXT_MIME.into()) }).await.unwrap();
        assert_eq!(peer.next().await, ClientMsg::ClipboardAbort { id: 6 });
        // Both transitions may queue before the worker sees the shared final
        // state. The leave edge must still withdraw the old selection.
        peer.input.send(Command::ClipboardActive(false)).await.unwrap();
        peer.input.send(Command::ClipboardActive(true)).await.unwrap();
        assert!(matches!(peer.next().await, ClientMsg::ClipboardOffer { ref mime_types, .. } if mime_types.is_empty()));
        peer.cancel.send_replace(true);
        assert_eq!(peer.next().await, ClientMsg::Bye);
    }).unwrap();
}

#[tokio::test(start_paused = true)]
async fn unresponsive_setup_has_bounded_deadline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let options = Options::parse_from(["gliff-desktop", "--no-clipboard"]);
            let state = Shared::default();
            let (_input, mut input_rx) = mpsc::channel(1);
            let (_cancel, mut cancel_rx) = watch::channel(false);
            let (client, _silent_peer) = tokio::io::duplex(64 * 1024);
            let (rd, wr) = tokio::io::split(client);
            let started = tokio::time::Instant::now();
            let err = session::run_stream(rd, wr, &options, &state, &mut input_rx, &mut cancel_rx)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("setup timed out"));
            assert!(started.elapsed() < Duration::from_secs(16));
        })
        .await;
}

#[test]
fn clipboard_caps_and_bad_offsets_abort_without_publishing_text() {
    for bad_offset in [false, true] {
        run(false, |mut peer| async move {
            peer.state.lock().unwrap().clipboard_active = true;
            peer.input.send(Command::ClipboardActive(true)).await.unwrap();
            peer.handshake(false, false).await;
            peer.wr.write_msg(&ServerMsg::ClipboardOffer { serial: 12, mime_types: vec!["text/plain".into()], files: vec![] }).await.unwrap();
            let ClientMsg::ClipboardRequest { id, .. } = peer.next().await else { panic!("request") };
            if !bad_offset {
                let chunk = vec![b'x'; gliff_proto::clipboard::CHUNK];
                for index in 0..4 {
                    peer.wr.write_msg_with_payloads(&ServerMsg::ClipboardData { id, offset: (index * chunk.len()) as u64, data_len: chunk.len() as u32, done: false }, &[&chunk]).await.unwrap();
                    assert!(matches!(peer.next().await, ClientMsg::ClipboardAck { id: got, .. } if got == id));
                }
            }
            peer.wr.write_msg_with_payloads(&ServerMsg::ClipboardData {
                id, offset: if bad_offset { 1 } else { gliff_desktop::clipboard::TEXT_CAP as u64 }, data_len: 1, done: true,
            }, &[b"x"]).await.unwrap();
            assert_eq!(peer.next().await, ClientMsg::ClipboardAbort { id });
            assert!(peer.state.lock().unwrap().clipboard.is_none());
            peer.cancel.send_replace(true);
            assert_eq!(peer.next().await, ClientMsg::Bye);
        }).unwrap();
    }
}

#[tokio::test]
async fn reconnect_survives_a_failed_setup_after_an_established_session() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut options = Options::parse_from(["gliff-desktop"]);
    options.connect = Some(listener.local_addr().unwrap());
    let mut session = session::Session::start("test".into(), options);
    for attempt in 0..3 {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("an established session must retry even when the next setup fails")
            .unwrap();
        if attempt == 1 {
            // Model a server still recovering during the first reconnect.
            drop(stream);
            continue;
        }
        let (read, write) = tokio::io::split(stream);
        let mut read = Framed::new(read);
        let mut write = Framed::new(write);
        assert!(matches!(
            read.read_msg::<ClientMsg>().await.unwrap(),
            ClientMsg::Hello { .. }
        ));
        write
            .write_msg(&ServerMsg::HelloAck {
                version: PROTOCOL_VERSION,
                session: SessionInfo {
                    headless: false,
                    output: "test".into(),
                },
                outputs: vec![],
                features: vec![],
            })
            .await
            .unwrap();
        write.write_msg(&config(false)).await.unwrap();
        write.write_msg(&ServerMsg::Ping { t: 47 }).await.unwrap();
        assert_eq!(
            read.read_msg::<ClientMsg>().await.unwrap(),
            ClientMsg::Pong { t: 47 }
        );
        if attempt == 2 {
            assert!(session.state.lock().unwrap().connected);
            session.disconnect();
            assert_eq!(read.read_msg::<ClientMsg>().await.unwrap(), ClientMsg::Bye);
        }
    }
    session.shutdown();
}

#[tokio::test]
async fn reconnect_stops_after_three_attempts_and_preserves_inactive_clipboard() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut options = Options::parse_from(["gliff-desktop"]);
    options.connect = Some(listener.local_addr().unwrap());
    let mut session = session::Session::start("test".into(), options);
    session.set_clipboard_active(true);
    session.set_clipboard_active(false);
    for _ in 0..3 {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let (read, write) = tokio::io::split(stream);
        let mut read = Framed::new(read);
        let mut write = Framed::new(write);
        assert!(matches!(
            read.read_msg::<ClientMsg>().await.unwrap(),
            ClientMsg::Hello { .. }
        ));
        write
            .write_msg(&ServerMsg::HelloAck {
                version: PROTOCOL_VERSION,
                session: SessionInfo {
                    headless: false,
                    output: "test".into(),
                },
                outputs: vec![],
                features: vec![],
            })
            .await
            .unwrap();
        write.write_msg(&config(false)).await.unwrap();
        write
            .write_msg(&ServerMsg::ClipboardOffer {
                serial: 23,
                mime_types: vec!["text/plain".into()],
                files: vec![],
            })
            .await
            .unwrap();
        write.write_msg(&ServerMsg::Ping { t: 77 }).await.unwrap();
        assert_eq!(
            read.read_msg::<ClientMsg>().await.unwrap(),
            ClientMsg::Pong { t: 77 }
        );
        // Drop each live peer to exercise the actual supervisor's backoff.
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while !session.finished() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!session.state.lock().unwrap().connected);
    assert!(session
        .state
        .lock()
        .unwrap()
        .status
        .contains("Disconnected"));
    assert!(session.state.lock().unwrap().clipboard.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    session.shutdown();
}
