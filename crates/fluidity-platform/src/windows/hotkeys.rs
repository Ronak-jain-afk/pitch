use std::sync::{Mutex, OnceLock};

use fluidity_core::{Hotkey, HotkeyError, HotkeyEvent, HotkeyListener, Key, Modifiers};
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
        let vk = kb.vkCode;

        let is_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        let is_up = wparam.0 as u32 == WM_KEYUP || wparam.0 as u32 == WM_SYSKEYUP;

        if (is_down || is_up) && vk != VK_MENU && vk != VK_CONTROL && !is_modifier(vk) {
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
    unsafe { CallNextHookEx(HHOOK::default(), code, wparam, lparam) }
}

fn is_modifier(vk: VIRTUAL_KEY) -> bool {
    vk == VK_LMENU || vk == VK_RMENU || vk == VK_LCONTROL || vk == VK_RCONTROL
        || vk == VK_LSHIFT || vk == VK_RSHIFT || vk == VK_LWIN || vk == VK_RWIN
}

fn key_matches(hotkey: &Hotkey, vk: VIRTUAL_KEY) -> bool {
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
    (unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } & 0x8000) != 0
}
fn has_ctrl() -> bool {
    (unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } & 0x8000) != 0
}
fn has_shift() -> bool {
    (unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } & 0x8000) != 0
}
fn has_win() -> bool {
    (unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) } & 0x8000) != 0
        || (unsafe { GetAsyncKeyState(VK_RWIN.0 as i32) } & 0x8000) != 0
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

fn vk_to_key(vk: VIRTUAL_KEY) -> Option<Key> {
    Some(match vk.0 as u16 {
        0x41..=0x5A => Key::KeyA + (vk.0 - 0x41) as u8,
        0x30..=0x39 => Key::Key0 + (vk.0 - 0x30) as u8,
        0x70..=0x7B => Key::F1 + (vk.0 - 0x70) as u8,
        VK_SPACE => Key::Space,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_TAB => Key::Tab,
        VK_BACK => Key::Backspace,
        VK_LMENU => Key::LeftAlt,
        VK_RMENU => Key::RightAlt,
        VK_LCONTROL => Key::LeftControl,
        VK_RCONTROL => Key::RightControl,
        VK_LSHIFT => Key::LeftShift,
        VK_RSHIFT => Key::RightShift,
        VK_LWIN => Key::LeftWin,
        VK_RWIN => Key::RightWin,
        VK_CAPITAL => Key::CapsLock,
        VK_OEM_COMMA => Key::Comma,
        VK_OEM_PERIOD => Key::Period,
        VK_OEM_2 => Key::Slash,
        VK_OEM_1 => Key::Semicolon,
        VK_OEM_7 => Key::Quote,
        VK_OEM_4 => Key::BracketLeft,
        VK_OEM_6 => Key::BracketRight,
        VK_OEM_5 => Key::Backslash,
        VK_OEM_MINUS => Key::Minus,
        VK_OEM_PLUS => Key::Equal,
        VK_OEM_3 => Key::Grave,
        VK_LBUTTON => Key::MouseLeft,
        VK_RBUTTON => Key::MouseRight,
        VK_MBUTTON => Key::MouseMiddle,
        _ => return None,
    })
}
