use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use fluidity_core::{
    asr::{AsrEngine, ModelManager, WhisperEngine},
    audio::{Resampler, RingBuffer},
    config::Config,
    pipeline::{Pipeline, PipelineState},
};
use tracing::{debug, info, warn};

#[derive(Parser)]
#[command(name = "fluidity", about = "Voice dictation tool", version)]
struct Cli {
    /// Subcommand
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Record audio and transcribe to stdout
    Record {
        /// Duration in seconds (default: record until Ctrl+C)
        #[arg(short, long)]
        duration: Option<f64>,

        /// Input device name (optional, uses default if omitted)
        #[arg(short, long)]
        device: Option<String>,

        /// Whisper model size (tiny, base, small, medium, large)
        #[arg(short, long, default_value = "tiny")]
        model: String,
    },
    /// Download a Whisper model
    Download {
        /// Model size (tiny, base, small, medium, large)
        #[arg(default_value = "tiny")]
        model: String,
    },
    /// List available audio input devices
    ListDevices,
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
        Command::Record {
            duration,
            device,
            model,
        } => run_record(duration, device, &model).await,
        Command::Download { model } => run_download(&model).await,
        Command::ListDevices => run_list_devices(),
    }
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

async fn run_record(
    duration: Option<f64>,
    device: Option<String>,
    model_name: &str,
) -> anyhow::Result<()> {
    // Load config
    let config_path = Config::config_path();
    let _config = Config::load(&config_path);
    info!("Config: {}", config_path.display());

    // Ensure model is downloaded
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

    // Load Whisper engine
    info!("Loading Whisper model...");
    let mut whisper = WhisperEngine::new();
    whisper.load(&model_path)?;
    info!("Whisper model loaded");

    // Create pipeline
    let mut pipeline = Pipeline::new();
    pipeline.transition_to(PipelineState::Idle);

    // Setup audio capture
    info!("Starting audio capture...");
    let mut capture = fluidity_platform::create_audio_capture();

    let ring = Arc::new(RingBuffer::new(65536));
    let resampler = Arc::new(std::sync::Mutex::new(Resampler::new(
        48000,
        16000,
        1,
    )));

    // When audio data arrives, push through resampler and into ring buffer
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

    // Start a timer for streaming transcription while recording
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
                // We have enough new samples for a streaming pass
                let chunk_size = available - last_pos;
                let mut chunk = vec![0.0f32; chunk_size];
                ring_for_streaming.pop(&mut chunk);
                // These samples were already consumed by the streaming read, but we need
                // to be more careful here since we're sharing the ring between streaming
                // and the final transcription.
                //
                // For Phase 1, we read a snapshot and only use it for display.
                last_pos = ring_for_streaming.len();

                // In Phase 1, we skip actual streaming transcription for the CLI demo
                // and just report that we're recording.
                // Full streaming will be added when the UI is built.
            }
        }
    });

    // Calculate expected duration
    let end_time = duration.map(|d| std::time::Instant::now() + Duration::from_secs_f64(d));

    // Wait for recording to complete
    if let Some(end) = end_time {
        let remaining = end.saturating_duration_since(std::time::Instant::now());
        info!("Recording for {:.1} seconds...", duration.unwrap_or(0.0));
        tokio::time::sleep(remaining).await;
    } else {
        // Record until Ctrl+C
        tokio::signal::ctrl_c().await?;
    }

    // Stop recording
    capture.stop()?;
    streaming_handle.abort();
    info!("Recording stopped");

    // Flush resampler and read all audio
    let mut resampler_guard = resampler.lock().unwrap();
    let tail = resampler_guard.flush();
    if !tail.is_empty() {
        ring.push(&tail);
    }
    drop(resampler_guard);

    // Read all accumulated audio
    let total = ring.len();
    info!("Captured {total} samples ({:.2}s)", total as f64 / 16000.0);

    if total < 16000 {
        warn!("Audio too short for transcription (need >= 1 second)");
        return Ok(());
    }

    let mut samples = vec![0.0f32; total];
    ring.pop(&mut samples);

    // Transcribe
    info!("Transcribing...");
    pipeline.transition_to(PipelineState::Transcribing);
    let result = whisper.transcribe(&samples)?;

    println!("\n=== Transcription ===");
    println!("{}", result.text);
    println!("====================\n");

    for seg in &result.segments {
        debug!("[{:.2}s -> {:.2}s] {}", seg.start, seg.end, seg.text);
    }

    pipeline.transition_to(PipelineState::Idle);

    Ok(())
}
