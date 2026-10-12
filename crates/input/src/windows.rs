//! Audited Win32 boundary. No driver, hooks, or privileged service.
#![allow(unsafe_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Mouse {
    x: i32,
    y: i32,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Key {
    vk: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
union Payload {
    mouse: Mouse,
    key: Key,
}
#[repr(C)]
struct Input {
    kind: u32,
    payload: Payload,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    fn MapVirtualKeyW(code: u32, kind: u32) -> u32;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetForegroundWindow() -> isize;
    fn SetForegroundWindow(window: isize) -> i32;
    fn IsWindow(window: isize) -> i32;
}
#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        handle: *mut std::ffi::c_void,
        buffer: *mut u8,
        size: u32,
        flags: u32,
    ) -> i32;
}
pub fn secret() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    // SAFETY: system-preferred RNG, live writable buffer with exact size.
    if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), 32, 2) } != 0 {
        return Err("Windows RNG failed".into());
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn send(input: Input) -> Result<(), String> {
    // SAFETY: INPUT has the documented C ABI; pointer remains valid during synchronous call.
    if unsafe { SendInput(1, &input, std::mem::size_of::<Input>() as i32) } != 1 {
        return Err("Windows 输入注入失败（请检查目标窗口权限）".into());
    }
    Ok(())
}
pub fn key(vk: u16, down: bool) -> Result<(), String> {
    // SAFETY: pure integer Win32 mapping; only validated virtual keys reach this function.
    let scan = unsafe { MapVirtualKeyW(vk as u32, 4) };
    if scan == 0 {
        return Err("Unmapped key".into());
    }
    let flags = 8 | if down { 0 } else { 2 } | if scan & 0xff00 == 0xe000 { 1 } else { 0 };
    send(Input {
        kind: 1,
        payload: Payload {
            key: Key {
                vk: 0,
                scan: (scan & 255) as u16,
                flags,
                time: 0,
                extra: 0,
            },
        },
    })
}
pub fn mouse(x: i32, y: i32, flags: u32, data: u32) -> Result<(), String> {
    send(Input {
        kind: 0,
        payload: Payload {
            mouse: Mouse {
                x,
                y,
                data,
                flags,
                time: 0,
                extra: 0,
            },
        },
    })
}
/// Diagnostic uses F24, which has no project/system shortcut assignment.
pub fn diagnostic_key_down() -> bool {
    // SAFETY: read-only system key-state query.
    unsafe { GetAsyncKeyState(135) as u16 & 0x8000 != 0 }
}
pub fn focus(target: Option<isize>) -> Result<(), String> {
    if let Some(window) = target {
        // SAFETY: handles are validated by the OS; no pointer dereference in Rust.
        unsafe {
            if IsWindow(window) == 0 {
                return Err("Steam 目标窗口已关闭".into());
            }
            if GetForegroundWindow() != window && SetForegroundWindow(window) == 0 {
                return Err("请先在电脑上聚焦 Steam 大屏幕窗口".into());
            }
        }
    }
    Ok(())
}
