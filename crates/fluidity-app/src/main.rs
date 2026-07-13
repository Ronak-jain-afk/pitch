use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use fluidity_core::{
    asr::{AsrEngine, ModelManager, WhisperEngine},
    audio::{Resampler, RingBuffer},
    config::Config,
    hotkey::{Hotkey, HotkeyEvent, Key, Modifiers},
    pipeline::{Pipeline, PipelineState},
};
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "fluidity", about = "Voice dictation tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Record audio and transcribe to stdout
    Record {
        #[arg(short, long)]
        duration: Option<f64>,
        #[arg(short, long)]
        device: Option<String>,
        #[arg(short, long, default_value = "tiny")]
        model: String,
    },
    /// Download a Whisper model
    Download {
        #[arg(default_value = "tiny")]
        model: String,
    },
    /// List available audio input devices
    ListDevices,
    /// Run as background daemon with hotkey and tray icon
    Daemon {
        #[arg(short, long, default_value = "tiny")]
        model: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Record { duration, device, model } => run_record(duration, device, &model).await?,
        Command::Download { model } => run_download(&model).await?,
        Command::ListDevices => run_list_devices()?,
        Command::Daemon { model } => run_daemon(&model)?,
    }

    Ok(())
}

async fn run_download(model_name: &str) -> anyhow::Result<()> {
    let cache_dir = Config::model_cache_dir();
    let manager = ModelManager::new(cache_dir);
    info!("Downloading {model_name} model...");
    let path = manager
        .download(model_name, |downloaded, total| {
            if total > 0 {
                let pct = (downloaded as f64 / total as f64) * 100.0;
                print!("\rProgress: {pct:.1}% ({downloaded} / {total} bytes)");
            } else {
                print!("\rDownloaded: {downloaded} bytes");
            }
            use std::io::Write;
            std::io::stdout().flush().ok();
        })
        .await?;
    println!("\nModel saved to: {}", path.display());
    Ok(())
}

fn run_list_devices() -> anyhow::Result<()> {
    let devices = fluidity_platform::list_input_devices();
    println!("Input devices:");
    for device in &devices {
        println!("  {device}");
    }
    Ok(())
}

// --- Daemon mode: hotkey → record → transcribe → insert text ---

fn run_daemon(model_name: &str) -> anyhow::Result<()> {
    let cache_dir = Config::model_cache_dir();
    let model_mgr = ModelManager::new(cache_dir.clone());
    let model_path = model_mgr.model_path(model_name);

    if !model_path.exists() {
        info!("Model {model_name} not cached. Run `fluidity download {model_name}` first.");
        return Ok(());
    }

    info!("Loading Whisper model...");
    let mut whisper = WhisperEngine::new();
    whisper.load(&model_path)?;
    info!("Whisper model loaded");

    let recording = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = crossbeam_channel::bounded::<HotkeyEvent>(16);

    // Setup hotkey listener (Ctrl+Shift+Space)
    let mut hotkey_listener = fluidity_platform::create_hotkey_listener();
    hotkey_listener.set_handler(Box::new(move |event| {
        cmd_tx.send(event).ok();
    }));
    hotkey_listener
        .register(Hotkey {
            modifiers: Modifiers::ControlShift,
            key: Key::Space,
        })
        .map_err(|e| anyhow::anyhow!("Hotkey registration: {e}"))?;

    // Start hotkey listener on its own thread
    std::thread::spawn(move || {
        hotkey_listener.run().ok();
    });

    info!("Daemon ready. Press Ctrl+Shift+Space to record.");
    info!("Press Ctrl+C to quit.");

    let mut pipeline = Pipeline::new();
    let mut recorder = RecorderState::new();
    let running = Arc::new(AtomicBool::new(true));

    // Ctrl+C handler
    let r = running.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1];
        let _ = std::io::stdin().read(&mut buf);
        r.store(false, Ordering::Release);
    });

    while running.load(Ordering::Acquire) {
        match cmd_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(HotkeyEvent::Down { .. }) => {
                if !recording.load(Ordering::Acquire) {
                    recording.store(true, Ordering::Release);
                    info!("Hotkey DOWN — recording");
                    if let Err(e) = recorder.start() {
                        error!("Failed to start recording: {e}");
                    }
                    start_recording(&mut pipeline);
                }
            }
            Ok(HotkeyEvent::Up { .. }) => {
                if recording.load(Ordering::Acquire) {
                    recording.store(false, Ordering::Release);
                    info!("Hotkey UP — transcribing & inserting");
                    match stop_and_transcribe(&mut pipeline, &mut recorder, &whisper) {
                        Ok(text) => {
                            if !text.is_empty() {
                                let inserter = fluidity_platform::create_text_inserter();
                                if let Err(e) = inserter.insert_with_fallback(&text) {
                                    error!("Text insertion failed: {e}");
                                } else {
                                    info!("Inserted {} chars", text.len());
                                }
                            }
                        }
                        Err(e) => error!("Transcription/insertion failed: {e}"),
                    }
                    pipeline.transition_to(PipelineState::Idle);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }

    info!("Daemon stopped");
    Ok(())
}

// --- Daemon recording helpers ---

struct RecorderState {
    capture: Box<dyn fluidity_core::AudioCapture>,
    ring: Arc<RingBuffer>,
    resampler: Arc<std::sync::Mutex<Resampler>>,
}

impl RecorderState {
    fn new() -> Self {
        let mut capture = fluidity_platform::create_audio_capture();
        let ring = Arc::new(RingBuffer::new(65536));
        let resampler = Arc::new(std::sync::Mutex::new(Resampler::new(48000, 16000, 1)));

        let r = ring.clone();
        let res = resampler.clone();
        capture.set_callback(Box::new(move |samples: &[f32]| {
            if let Ok(mut rs) = res.lock() {
                let out = rs.process(samples);
                if !out.is_empty() {
                    r.push(&out);
                }
            }
        }));

        Self { capture, ring, resampler }
    }

    fn start(&mut self) -> Result<(), fluidity_core::AudioError> {
        self.capture.start(None)
    }

    fn stop_and_drain(&mut self) -> Vec<f32> {
        let _ = self.capture.stop();
        let mut rs = self.resampler.lock().unwrap();
        let tail = rs.flush();
        if !tail.is_empty() {
            self.ring.push(&tail);
        }
        drop(rs);
        let total = self.ring.len();
        let mut samples = vec![0.0f32; total];
        self.ring.pop(&mut samples);
        samples
    }
}

fn start_recording(pipeline: &mut Pipeline) {
    pipeline.transition_to(PipelineState::Recording {
        accumulated_samples: 0,
        streaming_text: String::new(),
        started_at: std::time::Instant::now(),
    });
}

fn stop_and_transcribe(
    pipeline: &mut Pipeline,
    recorder: &mut RecorderState,
    whisper: &WhisperEngine,
) -> Result<String, anyhow::Error> {
    let samples = recorder.stop_and_drain();

    if samples.len() < 16000 {
        warn!("Audio too short ({} samples)", samples.len());
        pipeline.transition_to(PipelineState::Error {
            message: "Audio too short".into(),
            recoverable: true,
        });
        return Ok(String::new());
    }

    pipeline.transition_to(PipelineState::Transcribing);
    let result = whisper.transcribe(&samples)?;
    pipeline.transition_to(PipelineState::Inserting {
        text: result.text.clone(),
    });

    Ok(result.text)
}

// --- CLI record subcommand (Phase 1) ---

async fn run_record(
    duration: Option<f64>,
    device: Option<String>,
    model_name: &str,
) -> anyhow::Result<()> {
    let config_path = Config::config_path();
    let _config = Config::load(&config_path);
    info!("Config: {}", config_path.display());

    let cache_dir = Config::model_cache_dir();
    let model_mgr = ModelManager::new(cache_dir.clone());
    let model_path = model_mgr.model_path(model_name);

    if !model_path.exists() {
        info!("Model not found, downloading {model_name}...");
        model_mgr
            .download(model_name, |downloaded, total| {
                if total > 0 {
                    let pct = (downloaded as f64 / total as f64) * 100.0;
                    print!("\rDownloading: {pct:.1}%");
                }
                std::io::Write::flush(&mut std::io::stdout()).ok();
            })
            .await?;
        println!();
    }

    info!("Loading Whisper model...");
    let mut whisper = WhisperEngine::new();
    whisper.load(&model_path)?;
    info!("Whisper model loaded");

    let mut pipeline = Pipeline::new();
    pipeline.transition_to(PipelineState::Idle);

    info!("Starting audio capture...");
    let mut capture = fluidity_platform::create_audio_capture();

    let ring = Arc::new(RingBuffer::new(65536));
    let resampler = Arc::new(std::sync::Mutex::new(Resampler::new(48000, 16000, 1)));

    let ring_for_callback = ring.clone();
    let resampler_for_callback = resampler.clone();
    capture.set_callback(Box::new(move |samples: &[f32]| {
        let mut resampler = resampler_for_callback.lock().unwrap();
        let resampled = resampler.process(samples);
        if !resampled.is_empty() {
            ring_for_callback.push(&resampled);
        }
    }));

    let device_id = device.as_deref();
    capture.start(device_id)?;
    info!("Recording... (press Ctrl+C to stop)");

    let sample_rate = capture.sample_rate();
    info!("Capture sample rate: {sample_rate} Hz");

    let _pipeline_tx = pipeline.event_sender();
    let ring_for_streaming = ring.clone();
    let streaming_handle = tokio::spawn(async move {
        let mut last_pos = 0usize;
        let _partial_text = String::new();
        let interval = Duration::from_millis(300);
        loop {
            tokio::time::sleep(interval).await;
            let available = ring_for_streaming.len();
            if available > last_pos + 16000 {
                let chunk_size = available - last_pos;
                let mut _chunk = vec![0.0f32; chunk_size];
                ring_for_streaming.pop(&mut _chunk);
                last_pos = ring_for_streaming.len();
            }
        }
    });

    let end_time = duration.map(|d| std::time::Instant::now() + Duration::from_secs_f64(d));

    if let Some(end) = end_time {
        let remaining = end.saturating_duration_since(std::time::Instant::now());
        info!("Recording for {:.1} seconds...", duration.unwrap_or(0.0));
        tokio::time::sleep(remaining).await;
    } else {
        tokio::signal::ctrl_c().await?;
    }

    capture.stop()?;
    streaming_handle.abort();
    info!("Recording stopped");

    let mut resampler_guard = resampler.lock().unwrap();
    let tail = resampler_guard.flush();
    if !tail.is_empty() {
        ring.push(&tail);
    }
    drop(resampler_guard);

    let total = ring.len();
    info!("Captured {total} samples ({:.2}s)", total as f64 / 16000.0);

    if total < 16000 {
        warn!("Audio too short for transcription (need >= 1 second)");
        return Ok(());
    }

    let mut samples = vec![0.0f32; total];
    ring.pop(&mut samples);

    info!("Transcribing...");
    pipeline.transition_to(PipelineState::Transcribing);
    let result = whisper.transcribe(&samples)?;

    println!("\n=== Transcription ===");
    println!("{}", result.text);
    println!("====================");

    pipeline.transition_to(PipelineState::Idle);

    Ok(())
}
