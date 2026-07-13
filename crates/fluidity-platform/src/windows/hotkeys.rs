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
        let vk = kb.vkCode.0 as u16;

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
    Some(match vk {
        0x41..=0x5A => Key::KeyA + (vk - 0x41) as u8,
        0x30..=0x39 => Key::Key0 + (vk - 0x30) as u8,
        0x70..=0x7B => Key::F1 + (vk - 0x70) as u8,
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
