use std::sync::{Mutex, OnceLock};

use fluidity_core::hotkey::{Hotkey, HotkeyError, HotkeyEvent, HotkeyListener, Key, Modifiers};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static HK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

struct HookState {
    handler: Option<Box<dyn Fn(HotkeyEvent) + Send>>,
    hotkey: Option<Hotkey>,
}

pub struct WindowsHotkeyListener;

impl WindowsHotkeyListener {
    pub fn new() -> Self {
        HK_STATE.get_or_init(|| Mutex::new(HookState { handler: None, hotkey: None }));
        Self
    }
}

impl HotkeyListener for WindowsHotkeyListener {
    fn register(&mut self, shortcut: Hotkey) -> Result<(), HotkeyError> {
        HK_STATE.get().unwrap().lock().unwrap().hotkey = Some(shortcut);
        Ok(())
    }

    fn set_handler(&mut self, handler: Box<dyn Fn(HotkeyEvent) + Send>) {
        HK_STATE.get().unwrap().lock().unwrap().handler = Some(handler);
    }

    fn run(&self) -> Result<(), HotkeyError> {
        let hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), HINSTANCE::default(), 0)
        }
        .map_err(|e| HotkeyError::RegistrationFailed(format!("SetWindowsHookEx: {e}")))?;

        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        let _ = unsafe { UnhookWindowsHookEx(hook) };
        Ok(())
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode as u16;

        let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        let is_up = wparam.0 as u32 == WM_KEYUP || wparam.0 as u32 == WM_SYSKEYUP;

        if (is_down || is_up) && !is_modifier(vk) {
            if let Some(state) = HK_STATE.get() {
                if let Ok(s) = state.lock() {
                    if let Some(ref hotkey) = s.hotkey {
                        if key_matches(hotkey, vk) && modifiers_match(hotkey.modifiers) {
                            let event = if is_down {
                                HotkeyEvent::Down { shortcut: hotkey.clone() }
                            } else {
                                HotkeyEvent::Up { shortcut: hotkey.clone() }
                            };
                            if let Some(ref handler) = s.handler {
                                handler(event);
                            }
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0xA4 | 0xA5 | 0xA2 | 0xA3 | 0xA0 | 0xA1 | 0x5B | 0x5C)
}

fn key_matches(hotkey: &Hotkey, vk: u16) -> bool {
    vk_to_key(vk).map(|k| k == hotkey.key).unwrap_or(false)
}

fn modifiers_match(expected: Modifiers) -> bool {
    let alt = has_alt();
    let ctrl = has_ctrl();
    let shift = has_shift();
    let win = has_win();
    modifiers_from_bools(alt, ctrl, shift, win) == expected
}

fn has_alt() -> bool {
    (unsafe { GetAsyncKeyState(0xA4) } & 0x8000) != 0
}
fn has_ctrl() -> bool {
    (unsafe { GetAsyncKeyState(0xA2) } & 0x8000) != 0
}
fn has_shift() -> bool {
    (unsafe { GetAsyncKeyState(0xA0) } & 0x8000) != 0
}
fn has_win() -> bool {
    (unsafe { GetAsyncKeyState(0x5B) } & 0x8000) != 0
        || (unsafe { GetAsyncKeyState(0x5C) } & 0x8000) != 0
}

fn modifiers_from_bools(alt: bool, ctrl: bool, shift: bool, win: bool) -> Modifiers {
    match (win, alt, ctrl, shift) {
        (false, false, false, false) => Modifiers::None,
        (false, true, false, false) => Modifiers::Alt,
        (false, false, true, false) => Modifiers::Control,
        (false, false, false, true) => Modifiers::Shift,
        (false, true, true, false) => Modifiers::AltControl,
        (false, true, false, true) => Modifiers::AltShift,
        (false, false, true, true) => Modifiers::ControlShift,
        (false, true, true, true) => Modifiers::AltControlShift,
        (true, false, false, false) => Modifiers::Win,
        (true, true, false, false) => Modifiers::WinAlt,
        (true, false, true, false) => Modifiers::WinControl,
        (true, false, false, true) => Modifiers::WinShift,
        (true, true, true, false) => Modifiers::WinAltControl,
        (true, true, false, true) => Modifiers::WinAltShift,
        (true, false, true, true) => Modifiers::WinControlShift,
        (true, true, true, true) => Modifiers::WinAltControlShift,
    }
}

fn vk_to_key(vk: u16) -> Option<Key> {
    // Ranges A-Z, 0-9, F1-F12 — Key is an enum without Add impl, so map explicitly
    Some(match vk {
        0x41 => Key::KeyA,
        0x42 => Key::KeyB,
        0x43 => Key::KeyC,
        0x44 => Key::KeyD,
        0x45 => Key::KeyE,
        0x46 => Key::KeyF,
        0x47 => Key::KeyG,
        0x48 => Key::KeyH,
        0x49 => Key::KeyI,
        0x4A => Key::KeyJ,
        0x4B => Key::KeyK,
        0x4C => Key::KeyL,
        0x4D => Key::KeyM,
        0x4E => Key::KeyN,
        0x4F => Key::KeyO,
        0x50 => Key::KeyP,
        0x51 => Key::KeyQ,
        0x52 => Key::KeyR,
        0x53 => Key::KeyS,
        0x54 => Key::KeyT,
        0x55 => Key::KeyU,
        0x56 => Key::KeyV,
        0x57 => Key::KeyW,
        0x58 => Key::KeyX,
        0x59 => Key::KeyY,
        0x5A => Key::KeyZ,
        0x30 => Key::Key0,
        0x31 => Key::Key1,
        0x32 => Key::Key2,
        0x33 => Key::Key3,
        0x34 => Key::Key4,
        0x35 => Key::Key5,
        0x36 => Key::Key6,
        0x37 => Key::Key7,
        0x38 => Key::Key8,
        0x39 => Key::Key9,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0x20 => Key::Space,
        0x0D => Key::Enter,
        0x1B => Key::Escape,
        0x09 => Key::Tab,
        0x08 => Key::Backspace,
        0xA4 => Key::LeftAlt,
        0xA5 => Key::RightAlt,
        0xA2 => Key::LeftControl,
        0xA3 => Key::RightControl,
        0xA0 => Key::LeftShift,
        0xA1 => Key::RightShift,
        0x5B => Key::LeftWin,
        0x5C => Key::RightWin,
        0x14 => Key::CapsLock,
        0xBC => Key::Comma,
        0xBE => Key::Period,
        0xBF => Key::Slash,
        0xBA => Key::Semicolon,
        0xDE => Key::Quote,
        0xDB => Key::BracketLeft,
        0xDD => Key::BracketRight,
        0xDC => Key::Backslash,
        0xBD => Key::Minus,
        0xBB => Key::Equal,
        0xC0 => Key::Grave,
        0x01 => Key::MouseLeft,
        0x02 => Key::MouseRight,
        0x04 => Key::MouseMiddle,
        _ => return None,
    })
}
