use fluidity_core::{AudioCapture, HotkeyListener, TextInserter};

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::audio::WindowsAudioCapture;

/// List available audio input devices with their specs.
pub fn list_input_devices() -> Vec<String> {
    #[cfg(target_os = "windows")] { windows::audio::list_devices() }
    #[cfg(not(target_os = "windows"))] { vec!["Device listing only available on Windows".to_string()] }
}

/// Create the platform's default audio capture implementation.
pub fn create_audio_capture() -> Box<dyn AudioCapture> {
    #[cfg(target_os = "windows")]
    { Box::new(WindowsAudioCapture::new()) }
    #[cfg(target_os = "linux")]
    { Box::new(NullCapture) }
}

/// Create the platform's hotkey listener.
pub fn create_hotkey_listener() -> Box<dyn HotkeyListener> {
    #[cfg(target_os = "windows")]
    { Box::new(windows::hotkeys::WindowsHotkeyListener::new()) }
    #[cfg(target_os = "linux")]
    { Box::new(NullHotkeyListener) }
}

/// Create the platform's text inserter.
pub fn create_text_inserter() -> Box<dyn TextInserter> {
    #[cfg(target_os = "windows")]
    { Box::new(windows::typing::WindowsTextInserter) }
    #[cfg(target_os = "linux")]
    { Box::new(NullTextInserter) }
}

#[cfg(target_os = "linux")]
struct NullCapture;
#[cfg(target_os = "linux")]
impl AudioCapture for NullCapture {
    fn start(&mut self, _: Option<&str>) -> Result<(), fluidity_core::AudioError> { Ok(()) }
    fn stop(&mut self) -> Result<(), fluidity_core::AudioError> { Ok(()) }
    fn sample_rate(&self) -> u32 { 16000 }
    fn set_callback(&mut self, _: Box<dyn FnMut(&[f32]) + Send>) {}
}

#[cfg(target_os = "linux")]
struct NullHotkeyListener;
#[cfg(target_os = "linux")]
impl HotkeyListener for NullHotkeyListener {
    fn register(&mut self, _: fluidity_core::Hotkey) -> Result<(), fluidity_core::HotkeyError> { Ok(()) }
    fn set_handler(&mut self, _: Box<dyn Fn(fluidity_core::HotkeyEvent) + Send>) {}
    fn run(&self) -> Result<(), fluidity_core::HotkeyError> { Ok(()) }
}

#[cfg(target_os = "linux")]
struct NullTextInserter;
#[cfg(target_os = "linux")]
impl TextInserter for NullTextInserter {
    fn insert(&self, _: &str, _: fluidity_core::InsertTarget) -> Result<(), fluidity_core::InsertError> { Ok(()) }
    fn insert_with_fallback(&self, _: &str) -> Result<(), fluidity_core::InsertError> { Ok(()) }
}
