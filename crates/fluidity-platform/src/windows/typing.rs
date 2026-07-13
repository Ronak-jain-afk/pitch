use fluidity_core::{InsertError, InsertTarget, TextInserter};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData, CF_UNICODETEXT, VK_CONTROL,
};

pub struct WindowsTextInserter;

impl TextInserter for WindowsTextInserter {
    fn insert(&self, text: &str, _target: InsertTarget) -> Result<(), InsertError> {
        if text.is_empty() {
            return Err(InsertError::EmptyText);
        }
        sendinput_type(text)
    }

    fn insert_with_fallback(&self, text: &str) -> Result<(), InsertError> {
        if text.is_empty() {
            return Err(InsertError::EmptyText);
        }
        sendinput_type(text).or_else(|_| clipboard_paste(text))
    }
}

fn sendinput_type(text: &str) -> Result<(), InsertError> {
    for ch in text.encode_utf16() {
        // Key down with UNICODE flag
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: ch,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            })},
        };
        // Key up
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: ch,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            })},
        };
        let inputs = [down, up];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
    Ok(())
}

unsafe fn transmute_keybd(kb: KEYBDINPUT) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
    // KEYBDINPUT and INPUT_0 have the same layout; SAFETY: confirmed by windows crate docs
    std::mem::transmute(kb)
}

fn clipboard_paste(text: &str) -> Result<(), InsertError> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let bytes = wide.len() * 2;

    unsafe {
        if !OpenClipboard(None).as_bool() {
            return Err(InsertError::ClipboardFailed);
        }

        EmptyClipboard();

        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if h.is_invalid() {
            CloseClipboard();
            return Err(InsertError::ClipboardFailed);
        }

        let p = GlobalLock(h);
        if p.is_null() {
            CloseClipboard();
            return Err(InsertError::ClipboardFailed);
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p as *mut u16, wide.len());
        GlobalUnlock(h);

        SetClipboardData(CF_UNICODETEXT, h);
        CloseClipboard();
    }

    // Send Ctrl+V
    let ctrl_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
            wVk: VK_CONTROL,
            wScan: 0,
            dwFlags: 0,
            time: 0,
            dwExtraInfo: 0,
        })},
    };
    let v_down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x56), // 'V'
            wScan: 0,
            dwFlags: 0,
            time: 0,
            dwExtraInfo: 0,
        })},
    };
    let v_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x56),
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        })},
    };
    let ctrl_up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: unsafe { transmute_keybd(KEYBDINPUT {
            wVk: VK_CONTROL,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        })},
    };

    // Small sleep to let clipboard settle
    std::thread::sleep(std::time::Duration::from_millis(50));

    unsafe {
        SendInput(&[ctrl_down, v_down, v_up, ctrl_up], std::mem::size_of::<INPUT>() as i32);
    }

    Ok(())
}
