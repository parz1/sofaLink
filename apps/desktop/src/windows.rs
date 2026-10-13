//! Windows-only adapter and native view. All focus policy lives in FocusCore.
#![allow(unsafe_code)]
use crate::space_backend::{Config as SpaceConfig, SpaceBackend};
use sofalink_core::*;
use sofalink_input::focus_adapter::{
    activate, activate_in_space, alive, foreground, game_window, identity, in_space, move_to_space,
    work_candidate,
};
use std::{
    cell::RefCell,
    fs::OpenOptions,
    io::Write,
    sync::atomic::{AtomicIsize, Ordering},
    time::Instant,
};

type H = isize;
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Msg {
    window: H,
    message: u32,
    w: usize,
    l: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct WndClass {
    style: u32,
    proc: Option<unsafe extern "system" fn(H, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: H,
    icon: H,
    cursor: H,
    background: H,
    menu: *const u16,
    name: *const u16,
}
#[repr(C)]
struct DrawItem {
    kind: u32,
    id: u32,
    item: u32,
    action: u32,
    state: u32,
    window: H,
    dc: H,
    rect: Rect,
    data: usize,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        text: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: H,
        menu: H,
        instance: H,
        param: *mut std::ffi::c_void,
    ) -> H;
    fn DefWindowProcW(window: H, message: u32, w: usize, l: isize) -> isize;
    fn DestroyWindow(window: H) -> i32;
    fn GetMessageW(msg: *mut Msg, window: H, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(window: H, message: u32, w: usize, l: isize) -> i32;
    fn ShowWindow(window: H, command: i32) -> i32;
    fn SetWindowTextW(window: H, text: *const u16) -> i32;
    fn SendMessageW(window: H, message: u32, w: usize, l: isize) -> isize;
    fn EnableWindow(window: H, enabled: i32) -> i32;
    fn SetTimer(
        window: H,
        id: usize,
        ms: u32,
        callback: Option<unsafe extern "system" fn(H, u32, usize, u32)>,
    ) -> usize;
    fn KillTimer(window: H, id: usize) -> i32;
    fn GetWindowTextW(window: H, text: *mut u16, size: i32) -> i32;
    fn RegisterHotKey(window: H, id: i32, modifiers: u32, key: u32) -> i32;
    fn UnregisterHotKey(window: H, id: i32) -> i32;
    fn SetWinEventHook(
        min: u32,
        max: u32,
        module: H,
        callback: Option<unsafe extern "system" fn(H, u32, H, i32, i32, u32, u32)>,
        pid: u32,
        tid: u32,
        flags: u32,
    ) -> H;
    fn UnhookWinEvent(hook: H) -> i32;
    fn FillRect(dc: H, rect: *const Rect, brush: H) -> i32;
    fn DrawTextW(dc: H, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
    fn DrawFocusRect(dc: H, rect: *const Rect) -> i32;
    fn InvalidateRect(window: H, rect: *const Rect, erase: i32) -> i32;
    fn MessageBoxW(window: H, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> H;
    fn CreateMutexW(attributes: *const std::ffi::c_void, owner: i32, name: *const u16) -> H;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: H) -> i32;
}
struct ManagerLock(H);
impl Drop for ManagerLock {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn manager_lock() -> Result<ManagerLock, String> {
    let handle = unsafe {
        CreateMutexW(
            std::ptr::null(),
            0,
            wide("Global\\sofaLink-DisplayManager").as_ptr(),
        )
    };
    if handle == 0 {
        return Err("无法取得虚拟屏幕管理锁，可能有其他会话正在使用".into());
    }
    if unsafe { GetLastError() } == 183 {
        unsafe {
            CloseHandle(handle);
        }
        return Err("sofaLink 桌面管理已在运行".into());
    }
    Ok(ManagerLock(handle))
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> H;
    fn DeleteObject(object: H) -> i32;
    fn SetTextColor(dc: H, color: u32) -> u32;
    fn SetBkMode(dc: H, mode: i32) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escape: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        name: *const u16,
    ) -> H;
    fn SelectObject(dc: H, object: H) -> H;
}

const OBSERVE: u32 = 0x8001;
const BG: u32 = 0x00221b14;
const FG: u32 = 0x00f4f7ed;
const MINT: u32 = 0x00cfef9a;
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static BRUSH: AtomicIsize = AtomicIsize::new(0);
thread_local! {static APP:RefCell<Option<App>>=const {RefCell::new(None)};}

struct App {
    manager: Option<Manager>,
    session_status: H,
    session_button: H,
    settings_button: H,
    uninstall_button: H,
    settings_info: H,
    settings: bool,
    shown_space_revision: u64,
    controls_revision: u64,
    shown_settings: bool,
    monitor: Option<u64>,
    space: H,
    move_game: H,
    core: FocusCore,
    start: Instant,
    sequence: u64,
    shown_revision: u64,
    status: H,
    detail: H,
    issue: H,
    take: H,
    game: H,
    font: H,
    evidence: std::fs::File,
    shortcuts: bool,
}
struct Manager {
    core: GameSpaceCore,
    backend: SpaceBackend,
    closing: bool,
    work_before_start: Option<WindowRef>,
    session_request: Option<u64>,
}
fn space_command(command: SpaceCommand) {
    APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let app = slot.as_mut().unwrap();
        let work = work_candidate(foreground(), app.core.snapshot().game);
        if let Some(manager) = app.manager.as_mut() {
            if command == SpaceCommand::Start {
                manager.work_before_start = work;
            }
            if let Ok(Some(effect)) = manager.core.command(command) {
                manager.backend.execute(effect);
            }
        }
    });
    render();
}
fn pump_space() {
    let close = APP.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        let Some(app) = slot.as_mut() else {
            return false;
        };
        let Some(manager) = app.manager.as_mut() else {
            return false;
        };
        for event in manager.backend.poll() {
            if manager.core.snapshot().pending != Some(event.effect) {
                continue;
            }
            if event.effect.operation == SpaceOperation::StartMedia
                && event.error.is_none()
                && let Some(binding) = event.binding
            {
                app.monitor = Some(binding.monitor);
                manager.session_request = Some(event.effect.request);
                app.core
                    .attach_with_work(binding.game, manager.work_before_start);
            }
            if let Some(effect) = manager
                .core
                .complete(event.effect, event.resources, event.error)
            {
                manager.backend.execute(effect);
            }
        }
        for request in manager.backend.lost_sessions() {
            if manager.session_request == Some(request)
                && let Some(effect) = manager
                    .core
                    .lost("电视音视频或媒体进程已中断，正在恢复并回收".into())
            {
                manager.backend.execute(effect);
            }
        }
        if manager.core.snapshot().phase == SpacePhase::Streaming
            && app.core.snapshot().game.is_none()
            && let Some(effect) = manager.core.lost("游戏窗口已失效，正在恢复并回收".into())
        {
            manager.backend.execute(effect);
        }
        if manager.closing && manager.core.snapshot().pending.is_none() {
            if manager.core.snapshot().resources.clear() {
                return true;
            }
            manager.closing = false;
        }
        false
    });
    if close {
        unsafe {
            DestroyWindow(WINDOW.load(Ordering::Relaxed));
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn text(window: H, s: &str) {
    unsafe {
        SetWindowTextW(window, wide(s).as_ptr());
    }
}

fn observe() {
    pump_space();
    APP.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return;
        };
        let Some(app) = slot.as_mut() else {
            return;
        };
        let foreground = foreground();
        let game = app.core.snapshot().game;
        let work = app.core.snapshot().work;
        app.sequence += 1;
        let now = app.start.elapsed().as_millis() as u64;
        app.core.observe(
            FocusObservation {
                sequence: app.sequence,
                foreground: identity(foreground),
                game_alive: game.is_some_and(alive),
                game_in_space: app
                    .monitor
                    .is_none_or(|monitor| game.is_some_and(|game| in_space(game, monitor))),
                work_alive: work.is_some_and(alive),
                work_candidate: work_candidate(foreground, game),
            },
            now,
        );
    });
    render();
}
fn command(command: FocusCommand) {
    observe();
    let allowed = APP.with(|slot| {
        let slot = slot.borrow();
        let app = slot.as_ref().unwrap();
        app.manager.as_ref().is_none_or(|m| {
            m.core
                .allowed_focus_commands(app.core.snapshot())
                .contains(&command)
        })
    });
    if !allowed {
        return;
    }
    let effect = APP.with(|slot| {
        slot.borrow_mut().as_mut().and_then(|app| {
            app.core
                .command(command, app.start.elapsed().as_millis() as u64)
                .ok()
        })
    });
    if let Some(effect) = effect {
        let monitor = APP.with(|slot| slot.borrow().as_ref().and_then(|app| app.monitor));
        let result = match (command, monitor) {
            (FocusCommand::ReturnGame, Some(monitor)) => activate_in_space(effect.target, monitor),
            (FocusCommand::MoveGameToSpace, Some(monitor)) => move_to_space(effect.target, monitor),
            (FocusCommand::MoveGameToSpace, None) => Err("未配置串流屏幕".into()),
            _ => activate(effect.target),
        };
        if let Err(error) = result {
            APP.with(|slot| {
                if let Some(app) = slot.borrow_mut().as_mut() {
                    app.core.failed(effect.request, error);
                }
            });
        }
        // A successful API return is only a request; core waits for observed ownership.
        observe();
    }
}
fn render() {
    render_space();
    let view = APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let app = slot.as_mut()?;
        if app.status == 0 {
            return None;
        }
        let state = app.core.snapshot().clone();
        let space_revision=app.manager.as_ref().map_or(0,|m|m.core.snapshot().revision);
        if app.shown_revision == state.revision && app.shown_space_revision==space_revision {
            return None;
        }
        app.shown_revision = state.revision;
        app.shown_space_revision=space_revision;
        let _ = writeln!(
            app.evidence,
            "{}",
            serde_json::json!({"host_uptime_ms":app.start.elapsed().as_millis(),"focus":state,"space":app.manager.as_ref().map(|m|m.core.snapshot())})
        );
        let _ = app.evidence.flush();
        let managed_focus=app.manager.as_ref().map(|m|(m.core.focus_message(&state),m.core.allowed_focus_commands(&state)));
        Some((
            state,
            app.status,
            app.detail,
            app.issue,
            app.take,
            app.game,
            app.shortcuts,
            app.space,
            app.move_game,
            app.monitor.is_some(),
            managed_focus,
        ))
    });
    if let Some((
        state,
        status,
        detail,
        issue,
        take,
        game,
        shortcuts,
        space,
        move_game,
        has_space,
        managed_focus,
    )) = view
    {
        let (message, commands) =
            managed_focus.unwrap_or((state.message.clone(), state.available_commands.clone()));
        text(status, &message);
        text(
            space,
            if !has_space {
                "游戏位置：当前屏幕"
            } else if state.game.is_none() {
                "游戏位置：窗口已退出"
            } else if state.game_in_space {
                "游戏位置：电视专用屏幕"
            } else {
                "游戏位置：其他屏幕 · 电视未捕获游戏"
            },
        );
        let work_title = state.work.map(|w| {
            let mut buf = [0u16; 128];
            let len = unsafe { GetWindowTextW(w.handle as H, buf.as_mut_ptr(), 128) };
            String::from_utf16_lossy(&buf[..len.max(0) as usize])
        });
        text(
            detail,
            &format!(
                "工作窗口：{}",
                work_title
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "先点击一个工作应用".into())
            ),
        );
        text(
            issue,
            state.issue.as_deref().unwrap_or(if shortcuts {
                "Ctrl+Alt+F9 拿回  ·  Ctrl+Alt+F10 返回"
            } else {
                "快捷键被占用，请使用按钮"
            }),
        );
        unsafe {
            EnableWindow(
                take,
                i32::from(commands.contains(&FocusCommand::TakeComputer)),
            );
            EnableWindow(
                game,
                i32::from(commands.contains(&FocusCommand::ReturnGame)),
            );
            EnableWindow(
                move_game,
                i32::from(commands.contains(&FocusCommand::MoveGameToSpace)),
            );
            InvalidateRect(take, std::ptr::null(), 0);
            InvalidateRect(game, std::ptr::null(), 0);
            InvalidateRect(move_game, std::ptr::null(), 0);
        }
    }
}
fn render_space() {
    let view = APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let app = slot.as_mut()?;
        let m = app.manager.as_ref()?;
        if app.session_status == 0
            || (app.controls_revision == m.core.snapshot().revision
                && app.shown_settings == app.settings)
        {
            return None;
        }
        app.controls_revision = m.core.snapshot().revision;
        app.shown_settings = app.settings;
        Some((
            m.core.snapshot().clone(),
            app.session_status,
            app.session_button,
            app.settings_button,
            app.uninstall_button,
            app.settings_info,
            app.settings,
        ))
    });
    if let Some((state, status, button, settings, uninstall, info, open)) = view {
        text(status, state.issue.as_deref().unwrap_or(&state.message));
        text(
            button,
            if state.available_commands.contains(&SpaceCommand::Start) {
                "开始串流"
            } else if state.available_commands.contains(&SpaceCommand::Recover) {
                "恢复并回收"
            } else if state.available_commands.contains(&SpaceCommand::Stop) {
                "结束串流"
            } else {
                "正在处理…"
            },
        );
        text(settings, if open { "收起设置" } else { "设置" });
        unsafe {
            EnableWindow(
                button,
                i32::from(
                    state
                        .available_commands
                        .iter()
                        .any(|c| *c != SpaceCommand::Uninstall),
                ),
            );
            EnableWindow(
                uninstall,
                i32::from(state.available_commands.contains(&SpaceCommand::Uninstall)),
            );
            ShowWindow(uninstall, if open { 4 } else { 0 });
            ShowWindow(info, if open { 4 } else { 0 });
        }
    }
}
unsafe extern "system" fn focus_event(_: H, _: u32, _: H, _: i32, _: i32, _: u32, _: u32) {
    // Out-of-context observer posts only; it never injects or switches focus.
    unsafe {
        PostMessageW(WINDOW.load(Ordering::Relaxed), OBSERVE, 0, 0);
    }
}
unsafe extern "system" fn proc(window: H, message: u32, w: usize, l: isize) -> isize {
    match message {
        OBSERVE | 0x0113 => {
            observe();
            0
        }
        0x0111 => {
            match w & 0xffff {
                1001 => command(FocusCommand::TakeComputer),
                1002 => command(FocusCommand::ReturnGame),
                1003 => command(FocusCommand::MoveGameToSpace),
                1004 => {
                    let command = APP.with(|slot| {
                        slot.borrow()
                            .as_ref()
                            .and_then(|app| app.manager.as_ref())
                            .and_then(|m| {
                                m.core
                                    .snapshot()
                                    .available_commands
                                    .iter()
                                    .copied()
                                    .find(|c| *c != SpaceCommand::Uninstall)
                            })
                    });
                    if let Some(command) = command {
                        space_command(command);
                    }
                }
                1005 => {
                    APP.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let app = slot.as_mut().unwrap();
                        app.settings = !app.settings;
                    });
                    render_space();
                }
                1006 => space_command(SpaceCommand::Uninstall),
                _ => {}
            };
            0
        }
        0x0312 => {
            match w {
                1 => command(FocusCommand::TakeComputer),
                2 => command(FocusCommand::ReturnGame),
                _ => {}
            };
            0
        }
        0x0138 => {
            unsafe {
                SetTextColor(w as H, FG);
                SetBkMode(w as H, 1);
            }
            BRUSH.load(Ordering::Relaxed)
        }
        0x002b => {
            // SAFETY: WM_DRAWITEM lParam is a live DRAWITEMSTRUCT for this synchronous callback.
            let item = unsafe { &*(l as *const DrawItem) };
            let disabled = item.state & 4 != 0;
            unsafe {
                let brush = CreateSolidBrush(if disabled { 0x003f352b } else { MINT });
                FillRect(item.dc, &item.rect, brush);
                DeleteObject(brush);
                SetBkMode(item.dc, 1);
                SetTextColor(item.dc, if disabled { 0x00978c80 } else { BG });
                APP.with(|slot| {
                    if let Ok(slot) = slot.try_borrow()
                        && let Some(app) = slot.as_ref()
                    {
                        SelectObject(item.dc, app.font);
                    }
                });
                let mut label_buffer = [0u16; 128];
                let length = GetWindowTextW(item.window, label_buffer.as_mut_ptr(), 128);
                let label = &label_buffer[..length.max(0) as usize];
                let mut rect = item.rect;
                DrawTextW(item.dc, label.as_ptr(), length, &mut rect, 0x25);
                if item.state & 0x10 != 0 {
                    rect.left += 4;
                    rect.right -= 4;
                    rect.top += 4;
                    rect.bottom -= 4;
                    DrawFocusRect(item.dc, &rect);
                }
            }
            1
        }
        0x0010 => {
            let wait = APP.with(|slot| {
                let mut slot = slot.borrow_mut();
                let app = slot.as_mut().unwrap();
                if let Some(m) = app.manager.as_mut() {
                    m.closing = true;
                    if let Some(effect) = m.core.shutdown() {
                        m.backend.execute(effect);
                    }
                    return !m.core.snapshot().resources.clear()
                        || m.core.snapshot().pending.is_some();
                }
                false
            });
            if !wait {
                unsafe {
                    DestroyWindow(window);
                }
            } else {
                render();
            }
            0
        }
        0x0002 => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, w, l) },
    }
}
pub fn show_error(error: &str) {
    unsafe {
        MessageBoxW(0, wide(error).as_ptr(), wide("sofaLink").as_ptr(), 0x10);
    }
}

pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let monitor = args
        .windows(2)
        .find(|a| a[0] == "--monitor-handle")
        .map(|a| a[1].parse::<u64>())
        .transpose()
        .map_err(|e| e.to_string())?;
    let value = args
        .windows(2)
        .find(|a| a[0] == "--game-pid")
        .map(|a| a[1].parse::<u32>())
        .transpose()
        .map_err(|e| e.to_string())?;
    let root = args
        .windows(2)
        .find(|a| a[0] == "--root")
        .map(|a| std::path::PathBuf::from(&a[1]))
        .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
    let duration = args
        .windows(2)
        .find(|a| a[0] == "--run-seconds")
        .map(|a| a[1].parse::<u64>())
        .transpose()
        .map_err(|e| e.to_string())?;
    let managed = args.iter().any(|a| a == "--manage-space");
    let _lock = if managed { Some(manager_lock()?) } else { None };
    let game = value.and_then(game_window);
    if !managed && game.is_none() {
        return Err("Use --game-pid with a running game".into());
    }
    let evidence_dir = root.join("local/game-space");
    std::fs::create_dir_all(&evidence_dir).map_err(|e| e.to_string())?;
    let evidence = OpenOptions::new()
        .create(true)
        .append(true)
        .open(evidence_dir.join(format!("focus-desktop-{}.jsonl", std::process::id())))
        .map_err(|e| e.to_string())?;
    let mut core = FocusCore::default();
    if let Some(game) = game {
        core.attach(game);
    }
    let manager = if managed {
        let argument = |name: &str| {
            args.windows(2)
                .find(|a| a[0] == name)
                .map(|a| a[1].clone())
                .ok_or_else(|| format!("Missing {name}"))
        };
        let backend = SpaceBackend::new(SpaceConfig {
            root: root.clone(),
            shell: argument("--shell")?.into(),
            device: argument("--device")?,
            python: argument("--python")?.into(),
        });
        let mut state = GameSpaceCore::default();
        let effect = state.initialize();
        backend.execute(effect);
        Some(Manager {
            core: state,
            backend,
            closing: false,
            work_before_start: None,
            session_request: None,
        })
    } else {
        None
    };
    APP.with(|slot| {
        *slot.borrow_mut() = Some(App {
            manager,
            session_status: 0,
            session_button: 0,
            settings_button: 0,
            uninstall_button: 0,
            settings_info: 0,
            settings: false,
            shown_space_revision: 0,
            controls_revision: u64::MAX,
            shown_settings: false,
            monitor,
            space: 0,
            move_game: 0,
            core,
            start: Instant::now(),
            sequence: 0,
            shown_revision: 0,
            status: 0,
            detail: 0,
            issue: 0,
            take: 0,
            game: 0,
            font: 0,
            evidence,
            shortcuts: false,
        })
    });
    observe();
    // SAFETY: all Win32 structs use repr(C); UTF-16 buffers live through synchronous calls.
    unsafe {
        SetProcessDpiAwarenessContext(-2); // System aware: stable layout on current primary screen.
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("SofaLinkFocusOverlay");
        let brush = CreateSolidBrush(BG);
        BRUSH.store(brush, Ordering::Relaxed);
        let wc = WndClass {
            style: 0,
            proc: Some(proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: 0,
            cursor: 0,
            background: brush,
            menu: std::ptr::null(),
            name: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return Err("Cannot register desktop window".into());
        }
        // TOPMOST | TOOLWINDOW | NOACTIVATE: display changes never take working/game focus.
        let window = CreateWindowExW(
            0x08000088,
            class.as_ptr(),
            wide("sofaLink · 焦点控制").as_ptr(),
            0x80c80000,
            40,
            40,
            470,
            if managed { 560 } else { 330 },
            0,
            0,
            instance,
            std::ptr::null_mut(),
        );
        if window == 0 {
            return Err("Cannot create desktop overlay".into());
        }
        WINDOW.store(window, Ordering::Relaxed);
        let font = CreateFontW(
            -18,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        let child = |class: &str,
                     label: &str,
                     style: u32,
                     x: i32,
                     y: i32,
                     width: i32,
                     height: i32,
                     id: H| {
            let h = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(label).as_ptr(),
                0x50000000 | style,
                x,
                y,
                width,
                height,
                window,
                id,
                instance,
                std::ptr::null_mut(),
            );
            SendMessageW(h, 0x30, font as usize, 1);
            h
        };
        let status = child("STATIC", "", 0, 20, 18, 420, 50, 0);
        let detail = child("STATIC", "", 0, 20, 70, 420, 30, 0);
        let space = child("STATIC", "", 0, 20, 100, 420, 28, 0);
        let take = child("BUTTON", "拿回电脑", 0xb, 20, 138, 200, 44, 1001);
        let game_button = child("BUTTON", "返回游戏", 0xb, 234, 138, 200, 44, 1002);
        let move_game = child("BUTTON", "移到电视屏幕", 0xb, 20, 194, 414, 40, 1003);
        if monitor.is_none() {
            ShowWindow(move_game, 0);
        }
        let issue = child("STATIC", "", 0, 20, 247, 420, 48, 0);
        if managed {
            let session_status = child("STATIC", "", 0, 20, 301, 420, 48, 0);
            let session_button = child("BUTTON", "开始串流", 0xb, 20, 359, 278, 44, 1004);
            let settings_button = child("BUTTON", "设置", 0xb, 310, 359, 124, 44, 1005);
            let settings_info = child(
                "STATIC",
                "正常结束会回收屏幕。彻底卸载会删除 Windows 驱动包，再次开始时需要重新安装。",
                0,
                20,
                414,
                420,
                52,
                0,
            );
            let uninstall_button = child(
                "BUTTON",
                "彻底卸载虚拟显示驱动",
                0xb,
                20,
                475,
                414,
                40,
                1006,
            );
            APP.with(|slot| {
                let mut slot = slot.borrow_mut();
                let app = slot.as_mut().unwrap();
                app.session_status = session_status;
                app.session_button = session_button;
                app.settings_button = settings_button;
                app.uninstall_button = uninstall_button;
                app.settings_info = settings_info;
            });
            ShowWindow(settings_info, 0);
            ShowWindow(uninstall_button, 0);
        }
        let shortcut_take = RegisterHotKey(window, 1, 0x4003, 0x78) != 0;
        let shortcut_game = RegisterHotKey(window, 2, 0x4003, 0x79) != 0;
        APP.with(|slot| {
            let mut slot = slot.borrow_mut();
            let app = slot.as_mut().unwrap();
            app.status = status;
            app.detail = detail;
            app.issue = issue;
            app.take = take;
            app.game = game_button;
            app.space = space;
            app.move_game = move_game;
            app.font = font;
            app.shortcuts = shortcut_take && shortcut_game;
            app.shown_revision = u64::MAX;
        });
        let hook = SetWinEventHook(3, 3, 0, Some(focus_event), 0, 0, 0);
        if hook == 0 {
            return Err("Foreground event listener unavailable".into());
        }
        // 100 ms timer verifies liveness and pending deadlines; foreground changes are event-driven.
        SetTimer(window, 1, 100, None);
        // Consume STARTUPINFO's visibility override before showing without activation.
        ShowWindow(window, 0);
        ShowWindow(window, 4);
        observe();
        let mut msg = Msg {
            window: 0,
            message: 0,
            w: 0,
            l: 0,
            time: 0,
            point: Point { x: 0, y: 0 },
            private: 0,
        };
        loop {
            let result = GetMessageW(&mut msg, 0, 0, 0);
            if result <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if duration.is_some_and(|seconds| {
                APP.with(|slot| {
                    slot.borrow().as_ref().unwrap().start.elapsed().as_secs() >= seconds
                })
            }) {
                let closing = APP.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .and_then(|a| a.manager.as_ref())
                        .is_some_and(|m| m.closing)
                });
                if !closing {
                    PostMessageW(window, 0x0010, 0, 0);
                }
            }
        }
        UnhookWinEvent(hook);
        UnregisterHotKey(window, 1);
        UnregisterHotKey(window, 2);
        KillTimer(window, 1);
        DeleteObject(font);
        DeleteObject(brush);
    }
    Ok(())
}
