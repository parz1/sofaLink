//! Explicit opt-in system test: inject only F24, then prove release through Windows state.
use sofalink_input::{InputServer, diagnostic_key_down};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    thread,
    time::{Duration, Instant},
};
fn wait(down: bool) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs(2);
    while diagnostic_key_down() != down {
        if Instant::now() > end {
            return Err(format!("F24 state did not become {down}"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if diagnostic_key_down() {
        return Err("F24 already held; refusing diagnostic".into());
    }
    let mut server = InputServer::start("diagnostic".into(), None, || true, |_| {})?;
    for timeout in [false, true] {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port))?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        let p = serde_json::json!({"version":1,"session":"diagnostic","key":server.key,"sequence":1,"keys":[135],"buttons":0});
        writeln!(stream, "{p}")?;
        let mut ack = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut ack)?;
        wait(true)?;
        if timeout {
            wait(false)?;
        } else {
            drop(stream);
            wait(false)?;
        }
    }
    server.shutdown()?;
    println!("PASS: real Win32 F24 injection, disconnect release, heartbeat timeout release");
    Ok(())
}
