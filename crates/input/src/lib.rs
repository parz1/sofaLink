//! Independent reliable input channel. Only the Windows adapter knows OS APIs.
#[cfg(windows)]
pub mod focus_adapter;
#[cfg(windows)]
mod windows;
use serde::Deserialize;
use sofalink_core::{InputState, InputStatus};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    version: u8,
    session: String,
    key: String,
    sequence: u64,
    keys: Vec<u16>,
    buttons: u8,
    #[serde(default)]
    dx: i32,
    #[serde(default)]
    dy: i32,
    #[serde(default)]
    wheel: i32,
}
trait Sink {
    fn key(&mut self, vk: u16, down: bool) -> Result<(), String>;
    fn mouse(&mut self, x: i32, y: i32, flags: u32, data: u32) -> Result<(), String>;
}
struct Windows {
    target: Option<isize>,
}
#[cfg(windows)]
impl Sink for Windows {
    fn key(&mut self, vk: u16, down: bool) -> Result<(), String> {
        if down {
            windows::focus(self.target)?;
            self.target = None;
        }
        windows::key(vk, down)
    }
    fn mouse(&mut self, x: i32, y: i32, flags: u32, data: u32) -> Result<(), String> {
        if !matches!(flags, 4 | 16 | 64) {
            windows::focus(self.target)?;
            self.target = None;
        }
        windows::mouse(x, y, flags, data)
    }
}
#[cfg(not(windows))]
impl Sink for Windows {
    fn key(&mut self, _: u16, _: bool) -> Result<(), String> {
        Err("Windows only".into())
    }
    fn mouse(&mut self, _: i32, _: i32, _: u32, _: u32) -> Result<(), String> {
        Err("Windows only".into())
    }
}
#[derive(Default)]
struct Held {
    keys: BTreeSet<u16>,
    buttons: u8,
    sequence: u64,
    applied: u64,
}
fn valid_key(k: u16) -> bool {
    matches!(k,8|9|13|27|32..=40|45|46|48..=90|112..=135|160..=165|186..=192|219..=222)
}
impl Held {
    fn apply(&mut self, p: &Packet, sink: &mut impl Sink) -> Result<(), String> {
        if p.sequence <= self.sequence
            || p.keys.len() > 32
            || p.keys.iter().any(|k| !valid_key(*k))
            || p.buttons > 7
            || p.dx.abs_diff(0) > 4096
            || p.dy.abs_diff(0) > 4096
            || p.wheel.abs_diff(0) > 1200
        {
            return Err("Invalid/stale input packet".into());
        }
        let next: BTreeSet<_> = p.keys.iter().copied().collect();
        // Update owned state after each successful injection, so partial failures can be released.
        for vk in self.keys.difference(&next).copied().collect::<Vec<_>>() {
            sink.key(vk, false)?;
            self.keys.remove(&vk);
            self.applied += 1;
        }
        let mut pressed = next.difference(&self.keys).copied().collect::<Vec<_>>();
        pressed.sort_by_key(|vk| (!matches!(vk, 160..=165), *vk));
        for vk in pressed {
            sink.key(vk, true)?;
            self.keys.insert(vk);
            self.applied += 1;
        }
        for (bit, down, up) in [(1, 2, 4), (2, 8, 16), (4, 32, 64)] {
            if (self.buttons ^ p.buttons) & bit != 0 {
                sink.mouse(0, 0, if p.buttons & bit != 0 { down } else { up }, 0)?;
                self.buttons ^= bit;
                self.applied += 1;
            }
        }
        if p.dx != 0 || p.dy != 0 {
            sink.mouse(p.dx, p.dy, 1, 0)?;
            self.applied += 1;
        }
        if p.wheel != 0 {
            sink.mouse(0, 0, 0x800, p.wheel as u32)?;
            self.applied += 1;
        }
        self.sequence = p.sequence;
        Ok(())
    }
    fn release(&mut self, sink: &mut impl Sink) -> Result<(), String> {
        let p = Packet {
            version: 1,
            session: String::new(),
            key: String::new(),
            sequence: self.sequence + 1,
            keys: vec![],
            buttons: 0,
            dx: 0,
            dy: 0,
            wheel: 0,
        };
        self.apply(&p, sink)
    }
}
pub struct InputServer {
    pub port: u16,
    pub key: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
    shutdown_error: Option<String>,
}

/// OS-generated credential shared by control/input adapters, never by core.
pub fn random_secret() -> Result<String, String> {
    #[cfg(windows)]
    {
        windows::secret()
    }
    #[cfg(not(windows))]
    {
        Err("Windows random provider unavailable".into())
    }
}
pub fn window_exists(window: isize, owner: u32) -> bool {
    #[cfg(windows)]
    {
        windows::window_exists(window, owner)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, owner);
        false
    }
}
impl InputServer {
    pub fn start(
        session: String,
        target: Option<isize>,
        active: impl Fn() -> bool + Send + 'static,
        report: impl FnMut(InputState) + Send + 'static,
    ) -> Result<Self, String> {
        Self::start_on("127.0.0.1", session, target, active, report)
    }
    pub fn start_on(
        bind_ip: &str,
        session: String,
        target: Option<isize>,
        active: impl Fn() -> bool + Send + 'static,
        mut report: impl FnMut(InputState) + Send + 'static,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        let key = windows::secret()?;
        #[cfg(not(windows))]
        let key = String::from("unsupported");
        if !cfg!(windows) {
            return Err("Windows only".into());
        }
        // Emulator uses loopback; Fire TV binds only its route's host interface.
        // Both paths require the per-session secret before applying input.
        let listener = TcpListener::bind((bind_ip, 0)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let secret = key.clone();
        let worker = thread::spawn(move || {
            let mut sink = Windows { target };
            report(InputState::new(
                InputStatus::Waiting,
                "等待客户端输入握手",
                0,
            ));
            while !flag.load(Ordering::Acquire) && active() {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let mut held = Held::default();
                        let result = connection(
                            stream,
                            (&session, &secret),
                            &flag,
                            &active,
                            &mut held,
                            &mut sink,
                            &mut report,
                        );
                        // Always release before accepting a replacement connection or ending session.
                        if let Err(e) = held.release(&mut sink) {
                            report(InputState::new(InputStatus::Error, &e, held.applied));
                            return Err(e);
                        }
                        report(InputState::new(
                            if result.is_ok() {
                                InputStatus::Disconnected
                            } else {
                                InputStatus::Error
                            },
                            result
                                .err()
                                .as_deref()
                                .unwrap_or("输入连接断开，按键已释放"),
                            held.applied,
                        ));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        Ok(Self {
            port,
            key,
            stop,
            worker: Some(worker),
            shutdown_error: None,
        })
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            self.shutdown_error = worker
                .join()
                .unwrap_or_else(|_| Err("Input worker panicked".into()))
                .err();
        }
        match &self.shutdown_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}
impl Drop for InputServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn connection(
    mut stream: TcpStream,
    credentials: (&str, &str),
    stop: &AtomicBool,
    active: &impl Fn() -> bool,
    held: &mut Held,
    sink: &mut impl Sink,
    report: &mut impl FnMut(InputState),
) -> Result<(), String> {
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    let mut line = Vec::new();
    let mut buffer = [0u8; 1024];
    let mut last = Instant::now();
    while !stop.load(Ordering::Acquire) && active() {
        if last.elapsed() > Duration::from_millis(750) {
            return Err("输入心跳超时，按键已释放".into());
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                for b in &buffer[..n] {
                    if *b == b'\n' {
                        let p: Packet =
                            serde_json::from_slice(&line).map_err(|_| "Invalid input JSON")?;
                        line.clear();
                        if p.version != 1 || p.session != credentials.0 || p.key != credentials.1 {
                            return Err("Input session rejected".into());
                        }
                        if stop.load(Ordering::Acquire) || !active() {
                            return Ok(());
                        }
                        held.apply(&p, sink)?;
                        last = Instant::now();
                        let ack = format!(
                            "{{\"sequence\":{},\"applied\":{}}}\n",
                            held.sequence, held.applied
                        );
                        stream
                            .write_all(ack.as_bytes())
                            .map_err(|e| e.to_string())?;
                        report(InputState::new(
                            InputStatus::Ready,
                            "键鼠控制可用",
                            held.applied,
                        ));
                    } else {
                        line.push(*b);
                        if line.len() > 1024 {
                            return Err("Input packet too large".into());
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
#[cfg(windows)]
pub fn diagnostic_key_down() -> bool {
    windows::diagnostic_key_down()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        keys: Vec<(u16, bool)>,
        mouse: Vec<(i32, i32, u32, u32)>,
    }
    impl Sink for Fake {
        fn key(&mut self, k: u16, d: bool) -> Result<(), String> {
            self.keys.push((k, d));
            Ok(())
        }
        fn mouse(&mut self, x: i32, y: i32, f: u32, d: u32) -> Result<(), String> {
            self.mouse.push((x, y, f, d));
            Ok(())
        }
    }
    fn packet(seq: u64, keys: Vec<u16>) -> Packet {
        Packet {
            version: 1,
            session: "test".into(),
            key: "test".into(),
            sequence: seq,
            keys,
            buttons: 0,
            dx: 0,
            dy: 0,
            wheel: 0,
        }
    }
    #[test]
    fn hold_repeat_and_disconnect_release() {
        let mut h = Held::default();
        let mut f = Fake::default();
        h.apply(&packet(1, vec![87, 160]), &mut f).unwrap();
        h.apply(&packet(2, vec![87, 160]), &mut f).unwrap();
        assert_eq!(f.keys.len(), 2);
        h.release(&mut f).unwrap();
        assert!(h.keys.is_empty());
        assert_eq!(
            f.keys,
            vec![(160, true), (87, true), (87, false), (160, false)]
        );
    }
    #[test]
    fn invalid_packets_never_inject() {
        let mut h = Held::default();
        let mut f = Fake::default();
        for p in [
            packet(0, vec![87]),
            packet(1, vec![255]),
            packet(1, vec![87; 33]),
        ] {
            assert!(h.apply(&p, &mut f).is_err());
        }
        assert!(f.keys.is_empty());
    }
    #[test]
    fn mouse_buttons_release_and_movement_once() {
        let mut h = Held::default();
        let mut f = Fake::default();
        let mut p = packet(1, vec![]);
        p.buttons = 3;
        p.dx = 4;
        p.dy = -2;
        h.apply(&p, &mut f).unwrap();
        assert!(h.apply(&p, &mut f).is_err());
        h.release(&mut f).unwrap();
        assert_eq!(
            f.mouse,
            vec![
                (0, 0, 2, 0),
                (0, 0, 8, 0),
                (4, -2, 1, 0),
                (0, 0, 4, 0),
                (0, 0, 16, 0)
            ]
        );
    }
    #[test]
    fn unauthenticated_connection_and_replay_cannot_inject_more_input() {
        use std::io::{BufRead, BufReader};
        for authenticated in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let (stream, _) = listener.accept().unwrap();
            let worker = thread::spawn(move || {
                let mut held = Held::default();
                let mut sink = Fake::default();
                let result = connection(
                    stream,
                    ("s", "secret"),
                    &AtomicBool::new(false),
                    &|| true,
                    &mut held,
                    &mut sink,
                    &mut |_| {},
                );
                held.release(&mut sink).unwrap();
                (result, sink)
            });
            let p = serde_json::json!({"version":1,"session":"s","key":if authenticated{"secret"}else{"forged"},"sequence":1,"keys":[87],"buttons":0});
            writeln!(client, "{p}").unwrap();
            if authenticated {
                let mut line = String::new();
                BufReader::new(client.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                writeln!(client, "{p}").unwrap();
            }
            drop(client);
            let (result, sink) = worker.join().unwrap();
            assert!(result.is_err());
            assert_eq!(
                sink.keys,
                if authenticated {
                    vec![(87, true), (87, false)]
                } else {
                    vec![]
                }
            );
        }
    }
}
