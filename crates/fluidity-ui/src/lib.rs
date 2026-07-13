use eframe::egui;
use fluidity_core::pipeline::{PipelineEvent, PipelineState};

pub struct FluidityApp {
    event_rx: crossbeam_channel::Receiver<PipelineEvent>,
    state: PipelineState,
    partial_text: String,
    recording_started: Option<std::time::Instant>,
}

impl FluidityApp {
    pub fn new(event_rx: crossbeam_channel::Receiver<PipelineEvent>) -> Self {
        Self {
            event_rx,
            state: PipelineState::Idle,
            partial_text: String::new(),
            recording_started: None,
        }
    }
}

impl eframe::App for FluidityApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                PipelineEvent::StateChanged(s) => {
                    if matches!(&s, PipelineState::Recording { .. }) {
                        self.recording_started = Some(std::time::Instant::now());
                    }
                    if matches!(&s, PipelineState::Idle) {
                        self.partial_text.clear();
                    }
                    self.state = s;
                }
                PipelineEvent::PartialTranscription(t) => self.partial_text = t,
                PipelineEvent::AudioLevel(_) => {}
                PipelineEvent::Error(e) => {
                    // ponytail: show errors in overlay, no dedicated error toast yet
                    self.partial_text = format!("Error: {e}");
                }
            }
        }

        let active = !matches!(self.state, PipelineState::Idle);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(active));

        if !active {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_black_alpha(200)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (icon, color) = match &self.state {
                        PipelineState::Recording { .. } => ("\u{25CF}", egui::Color32::RED),
                        PipelineState::Transcribing => ("\u{25CB}", egui::Color32::YELLOW),
                        PipelineState::Enhancing { .. } => ("\u{25CB}", egui::Color32::YELLOW),
                        PipelineState::Inserting { .. } => ("\u{2192}", egui::Color32::GREEN),
                        PipelineState::Error { .. } => ("!", egui::Color32::RED),
                        PipelineState::Idle => ("", egui::Color32::WHITE),
                    };
                    ui.label(
                        egui::RichText::new(format!("{} {}", icon, self.state.name()))
                            .color(color)
                            .size(14.0),
                    );
                    if let PipelineState::Recording { .. } = &self.state {
                        if let Some(start) = self.recording_started {
                            let secs = start.elapsed().as_secs_f64();
                            ui.label(
                                egui::RichText::new(format!("  {secs:.1}s"))
                                    .color(egui::Color32::GRAY)
                                    .size(12.0),
                            );
                        }
                    }
                });
                let display_text = match &self.state {
                    PipelineState::Recording { streaming_text, .. } => {
                        if !streaming_text.is_empty() {
                            streaming_text.as_str()
                        } else {
                            self.partial_text.as_str()
                        }
                    }
                    PipelineState::Inserting { text } => text.as_str(),
                    PipelineState::Error { message, .. } => message.as_str(),
                    _ => "",
                };
                if !display_text.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(display_text)
                            .color(egui::Color32::WHITE)
                            .size(13.0),
                    );
                }
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

pub fn run_overlay(
    event_rx: crossbeam_channel::Receiver<PipelineEvent>,
) -> anyhow::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_transparent(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_inner_size(egui::vec2(400.0, 60.0));
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Fluidity Overlay",
        options,
        Box::new(|_cc| Box::new(FluidityApp::new(event_rx))),
    );
    Ok(())
}
