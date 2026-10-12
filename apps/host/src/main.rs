//! User-session Windows host: one actor owns Runtime; bounded sockets cannot block media/core.
mod protocol;
use protocol::*;
use serde_json::{Value, json};
use sofalink_core::*;
use sofalink_emulator::{Config, EmulatorBackend, remote::RemoteBridge};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

struct Call {
    request: Request,
    peer: SocketAddr,
    reply: mpsc::SyncSender<Value>,
}
fn read_deadline(
    stream: &mut TcpStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), String> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("Control read deadline expired")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let count = stream.read(bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Control connection closed".into());
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}
fn socket(mut stream: TcpStream, peer: SocketAddr, tx: mpsc::SyncSender<Call>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_nodelay(true);
    let response = (|| -> Result<Value, String> {
        let mut header = [0; 4];
        let deadline = Instant::now() + Duration::from_secs(1);
        read_deadline(&mut stream, &mut header, deadline)?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > 65536 {
            return Err("Invalid request size".into());
        }
        let mut bytes = vec![0; length];
        read_deadline(&mut stream, &mut bytes, deadline)?;
        let request =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid request: {e}"))?;
        let (reply, rx) = mpsc::sync_channel(1);
        tx.try_send(Call {
            request,
            peer,
            reply,
        })
        .map_err(|_| "Host busy".to_owned())?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Host unavailable".into())
    })()
    .unwrap_or_else(|e| json!({"version":1,"error":e}));
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes());
        let _ = stream.write_all(&bytes);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut root = std::env::current_dir()?;
    let mut bind = "0.0.0.0:47990".to_string();
    let mut python = None;
    let mut duration = None;
    let mut diagnostics = false;
    let mut reset = false;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                root = PathBuf::from(args.get(i).ok_or("Missing root")?);
            }
            "--bind" => {
                i += 1;
                bind = args.get(i).ok_or("Missing bind")?.clone();
            }
            "--python" => {
                i += 1;
                python = Some(PathBuf::from(args.get(i).ok_or("Missing python")?));
            }
            "--run-seconds" => {
                i += 1;
                duration = Some(args.get(i).ok_or("Missing seconds")?.parse::<u64>()?);
            }
            "--diagnostics" => diagnostics = true,
            "--reset-pairing" => reset = true,
            "--help" => {
                println!(
                    "sofaLink host · run in logged-in Windows user session\n--bind IPv4:47990 --python path --diagnostics --root directory\nTV pairs using the displayed code. Ctrl+C ends the session and restores PC state."
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown argument {}", args[i]).into()),
        }
        i += 1;
    }
    let store = root.join("local/host-pairing.json");
    std::fs::create_dir_all(root.join("local"))?;
    let mut auth = Pairing::load(store, reset)?;
    if auth.is_unpaired() {
        println!(
            "sofaLink pairing code: {} (expires in 5 minutes)",
            auth.code()
        );
    } else {
        println!("sofaLink: paired TV loaded");
    }
    let listener = TcpListener::bind(&bind)?;
    listener.set_nonblocking(true)?;
    println!("sofaLink host listening on {}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let handler = stop.clone();
    ctrlc::set_handler(move || handler.store(true, Ordering::Release))?;
    let (tx, rx) = mpsc::sync_channel::<Call>(16);
    let sockets = Arc::new(AtomicUsize::new(0));
    let accepting = Arc::new(AtomicBool::new(false));
    let accept_done = accepting.clone();
    let active = sockets.clone();
    let acceptor = thread::spawn(move || {
        while !accepting.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    if active.load(Ordering::Acquire) >= 8 {
                        drop(stream);
                        continue;
                    }
                    active.fetch_add(1, Ordering::AcqRel);
                    let active = active.clone();
                    let tx = tx.clone();
                    thread::spawn(move || {
                        socket(stream, peer, tx);
                        active.fetch_sub(1, Ordering::AcqRel);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(_) => break,
            }
        }
    });
    let clock = Instant::now();
    let instance = sofalink_input::random_secret()?;
    let mut evidence = std::io::BufWriter::new(std::fs::File::create(
        root.join(format!("local/host-state-{}.jsonl", std::process::id())),
    )?);
    let mut recorded = None;
    let mut next_record = Instant::now();
    let bridge = RemoteBridge::default();
    let mut runtime: Option<Runtime<EmulatorBackend>> = None;
    let mut device = None;
    let mut stopping_since = None;
    let mut shutdown_failed = false;
    loop {
        let now = clock.elapsed().as_millis() as TimeMs;
        if let Some(rt) = runtime.as_mut() {
            rt.advance(now);
            if recorded != Some(rt.snapshot().revision) && Instant::now() >= next_record {
                writeln!(
                    evidence,
                    "{}",
                    json!({"host_uptime_ms":now,"snapshot":rt.snapshot()})
                )?;
                evidence.flush()?;
                recorded = Some(rt.snapshot().revision);
                next_record = Instant::now() + Duration::from_millis(250);
            }
        }
        for call in rx.try_iter().take(16) {
            let response=(||->Result<Value,String>{
                let request=&call.request;request.validate()?;
                if !matches!(call.peer.ip(),std::net::IpAddr::V4(ip) if ip.is_private()||ip.is_loopback()||ip.is_link_local()){return Err("LAN IPv4 client required".into());}
                if request.op=="pair" {let key=auth.pair(request,&call.peer.ip().to_string())?;return Ok(json!({"version":1,"paired_key":key}));}
                auth.authorize(request)?;
                if stop.load(Ordering::Acquire) && request.op=="command" {return Err("Host stopping".into());}
                if runtime.is_none(){
                    let ip=call.peer.ip().to_string();let mut config=Config::fire_tv(root.clone(),&ip,false)?;
                    config.remote=Some(bridge.clone());config.steam_big_picture=true;config.diagnostics=diagnostics;
                    if let Some(path)=&python{config.python=path.clone();}
                    let backend=EmulatorBackend::new(config)?;
                    runtime=Some(Runtime::with_host_mute(backend,Timeouts{startup_ms:60000,feedback_ms:2000,recovery_ms:90000,cleanup_ms:25000},true));device=Some(ip);
                }
                if device.as_deref()!=Some(&call.peer.ip().to_string()){return Err("This host session belongs to another TV address".into());}
                let rt=runtime.as_mut().expect("runtime");
                bridge.report(&request.client_session,request.client_running,&request.client_stopped_session,&request.probe_complete,&request.probe_error,request.probe.clone());
                let mut command_error=None;
                if request.op=="command" {
                    if request.expected_instance!=instance{return Err("Stale host command".into());}
                    let expected=request.expected_token.ok_or("Session token required")?;
                    if expected!=rt.snapshot().token{return Err("Stale session command".into());}
                    if let Err(e)=rt.command(request.command.ok_or("Command required")?,now){command_error=Some(e.to_string());}
                }
                Ok(json!({"version":1,"instance":instance,"snapshot":rt.snapshot(),"action":bridge.action(),"error":command_error}))
            })().unwrap_or_else(|error|json!({"version":1,"pairing_required":error=="Pairing required","error":error}));
            let _ = call.reply.try_send(response);
        }
        if duration.is_some_and(|seconds| clock.elapsed() >= Duration::from_secs(seconds)) {
            stop.store(true, Ordering::Release);
        }
        if stop.load(Ordering::Acquire) {
            let since = *stopping_since.get_or_insert(Instant::now());
            if let Some(rt) = runtime.as_mut() {
                let phase = rt.snapshot().phase;
                if rt.snapshot().can(Command::Stop) {
                    let _ = rt.command(Command::Stop, now);
                }
                if phase == Phase::Idle
                    || (phase == Phase::Faulted && rt.snapshot().resources.clear())
                {
                    break;
                }
                if since.elapsed() > Duration::from_secs(27) {
                    shutdown_failed = true;
                    eprintln!(
                        "Host cleanup could not be confirmed; inspect local logs and restart to recover"
                    );
                    break;
                }
            } else {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    accept_done.store(true, Ordering::Release);
    let _ = acceptor.join();
    drop(runtime);
    if shutdown_failed {
        return Err("Host cleanup was not confirmed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drip_feed_cannot_extend_absolute_socket_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (go, wait) = mpsc::sync_channel(0);
        let writer = thread::spawn(move || {
            let mut s = TcpStream::connect(address).unwrap();
            wait.recv().unwrap();
            for _ in 0..5 {
                let _ = s.write_all(&[1]);
                thread::sleep(Duration::from_millis(80));
            }
        });
        let (mut s, _) = listener.accept().unwrap();
        let start = Instant::now();
        let mut bytes = [0; 5];
        let deadline = Instant::now() + Duration::from_millis(180);
        go.send(()).unwrap();
        assert!(read_deadline(&mut s, &mut bytes, deadline).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(s);
        writer.join().unwrap();
    }
}
