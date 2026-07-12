/// Hotkey listener trait — platform crate implements this.
pub trait HotkeyListener: Send {
    fn register(&mut self, shortcut: Hotkey) -> Result<(), HotkeyError>;
    fn set_handler(&mut self, handler: Box<dyn Fn(HotkeyEvent) + Send>);
    fn run(&self) -> Result<(), HotkeyError>;
}

#[derive(Debug, Clone)]
pub struct Hotkey {
    pub modifiers: Modifiers,
    pub key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifiers {
    None,
    Alt,
    Control,
    Shift,
    Win,
    AltControl,
    AltShift,
    ControlShift,
    AltControlShift,
    WinAlt,
    WinControl,
    WinShift,
    WinAltControl,
    WinAltShift,
    WinControlShift,
    WinAltControlShift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    LeftAlt,
    RightAlt,
    LeftControl,
    RightControl,
    LeftShift,
    RightShift,
    LeftWin,
    RightWin,
    CapsLock,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Backslash,
    Minus,
    Equal,
    Grave,
    MouseLeft,
    MouseRight,
    MouseMiddle,
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    Down { shortcut: Hotkey },
    Up { shortcut: Hotkey },
}

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("Failed to register hotkey: {0}")]
    RegistrationFailed(String),
    #[error("Hotkey already registered")]
    AlreadyRegistered,
    #[error("Platform error: {0}")]
    Platform(String),
    #[error("Permission denied")]
    PermissionDenied,
}
