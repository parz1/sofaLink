//! Read-only presentation: receives a core snapshot and local display options.
//! Never executes commands, polls a backend, or decides whether playback is healthy.
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use sofalink_core::*;
use std::collections::VecDeque;

pub struct ViewContext<'a> {
    pub real_device: bool,
    pub scenario_label: &'a str,
    pub scenario_description: &'a str,
    pub details: bool,
    pub quitting: bool,
    pub notice: Option<&'a str>,
    pub history: &'a VecDeque<Phase>,
}

pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "准备就绪",
        Phase::Checking => "检查设备",
        Phase::StartingHost => "准备主机",
        Phase::StartingClient => "启动客户端",
        Phase::WaitingMedia => "等待音画",
        Phase::Playing => "正在播放",
        Phase::Degraded => "播放异常",
        Phase::Recovering => "正在恢复",
        Phase::Stopping => "正在结束",
        Phase::Faulted => "需要处理",
    }
}

pub fn primary_label(snapshot: &Snapshot) -> &'static str {
    match snapshot.primary_command() {
        Some(Command::Start { .. }) => "开始",
        Some(Command::Retry) => "重试启动",
        Some(Command::Recover) if snapshot.phase == Phase::Faulted => "重试清理",
        Some(Command::Recover) => "恢复连接",
        Some(Command::Stop) if matches!(snapshot.phase, Phase::Playing | Phase::Degraded) => "结束",
        Some(Command::Stop) => "取消",
        None => "等待清理确认…",
    }
}

fn signal_label(signal: SignalState) -> &'static str {
    match signal {
        SignalState::Waiting => "等待确认",
        SignalState::Healthy => "正常",
        SignalState::Stalled => "停止推进",
        SignalState::Missing => "反馈中断",
        SignalState::Stopped => "未播放",
    }
}
fn resource_label(resource: ResourceState) -> &'static str {
    match resource {
        ResourceState::Stopped => "已释放",
        ResourceState::Starting => "准备中",
        ResourceState::Running => "运行中",
        ResourceState::Unknown => "尚未确认释放",
    }
}

pub fn render(frame: &mut Frame, snapshot: &Snapshot, context: &ViewContext<'_>) {
    let area = frame.area();
    let color = match snapshot.phase {
        Phase::Playing => Color::Green,
        Phase::Degraded | Phase::Faulted => Color::Yellow,
        Phase::Idle => Color::White,
        _ => Color::Cyan,
    };
    if area.width < 56 || area.height < 21 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(
                    if context.real_device {
                        "sofaLink · Android TV"
                    } else {
                        "sofaLink · 模拟环境"
                    }
                    .yellow(),
                ),
                Line::from(phase_label(snapshot.phase).fg(color).bold()),
                Line::from(snapshot.message.clone()),
                Line::from(format!(
                    "画面 {} / 声音 {}",
                    signal_label(snapshot.video),
                    signal_label(snapshot.audio)
                )),
                Line::from(if snapshot.host_audio.enabled {
                    snapshot.host_audio.message.as_str()
                } else {
                    "电脑声音保持原状"
                }),
                Line::from(format!(
                    "Enter {} · S 结束/取消 · Q 退出",
                    primary_label(snapshot)
                )),
                Line::from(if context.real_device {
                    "建议终端至少 56 列 × 21 行"
                } else {
                    "建议终端至少 56 列 × 21 行；1–7 选择场景"
                }),
            ])
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("sofaLink".bold().cyan()),
            Line::from(
                if context.real_device {
                    "Android TV 设备 · 实际音画反馈"
                } else {
                    "模拟环境 · 不连接电视 · 音画指标为合成测试数据"
                }
                .yellow(),
            ),
        ]),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(
                format!(
                    "{}  {}",
                    if context.real_device {
                        "设备"
                    } else {
                        "场景"
                    },
                    context.scenario_label
                )
                .bold(),
            ),
            Line::from(context.scenario_description),
        ])
        .wrap(Wrap { trim: false }),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(phase_label(snapshot.phase).fg(color).bold()),
            Line::from(snapshot.message.clone()),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 会话状态 "))
        .wrap(Wrap { trim: false }),
        sections[2],
    );
    let media = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(sections[3]);
    let video_detail = if snapshot.video == SignalState::Healthy {
        snapshot
            .metrics
            .as_ref()
            .map(|m| {
                format!(
                    "{:.1} fps · {}×{}",
                    m.video_fps,
                    snapshot.stream_request.video.width,
                    snapshot.stream_request.video.height
                )
            })
            .unwrap_or_default()
    } else {
        "以画面呈现进度确认".into()
    };
    let audio_detail = if snapshot.audio == SignalState::Healthy {
        format!(
            "{} Hz · {} 声道",
            snapshot.stream_request.audio.sample_rate, snapshot.stream_request.audio.channels
        )
    } else {
        "以声音播放进度确认".into()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(signal_label(snapshot.video).bold()),
            Line::from(video_detail),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 画面 ")),
        media[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(signal_label(snapshot.audio).bold()),
            Line::from(audio_detail),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 声音 ")),
        media[1],
    );
    let mut details = vec![];
    if snapshot.host_audio.enabled {
        details.push(Line::from(snapshot.host_audio.message.clone()).fg(
            if snapshot.host_audio.status == HostAudioStatus::Error {
                Color::Red
            } else {
                Color::Cyan
            },
        ));
    }
    details.push(Line::from(format!(
        "主机 {} · 客户端 {}",
        resource_label(snapshot.resources.host),
        resource_label(snapshot.resources.client)
    )));
    if context.real_device {
        details.push(Line::from(format!(
            "电脑控制 {} · 已注入 {} 次",
            match snapshot.input.status {
                InputStatus::Unavailable => "未启用",
                InputStatus::Waiting => "等待握手",
                InputStatus::Ready => "可用",
                InputStatus::Disconnected => "已断开",
                InputStatus::Error => "异常",
            },
            snapshot.input.applied_events
        )));
        details.push(Line::from(snapshot.performance.message.clone()));
    }
    if context.details {
        let request = &snapshot.stream_request;
        details.push(Line::from(format!(
            "目标 {}×{} / {} fps · {} kbit/s",
            request.video.width, request.video.height, request.video.fps, request.bitrate_kbit
        )));
        if let Some(result) = &snapshot.media_negotiation {
            details.push(Line::from(format!(
                "协商 AVC {} · {} Hz / {} 声道 · 保留显示模式",
                result.plan.profile,
                result.plan.request.audio.sample_rate,
                result.plan.request.audio.channels
            )));
        }
        if let Some(p) = &snapshot.performance.client
            && !snapshot.performance.stale
        {
            details.push(Line::from(format!(
                "P95 排队 {:.1} / 解码输出 {:.1} / 同步 {:.1} ms · 帧间隔 {:.1} ms",
                p.queue_p95_ms, p.decode_p95_ms, p.sync_wait_p95_ms, p.render_gap_p95_ms
            )));
        }
        if let Some(p) = &snapshot.performance.host
            && !snapshot.performance.host_stale
        {
            details.push(Line::from(format!(
                "主机 {:.1} fps · 编码 P95 {}",
                p.encoded_fps,
                p.encoder_p95_ms
                    .map(|n| format!("{n:.1} ms"))
                    .unwrap_or_else(|| "待测".into())
            )));
        }
        if context.real_device
            && matches!(
                snapshot.input.status,
                InputStatus::Error | InputStatus::Disconnected
            )
        {
            details.push(Line::from(snapshot.input.message.clone()));
        }
        if let Some(m) = &snapshot.metrics {
            let presentation = if m.receive_to_render_ms > 0.0 {
                format!("{:.1} ms", m.receive_to_render_ms)
            } else {
                "待测".into()
            };
            details.push(Line::from(format!(
                "{}呈现 {} · {}音频缓冲 {:.1} ms",
                if context.real_device {
                    "客户端"
                } else {
                    "模拟"
                },
                presentation,
                if context.real_device { "" } else { "模拟" },
                m.audio_buffer_ms
            )));
        }
        if area.height >= 28 {
            details.push(Line::from(format!(
                "会话 {} / 代次 {} · 状态修订 {}",
                snapshot.token.session, snapshot.token.generation, snapshot.revision
            )));
            details.push(Line::from(
                context
                    .history
                    .iter()
                    .map(|p| phase_label(*p))
                    .collect::<Vec<_>>()
                    .join(" → "),
            ));
        }
    } else {
        details.push(Line::from(if context.real_device {
            "Enter 执行当前操作；D 查看实际反馈与会话状态"
        } else if snapshot.phase == Phase::Idle {
            "1 正常  2 启动失败  3 超时  4 声音停止  5 画面停止  6 断线  7 结束失败"
        } else {
            "结束后按 1–7 切换测试场景；D 查看状态详情"
        }));
    }
    if let Some(notice) = &context.notice {
        details.push(Line::from(notice.to_string().yellow()));
    }
    frame.render_widget(
        Paragraph::new(details).wrap(Wrap { trim: false }),
        sections[4],
    );
    let primary = if context.quitting {
        "退出前正在清理…".into()
    } else {
        format!("Enter  {}", primary_label(snapshot))
    };
    let mut actions = vec![primary];
    if snapshot.can(Command::Stop) && snapshot.primary_command() != Some(Command::Stop) {
        actions.push("S  结束".into());
    }
    actions.push("D  详情".into());
    actions.push("Q  退出".into());
    frame.render_widget(
        Paragraph::new(actions.join("    "))
            .style(Style::default().fg(color))
            .block(Block::default().borders(Borders::TOP)),
        sections[5],
    );
}
