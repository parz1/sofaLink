//! Bounded real-device sampling and offline, paired frame analysis.
use crate::App;
use serde_json::Value;
use sofalink_core::{Phase, TimeMs};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub fn measure(app: &mut App, seconds: u64) -> Result<(), String> {
    let clock = Instant::now();
    let mut started = None;
    let mut last_print = 0;
    app.primary(0);
    let result = loop {
        let now = clock.elapsed().as_millis() as TimeMs;
        app.advance(now);
        let s = app.runtime.snapshot();
        if s.phase == Phase::Playing {
            started.get_or_insert(now);
        }
        if now >= last_print + 1000 {
            println!("{}", serde_json::to_string(s).map_err(|e| e.to_string())?);
            last_print = now;
        }
        if s.phase == Phase::Faulted {
            break Err(s.message.clone());
        }
        if started.is_some_and(|t| now - t >= seconds * 1000) {
            break Ok(());
        }
        if started.is_none() && now > 30_000 {
            break Err("measurement startup timed out".into());
        }
        thread::sleep(Duration::from_millis(25));
    };
    app.request_exit(clock.elapsed().as_millis() as TimeMs);
    let cleanup = Instant::now();
    while !app.can_exit() && cleanup.elapsed() < Duration::from_secs(15) {
        app.advance(clock.elapsed().as_millis() as TimeMs);
        thread::sleep(Duration::from_millis(25));
    }
    if !app.can_exit() {
        return Err("measurement cleanup not confirmed".into());
    }
    println!(
        "{}",
        serde_json::to_string(app.runtime.snapshot()).map_err(|e| e.to_string())?
    );
    result
}

#[derive(Default)]
struct Analysis {
    target_fps: HashMap<String, u32>,
    hosts: HashMap<(String, u64), Vec<Option<i64>>>,
    clients: BTreeMap<(String, u64), ClientRow>,
    arrivals: BTreeMap<(String, u64), [i64; 2]>,
    outputs: BTreeMap<(String, u64), [i64; 5]>,
    invalid: usize,
    duplicate: usize,
    host_drops: u64,
    client_drops: u64,
    telemetry_drops: u64,
    arrival_drops: u64,
    client_counters: Option<String>,
    audio_diagnostics: BTreeMap<&'static str, Vec<f64>>,
    audio_output: Option<String>,
}
type ClientRow = (Vec<i64>, Option<(i64, i64)>);
impl Analysis {
    fn ingest(&mut self, v: &Value) {
        let Some(session) = v["session_id"].as_str() else {
            self.invalid += 1;
            return;
        };
        if session.len() > 128
            || !session
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            self.invalid += 1;
            return;
        }
        match v["type"].as_str() {
            Some("configuration") => {
                if let Some(fps) = v["fps"].as_u64().filter(|fps| (1..=240).contains(fps)) {
                    self.target_fps.insert(session.into(), fps as u32);
                }
            }
            Some("host_frames") => {
                if let Some(rows) = v["rows"].as_array() {
                    for row in rows {
                        let Some(a) = row.as_array().filter(|a| a.len() == 5) else {
                            self.invalid += 1;
                            continue;
                        };
                        let Some(rtp) = a[0].as_u64() else {
                            self.invalid += 1;
                            continue;
                        };
                        let ns: Vec<_> = a.iter().map(Value::as_i64).collect();
                        if rtp > u32::MAX as u64
                            || ns[2..]
                                .iter()
                                .any(|n| n.is_none_or(|n| n <= 0 || n > i64::MAX / 4))
                            || ns[3] < ns[2]
                            || ns[4] < ns[3]
                            || ns[1].is_some_and(|cap| {
                                cap <= 0 || ns[2].is_none_or(|submit| cap > submit)
                            })
                        {
                            self.invalid += 1;
                            continue;
                        }
                        if self.hosts.insert((session.into(), rtp), ns).is_some() {
                            self.duplicate += 1;
                        }
                    }
                }
            }
            Some("client_frames") => {
                let t = &v["trace"];
                if t["sessionId"].as_str() != Some(session) {
                    self.invalid += 1;
                    return;
                }
                if let Some(rows) = t["arrivals"].as_array() {
                    for row in rows {
                        let ns: Option<Vec<i64>> = row
                            .as_array()
                            .filter(|r| r.len() == 3)
                            .and_then(|r| r.iter().map(Value::as_i64).collect());
                        if let Some(ns) = ns.filter(|n| {
                            n[0] >= 0
                                && n[0] <= u32::MAX as i64
                                && n[1] > 0
                                && n[1] <= n[2]
                                && n[2] < i64::MAX / 4
                        }) {
                            self.arrivals
                                .insert((session.into(), ns[0] as u64), [ns[1], ns[2]]);
                        } else {
                            self.invalid += 1;
                        }
                    }
                }
                if let Some(rows) = t["outputs"].as_array() {
                    for row in rows {
                        let ns: Option<Vec<i64>> = row
                            .as_array()
                            .filter(|r| r.len() == 6)
                            .and_then(|r| r.iter().map(Value::as_i64).collect());
                        if let Some(n) = ns.filter(|n| {
                            n[0] >= 0
                                && n[0] <= u32::MAX as i64
                                && n[1] > 0
                                && n[1] <= n[2]
                                && n[2] <= n[4]
                                && n[4] < i64::MAX / 4
                                && n[3] >= 0
                                && n[3] < i64::MAX / 4
                                && (n[5] == 0 || n[5] == 1)
                        }) {
                            self.outputs.insert(
                                (session.into(), n[0] as u64),
                                [n[1], n[2], n[3], n[4], n[5]],
                            );
                        } else {
                            self.invalid += 1;
                        }
                    }
                }
                let clock = t["clockSampleClientNs"]
                    .as_i64()
                    .zip(t["hostMinusClientNs"].as_i64())
                    .zip(t["clockUncertaintyNs"].as_i64());
                if let Some(rows) = t["rows"].as_array() {
                    for row in rows {
                        let Some(a) = row.as_array().filter(|a| a.len() == 12 || a.len() == 13)
                        else {
                            self.invalid += 1;
                            continue;
                        };
                        let Some(ns): Option<Vec<i64>> = a.iter().map(Value::as_i64).collect()
                        else {
                            self.invalid += 1;
                            continue;
                        };
                        if ns.iter().any(|n| *n < 0 || *n > i64::MAX / 4)
                            || ns[1] == 0
                            || ns[0] > u32::MAX as i64
                            || ns[11] > 5
                        {
                            self.invalid += 1;
                            continue;
                        }
                        // Dropped frames have partial timestamps. Only rendered rows require complete ordering.
                        if ns[11] == 0
                            && (ns[1] <= 0
                                || ns[2] < ns[1]
                                || ns[3] < ns[2]
                                || ns[4] < ns[3]
                                || ns[6] < ns[4]
                                || ns[7] < ns[6])
                        {
                            self.invalid += 1;
                            continue;
                        }
                        let estimate = clock
                            .filter(|((sample, _), uncertainty)| {
                                *uncertainty >= 0
                                    && *sample > 0
                                    && *sample < i64::MAX / 4
                                    && (ns[7] - sample).abs() <= 5_000_000_000
                            })
                            .filter(|((_, offset), u)| {
                                *offset > -i64::MAX / 4
                                    && *offset < i64::MAX / 4
                                    && *u <= 500_000_000
                            })
                            .map(|((_, offset), u)| (offset, u));
                        if self
                            .clients
                            .insert((session.into(), ns[0] as u64), (ns, estimate))
                            .is_some()
                        {
                            self.duplicate += 1;
                        }
                    }
                }
            }
            Some("host_diagnostics") => {
                self.host_drops = self.host_drops.max(v["trace_drops"].as_u64().unwrap_or(0));
                for (field, label) in [
                    ("audio_queue_ms_p95", "主机音频队列每秒 P95"),
                    ("audio_send_ms_p95", "主机音频发送阻塞每秒 P95"),
                ] {
                    if let Some(n) = v[field].as_f64().filter(|n| n.is_finite() && *n >= 0.) {
                        self.audio_diagnostics.entry(label).or_default().push(n);
                    }
                }
            }
            Some("client") => {
                self.arrival_drops = self
                    .arrival_drops
                    .max(v["metrics"]["arrivalTraceDrops"].as_u64().unwrap_or(0));
                if v["metrics"]["decodedOutputs"].is_number() {
                    let m = &v["metrics"];
                    self.client_counters = Some(format!(
                        "收到帧 {} / 提交 {} / 解码输出 {} / 请求呈现 {} / 获得呈现回调 {} / 等待 IDR 或队列丢弃 {} / 主动过晚丢弃 {}",
                        m["receivedFrames"],
                        m["submittedFrames"],
                        m["decodedOutputs"],
                        m["releasedForRender"],
                        m["rendered"],
                        m["queueDrops"],
                        m["videoLateDrops"]
                    ));
                }
                self.client_drops = self
                    .client_drops
                    .max(v["metrics"]["traceDrops"].as_u64().unwrap_or(0));
                self.telemetry_drops = self
                    .telemetry_drops
                    .max(v["metrics"]["telemetryPacketDrops"].as_u64().unwrap_or(0));
                if v["metrics"]["audioOutputBufferFrames"].is_number() {
                    self.audio_output = Some(format!(
                        "{} / mode {} / buffer {} frames ({:.1}ms) / burst {} / capacity {} / PCM queue limit {} / device xruns {} / source starvation {} / silence frames {} / clock drops {} / resets {} / error {} / sync scheduled {} / sync clock misses {} / stale network PCM packets {}",
                        v["metrics"]["audioBackend"],
                        v["metrics"]["audioOutputMode"],
                        v["metrics"]["audioOutputBufferFrames"],
                        v["metrics"]["audioOutputBufferFrames"]
                            .as_f64()
                            .unwrap_or(0.)
                            / 48.,
                        v["metrics"]["audioOutputBurstFrames"],
                        v["metrics"]["audioOutputCapacityFrames"],
                        v["metrics"]["audioQueueLimitFrames"],
                        v["metrics"]["audioUnderruns"],
                        v["metrics"]["audioSourceUnderruns"],
                        v["metrics"]["audioSilenceFrames"],
                        v["metrics"]["audioClockDrops"],
                        v["metrics"]["audioResets"],
                        v["metrics"]["audioError"],
                        v["metrics"]["videoSyncScheduledFrames"],
                        v["metrics"]["videoSyncClockMisses"],
                        v["metrics"]["audioNetworkDrops"]
                    ));
                }
            }
            _ => {}
        }
    }
}
fn percentile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.;
    }
    let mut sorted = v.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() as f64 * q).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)]
}
fn add(m: &mut BTreeMap<&'static str, Vec<f64>>, name: &'static str, ns: i64) {
    m.entry(name).or_default().push(ns as f64 / 1e6);
}
fn gaps(times: &mut [(String, i64)]) -> Vec<f64> {
    times.sort();
    times
        .windows(2)
        .filter(|w| w[0].0 == w[1].0 && w[1].1 > w[0].1)
        .map(|w| (w[1].1 - w[0].1) as f64 / 1e6)
        .collect()
}
fn long_gap_count(times: &[(String, i64)], fps: &HashMap<String, u32>) -> usize {
    times
        .windows(2)
        .filter(|w| {
            w[0].0 == w[1].0
                && w[1].1 > w[0].1
                && (w[1].1 - w[0].1) as f64 / 1e6
                    > 1500.0
                        / f64::from(
                            *fps.get(&w[0].0)
                                .unwrap_or(&sofalink_core::StreamRequest::default().video.fps),
                        )
        })
        .count()
}

pub fn report(path: &Path, out: &Path) -> Result<(), String> {
    let mut a = Analysis::default();
    for line in BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?).lines() {
        match line
            .map_err(|e| e.to_string())
            .and_then(|l| serde_json::from_str::<Value>(&l).map_err(|e| e.to_string()))
        {
            Ok(v) => a.ingest(&v),
            Err(_) => a.invalid += 1,
        }
    }
    if a.hosts.is_empty() || a.clients.is_empty() {
        return Err("日志缺少逐帧记录；采样时需要 --diagnostics".into());
    }
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut receives = fs::File::create(out.join("receive.csv")).map_err(|e| e.to_string())?;
    writeln!(receives, "session,rtp,first_rx_ns,assembled_ns").map_err(|e| e.to_string())?;
    for ((s, rtp), ns) in &a.arrivals {
        writeln!(receives, "{s},{rtp},{},{}", ns[0], ns[1]).map_err(|e| e.to_string())?;
    }
    let mut outputs = fs::File::create(out.join("decoder.csv")).map_err(|e| e.to_string())?;
    writeln!(
        outputs,
        "session,rtp,submitted_ns,decoded_ns,target_ns,release_ns,requested_render"
    )
    .map_err(|e| e.to_string())?;
    for ((s, rtp), ns) in &a.outputs {
        writeln!(
            outputs,
            "{s},{rtp},{},{},{},{},{}",
            ns[0], ns[1], ns[2], ns[3], ns[4]
        )
        .map_err(|e| e.to_string())?;
    }
    let mut csv = fs::File::create(out.join("frames.csv")).map_err(|e| e.to_string())?;
    writeln!(csv,"session,rtp,outcome,warmup,rx_ns,assembled_ns,submitted_ns,decoded_ns,target_ns,presented_ns,callback_ns,queue_depth,in_flight,audio_buffer_ms,capture_ready_ns,encoder_submit_ns,encoder_output_ns,rtp_handoff_ns,host_minus_client_ns,clock_uncertainty_ms,estimated_capture_to_surface_ms,surface_request_ns").map_err(|e|e.to_string())?;
    let mut starts: HashMap<String, i64> = HashMap::new();
    for ((session, _), (c, _)) in &a.clients {
        if c[1] > 0 {
            starts
                .entry(session.clone())
                .and_modify(|t| *t = (*t).min(c[1]))
                .or_insert(c[1]);
        }
    }
    let mut stats = BTreeMap::new();
    let mut presents = vec![];
    let mut arrivals = vec![];
    let mut matched = 0;
    let mut rendered = 0;
    let mut warm = 0;
    let mut outcomes = BTreeMap::new();
    let mut uncertainty = vec![];
    for (key, (c, clock)) in &a.clients {
        let warmup = c[1] < starts[&key.0] + 5_000_000_000;
        let h = a.hosts.get(key);
        let end_to_end = h
            .and_then(|h| h[1])
            .zip(*clock)
            .map(|(capture, (offset, _))| (c[6] + offset - capture) as f64 / 1e6)
            .filter(|_| c[11] == 0);
        let host_fields = (1..5)
            .map(|i| {
                h.and_then(|h| h[i])
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{},{},{},{},{}",
            key.0,
            key.1,
            c[11],
            warmup,
            c[1],
            c[2],
            c[3],
            c[4],
            c[5],
            c[6],
            c[7],
            c[8],
            c[9],
            c[10] as f64 / 1000.,
            host_fields,
            clock.map(|(o, _)| o.to_string()).unwrap_or_default(),
            clock
                .map(|(_, u)| format!("{:.3}", u as f64 / 1e6))
                .unwrap_or_default(),
            end_to_end.map(|n| format!("{n:.3}")).unwrap_or_default(),
            c.get(12).copied().unwrap_or(0)
        )
        .map_err(|e| e.to_string())?;
        if warmup {
            warm += 1;
            continue;
        }
        *outcomes.entry(c[11]).or_insert(0usize) += 1;
        arrivals.push((key.0.clone(), c[1]));
        if c[11] != 0 {
            continue;
        }
        rendered += 1;
        presents.push((key.0.clone(), c[6]));
        add(&mut stats, "客户端重组", c[2] - c[1]);
        add(&mut stats, "客户端排队", c[3] - c[2]);
        add(&mut stats, "解码输出等待", c[4] - c[3]);
        add(&mut stats, "输出到 Surface", c[6] - c[4]);
        add(&mut stats, "音画同步请求等待", (c[5] - c[4]).max(0));
        add(&mut stats, "Surface 超过目标", (c[6] - c[5]).max(0));
        add(
            &mut stats,
            "MediaCodec 通知与音频目标差（有符号）",
            c[6] - c[5],
        );
        if c.len() > 12 && c[12] > 0 {
            add(&mut stats, "MediaCodec 通知时间与请求差", c[6] - c[12]);
            add(&mut stats, "Surface 请求提前量", c[5] - c[12]);
        }
        add(&mut stats, "统计回调延迟", (c[7] - c[6]).max(0));
        add(&mut stats, "客户端首包到 Surface", c[6] - c[1]);
        stats
            .entry("音频缓冲")
            .or_insert_with(Vec::new)
            .push(c[10] as f64 / 1000.);
        if let Some(h) = h {
            matched += 1;
            add(
                &mut stats,
                "主机编码提交到输出",
                h[3].unwrap() - h[2].unwrap(),
            );
            add(
                &mut stats,
                "编码输出到 RTP 边界",
                h[4].unwrap() - h[3].unwrap(),
            );
            if let Some(cap) = h[1].filter(|cap| *cap > 0 && *cap <= h[2].unwrap()) {
                add(&mut stats, "采集输出到编码提交", h[2].unwrap() - cap);
            }
            if let Some((offset, u)) = clock {
                uncertainty.push(*u as f64 / 1e6);
                add(
                    &mut stats,
                    "RTP 边界到客户端首包（估计）",
                    c[1] + offset - h[4].unwrap(),
                );
                if let Some(e) = end_to_end {
                    stats
                        .entry("采集输出到 Surface（估计）")
                        .or_insert_with(Vec::new)
                        .push(e);
                }
            }
        }
    }
    let render_gaps = gaps(&mut presents);
    let receive_gaps = gaps(&mut arrivals);
    let long = long_gap_count(&presents, &a.target_fps);
    stats.insert("Surface 呈现帧间隔", render_gaps.clone());
    stats.insert("结局样本接收间隔（有选择偏差）", receive_gaps);
    if !a.arrivals.is_empty() {
        let mut arrival_starts: HashMap<&str, i64> = HashMap::new();
        for ((session, _), ns) in &a.arrivals {
            arrival_starts
                .entry(session)
                .and_modify(|t| *t = (*t).min(ns[0]))
                .or_insert(ns[0]);
        }
        let mut all_arrivals: Vec<_> = a
            .arrivals
            .iter()
            .filter(|((s, _), ns)| ns[0] >= arrival_starts[s.as_str()] + 5_000_000_000)
            .map(|((s, _), ns)| (s.clone(), ns[0]))
            .collect();
        stats.insert("客户端全部收到帧间隔", gaps(&mut all_arrivals));
    }
    if !a.outputs.is_empty() {
        let mut output_starts: HashMap<&str, i64> = HashMap::new();
        for ((s, _), ns) in &a.outputs {
            output_starts
                .entry(s)
                .and_modify(|t| *t = (*t).min(ns[1]))
                .or_insert(ns[1]);
        }
        let mut output_times = vec![];
        for ((s, _), ns) in &a.outputs {
            if ns[1] < output_starts[s.as_str()] + 5_000_000_000 {
                continue;
            }
            output_times.push((s.clone(), ns[1]));
            add(&mut stats, "全部解码输出等待", ns[1] - ns[0]);
        }
        stats.insert("全部解码输出间隔", gaps(&mut output_times));
    }
    let mut host_times: Vec<_> = a
        .hosts
        .iter()
        .filter_map(|((session, _), h)| h[4].map(|n| (session.clone(), n)))
        .collect();
    stats.insert("主机 RTP 输出帧间隔（含预热）", gaps(&mut host_times));
    for (name, v) in &a.audio_diagnostics {
        stats.insert(name, v.clone());
    }
    let mut md = format!(
        "# 逐帧性能诊断\n\n日志：`{}`\n\n排除每个会话前 5 秒预热（{} 行）。稳态呈现 {} 帧；与主机匹配 {} 帧；呈现间隔超过目标帧周期的 1.5 倍：{}/{}（{:.2}%）。缺少配置的历史会话按默认目标估算。\n\n| 测量边界 | 样本 | P50 ms | P95 ms | P99 ms | 最大 ms |\n|---|---:|---:|---:|---:|---:|\n",
        path.display(),
        warm,
        rendered,
        matched,
        long,
        render_gaps.len(),
        100. * long as f64 / render_gaps.len().max(1) as f64
    );
    for (name, v) in &stats {
        if !v.is_empty() {
            md += &format!(
                "| {} | {} | {:.3} | {:.3} | {:.3} | {:.3} |\n",
                name,
                v.len(),
                percentile(v, 0.5),
                percentile(v, 0.95),
                percentile(v, 0.99),
                percentile(v, 1.)
            );
        }
    }
    md += &format!(
        "\n根据观察到的 Surface 时间间隔计算的稳态呈现速率：{:.2} fps（窗口总时长 {:.2}s）；仅统计获得有效回调的帧。\n",
        1000. * render_gaps.len() as f64 / render_gaps.iter().sum::<f64>().max(1.),
        render_gaps.iter().sum::<f64>() / 1000.
    );
    md += &format!(
        "\n数据质量：主机记录 {} 帧、客户端记录 {} 帧；非法行 {}、重复帧 {}；主机 trace 丢弃 {}、客户端 trace 丢弃 {}。客户端结局计数：{:?}（0 呈现 / 1 等待 IDR / 2 队列丢弃 / 3 过晚 / 4 flush / 5 in-flight 淘汰）。\n",
        a.hosts.len(),
        a.clients.len(),
        a.invalid,
        a.duplicate,
        a.host_drops,
        a.client_drops,
        outcomes
    );
    md += &format!(
        "\n跨设备时钟：四时间戳探测，取近 5 秒内最小 RTT；匹配帧时钟半 RTT 不确定度 P50 {:.3}ms / P95 {:.3}ms。偏移估计假设上下行近似对称，仍可能受不对称和时钟漂移影响。无新鲜探测的帧不进入跨时钟统计。\n",
        percentile(&uncertainty, 0.5),
        percentile(&uncertainty, 0.95)
    );
    md += &format!(
        "\n反馈包发送队列丢弃 {}；音频输出配置：{}。音频发送统计是每秒 P95 的分布，不能解释成所有音频包的全局 P95。负的 Surface 与目标差表示平台报告早于请求时间，需保留该证据，不能默认请求调度完全兑现。\n",
        a.telemetry_drops,
        a.audio_output.as_deref().unwrap_or("旧日志未记录")
    );
    md += &format!(
        "\n独立接收记录 {} 帧，接收 trace 队列丢弃 {}；最后一次客户端累计计数：{}。结局样本接收间隔存在选择偏差，只能用独立全部接收记录或客户端滚动 arrival 窗口判断接收节奏。\n",
        a.arrivals.len(),
        a.arrival_drops,
        a.client_counters.as_deref().unwrap_or("旧日志未记录")
    );
    if !a.outputs.is_empty() {
        let mut latest: HashMap<&str, i64> = HashMap::new();
        for ((s, _), (c, _)) in &a.clients {
            if c[11] == 0 {
                latest
                    .entry(s)
                    .and_modify(|n| *n = (*n).max(c[7]))
                    .or_insert(c[7]);
            }
        }
        let eligible: Vec<_> = a
            .outputs
            .iter()
            .filter(|((s, _), ns)| {
                ns[4] == 1
                    && latest
                        .get(s.as_str())
                        .is_some_and(|last| ns[1] < last - 2_000_000_000)
            })
            .collect();
        let missing = eligible
            .iter()
            .filter(|(key, _)| a.clients.get(*key).is_none_or(|(c, _)| c[11] != 0))
            .count();
        md += &format!(
            "\n独立解码输出记录 {} 帧。早于最后回调 2 秒的请求呈现输出 {} 帧，其中 {} 帧没有对应呈现记录；这把正常的末尾回调延迟与持续缺失分开，仍不等于物理屏幕丢帧测量。\n",
            a.outputs.len(),
            eligible.len(),
            missing
        );
    }
    md += "\n边界说明：Surface 指标保留 MediaCodec.OnFrameRendered 原始时间；厂商实现可能与 SurfaceFlinger 呈现围栏相差多个刷新周期，不能将该差值直接当成显示等待。独立核对使用 analyze-presentation.py，物理面板仍需光学测量。\n";
    md += "\n解码输出等待包含 MediaCodec 调度与背压；音画同步请求等待是输出到 Surface 的组成部分，不能相加。RTP 边界在 payloader 首包输出处，包含发送排队/传输/接收调度，不能当成纯网络延迟。采集输出到 Surface 不包含游戏内部、采集前等待或显示器物理显示延迟。Surface 使用 OnFrameRendered 提供的呈现时间，而非回调送达时间。不能把各阶段 P95 相加为端到端 P95。\n\nCSV 保留所有帧与丢弃结局，可按 RTP 时间戳逐帧关联。零 trace 丢弃不能证明 UDP 诊断包零丢失；主机/客户端匹配覆盖率必须一起看。完整 trace 的观察开销需与关闭 trace 的相同场景对照。\n";
    fs::write(out.join("report.md"), md).map_err(|e| e.to_string())?;
    println!("Report: {}", out.join("report.md").display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_frames_use_each_sessions_target() {
        let mut times = vec![
            ("a".into(), 0),
            ("a".into(), 33_333_333),
            ("b".into(), 0),
            ("b".into(), 33_333_333),
        ];
        let fps = HashMap::from([("a".into(), 30), ("b".into(), 60)]);
        assert_eq!(gaps(&mut times).len(), 2);
        assert_eq!(long_gap_count(&times, &fps), 1);
    }
    use super::*;
    #[test]
    fn nearest_rank_and_session_gaps() {
        assert_eq!(percentile(&[3., 1., 2.], 0.95), 3.);
        let mut t = vec![
            ("b".into(), 100),
            ("a".into(), 30_000_000),
            ("a".into(), 10_000_000),
        ];
        assert_eq!(gaps(&mut t), vec![20.]);
    }
    #[test]
    fn rejects_incomplete_rendered_and_foreign_session() {
        let mut a = Analysis::default();
        a.ingest(&serde_json::json!({"session_id":"a","type":"client_frames","trace":{"sessionId":"b","rows":[]}}));
        assert_eq!(a.invalid, 1);
        a.ingest(&serde_json::json!({"session_id":"a","type":"client_frames","trace":{"sessionId":"a","rows":[[1,10,11,0,0,0,0,0,0,0,0,0]]}}));
        assert_eq!(a.invalid, 2);
        assert!(a.clients.is_empty());
    }
    #[test]
    fn clock_is_fresh_and_not_raw_clock_subtraction() {
        let mut a = Analysis::default();
        a.ingest(&serde_json::json!({"session_id":"a","type":"client_frames","trace":{"sessionId":"a","clockSampleClientNs":100,"hostMinusClientNs":10000000,"clockUncertaintyNs":50,"rows":[[1,100,110,120,130,140,150,160,1,1,1000,0]]}}));
        let (c, clock) = a.clients.values().next().unwrap();
        assert_eq!(c[6] + clock.unwrap().0 - 10_000_100, 50);
    }
    #[test]
    fn arrivals_include_frames_without_presentation_callbacks() {
        let mut a = Analysis::default();
        a.ingest(&serde_json::json!({"session_id":"test","type":"client_frames","trace":{"sessionId":"test","rows":[],"arrivals":[[1,100,101],[2,200,201]]}}));
        assert_eq!(a.arrivals.len(), 2);
        assert!(a.clients.is_empty());
        assert_eq!(a.invalid, 0);
    }
    #[test]
    fn report_pairs_clocks_excludes_warmup_and_exports_csv() {
        let dir =
            std::env::temp_dir().join(format!("livingroom-report-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.jsonl");
        let out = dir.join("out");
        let mut log = String::new();
        for i in 0..8 {
            let c = 1_000_000_000i64 + i * 1_000_000_000;
            let host = c + 10_000_000;
            let h = serde_json::json!({"session_id":"test","type":"host_frames","rows":[[i,host,host+1_000_000,host+2_000_000,host+2_100_000]]});
            let row = serde_json::json!({"session_id":"test","type":"client_frames","trace":{"sessionId":"test","clockSampleClientNs":c,"hostMinusClientNs":10_000_000,"clockUncertaintyNs":100_000,"rows":[[i,c+2_200_000,c+2_300_000,c+3_000_000,c+4_000_000,c+5_000_000,c+6_000_000,c+26_000_000,0,1,1000,0]]}});
            log += &format!("{h}\n{row}\n");
        }
        fs::write(&source, log).unwrap();
        report(&source, &out).unwrap();
        let md = fs::read_to_string(out.join("report.md")).unwrap();
        assert!(md.contains("预热（5 行）"));
        assert!(md.contains("| 采集输出到 Surface（估计） | 3 | 6.000 |"));
        assert!(md.contains("| 统计回调延迟 | 3 | 20.000 |"));
        assert_eq!(
            fs::read_to_string(out.join("frames.csv"))
                .unwrap()
                .lines()
                .count(),
            9
        );
        for path in [
            source,
            out.join("report.md"),
            out.join("frames.csv"),
            out.join("receive.csv"),
            out.join("decoder.csv"),
        ] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(out).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
