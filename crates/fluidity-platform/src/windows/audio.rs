use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fluidity_core::{AudioCapture, AudioError, RingBuffer};

/// cpal::Stream is !Send. Wrap for our single-threaded usage.
struct SendStream(Option<cpal::Stream>);
unsafe impl Send for SendStream {}

/// List all available input devices with descriptions.
pub fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    let devices: Vec<String> = match host.input_devices() {
        Ok(devs) => devs
            .filter_map(|d| {
                let name = d.name().ok()?;
                let config = d.default_input_config().ok()?;
                Some(format!(
                    "{name}  [{}Hz, {}ch, {:?}]",
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format()
                ))
            })
            .collect(),
        Err(e) => return vec![format!("Error: {e}")],
    };

    if devices.is_empty() {
        vec!["No input devices found".to_string()]
    } else {
        devices
    }
}

/// Windows audio capture via cpal (WASAPI backend).
pub struct WindowsAudioCapture {
    device: Option<cpal::Device>,
    stream: SendStream,
    config: Option<cpal::StreamConfig>,
    callback: Option<Box<dyn FnMut(&[f32]) + Send>>,
    ring: Arc<RingBuffer>,
}

impl WindowsAudioCapture {
    pub fn new() -> Self {
        Self {
            device: None,
            stream: SendStream(None),
            config: None,
            callback: None,
            ring: Arc::new(RingBuffer::new(16384)),
        }
    }

    fn resolve_device(&mut self, device_id: Option<&str>) -> Result<cpal::Device, AudioError> {
        let host = cpal::default_host();

        let device = if let Some(id) = device_id {
            host.input_devices()
                .map_err(|e| AudioError::Platform(format!("Enumerate devices: {e}")))?
                .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(id.to_string()))?
        } else {
            host.default_input_device()
                .ok_or(AudioError::NoDevice)?
        };

        Ok(device)
    }
}

impl AudioCapture for WindowsAudioCapture {
    fn start(&mut self, device_id: Option<&str>) -> Result<(), AudioError> {
        self.device = Some(self.resolve_device(device_id)?);
        let device = self.device.as_ref().unwrap();

        let config = device
            .default_input_config()
            .map_err(|e| AudioError::StreamError(format!("Default config: {e}")))?;

        tracing::info!(
            device = %device.name().unwrap_or_default(),
            sample_rate = config.sample_rate().0,
            channels = config.channels(),
            format = ?config.sample_format(),
            "Starting audio capture"
        );

        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        self.config = Some(stream_config.clone());

        let ring = self.ring.clone();
        let err_channel: tokio::sync::mpsc::UnboundedSender<String> = {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            std::thread::spawn(move || {
                while let Some(err) = rx.blocking_recv() {
                    tracing::error!("Audio stream error: {err}");
                }
            });
            tx
        };

        // Build callback: cpal delivers samples in platform-native format.
        // We normalize to f32 mono and push into the ring buffer.
        let channels = stream_config.channels as usize;

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let err_tx = err_channel.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        let mono: Vec<f32> = if channels > 1 {
                            data.chunks(channels)
                                .map(|ch| ch.iter().sum::<f32>() / channels as f32)
                                .collect()
                        } else {
                            data.to_vec()
                        };
                        ring.push(&mono);
                    },
                    move |err| {
                        let _ = err_tx.send(format!("{err}"));
                    },
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let err_tx = err_channel.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let mono: Vec<f32> = if channels > 1 {
                            data.chunks(channels)
                                .map(|ch| {
                                    ch.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                        / channels as f32
                                })
                                .collect()
                        } else {
                            data.iter().map(|&s| s as f32 / 32768.0).collect()
                        };
                        ring.push(&mono);
                    },
                    move |err| {
                        let _ = err_tx.send(format!("{err}"));
                    },
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let err_tx = err_channel.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        let mono: Vec<f32> = if channels > 1 {
                            data.chunks(channels)
                                .map(|ch| {
                                    ch.iter().map(|&s| s as f32 / 65535.0).sum::<f32>()
                                        / channels as f32
                                })
                                .collect()
                        } else {
                            data.iter().map(|&s| s as f32 / 65535.0).collect()
                        };
                        ring.push(&mono);
                    },
                    move |err| {
                        let _ = err_tx.send(format!("{err}"));
                    },
                    None,
                )
            }
            _ => {
                return Err(AudioError::StreamError(format!(
                    "Unsupported sample format: {sample_format:?}"
                )));
            }
        }
        .map_err(|e| AudioError::StreamError(format!("Build stream: {e}")))?;

        stream
            .play()
            .map_err(|e| AudioError::StreamError(format!("Play stream: {e}")))?;

        self.stream = SendStream(Some(stream));
        tracing::info!("Audio capture started");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(stream) = self.stream.0.take() {
            drop(stream);
        }
        tracing::info!("Audio capture stopped");
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.config
            .as_ref()
            .map(|c| c.sample_rate.0)
            .unwrap_or(48000)
    }

    fn set_callback(&mut self, cb: Box<dyn FnMut(&[f32]) + Send>) {
        self.callback = Some(cb);
    }
}
