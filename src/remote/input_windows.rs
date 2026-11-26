#![cfg(target_os = "windows")]
#![allow(non_snake_case, non_camel_case_types)]

use std::mem::size_of;

type LONG = i32;
type DWORD = u32;
type ULONG_PTR = usize;
type UINT = u32;

#[repr(C)]
#[derive(Clone, Copy)]
struct MOUSEINPUT {
    dx: LONG,
    dy: LONG,
    mouseData: DWORD,
    dwFlags: DWORD,
    time: DWORD,
    dwExtraInfo: ULONG_PTR,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KEYBDINPUT {
    wVk: u16,
    wScan: u16,
    dwFlags: DWORD,
    time: DWORD,
    dwExtraInfo: ULONG_PTR,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HARDWAREINPUT {
    uMsg: DWORD,
    wParamL: u16,
    wParamH: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
union INPUTUNION {
    mi: MOUSEINPUT,
    ki: KEYBDINPUT,
    hi: HARDWAREINPUT,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct INPUT {
    type_: DWORD,
    u: INPUTUNION,
}

const INPUT_MOUSE: DWORD = 0;
const INPUT_KEYBOARD: DWORD = 1;

const MOUSEEVENTF_MOVE: DWORD = 0x0001;
const MOUSEEVENTF_ABSOLUTE: DWORD = 0x8000;
const MOUSEEVENTF_LEFTDOWN: DWORD = 0x0002;
const MOUSEEVENTF_LEFTUP: DWORD = 0x0004;
const MOUSEEVENTF_RIGHTDOWN: DWORD = 0x0008;
const MOUSEEVENTF_RIGHTUP: DWORD = 0x0010;
const MOUSEEVENTF_MIDDLEDOWN: DWORD = 0x0020;
const MOUSEEVENTF_MIDDLEUP: DWORD = 0x0040;
const MOUSEEVENTF_WHEEL: DWORD = 0x0800;

const KEYEVENTF_KEYUP: DWORD = 0x0002;
const KEYEVENTF_UNICODE: DWORD = 0x0004;
const WHEEL_DELTA: i32 = 120;

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

#[link(name = "user32")]
unsafe extern "system" {
    fn SendInput(cInputs: UINT, pInputs: *const INPUT, cbSize: i32) -> UINT;
    fn GetSystemMetrics(nIndex: i32) -> i32;
}

pub fn mouse_move_norm(x: f64, y: f64) {
    let (nx, ny) = norm_to_abs(x, y);
    let input = INPUT {
        type_: INPUT_MOUSE,
        u: INPUTUNION {
            mi: MOUSEINPUT {
                dx: nx,
                dy: ny,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &input, size_of::<INPUT>() as i32);
    }
}

pub fn mouse_button(x: f64, y: f64, button: u32, down: bool) {
    let (nx, ny) = norm_to_abs(x, y);
    let flag = match (button, down) {
        (0, true) => MOUSEEVENTF_LEFTDOWN,
        (0, false) => MOUSEEVENTF_LEFTUP,
        (2, true) => MOUSEEVENTF_RIGHTDOWN,
        (2, false) => MOUSEEVENTF_RIGHTUP,
        (1, true) => MOUSEEVENTF_MIDDLEDOWN,
        (1, false) => MOUSEEVENTF_MIDDLEUP,
        _ => return,
    };
    let inputs = [
        INPUT {
            type_: INPUT_MOUSE,
            u: INPUTUNION {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            type_: INPUT_MOUSE,
            u: INPUTUNION {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: flag,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(
            inputs.len() as UINT,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        );
    }
}

pub fn mouse_wheel(delta_y: i32) {
    // JS wheel deltaY: positive down; Windows wheel: positive up
    let steps = (-delta_y / 100).clamp(-10, 10);
    if steps == 0 {
        return;
    }
    let data = (steps * WHEEL_DELTA) as i32 as u32;
    let input = INPUT {
        type_: INPUT_MOUSE,
        u: INPUTUNION {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &input, size_of::<INPUT>() as i32);
    }
}

pub fn key_vk(vk: u32, down: bool) {
    let flags = if down { 0 } else { KEYEVENTF_KEYUP };
    let input = INPUT {
        type_: INPUT_KEYBOARD,
        u: INPUTUNION {
            ki: KEYBDINPUT {
                wVk: vk as u16,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &input, size_of::<INPUT>() as i32);
    }
}

pub fn key_text(text: &str) {
    let mut inputs: Vec<INPUT> = Vec::new();
    for ch in text.encode_utf16() {
        inputs.push(INPUT {
            type_: INPUT_KEYBOARD,
            u: INPUTUNION {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        inputs.push(INPUT {
            type_: INPUT_KEYBOARD,
            u: INPUTUNION {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    if inputs.is_empty() {
        return;
    }
    unsafe {
        let _ = SendInput(
            inputs.len() as UINT,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        );
    }
}

fn norm_to_abs(x: f64, y: f64) -> (LONG, LONG) {
    let x = x.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    let w = unsafe { GetSystemMetrics(SM_CXSCREEN).max(1) } as f64;
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN).max(1) } as f64;
    let nx = (x * 65535.0 / (w - 1.0).max(1.0) * (w - 1.0)).round() as i32;
    let ny = (y * 65535.0 / (h - 1.0).max(1.0) * (h - 1.0)).round() as i32;
    (nx, ny)
}
