use std::sync::Arc;

use fluidity_core::{Hotkey, HotkeyError, HotkeyEvent, HotkeyListener, Modifiers};

/// Windows global hotkey listener using SetWindowsHookEx.
/// Stub for Phase 1 — will be implemented in Phase 2.
pub struct WindowsHotkeyListener {
    _handler: Option<Box<dyn Fn(HotkeyEvent) + Send>>,
}

impl WindowsHotkeyListener {
    pub fn new() -> Self {
        Self { _handler: None }
    }
}

impl HotkeyListener for WindowsHotkeyListener {
    fn register(&mut self, _shortcut: Hotkey) -> Result<(), HotkeyError> {
        Ok(())
    }

    fn set_handler(&mut self, handler: Box<dyn Fn(HotkeyEvent) + Send>) {
        self._handler = Some(handler);
    }

    fn run(&self) -> Result<(), HotkeyError> {
        Ok(())
    }
}
