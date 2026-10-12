use sofalink_core::*;
fn result(request: &StreamRequest) -> MediaNegotiation {
    let caps = DeviceCapabilities {
        version: MEDIA_SCHEMA_VERSION,
        video: vec![VideoCapability {
            implementation: "adapter-codec".into(),
            codec: VideoCodec::Avc,
            mode: request.video,
            high: true,
            baseline: true,
            hardware: true,
            on_demand_idr: true,
            color: request.color,
            max_bitrate_kbit: 100000,
        }],
        audio: vec![AudioCapability {
            format: request.audio,
            low_latency_output: Some(true),
        }],
        display: Some(DisplayMode {
            width: 3840,
            height: 2160,
            refresh_hz: 59.94,
        }),
    };
    let plan = negotiate_media(request, &caps, &caps).unwrap();
    MediaNegotiation {
        host: caps.clone(),
        client: caps,
        plan,
    }
}
#[test]
fn only_current_valid_negotiation_can_start_host_and_replay_is_ignored() {
    let request = StreamRequest {
        video: VideoMode {
            width: 1280,
            height: 720,
            fps: 30,
        },
        ..Default::default()
    };
    let mut core = Core::default().with_stream_request(request.clone());
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    let token = core.snapshot().token;
    let negotiated = result(&request);
    core.event(
        Event {
            token: Token {
                session: token.session + 1,
                ..token
            },
            kind: EventKind::Negotiated(negotiated.clone()),
        },
        1,
    );
    assert_eq!(core.snapshot().phase, Phase::Checking);
    assert!(core.snapshot().media_negotiation.is_none());
    let effects = core.event(
        Event {
            token,
            kind: EventKind::Negotiated(negotiated.clone()),
        },
        2,
    );
    assert_eq!(effects[0].kind, EffectKind::StartHost);
    assert_eq!(
        core.snapshot()
            .media_negotiation
            .as_ref()
            .unwrap()
            .plan
            .request,
        request
    );
    assert!(
        core.event(
            Event {
                token,
                kind: EventKind::Negotiated(negotiated)
            },
            3
        )
        .is_empty()
    );
    core.command(Command::Stop, 4).unwrap();
    core.event(
        Event {
            token: core.snapshot().token,
            kind: EventKind::CleanupResult {
                host_stopped: true,
                client_stopped: true,
            },
        },
        5,
    );
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        6,
    )
    .unwrap();
    assert!(core.snapshot().media_negotiation.is_none());
}
#[test]
fn altered_plan_is_rejected_before_acquiring_host() {
    let request = StreamRequest::default();
    let mut core = Core::default();
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    let mut negotiated = result(&request);
    negotiated.plan.request.video.fps = 30;
    core.event(
        Event {
            token: core.snapshot().token,
            kind: EventKind::Negotiated(negotiated),
        },
        1,
    );
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    assert_eq!(core.snapshot().resources.host, ResourceState::Stopped);
    assert!(core.snapshot().media_negotiation.is_none());
}
