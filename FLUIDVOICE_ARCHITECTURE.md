# FluidVoice — Architecture & Analysis

> macOS app for local-first voice dictation. Open-source alternative to Wispr Flow.
> Written in Swift/SwiftUI. Targeted for macOS 15+ (Sequoia).

---

## 1. High-Level Architecture

FluidVoice is a **menu bar app** (can hide from dock) that:

1. Listens for a **global hotkey**
2. Captures microphone audio via **CoreAudio** (direct ring-buffer or AVAudioEngine)
3. Runs **on-device ASR** through one of several provider backends (Parakeet TDT via FluidAudio/Apple CoreML, Whisper.cpp, Apple Speech, etc.)
4. Applies **post-processing** (punctuation, custom dictionary, filler-word removal)
5. Optionally sends text through an **AI enhancement** step (user-provided LLM API key — OpenAI, Anthropic, local Ollama, etc.)
6. **Inserts text** into the focused app via CGEvent keyboard simulation, Accessibility API, or clipboard paste

### Data Flow (end-to-end)

```
Hotkey press → Event tap intercepts keystroke
  → ASRService.start()
    → Microphone capture begins (Direct CoreAudio OR AVAudioEngine tap)
    → Audio samples accumulate in ThreadSafeAudioBuffer
    → Streaming transcription runs periodically on background queue
      → Transcribes chunks → publishes `partialTranscription`
    → Notch overlay shows live audio levels + partial text
  → Hotkey release (or toggle)
    → ASRService.stop()
      → Final transcription on full buffer via provider.transcribeFinal()
      → Post-processing: filler removal → custom dictionary → punctuation
      → Optional AI enhancement via LLM API
      → Text insertion via TypingService (CGEvent → Accessibility → Clipboard fallback)
  → Notch overlay dismisses
```

---

## 2. Core Subsystems

### 2.1 Global Hotkey System

**File:** `Services/GlobalHotkeyManager.swift`
**Concept:** Uses macOS `CGEvent.tap` (Quartz Event Services) to intercept keyboard events globally.

- Creates an **event tap** at `.cgAnnotatedSessionEventTap` level
- Intercepts `.keyDown` / `.keyUp` / `.flagsChanged` / mouse events
- Three activation modes:
  - **Hold:** Press-and-hold to record, release to stop
  - **Toggle:** Press to start, press again to stop
  - **Automatic:** Tap (< threshold) to toggle, hold (> threshold) for press-and-hold
- Supports primary dictation hotkey + prompt mode + command mode + rewrite mode
- Modifier-only shortcuts (e.g., just hold Right Option)
- Mouse button shortcuts supported too
- Health-check timer re-enables event tap if macOS disables it (~every 10s)
- Supports **prompt shortcut assignments** — different hotkeys trigger different AI prompts

**Platform-specific:** CGEvent tap. On Linux, use `x11`/`evdev`/`libinput`/`uinput`/DBus. On Windows, use `SetWindowsHookEx` (WH_KEYBOARD_LL) or `RegisterHotKey`.

### 2.2 Audio Capture

Two backends, both use **CoreAudio**:

#### a) Direct CoreAudio (preferred)

**Files:** `CoreAudioCaptureSupport/CoreAudioCaptureSupport.c` + `Services/DirectCoreAudioInput.swift`

- C code: Registers `AudioDeviceIOProc` callback on the input AudioObjectID
- Fixed **SPSC ring buffer** (64 slots, 8192 frames max per slot) — allocation-free on realtime thread
- AudioUnit callback writes float samples interleaved-to-mono into ring
- Swift wrapper (`DirectCoreAudioInput`) drains the ring from a dispatch queue
- Supports any PCM format (float32, int16, int24, int32) — normalizes to float32
- Significantly lower latency than AVAudioEngine (no mixer graph overhead)

#### b) AVAudioEngine (fallback)

- Standard `AVAudioEngine` + `installTap(onBus:...)` on input node
- Required on Intel Macs when direct CoreAudio fails (aggregate/bluetooth devices)
- No AVAudioEngine object stored as @Published property (crash mitigation — stored as `AnyObject?`)

**Audio Processing Pipeline:**
- Raw hardware samples → normalised float32 mono → resampled to **16kHz** → pushed to `ThreadSafeAudioBuffer`
- `AudioCapturePipeline` class handles level calculation (RMS → CGFloat for visualizer)
- Audio level published via Combine `PassthroughSubject<CGFloat, Never>`
- First-audio detection benchmark logging
- Tracks audio session start time for diagnostics

**Platform-specific:** CoreAudio is macOS-only. Linux needs ALSA/PulseAudio/pipewire. Windows needs WASAPI or MMDevice API.

### 2.3 Transcription Providers

**Protocol:** `TranscriptionProvider` (in `Services/TranscriptionProvider.swift`)

Methods:
- `prepare(progressHandler:)` — download/cache model, load into memory
- `transcribe([Float])` — transcribe audio samples at 16kHz
- `transcribeStreaming([Float])` — lightweight pass for live preview
- `transcribeFinal([Float])` — high-quality pass with optional vocabulary rescoring
- `transcribeDictionaryTraining([Float])` — bypasses final output transforms
- `transcribeFile(at:)` — native file transcription (if supported)
- `modelsExistOnDisk()`, `clearCache()`

#### Available Providers:

| Provider | Model | Platform | Notes |
|----------|-------|----------|-------|
| **FluidAudio** (Parakeet TDT v3/v2) | Nvidia Parakeet via CoreML | Apple Silicon only | Default. ~0.6B param. Uses `AsrManager` from `FluidAudio` dependency. Two instances: streaming (no vocab boost) + final (with CTC vocab rescoring) |
| **Parakeet Realtime** | Parakeet real-time variant | Apple Silicon | Lower latency, lower accuracy |
| **Whisper** | whisper.cpp (ggml models) | All (Intel + AS) | Downloads from HuggingFace. tiny/base/small/medium/large. 99 languages |
| **Apple Speech** | SFSpeechRecognizer | All macOS | System API. No model download. Uses system language list |
| **Apple Speech Analyzer** | `SpeechTranscriber` (macOS 26+) | macOS 26+ | Newer Apple API |
| **External CoreML** | Cohere Transcribe 6-bit | Apple Silicon | External CoreML models |
| **Nemotron** | Nemotron offline/streaming | Apple Silicon | Nvidia Nemotron models |

**Transcription Executor:** An `actor` serializes all CoreML operations to prevent concurrent-access crashes. Chained task pattern ensures sequential execution.

**Model Management:**
- Models downloaded from HuggingFace (for Whisper, Parakeet via FluidAudio dependency)
- Cached in app's Caches directory
- Download progress reported via `ModelPreparationProgress` enum phases
- Cancellation support with cache cleanup
- Auto-loading on startup if models exist on disk

### 2.4 Text Insertion (TypingService)

**File:** `Services/TypingService.swift`

Multi-strategy text insertion pipeline, tried in order:

1. **Reliable Paste Mode** (if enabled):
   - Set clipboard text → save restoration snapshot → post Cmd+V to target PID → async clipboard restore
2. **CGEvent Unicode Insertion** (preferred):
   - `CGEvent.keyboardSetUnicodeString()` — posts Unicode text as keyboard events
   - Targets specific PID via `event.postToPid(pid)` 
   - Chunks text into 200-codepoint segments (surrogate-safe)
3. **Accessibility API**:
   - Find focused text element via `AXUIElementCopyAttributeValue`
   - Set `kAXValueAttribute` directly, or use selected-range manipulation
   - Multiple fallback strategies for different app types
4. **Clipboard Paste**:
   - Save clipboard → set text → Cmd+V → async restore after verification
5. **Menu Paste** (AppleScript):
   - `tell application "System Events" to click menu item "Paste"`
6. **Character-by-character** (last resort):
   - Individual CGEvent per character with 1ms delays

**Focus Management:**
- Captures system-focused PID/element before recording
- Restores focus after transcription for PID-targeted text injection
- Uses AX element snapshot + PID polling for verification

**Platform-specific:** CGEvent + AX APIs are macOS-only. Windows: `SendInput` / `UI Automation`. Linux: `xdotool` / `libxdo` / `at-spi2` / `uinput` / `wlr-data-control` (Wayland).

### 2.5 Notch Overlay

**File:** `Services/NotchOverlayManager.swift` + `DynamicNotchKit` dependency

- Uses `DynamicNotchKit` package for MacBook Pro notch-style UI
- Shows during recording: audio level visualizer + live partial transcription
- Different modes: Dictation, Edit, Command, Rewrite
- Command mode has an **expanded notch** with action buttons (accept/retry/copy)
- Bottom overlay variant for non-notch Macs
- Escape key dismisses overlay / cancels recording
- Processing state overlay (during AI post-processing after recording stops)

### 2.6 AI Post-Processing

**File:** `Services/DictationPostProcessingService.swift`

After transcription, text can be sent to an LLM for:
- Punctuation and formatting
- Grammar correction
- Command interpretation
- Rewrite/edit based on user prompt

**Provider Routing:**
- Multiple built-in providers: OpenAI, Anthropic, xAI, Groq, Cerebras, Google, OpenRouter
- Custom providers (Ollama, LM Studio, self-hosted)
- **Private AI** feature — runs selected models locally via PrivateAIIntegrationService
- Apple Intelligence integration (macOS 26+)
- Provider config stored in UserDefaults + Keychain (API keys)
- Verification system: API key fingerprint hashing (SHA-256 of baseURL|apiKey)

**Prompt Profiles:**
- User-defined system prompts
- App-specific prompt bindings (different prompt per application)
- Dictate mode, Edit mode, Command mode, Rewrite mode
- Reasoning config support (e.g., `enable_thinking` for deepseek-reasoner)

### 2.7 Menu Bar

**File:** `Services/MenuBarManager.swift`

- `NSStatusItem` with dynamic icon (recording state, idle state)
- Menu items: toggle recording, copy last transcript, mode switching, preferences
- Observes ASRService state via Combine
- Overlay lifecycle tied to recording state + processing state

### 2.8 Settings & Persistence

**File:** `Persistence/SettingsStore.swift`

- Singleton `UserDefaults`-backed store
- Keychain for API keys (`KeychainService.swift`)
- Transcription history stored in SQLite-like file store
- Audio snapshot capture for history playback
- Hotkey shortcuts persisted as `HotkeyShortcut` models
- Onboarding state machine
- Auto-launch via `SMAppService` (macOS login items)
- Backup/restore service
- Multiple migration helpers for legacy settings

### 2.9 Local API Server

**File:** `Services/LocalAPI/LocalAPIServer.swift`

- HTTP server on loopback (`127.0.0.1`) using `NWListener` (Network framework)
- Provides programmatic access to ASR (transcribe audio, transcribe files)
- REST endpoints for integration with other tools
- Configurable port (default in config)
- Only accepts loopback connections

---

## 3. Detailed Component Breakdown

### 3.1 App Lifecycle

```
fluidApp.swift (SwiftUI @main)
  └── init()
       └── Creates AppServices singleton (services NOT initialized yet)
       └── MenuBarManager created
  └── AppDelegate (NSApplicationDelegateAdaptor)
       └── applicationDidFinishLaunching:
            ├── FileLogger init (crash handler setup)
            ├── Detect login-item launch
            ├── SettingsStore.initializeAppSettings()
            ├── LocalAPIServer.shared.start()
            ├── Analytics bootstrap
            ├── Update checker setup
            └── Main window reveal (with retry)
  └── ContentView.onAppear:
       ├── 1.5s delay (SwiftUI AttributeGraph crash mitigation)
       ├── AppServices.signalUIReady()
       ├── AppServices.initializeServicesIfNeeded()
       │    ├── Lazy-init AudioHardwareObserver
       │    └── Lazy-init ASRService
       ├── ASRService.initialize()
       │    ├── Check mic permission
       │    ├── Register CoreAudio device listeners
       │    ├── Prewarm audio engine
       │    └── Check models on disk → auto-load if present
       └── GlobalHotkeyManager setup with callbacks
```

### 3.2 Recording Lifecycle

```
User presses hotkey
  → GlobalHotkeyManager intercepts key event
  → Calls dictationModeCallback / startRecordingCallback
  → ContentView.beginDictationRecording()
       ├── setActiveRecordingMode(.dictate)
       ├── ASRService.start()
       │    ├── Ensure ASR model ready (ensureAsrReady)
       │    ├── Pause media playback (if setting enabled)
       │    ├── Start preferred audio capture
       │    │    ├── DirectCoreAudio → start() + consumer queue
       │    │    └── OR AVAudioEngine → configure + start + tap
       │    ├── Set isRunning = true
       │    └── Start streaming transcription timer
       │         └── Every N seconds, transcribe accumulated buffer
       │             → publish partialTranscription
       └── MenuBarManager shows recording overlay

Audio flows:
  Hardware → CoreAudio IOProc → SPSC ring buffer
    → DirectCoreAudio consumer queue → AudioCapturePipeline
      → ThreadSafeAudioBuffer.append()
      → Periodic: trim buffer → transcribeStreaming() → partialText
      → Audio level → audioLevelSubject.send()

User releases hotkey
  → GlobalHotkeyManager intercepts key up
  → Calls stopAndProcessCallback
  → ContentView.stopAndProcessTranscription()
       ├── ASRService.stop()
       │    ├── Set isRunning = false
       │    ├── Stop streaming timer + await pending transcription
       │    ├── Stop audio capture (stop DirectCoreAudio / stop engine)
       │    ├── Get full audio buffer
       │    ├── transcribeFinal() on full buffer
       │    ├── Post-processing:
       │    │    ├── removeFillerWords ("um", "uh", etc.)
       │    │    ├── applyCustomDictionary (user-defined replacements)
       │    │    └── applySpokenPunctuationFormatting
       │    ├── Record word-boost hits
       │    └── Return final text
       ├── Optional: DictationPostProcessingService.process()
       │    └── Send text to configured LLM for enhancement
       ├── Optional: RewriteModeService / CommandModeService
       └── TypingService.typeOutputPlanInstantly()
            └── Insert text into focused app
```

### 3.3 Post-Processing Formatting

**File:** `Services/ASRService+DictationLiteralFormatting.swift`, `ASRService+SpokenPunctuationFormatting.swift`

- **Filler word removal:** Regex-based removal of "um", "uh", "like", "you know", etc.
- **Custom dictionary:** User-defined word/phrase replacements (e.g., "op code" → "opcode")
- **Spoken punctuation:** "period" → ".", "comma" → ",", "new line" → "\n", "question mark" → "?", etc.
- **Capitalization:** First word capitalized, proper noun rules
- **GAAV formatting:** Grammar, Articles, Apostrophes, Verbs normalization pass

### 3.4 Meeting Transcription

**File:** `Services/MeetingTranscriptionService.swift`

Separate from real-time dictation:
- Transcribes audio/video files
- Uses the same ASR providers (shares loaded model)
- Long-audio chunking + reassembly for providers that don't support native file transcription
- Progress tracking per chunk

### 3.5 Command Mode

**File:** `Services/CommandModeService.swift`

Voice commands: user speaks a command (e.g., "undo", "select all", "bold that") and it gets:
1. Transcribed via ASR
2. Sent through LLM for command interpretation
3. Executed as system action or text manipulation

### 3.6 Rewrite Mode

**File:** `Services/RewriteModeService.swift`

- Captures selected text via Accessibility API (`TextSelectionService`)
- User speaks rewrite instructions
- Original text + instruction sent to LLM
- Replaced in-place via TypingService

---

## 4. Dependencies

| Package | Purpose |
|---------|---------|
| `FluidAudio` (altic-dev/FluidAudio) | CoreML-based ASR (Parakeet TDT models). Apple Silicon only. Primary transcription backend |
| `SwiftWhisper` (exPHAT/SwiftWhisper) | Whisper.cpp Swift bindings. Intel Mac fallback |
| `DynamicNotchKit` (altic-dev/DynamicNotchKit) | MacBook Pro notch UI overlay |
| `PromiseKit` | Async utilities (update checking) |
| `AppUpdater` | GitHub releases-based auto-update |
| `PostHog` | Product analytics (opt-in) |

---

## 5. Platform-Specific APIs (for cross-platform porting)

| macOS API | Purpose | Windows Replacement | Linux Replacement |
|-----------|---------|--------------------|--------------------|
| `CGEventTap` | Global hotkeys | `SetWindowsHookEx(WH_KEYBOARD_LL)` / `RegisterHotKey` | `x11`/`evdev`/`libinput` |
| `CoreAudio` (IOProc) | Low-latency audio capture | WASAPI / MMDevice | ALSA / PulseAudio / pipewire |
| `AVAudioEngine` | Audio capture fallback | WASAPI | PulseAudio / pipewire |
| `SFSpeechRecognizer` | System speech API | Azure Speech / Windows.Media.SpeechRecognition | N/A (use Whisper) |
| `AXUIElement` | Accessibility text insertion | UI Automation (`IUIAutomation`) | at-spi2 (`dbus`) |
| `CGEvent` (keyboard) | Text injection via keyboard | `SendInput` | `xdotool` / `uinput` / wlr-data-control |
| `NSPasteboard` | Clipboard ops | `Clipboard` API | `xclip` / `wl-clipboard` |
| `NSStatusItem` | Menu bar icon | System tray icon (`NOTIFYICONDATA`) | Tray (`libappindicator`/`ayatana`) |
| `CoreML` | On-device ML inference | ONNX Runtime / DirectML | ONNX Runtime / OpenVINO |
| `Network` (NWListener) | Local HTTP API | `HttpListener` / ASP.NET minimal API | `libmicrohttpd` / UNIX socket |
| `UserDefaults` | Settings persistence | Registry / JSON file | JSON file (XDG config) |
| `Keychain` | Secure API key storage | Credential Manager / DPAPI | `libsecret` / `GNOME Keyring` |
| `SMAppService` | Login item / auto-launch | Registry `Run` key | XDG autostart `.desktop` file |

---

## 6. Speech Model Options (cross-platform)

| Model | Format | Quality | Speed | RAM | Portability |
|-------|--------|---------|-------|-----|-------------|
| **Whisper (cpp)** | GGML | High | Medium | 1-16 GB | Most portable (every platform) |
| **Whisper (ONNX)** | ONNX | High | Medium | 1-16 GB | Good (needs ONNX Runtime) |
| **Whisper (Systran/faster-whisper)** | CTranslate2 | Higher | Faster | 1-16 GB | Good (C++/Python/C# bindings) |
| **Parakeet TDT** | CoreML | High | Fast | ~2 GB | Apple Silicon only |
| **SenseVoice** (FunASR) | ONNX | High | Fast | ~1 GB | Good (Chinese + multilingual) |
| **wav2vec2** | ONNX | Medium | Fast | ~1 GB | Good |
| **Silero VAD + Whisper** | Combined | High | Fast | Varies | Good |

**Recommended for cross-platform:** **Whisper.cpp** (via bindings in Rust/C/Python/C++) — same model everywhere, well-optimized, no GPU required.

---

## 7. Key Design Decisions & Patterns

1. **Actor-based serialization:** `TranscriptionExecutor` actor serializes all CoreML operations to prevent race conditions
2. **Startup crash mitigation:** Multiple layers — lazy services, delayed initialization, `AnyObject?` storage for AVFoundation objects
3. **Thread-safe audio buffer:** `NSLock`-guarded `ThreadSafeAudioBuffer` prevents concurrent read/write between audio callback and transcription threads
4. **Multi-strategy text insertion:** Falls through 5+ methods to handle all target apps (terminals, Electron, native, web)
5. **Focus tracking:** System-wide AX element PID capture before recording ensures text targets the right app even if overlay temporarily steals focus
6. **Model-agnostic architecture:** `TranscriptionProvider` protocol allows swapping backends without changing the app
7. **Clipboard preservation:** Paste-style text insertion saves/restores clipboard contents asynchronously
8. **Health monitoring:** Periodic event-tap health checks + auto-recovery
9. **Analytics:** PostHog integration with privacy controls, benchmark logging throughout

---

## 8. Project Structure (Sources/Fluid/)

```
Sources/
├── Fluid/
│   ├── fluidApp.swift              # SwiftUI @main entry
│   ├── AppDelegate.swift           # NSApplication lifecycle
│   ├── ContentView.swift           # Main UI orchestrator (~3000 lines)
│   ├── Services/
│   │   ├── ASRService.swift        # Central ASR orchestrator (~4000 lines)
│   │   ├── ASRService+DictationLiteralFormatting.swift
│   │   ├── ASRService+SpokenPunctuationFormatting.swift
│   │   ├── TranscriptionProvider.swift  # Protocol + ModelPreparationProgress
│   │   ├── FluidAudioProvider.swift     # Parakeet via CoreML (Apple Silicon)
│   │   ├── WhisperProvider.swift        # Whisper.cpp (Intel/Universal)
│   │   ├── AppleSpeechProvider.swift    # SFSpeechRecognizer
│   │   ├── AppleSpeechAnalyzerProvider.swift
│   │   ├── ParakeetRealtimeProvider.swift
│   │   ├── ParakeetVocabularyStore.swift
│   │   ├── NemotronProvider.swift
│   │   ├── ExternalCoreMLTranscriptionProvider.swift
│   │   ├── DirectCoreAudioInput.swift   # Low-level CoreAudio wrapper
│   │   ├── GlobalHotkeyManager.swift    # CGEvent tap hotkeys
│   │   ├── TypingService.swift          # Text insertion pipeline
│   │   ├── TextSelectionService.swift   # AX selected-text capture
│   │   ├── ClipboardService.swift       # Simple clipboard ops
│   │   ├── DictationPostProcessingService.swift  # LLM enhancement
│   │   ├── DictationAIPostProcessingGate.swift   # Provider verification
│   │   ├── AudioDeviceService.swift     # CoreAudio device management
│   │   ├── ThreadSafeAudioBuffer.swift  # Lock-protected audio buffer
│   │   ├── MenuBarManager.swift         # NSStatusItem + menu
│   │   ├── NotchOverlayManager.swift    # DynamicNotch overlay
│   │   ├── ActiveAppMonitor.swift
│   │   ├── AppServices.swift            # Lazy-init service container
│   │   ├── CommandModeService.swift     # Voice commands
│   │   ├── RewriteModeService.swift     # Select + speak + rewrite
│   │   ├── MeetingTranscriptionService.swift
│   │   ├── MediaPlaybackService.swift   # Auto-pause/resume media
│   │   ├── TranscriptionSoundPlayer.swift
│   │   ├── ModelRepository.swift        # Provider/model configs
│   │   ├── LLMClient.swift              # HTTP client for LLM APIs
│   │   ├── ... (more service files)
│   ├── Models/
│   │   └── HotkeyShortcut.swift
│   ├── Persistence/
│   │   ├── SettingsStore.swift          # Main UserDefaults store
│   │   ├── SettingsStore+*.swift        # Extensions per domain
│   │   ├── KeychainService.swift
│   │   ├── TranscriptionHistoryStore.swift
│   │   └── ...
│   ├── UI/
│   │   ├── RecordingView.swift
│   │   ├── SettingsView.swift
│   │   ├── WelcomeView.swift
│   │   ├── Onboarding...  (multi-step onboarding)
│   │   └── ...
│   ├── Networking/
│   │   ├── AIProvider.swift
│   │   ├── AppleIntelligenceProvider.swift
│   │   ├── FunctionCallingProvider.swift
│   │   └── ModelDownloader.swift
│   ├── Views/ ... (SwiftUI view files)
│   └── Theme/ ... (appearance)
└── CoreAudioCaptureSupport/
    ├── CoreAudioCaptureSupport.c        # C SPSC ring-buffer capture
    └── include/...
```

---

## 9. Summary for Rebuilding

To rebuild for Windows/Linux in a different language, the core pieces needed are:

1. **Hotkey system** — platform-specific key interception
2. **Audio capture** — platform-specific audio API pulling raw PCM
3. **ASR engine** — Whisper.cpp is the most portable choice; load ggml models, feed 16kHz float32 PCM
4. **Text insertion** — platform-specific: `SendInput` (Windows), `at-spi2` (Linux), `uinput` (Linux), or clipboard-based
5. **UI** — System tray + recording overlay window (could be cross-platform with e.g. Tauri/Electron or native per platform)
6. **Settings storage** — TOML/JSON/YAML file (XDG on Linux, AppData on Windows)
7. **Optional LLM enhancement** — HTTP client for OpenAI-compatible APIs (trivial, any language)
8. **VAD (Voice Activity Detection)** — The macOS app does NOT use VAD (it's push-to-talk). Consider Silero VAD if you want hands-free mode
9. **Update system** — GitHub Releases API check (trivial, ~50 lines in any language)
