//! Audited Windows focus adapter. Never injects input or continuously forces focus.
#![allow(unsafe_code)]
use sofalink_core::WindowRef;
type H = isize;
#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
#[derive(Default)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowRect(window: H, rect: *mut Rect) -> i32;
    fn MonitorFromWindow(window: H, flags: u32) -> H;
    fn GetMonitorInfoW(monitor: H, info: *mut MonitorInfo) -> i32;
    fn SetWindowPos(
        window: H,
        after: H,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn GetForegroundWindow() -> H;
    fn SetForegroundWindow(window: H) -> i32;
    fn GetWindowThreadProcessId(window: H, pid: *mut u32) -> u32;
    fn IsWindow(window: H) -> i32;
    fn IsWindowVisible(window: H) -> i32;
    fn IsIconic(window: H) -> i32;
    fn GetShellWindow() -> H;
    fn GetDesktopWindow() -> H;
    fn ShowWindow(window: H, command: i32) -> i32;
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(H, isize) -> i32>,
        param: isize,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
    fn GetProcessTimes(
        process: H,
        created: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn CloseHandle(handle: H) -> i32;
}
pub fn foreground() -> H {
    unsafe { GetForegroundWindow() }
}
pub fn in_space(window: WindowRef, monitor: u64) -> bool {
    if !alive(window) || unsafe { MonitorFromWindow(window.handle as H, 0) != monitor as H } {
        return false;
    }
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        ..Default::default()
    };
    let mut rect = Rect::default();
    if unsafe {
        GetMonitorInfoW(monitor as H, &mut info) == 0
            || GetWindowRect(window.handle as H, &mut rect) == 0
    } {
        return false;
    }
    rect.left >= info.monitor.left
        && rect.top >= info.monitor.top
        && rect.right <= info.monitor.right
        && rect.bottom <= info.monitor.bottom
}
pub fn move_to_space(window: WindowRef, monitor: u64) -> Result<(), String> {
    if !alive(window) {
        return Err("游戏窗口已退出".into());
    }
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor as H, &mut info) } == 0 {
        return Err("串流屏幕已断开，请结束实验或重新连接显示器".into());
    }
    let rect = info.monitor;
    // SWP_NOACTIVATE | SWP_NOZORDER: placement alone never hands over focus.
    if unsafe {
        SetWindowPos(
            window.handle as H,
            0,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            0x14,
        )
    } == 0
    {
        return Err("游戏窗口未能返回串流屏幕".into());
    }
    Ok(())
}
pub fn activate_in_space(window: WindowRef, monitor: u64) -> Result<(), String> {
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor as H, &mut info) } == 0 {
        return Err("串流屏幕已断开".into());
    }
    activate(window)?;
    if !in_space(window, monitor) {
        move_to_space(window, monitor)?;
    }
    Ok(())
}
/// Read-only identity, with a process creation timestamp guarding handle reuse.
pub fn identity(window: H) -> Option<WindowRef> {
    if window == 0 {
        return None;
    }
    unsafe {
        if IsWindow(window) == 0 {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        let process = OpenProcess(0x1000, 0, pid);
        if process == 0 {
            return None;
        }
        let mut created = FileTime::default();
        let mut exit = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        let ok = GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        Some(WindowRef {
            handle: window as u64,
            process_id: pid,
            process_created: (u64::from(created.high) << 32) | u64::from(created.low),
        })
    }
}
pub fn alive(window: WindowRef) -> bool {
    identity(window.handle as H) == Some(window)
}
pub fn work_candidate(window: H, game: Option<WindowRef>) -> Option<WindowRef> {
    unsafe {
        if window == GetDesktopWindow()
            || window == GetShellWindow()
            || IsWindowVisible(window) == 0
        {
            return None;
        }
    }
    let value = identity(window)?;
    if value.process_id == std::process::id()
        || game.is_some_and(|g| g.process_id == value.process_id)
    {
        None
    } else {
        Some(value)
    }
}
pub fn activate(target: WindowRef) -> Result<(), String> {
    if !alive(target) {
        return Err("目标窗口已退出，请重新选择工作窗口".into());
    }
    unsafe {
        if IsIconic(target.handle as H) != 0 {
            ShowWindow(target.handle as H, 9);
        }
        if SetForegroundWindow(target.handle as H) == 0 {
            return Err("Windows 拒绝切换，请用 Alt+Tab 选择目标窗口".into());
        }
    }
    Ok(())
}
unsafe extern "system" fn find_game(window: H, param: isize) -> i32 {
    // SAFETY: EnumWindows receives a live mutable tuple for the duration of the call.
    let search = unsafe { &mut *(param as *mut (u32, H)) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
        if pid == search.0 && IsWindowVisible(window) != 0 {
            search.1 = window;
            return 0;
        }
    }
    1
}
pub fn game_window(pid: u32) -> Option<WindowRef> {
    let mut search = (pid, 0);
    unsafe {
        EnumWindows(Some(find_game), (&mut search as *mut (u32, H)) as isize);
    }
    identity(search.1)
}
