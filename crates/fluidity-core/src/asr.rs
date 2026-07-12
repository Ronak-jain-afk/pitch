use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

/// ASR engine trait — supported backends implement this.
pub trait AsrEngine: Send {
    fn load(&mut self, model_path: &Path) -> Result<(), AsrError>;
    fn is_loaded(&self) -> bool;
    fn transcribe(&self, samples: &[f32]) -> Result<AsrResult, AsrError>;
    fn transcribe_streaming(&self, samples: &[f32]) -> Result<AsrResult, AsrError>;
}

#[derive(Debug, Clone)]
pub struct AsrResult {
    pub text: String,
    pub segments: Vec<AsrSegment>,
}

#[derive(Debug, Clone)]
pub struct AsrSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("Model not loaded")]
    NotLoaded,
    #[error("Model file not found: {0}")]
    ModelNotFound(String),
    #[error("Transcription failed: {0}")]
    TranscriptionFailed(String),
    #[error("Audio too short (need at least {0} samples)")]
    AudioTooShort(usize),
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Whisper.cpp engine via whisper-rs bindings.
pub struct WhisperEngine {
    ctx: Option<whisper_rs::WhisperContext>,
    model_path: Option<PathBuf>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        Self {
            ctx: None,
            model_path: None,
        }
    }
}

impl AsrEngine for WhisperEngine {
    fn load(&mut self, model_path: &Path) -> Result<(), AsrError> {
        if !model_path.exists() {
            return Err(AsrError::ModelNotFound(model_path.display().to_string()));
        }

        info!(path = %model_path.display(), "Loading Whisper model");

        let params = whisper_rs::WhisperContextParameters {
            use_gpu: false,
            ..Default::default()
        };

        let ctx = whisper_rs::WhisperContext::new_with_params(model_path.to_str().unwrap(), params)
            .map_err(|e| AsrError::TranscriptionFailed(format!("Failed to load model: {e}")))?;

        self.ctx = Some(ctx);
        self.model_path = Some(model_path.to_path_buf());
        info!("Whisper model loaded");
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.ctx.is_some()
    }

    fn transcribe(&self, samples: &[f32]) -> Result<AsrResult, AsrError> {
        let ctx = self.ctx.as_ref().ok_or(AsrError::NotLoaded)?;

        if samples.len() < 16000 {
            return Err(AsrError::AudioTooShort(16000));
        }

        let mut state = ctx
            .create_state()
            .map_err(|e| AsrError::TranscriptionFailed(format!("State creation: {e}")))?;

        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
            best_of: 1,
        });

        params.set_n_threads(4);
        params.set_progress_callback_safe(|progress| {
            debug!(progress, "Transcription progress");
        });

        state
            .full(params, samples)
            .map_err(|e| AsrError::TranscriptionFailed(format!("Full transcription: {e}")))?;

        let n_segments = state.full_n_segments() as usize;

        let mut segments = Vec::with_capacity(n_segments);

        let mut full_text = String::new();

        for i in 0..n_segments {
            let seg = state.get_segment(i as i32).ok_or_else(|| {
                AsrError::TranscriptionFailed(format!("Missing segment {i}"))
            })?;
            let text = seg.to_str().map_err(|e| {
                AsrError::TranscriptionFailed(format!("Segment text: {e}"))
            })?;

            segments.push(AsrSegment {
                start: seg.start_timestamp() as f64 / 100.0,
                end: seg.end_timestamp() as f64 / 100.0,
                text: text.to_string(),
            });

            if !full_text.is_empty() {
                full_text.push(' ');
            }
            full_text.push_str(text);
        }

        Ok(AsrResult { text: full_text, segments })
    }

    fn transcribe_streaming(&self, samples: &[f32]) -> Result<AsrResult, AsrError> {
        // For now, streaming just does a full pass on the chunk.
        // whisper.cpp does incremental decoding internally.
        self.transcribe(samples)
    }
}

/// Manages Whisper model files: download, cache, discover.
pub struct ModelManager {
    cache_dir: PathBuf,
}

impl ModelManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Known Whisper model sizes and their download URLs.
    pub const MODELS: &[ModelInfo] = &[
        ModelInfo { name: "tiny", filename: "ggml-tiny.bin", size_mb: 75, url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin" },
        ModelInfo { name: "base", filename: "ggml-base.bin", size_mb: 142, url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin" },
        ModelInfo { name: "small", filename: "ggml-small.bin", size_mb: 466, url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin" },
        ModelInfo { name: "medium", filename: "ggml-medium.bin", size_mb: 1500, url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin" },
        ModelInfo { name: "large", filename: "ggml-large-v3.bin", size_mb: 2900, url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin" },
    ];

    pub fn model_path(&self, name: &str) -> PathBuf {
        let info = Self::MODELS.iter().find(|m| m.name == name).unwrap_or(&Self::MODELS[0]);
        self.cache_dir.join(&info.filename)
    }

    pub fn is_downloaded(&self, name: &str) -> bool {
        self.model_path(name).exists()
    }

    /// Download a model from HuggingFace with progress reporting.
    pub async fn download(
        &self,
        name: &str,
        progress: impl Fn(u64, u64) + Send + 'static,
    ) -> Result<PathBuf, AsrError> {
        let info = Self::MODELS.iter().find(|m| m.name == name).ok_or_else(|| {
            AsrError::DownloadFailed(format!("Unknown model: {name}"))
        })?;

        let dest = self.model_path(name);
        if dest.exists() {
            info!("Model already cached at {}", dest.display());
            return Ok(dest);
        }

        tokio::fs::create_dir_all(&self.cache_dir).await?;

        info!("Downloading {} model from {}", info.name, info.url);

        let response = reqwest::get(info.url).await.map_err(|e| {
            AsrError::DownloadFailed(format!("HTTP request: {e}"))
        })?;

        let total = response.content_length().unwrap_or(0);
        let mut file = tokio::fs::File::create(&dest).await?;
        let mut downloaded: u64 = 0;
        let mut chunk = response.bytes_stream();

        use tokio_stream::StreamExt;
        while let Some(item) = chunk.next().await {
            let bytes = item.map_err(|e| AsrError::DownloadFailed(format!("Stream error: {e}")))?;
            file.write_all(&bytes).await?;
            downloaded += bytes.len() as u64;
            progress(downloaded, total);
        }

        file.flush().await?;
        info!("Model downloaded to {}", dest.display());
        Ok(dest)
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub size_mb: u64,
    pub url: &'static str,
}

/// Downloads a file from `url` to `dest` with progress callback.
/// Used when tokio-stream isn't available or for simpler cases.
#[allow(dead_code)]
pub(crate) async fn download_file(
    url: &str,
    dest: &Path,
    progress: impl Fn(u64, u64) + Send,
) -> Result<(), AsrError> {
    let response = reqwest::get(url).await.map_err(|e| {
        AsrError::DownloadFailed(format!("HTTP request: {e}"))
    })?;

    let total = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use tokio_stream::StreamExt;
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| AsrError::DownloadFailed(format!("Stream error: {e}")))?;
        file.write_all(&bytes).await?;
        downloaded += bytes.len() as u64;
        progress(downloaded, total);
    }

    file.flush().await?;
    Ok(())
}
