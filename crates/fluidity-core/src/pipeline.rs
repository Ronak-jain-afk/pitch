use std::time::Instant;

/// Events emitted by the pipeline for the UI layer to consume.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    StateChanged(PipelineState),
    PartialTranscription(String),
    AudioLevel(f32),
    Error(String),
}

/// Recording pipeline state machine.
#[derive(Debug, Clone)]
pub enum PipelineState {
    Idle,
    Recording {
        accumulated_samples: usize,
        streaming_text: String,
        started_at: Instant,
    },
    Transcribing,
    Enhancing {
        original_text: String,
        enhanced_text: Option<String>,
        provider_label: String,
    },
    Inserting {
        text: String,
    },
    Error {
        message: String,
        recoverable: bool,
    },
}

impl PipelineState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Recording { .. } => "Recording",
            Self::Transcribing => "Transcribing",
            Self::Enhancing { .. } => "Enhancing",
            Self::Inserting { .. } => "Inserting",
            Self::Error { .. } => "Error",
        }
    }
}

/// The pipeline orchestrates the recording → transcription → post-process → insert flow.
pub struct Pipeline {
    state: PipelineState,
    event_sender: tokio::sync::mpsc::UnboundedSender<PipelineEvent>,
    event_receiver: tokio::sync::mpsc::UnboundedReceiver<PipelineEvent>,
}

impl Pipeline {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state: PipelineState::Idle,
            event_sender: tx,
            event_receiver: rx,
        }
    }

    pub fn state(&self) -> &PipelineState {
        &self.state
    }

    pub fn event_receiver(&mut self) -> &mut tokio::sync::mpsc::UnboundedReceiver<PipelineEvent> {
        &mut self.event_receiver
    }

    pub fn event_sender(&self) -> tokio::sync::mpsc::UnboundedSender<PipelineEvent> {
        self.event_sender.clone()
    }

    pub fn transition_to(&mut self, new_state: PipelineState) {
        tracing::debug!(
            from = %self.state.name(),
            to = %new_state.name(),
            "Pipeline state transition"
        );
        self.state = new_state.clone();
        let _ = self.event_sender.send(PipelineEvent::StateChanged(new_state));
    }

    pub fn set_recording(&mut self, samples: usize, text: String) {
        if let PipelineState::Recording { ref mut accumulated_samples, ref mut streaming_text, .. } = self.state {
            *accumulated_samples = samples;
            *streaming_text = text;
        }
    }

    pub fn emit_partial(&self, text: String) {
        let _ = self.event_sender.send(PipelineEvent::PartialTranscription(text));
    }

    pub fn emit_audio_level(&self, level: f32) {
        let _ = self.event_sender.send(PipelineEvent::AudioLevel(level));
    }

    pub fn emit_error(&self, msg: String) {
        let _ = self.event_sender.send(PipelineEvent::Error(msg));
    }
}
