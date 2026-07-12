# FluidVoice — Cross-Platform Rebuild Plan (Rust)

## Target: Windows (first) → Linux | Language: Rust

---

## 1. Project Structure

```
fluidity/
├── Cargo.toml                     # Workspace root
├── crates/
│   ├── fluidity-core/             # Platform-independent core library
│   ├── fluidity-platform/         # Platform backend (resolves per OS)
│   ├── fluidity-ui/               # Overlay window + tray (egui)
│   └── fluidity-app/              # Binary entry point + wiring
├── models/                        # Whisper model download script
└── docs/
```

### Dependency Map

```
fluidity-app
  ├── fluidity-core     (ASR, pipeline, post-processing, config, LLM client)
  ├── fluidity-platform (audio capture, hotkeys, text insertion — resolved per-OS)
  └── fluidity-ui       (egui overlay window, tray icon)
```

---

## 2. Crate-by-Crate Architecture

### 2.1 `fluidity-core`

**Purpose:** Platform-independent business logic. Zero platform-specific dependencies.

| Module | Responsibility |
|--------|---------------|
| `audio` | `AudioCapture` trait (trait to be impl'd by platform crate). `RingBuffer` — lock-free SPSC float buffer. `Resampler` — any sample rate → 16kHz mono |
| `asr` | `AsrEngine` trait. Whisper backend via `whisper-rs`. Model management (download from HuggingFace, cache, progress). Streaming vs final transcription |
| `pipeline` | Recording state machine. Orchestrates: hotkey event → capture start → audio accumulation → streaming transcription → stop → final transcription → post-process → insert |
| `processing` | Filler word removal, custom dictionary replacement, spoken punctuation → symbols (same logic as FluidVoice) |
| `llm` | OpenAI-compatible HTTP client for AI enhancement. Provider config (base URL, API key, model). Prompt profile management |
| `config` | Settings store (XDG on Linux, %APPDATA% on Windows). API key storage via `keyring` crate |
| `hotkey` | `HotkeyListener` trait — just the trait definition |
| `typing` | `TextInserter` trait — just the trait definition |
| `state` | Enum: `Idle → Recording(accumulated_audio) → Transcribing → Enhancing → Inserting → Idle` |

**Key Crates** (core):
- `whisper-rs` — Whisper.cpp bindings
- `serde` / `serde_json` / `toml` — Config
- `keyring` — OS credential storage
- `reqwest` — HTTP client for LLM + model downloads
- `tracing` — Structured logging
- `tokio` — Async runtime (for model downloading, LLM calls)

### 2.2 `fluidity-platform`

**Purpose:** Per-OS implementation of audio, hotkey, and typing traits.

Uses `#[cfg(target_os = "windows")]` / `#[cfg(target_os = "linux")]` to select implementations. Each has its own sub-module.

#### Windows Backend

| Trait | Implementation | Crate/API |
|-------|---------------|-----------|
| `AudioCapture` | `cpal` with WASAPI backend. Device enumeration, stream config, callback → push samples into RingBuffer | `cpal` (with WASAPI feature) |
| `HotkeyListener` | `SetWindowsHookEx(WH_KEYBOARD_LL)` for low-level global hook. OR `RegisterHotKey` for simpler setup. Modifier detection + key up/down | `windows` crate (direct Win32 FFI) or `rdev` |
| `TextInserter` | Primary: `SendInput` with `KEYBDINPUT` for Unicode text. Fallback: clipboard + Ctrl+V. Accessibility: `UI Automation` via IUIAutomation | `windows` crate (winapi) + `enigo` |

**Key considerations for Windows:**
- `SetWindowsHookEx` requires a message pump (can run on a dedicated thread with `GetMessage`)
- `SendInput` works almost everywhere but fails in some elevated/UAC contexts
- UI Automation needs the `windows` crate's `UI Automation` bindings
- Clipboard paste fallback: save/restore clipboard with `OpenClipboard` / `SetClipboardData`

#### Linux Backend (later)

| Trait | Implementation | Crate/API |
|-------|---------------|-----------|
| `AudioCapture` | `cpal` with ALSA/PulseAudio backend | `cpal` |
| `HotkeyListener` | `evdev` (direct input device) OR X11 extension OR `inputbot` crate | `inputbot` or `rdev` |
| `TextInserter` | X11: `XTest` (`enigo`). Wayland: `wtype`/`ydotool` or `wlr-data-control` protocol. Accessibility: `at-spi2` via D-Bus | `enigo` + `zbus` (for at-spi2) |

### 2.3 `fluidity-ui`

**Purpose:** Floating overlay window + system tray icon.

Built with **egui** (immediate-mode GUI) via `eframe` for the overlay and `tray-icon` for the system tray.

#### Overlay Window

A small, always-on-top, transparent, borderless window that appears during recording:

```
┌──────────────────────┐
│ 🔴 ████████░░░░░░░░  │  ← Audio level bar
│ "Hello this is a..." │  ← Live partial transcription
│ [Dictate]            │  ← Mode label
└──────────────────────┘
```

- Positioned near the cursor (at recording start)
- Follows cursor during drag (optional)
- Auto-hides after text insertion completes
- Shows different states: recording, processing (spinner), enhancing (LLM), inserting

**egui advantages:** Cross-platform, small binary, transparent window support, custom rendering, immediate mode makes audio level animation trivial, no HTML/CSS/JS overhead.

**System Tray (`tray-icon` crate):**
- Start/stop recording
- Settings
- History
- Last transcription (copy/paste)
- Quit

### 2.4 `fluidity-app`

**Purpose:** Binary entry point. Wires everything together.

```rust
fn main() {
    // 1. Load config
    // 2. Initialize tracing/logging
    // 3. Start system tray (runs event loop)
    // 4. Initialize hotkey listener (platform-specific thread)
    // 5. Initialize ASR engine (load model)
    // 6. Event loop:
    //    - Hotkey events → Pipeline state machine
    //    - Pipeline state changes → UI updates
    //    - Tray events → Settings/app lifecycle
}
```

Architecture inside `fluidity-app`:

```
                    ┌──────────────────┐
                    │   System Tray    │  ← tray-icon event loop
                    │   (main thread)  │
                    └────────┬─────────┘
                             │ channel
                    ┌────────▼─────────┐
                    │  Pipeline State  │  ← enum driven
                    │    Machine       │
                    └────────┬─────────┘
                             │
          ┌──────────────────┼──────────────────┐
          ▼                  ▼                  ▼
   ┌────────────┐    ┌────────────┐    ┌──────────────┐
   │  Hotkey    │    │   Audio    │    │  ASR Engine  │
   │  Listener  │    │  Capture   │    │  (Whisper)   │
   │ (thread)   │    │ (thread)   │    │  (thread)    │
   └────────────┘    └────────────┘    └──────────────┘
```

Communication via `tokio::sync::mpsc` channels and `Arc<Mutex<State>>` for atomic state reads.

---

## 3. Crate Selection Summary

| Concern | Crate | Why |
|---------|-------|-----|
| **Audio capture** | `cpal` | Cross-platform, supports WASAPI/ALSA/PulseAudio. Callback-based stream |
| **ASR** | `whisper-rs` | Mature bindings to whisper.cpp. ggml models. Works everywhere |
| **Global hotkeys** | Windows: `windows` crate (direct) / Linux: `evdev` | Full control over key up/down/modifiers. Or `rdev` for simpler API |
| **Text insertion** | Windows: `windows` crate (`SendInput` + UIA) / Linux: `enigo` + `zbus` | Platform-specific but behind trait |
| **UI overlay** | `egui` + `eframe` | Transparent always-on-top window, immediate mode, small binary |
| **System tray** | `tray-icon` | Cross-platform tray icon + menu |
| **Config** | `serde` + `toml` | Structured config files |
| **Secrets** | `keyring` | Windows Credential Manager / Linux libsecret |
| **HTTP** | `reqwest` | LLM API calls + model downloads |
| **Async runtime** | `tokio` | Model downloads, LLM calls, audio processing offloading |
| **Audio resampling** | `rubato` or `sample` | Convert any sample rate to 16kHz |
| **Logging** | `tracing` + `tracing-subscriber` | Structured, file + console output |
| **CLI args** | `clap` | Command-line options (debug, config path, etc.) |

---

## 4. Data Flow (Detailed)

```
1. HOTKEY DOWN
   Hotkey thread detects key press via platform hook
   → sends StartRecording command over mpsc channel
   → Pipeline state: Idle → Recording
   → UI shows overlay near cursor position

2. RECORDING
   Audio thread: cpal stream callback receives PCM buffer
   → Push samples into RingBuffer (lock-free SPSC)
   → Resample to 16kHz mono if needed
   → Pipeline periodically (every ~300ms):
        Take chunk from RingBuffer
        Send to whisper‑rs (streaming transcribe)
        Live transcription text → channel → UI overlay

3. HOTKEY UP
   Hotkey thread detects key release
   → sends StopRecording command
   → Pipeline state: Recording → Transcribing
   → Stop audio stream
   → Take full accumulated buffer
   → Send to whisper‑rs (full transcribe, higher quality)
   → Apply post-processing:
        Remove filler words
        Apply custom dictionary
        Convert spoken punctuation
   → Pipeline state: Transcribing → Enhancing (if LLM enabled)
        Send text + system prompt to LLM via reqwest
        Wait for response
   → Pipeline state: Enhancing → Inserting
        Call TextInserter.insert(text)
        Restore clipboard after paste if used
   → Pipeline state: Inserting → Idle
   → UI hides overlay

4. IDLE (back to step 1)
```

---

## 5. State Machine

```rust
enum PipelineState {
    Idle,
    Recording {
        accumulated_samples: usize,
        streaming_text: String,
        started_at: Instant,
    },
    Transcribing {
        text: Option<String>,
    },
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
```

---

## 6. Trait Definitions (Core Abstractions)

```rust
// Audio capture — platform impl provides the stream
trait AudioCapture: Send {
    fn start(&mut self, device_id: Option<&str>) -> Result<(), AudioError>;
    fn stop(&mut self) -> Result<(), AudioError>;
    fn sample_rate(&self) -> u32;
    fn on_data(&mut self, callback: Box<dyn FnMut(&[f32]) + Send>);
}

// Hotkey listener — platform impl provides the hook
trait HotkeyListener: Send {
    fn register(&mut self, shortcut: Hotkey) -> Result<(), HotkeyError>;
    fn set_handler(&mut self, handler: Box<dyn Fn(HotkeyEvent) + Send>);
    fn run(&self) -> Result<(), HotkeyError>; // platform event loop
}

// Text insertion — platform impl provides the method
trait TextInserter: Send {
    fn insert(&self, text: &str, target: InsertTarget) -> Result<(), InsertError>;
    fn insert_with_fallback(&self, text: &str) -> Result<(), InsertError>;
}

enum InsertTarget {
    FocusedWindow,
    Pid(u32),
}

// ASR Engine — core provides Whisper impl
trait AsrEngine: Send {
    fn load(&mut self, model_path: &Path) -> Result<(), AsrError>;
    fn transcribe(&self, samples: &[f32]) -> Result<AsrResult, AsrError>;
    fn transcribe_streaming(&self, samples: &[f32]) -> Result<AsrResult, AsrError>;
}
```

---

## 7. Screen Recording & Overlay Considerations

### Floating Overlay Positioning

- On **hotkey press**: get cursor position via platform API
  - Windows: `GetCursorPos`
  - Linux X11: `XQueryPointer`
  - Linux Wayland: limited, may need a fixed position
- Create overlay window at (cursor_x + offset, cursor_y + offset)
- Keep window always-on-top (`WS_EX_TOPMOST` on Windows, `Above` in window managers)
- Transparent background for rounded/freeform shapes

### Overlay Rendering with egui

```rust
// egui setup in eframe:
let win_settings = NativeOptions {
    always_on_top: true,
    transparent: true,
    decorated: false,
    ..Default::default()
};
```

egui supports `Frame::none()` (no decorations), `Window::default_pos()` for positioning, and custom painting for audio level bars and text.

---

## 8. Windows-Specific Implementation Details

### Audio Capture (cpal + WASAPI)

```rust
// cpal stream configuration
let host = cpal::host_from_id(cpal::WASAPI_HOST)?;
let device = host.default_input_device()?;
let config = device.default_input_config()?;
// Convert to desired format, build stream with callback
```

### Hotkeys (SetWindowsHookEx)

Need a dedicated thread with a message pump:

```rust
// On dedicated thread:
unsafe {
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), instance, 0);
    // Message loop:
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
```

Alternative: `RegisterHotKey` is simpler but only detects key combinations (not hold/release fine-grained control). For press-and-hold, `SetWindowsHookEx` is required.

### Text Insertion (SendInput + UIA + Clipboard)

Fallback chain:
1. `SendInput` with Unicode packets (keybd_event) — works in most scenarios
2. UI Automation (`IUIAutomation::GetFocusedElement` → `SetValue` or `TextPattern`)
3. Clipboard + Ctrl+V (with clipboard save/restore)

```rust
// SendInput approach:
let inputs = vec![
    INPUT { type_: INPUT_KEYBOARD, u: .. }, // key down
    INPUT { type_: INPUT_KEYBOARD, u: .. }, // key up
    // One per UTF-16 unit
];
SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>());
```

### Whisper Model

- Download ggml models from HuggingFace (same as FluidVoice)
- Cache in `%LOCALAPPDATA%/fluidity/models/`
- Same model files work on Windows and Linux

---

## 9. Linux Implementation (Future)

### Audio Capture (cpal + ALSA/PulseAudio)
```rust
// Same cpal API, different backend
let host = cpal::host_from_id(cpal::ALSA_HOST)?;
// OR use PulseAudio backend
```

### Hotkeys (evdev or X11)
- X11: `XGrabKey` + `XNextEvent` loop on a separate thread
- Wayland: No global hotkey API. Workaround: `evdev` to read `/dev/input/...` directly (requires `input` group membership or root)

### Text Insertion
- X11: `enigo` crate uses `XTest` extension
- Wayland: No global input injection protocol. Options:
  - `wtype` CLI tool via `std::process::Command`
  - `ydotool` daemon
  - `libei` (new, experimental)
  - Clipboard + paste (Ctrl+V / Shift+Insert)
  - `wlr-data-control` protocol


### Whisper Key

whisper-rs works identically on both platforms (same model file format, same API). This is the biggest advantage of using whisper.cpp.

---

## 10. LLM Integration

Same architecture as FluidVoice — OpenAI-compatible API:

```rust
struct LlmClient {
    base_url: String,
    api_key: String,  // stored in OS keyring
    model: String,
}

impl LlmClient {
    async fn enhance(&self, text: &str, system_prompt: &str) -> Result<String> {
        reqwest::Client::new()
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&ChatRequest {
                model: self.model.clone(),
                messages: vec![
                    Message::system(system_prompt),
                    Message::user(text),
                ],
            })
            .send()
            .await?
            .json::<ChatResponse>()
            .await
            .map(|r| r.choices[0].message.content.clone())
    }
}
```

Built-in provider presets (same as FluidVoice): OpenAI, Anthropic, Groq, Cerebras, Google, OpenRouter, Ollama, LM Studio.

---

## 11. Binary Size Estimate

| Component | Size |
|-----------|------|
| Rust runtime + tokio | ~2 MB (stripped) |
| egui + eframe | ~1.5 MB |
| whisper-rs (statically linked whisper.cpp) | ~5-8 MB |
| cpal + platform audio | ~1 MB |
| Everything else | ~1 MB |
| **Total (without model)** | **~10-15 MB** |
| Whisper model (tiny) | 75 MB (download on first run) |

---

## 12. Development Phases

### Phase 1: Core + Audio + Whisper (Windows)
- `fluidity-core` crate with config, pipeline state machine, ASR trait
- `fluidity-platform` with Windows audio capture via cpal
- whisper-rs integration: load model, transcribe audio file
- CLI-only test tool: "record and transcribe to stdout"

### Phase 2: Hotkeys + Text Insertion (Windows)
- SetWindowsHookEx global hotkey listener
- SendInput text insertion
- End-to-end pipeline: hotkey → record → transcribe → inject text
- No UI yet, just tray icon with minimal menu

### Phase 3: Overlay UI + Tray (Windows)
- egui floating overlay window
- Audio level visualization
- Live streaming transcription display
- System tray icon with menu items
- State transitions visible in overlay

### Phase 4: Polish (Windows)
- Post-processing (dictionary, punctuation, filler words)
- LLM AI enhancement integration
- Settings UI (config file or simple overlay panel)
- Model download & management
- Error handling, recovery, logging

### Phase 5: Linux port
- Add Linux modules behind `#[cfg(target_os = "linux")]`
- cpal with ALSA/PulseAudio
- evdev or X11 hotkeys
- enigo text insertion
- Test across X11 and Wayland

---

## 13. Key Differences from FluidVoice

| Aspect | FluidVoice (macOS) | Fluidity (Windows/Linux) |
|--------|-------------------|-------------------------|
| **Framework** | SwiftUI | egui / eframe |
| **ASR** | FluidAudio (CoreML) | whisper-rs (whisper.cpp) |
| **Audio** | CoreAudio IOProc + AVAudioEngine | cpal + WASAPI/ALSA |
| **Hotkeys** | CGEvent.tap | SetWindowsHookEx / evdev |
| **Text insertion** | CGEvent + AX + Clipboard | SendInput + UIA + Clipboard |
| **VAD** | None (push-to-talk) | None (same, unless desired) |
| **Notch** | DynamicNotchKit (hardware notch) | Floating cursor popup (egui) |
| **Config** | UserDefaults + Keychain | TOML + keyring crate |
| **Analytics** | PostHog | None (privacy-first by default, opt-in later) |
| **Distribution** | Homebrew / direct download | WiX (Windows .msi) / .deb/.AppImage |

The biggest simplification: **no CoreML dependency**. whisper.cpp works identically everywhere, no Apple Silicon required. The overlay is a simple egui window instead of a MacBook-notch-integrated DynamicNotch dependency.
