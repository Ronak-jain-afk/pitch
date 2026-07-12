use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Audio capture trait — platform crate implements this.
pub trait AudioCapture: Send {
    fn start(&mut self, device_id: Option<&str>) -> Result<(), AudioError>;
    fn stop(&mut self) -> Result<(), AudioError>;
    fn sample_rate(&self) -> u32;
    fn set_callback(&mut self, cb: Box<dyn FnMut(&[f32]) + Send>);
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("No input device available")]
    NoDevice,
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Unsupported sample rate: {0}")]
    UnsupportedRate(u32),
    #[error("Platform error: {0}")]
    Platform(String),
}

/// Lock-free single-producer single-consumer ring buffer for float audio samples.
pub struct RingBuffer {
    buffer: UnsafeCell<Box<[f32]>>,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}

unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        Self {
            buffer: UnsafeCell::new(vec![0.0f32; cap].into_boxed_slice()),
            mask: cap - 1,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    fn buf(&self) -> &mut [f32] {
        unsafe { &mut *self.buffer.get() }
    }

    pub fn push(&self, samples: &[f32]) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire);
        let occupied = w.wrapping_sub(r);
        let cap = self.buf().len();
        let available = cap - occupied.min(cap);
        let to_write = samples.len().min(available);
        let buf = self.buf();

        for i in 0..to_write {
            let idx = (w + i) & self.mask;
            buf[idx] = samples[i];
        }
        self.write.store(w.wrapping_add(to_write), Ordering::Release);
        to_write
    }

    pub fn pop(&self, dest: &mut [f32]) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire);
        let available = w.wrapping_sub(r).min(self.buf().len());
        let to_read = dest.len().min(available);
        let buf = self.buf();

        for i in 0..to_read {
            let idx = (r + i) & self.mask;
            dest[i] = buf[idx];
        }
        self.read.store(r.wrapping_add(to_read), Ordering::Release);
        to_read
    }

    pub fn len(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire);
        w.wrapping_sub(r).min(self.buf().len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.read.store(self.write.load(Ordering::Acquire), Ordering::Release);
    }

    pub fn capacity(&self) -> usize {
        self.buf().len()
    }
}

/// Resamples audio from any sample rate to 16 kHz mono using rubato.
pub struct Resampler {
    from_rate: u32,
    to_rate: u32,
    channels: usize,
    resampler: Option<rubato::SincFixedIn<f32>>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<Vec<f32>>,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32, channels: usize) -> Self {
        Self {
            from_rate,
            to_rate,
            channels,
            resampler: None,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.from_rate == self.to_rate {
            return if self.channels > 1 {
                // Downmix multi-channel to mono
                input
                    .chunks(self.channels)
                    .map(|ch| ch.iter().sum::<f32>() / self.channels as f32)
                    .collect()
            } else {
                input.to_vec()
            };
        }

        self.input_buffer.extend_from_slice(input);

        if self.resampler.is_none() {
            let ratio = self.to_rate as f64 / self.from_rate as f64;
            let params = rubato::SincFixedIn::<f32>::new(
                ratio,
                1.0,
                rubato::SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    interpolation: rubato::SincInterpolationType::Linear,
                    oversampling_factor: 256,
                    window: rubato::WindowFunction::BlackmanHarris2,
                },
                self.channels,
                self.input_buffer.len().max(1024),
            )
            .expect("Failed to create resampler");
            self.resampler = Some(params);
            self.output_buffer = vec![Vec::new(); self.channels];
        }

        use rubato::Resampler as _;
        let resampler = self.resampler.as_mut().unwrap();
        let input_frames = self.input_buffer.len() / self.channels;
        let needed = resampler.input_frames_next();

        if input_frames < needed {
            return Vec::new();
        }

        let exact = input_frames - (input_frames % needed);
        let input_chunks: Vec<Vec<f32>> = (0..self.channels)
            .map(|ch| {
                self.input_buffer[ch..exact * self.channels]
                    .iter()
                    .step_by(self.channels)
                    .copied()
                    .collect()
            })
            .collect();

        let output = resampler
            .process(&input_chunks, None)
            .expect("Resampling failed");

        // Drop processed input
        self.input_buffer.drain(0..exact * self.channels);

        // Downmix to mono
        if self.channels > 1 {
            let mono_len = output[0].len();
            let mut mono = Vec::with_capacity(mono_len);
            for i in 0..mono_len {
                let sum: f32 = output.iter().map(|ch| ch[i]).sum();
                mono.push(sum / self.channels as f32);
            }
            mono
        } else {
            output.into_iter().next().unwrap_or_default()
        }
    }

    pub fn flush(&mut self) -> Vec<f32> {
        if let Some(resampler) = self.resampler.as_mut() {
            use rubato::Resampler as _;
            let output = resampler
                .process_partial(None::<&[Vec<f32>]>, None)
                .expect("Flush resampling failed");
            self.input_buffer.clear();
            if self.channels > 1 {
                let mono_len = output[0].len();
                let mut mono = Vec::with_capacity(mono_len);
                for i in 0..mono_len {
                    let sum: f32 = output.iter().map(|ch| ch[i]).sum();
                    mono.push(sum / self.channels as f32);
                }
                mono
            } else {
                output.into_iter().next().unwrap_or_default()
            }
        } else {
            // No resampling needed, just return what's left
            let mono: Vec<f32> = if self.channels > 1 {
                self.input_buffer
                    .chunks(self.channels)
                    .map(|ch| ch.iter().sum::<f32>() / self.channels as f32)
                    .collect()
            } else {
                self.input_buffer.clone()
            };
            self.input_buffer.clear();
            mono
        }
    }

    pub fn reset(&mut self, from_rate: u32, to_rate: u32, channels: usize) {
        self.from_rate = from_rate;
        self.to_rate = to_rate;
        self.channels = channels;
        self.resampler = None;
        self.input_buffer.clear();
        self.output_buffer.clear();
    }
}
