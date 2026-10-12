use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use sofalink_core::{Command, TimeMs};
use sofalink_emulator::Config;
use sofalink_simulator::Scenario;
use sofalink_tui::{App, render, render_text};
use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scenario = Scenario::Healthy;
    let mut preview = false;
    let mut snapshot_json = false;
    let mut emulator = false;
    let mut fire_tv = None;
    let mut test_source = false;
    let mut test_audio = false;
    let mut verify = false;
    let mut diagnostics = false;
    let mut measure = None;
    let mut no_sync_video = false;
    let mut encoder_gop = None;
    let mut encoder_vbv = None;
    let mut audio_backend = None;
    let mut stream_request = None;
    let mut reference_video = false;
    let mut presentation_mode = None;
    let mut report = None;
    let mut out = std::path::PathBuf::from("local/diagnostics");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "help" => {
                println!(
                    "音视频请求：--fire-tv <IPv4> --stream-request <JSON>；省略时使用 core 默认目标。JSON 包含 version、video、bitrate_kbit、color、audio。"
                );
                println!(
                    "逐帧采样：(--emulator | --fire-tv <IPv4>) --diagnostics --measure 35 [--test-source] [--no-sync-video]\n编码候选：--fire-tv <IPv4> --diagnostics --measure 35 --encoder-gop -1 --encoder-vbv 1\n离线报告：--report <session.jsonl> --out <directory>\n--no-sync-video 仅供有界对照采样；默认保留音画同步。"
                );
                println!(
                    "sofaLink · Rust TUI\n模拟状态：sofalink-tui [--scenario healthy|startup-fail|startup-timeout|audio-stall|video-stall|disconnect|stop-fail] [--preview | --snapshot]\n实际音画：sofalink-tui (--emulator | --fire-tv <IPv4>) [--test-source | --test-tone] [--verify]\n--test-tone 只替换音频源，用于诊断客户端听音。\nEnter 主操作 · S 结束/取消 · D 详情 · 1–7 模拟场景 · Q / Ctrl+C 清理后退出"
                );
                return Ok(());
            }
            "--scenario" => {
                i += 1;
                scenario = args
                    .get(i)
                    .and_then(|s| Scenario::parse(s))
                    .ok_or("unknown or missing scenario")?;
            }
            "--preview" => preview = true,
            "--snapshot" => snapshot_json = true,
            "--emulator" => emulator = true,
            "--fire-tv" => {
                i += 1;
                fire_tv = Some(args.get(i).ok_or("missing Fire TV IPv4 address")?.clone());
            }
            "--test-source" => test_source = true,
            "--reference-video" => reference_video = true,
            "--presentation-mode" => {
                i += 1;
                let value = args.get(i).ok_or("missing presentation mode")?;
                if !matches!(value.as_str(), "timed" | "vsync") {
                    return Err("presentation mode must be timed or vsync".into());
                }
                presentation_mode = Some(value.clone());
            }
            "--stream-request" => {
                i += 1;
                let path = args.get(i).ok_or("missing stream request JSON path")?;
                let request: sofalink_core::StreamRequest =
                    serde_json::from_slice(&std::fs::read(path)?)?;
                request.validate()?;
                stream_request = Some(request);
            }
            "--test-tone" => test_audio = true,
            "--verify" => verify = true,
            "--diagnostics" => diagnostics = true,
            "--audio-backend" => {
                i += 1;
                let value = args.get(i).ok_or("missing audio backend")?;
                if !matches!(
                    value.as_str(),
                    "aaudio" | "aaudio-exclusive" | "aaudio-routed" | "audiotrack"
                ) {
                    return Err(
                        "audio backend must be aaudio, aaudio-exclusive, aaudio-routed or audiotrack".into(),
                    );
                }
                audio_backend = Some(value.clone());
            }
            "--no-sync-video" => no_sync_video = true,
            "--encoder-gop" => {
                i += 1;
                let value: i32 = args.get(i).ok_or("missing encoder GOP")?.parse()?;
                if value != -1 && !(1..=600).contains(&value) {
                    return Err("encoder GOP must be -1 or 1..600".into());
                }
                encoder_gop = Some(value);
            }
            "--encoder-vbv" => {
                i += 1;
                let value: u32 = args
                    .get(i)
                    .ok_or("missing encoder VBV frame budget")?
                    .parse()?;
                if !(1..=8).contains(&value) {
                    return Err("encoder VBV must be 1..8".into());
                }
                encoder_vbv = Some(value);
            }
            "--measure" => {
                i += 1;
                let seconds: u64 = args.get(i).ok_or("missing measurement seconds")?.parse()?;
                if !(5..=300).contains(&seconds) {
                    return Err("measurement seconds must be 5..300".into());
                }
                measure = Some(seconds);
            }
            "--report" => {
                i += 1;
                report = Some(std::path::PathBuf::from(
                    args.get(i).ok_or("missing log path")?,
                ));
            }
            "--out" => {
                i += 1;
                out = args.get(i).ok_or("missing output directory")?.into();
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }
    if let Some(path) = report {
        return sofalink_tui::diagnostics::report(&path, &out).map_err(Into::into);
    }
    if emulator && fire_tv.is_some() {
        return Err("Choose --emulator or --fire-tv".into());
    }
    let device_mode = emulator || fire_tv.is_some();
    if (reference_video || presentation_mode.is_some())
        && !(fire_tv.is_some() && diagnostics && measure.is_some())
    {
        return Err("presentation experiments require bounded Fire TV diagnostics".into());
    }
    if stream_request.is_some() && !device_mode {
        return Err("--stream-request requires a device backend".into());
    }
    if (test_source || test_audio || verify || diagnostics || measure.is_some()) && !device_mode {
        return Err("媒体验证需要 --emulator 或 --fire-tv <IP>".into());
    }
    if no_sync_video && !(diagnostics && measure.is_some()) {
        return Err("--no-sync-video requires a bounded --measure with --diagnostics".into());
    }
    if (encoder_gop.is_some() || encoder_vbv.is_some() || audio_backend.is_some())
        && !(diagnostics && measure.is_some() && fire_tv.is_some())
    {
        return Err(
            "encoder overrides require --fire-tv --diagnostics and bounded --measure".into(),
        );
    }
    if device_mode && (preview || snapshot_json) {
        return Err("设备验证使用 --verify；--preview/--snapshot 仅用于状态模拟器".into());
    }
    let mut app = if device_mode {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let mut config = if let Some(ip) = fire_tv {
            Config::fire_tv(root, &ip, test_source)?
        } else {
            Config::local(root, test_source)?
        };
        config.diagnostics = diagnostics;
        config.reference_video = reference_video;
        config.presentation_mode_override = presentation_mode;
        if let Some(request) = stream_request {
            config.stream_request = request;
        }
        config.encoder_gop_override = encoder_gop;
        config.encoder_vbv_override = encoder_vbv;
        config.audio_backend_override = audio_backend;
        config.test_audio = test_audio;
        config.sync_video = !no_sync_video;
        App::emulator(config)?
    } else {
        App::new(scenario)
    };
    if let Some(seconds) = measure {
        return sofalink_tui::diagnostics::measure(&mut app, seconds).map_err(Into::into);
    }
    if verify {
        return sofalink_tui::verify::run(&mut app).map_err(Into::into);
    }
    if preview || snapshot_json {
        app.primary(0);
        let until = if scenario == Scenario::StartupTimeout {
            5_500
        } else {
            4_500
        };
        for now in (50..=until).step_by(50) {
            app.advance(now);
            if scenario == Scenario::StopFail && now == 2_000 {
                app.primary(now);
            }
        }
        if snapshot_json {
            println!("{}", serde_json::to_string_pretty(app.runtime.snapshot())?);
        } else {
            print!("{}", render_text(&app, 90, 25)?);
        }
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "TUI 需要交互终端。自动验证请运行 cargo run -p sofalink-simulator -- check".into(),
        );
    }
    let clock = Instant::now();
    let result = ratatui::run(|terminal| -> io::Result<()> {
        let mut dirty = true;
        let mut rendered_revision = u64::MAX;
        loop {
            let now = clock.elapsed().as_millis() as TimeMs;
            app.advance(now);
            if app.can_exit() {
                break Ok(());
            }
            if app
                .quit_started
                .is_some_and(|since| now.saturating_sub(since) > 15_000)
            {
                break Err(io::Error::other("退出清理超时，资源释放尚未确认"));
            }
            if dirty || rendered_revision != app.runtime.snapshot().revision {
                terminal.draw(|frame| render(frame, &app))?;
                rendered_revision = app.runtime.snapshot().revision;
                dirty = false;
            }
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }
            let input = event::read()?;
            if matches!(input, Event::Resize(_, _)) {
                dirty = true;
            }
            if let Event::Key(key) = input {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                dirty = true;
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    app.request_exit(now);
                    continue;
                }
                if app.quitting {
                    continue;
                }
                match key.code {
                    KeyCode::Enter => app.primary(now),
                    KeyCode::Esc | KeyCode::Char('s')
                        if app.runtime.snapshot().can(Command::Stop) =>
                    {
                        app.command(Command::Stop, now)
                    }
                    KeyCode::Char('d') => app.details = !app.details,
                    KeyCode::Char(n @ '1'..='7') => app.select_scenario(n as usize - '1' as usize),
                    _ => {}
                }
            }
        }
    });
    // Also release simulated resources on terminal I/O failure.
    let now = clock.elapsed().as_millis() as TimeMs;
    app.request_exit(now);
    let cleanup_start = clock.elapsed();
    while clock.elapsed().saturating_sub(cleanup_start) < Duration::from_secs(15) {
        if app.can_exit() {
            break;
        }
        let now = clock.elapsed().as_millis() as TimeMs;
        app.advance(now);
        if app.runtime.snapshot().can(Command::Recover) {
            app.command(Command::Recover, now);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    result?;
    if !app.runtime.snapshot().resources.clear() {
        return Err("退出时资源释放未确认".into());
    }
    println!("会话已退出，资源已释放。");
    Ok(())
}
