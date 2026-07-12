use fluidity_core::AudioCapture;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::audio::WindowsAudioCapture;

/// List available audio input devices with their specs.
pub fn list_input_devices() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        windows::audio::list_devices()
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec!["Device listing only available on Windows".to_string()]
    }
}

/// Create the platform's default audio capture implementation.
pub fn create_audio_capture() -> Box<dyn AudioCapture> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsAudioCapture::new())
    }
    #[cfg(target_os = "linux")]
    {
        // Placeholder: return a no-op capture
        struct NullCapture;
        impl AudioCapture for NullCapture {
            fn start(&mut self, _: Option<&str>) -> Result<(), fluidity_core::AudioError> { Ok(()) }
            fn stop(&mut self) -> Result<(), fluidity_core::AudioError> { Ok(()) }
            fn sample_rate(&self) -> u32 { 16000 }
            fn set_callback(&mut self, _: Box<dyn FnMut(&[f32]) + Send>) {}
        }
        Box::new(NullCapture)
    }
}
