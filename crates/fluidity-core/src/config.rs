use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Application configuration stored as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub audio: AudioConfig,
    pub hotkey: HotkeyConfig,
    pub whisper: WhisperConfig,
    pub llm: LlmConfig,
    pub processing: ProcessingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub input_device: Option<String>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub key: Option<String>,
    pub modifiers: Option<String>,
    pub activation_mode: ActivationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivationMode {
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    pub model_name: String,
    pub model_path: Option<String>,
    pub num_threads: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub provider_id: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingConfig {
    pub fillers_enabled: bool,
    pub dictionary: Vec<DictionaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub from: String,
    pub to: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            audio: AudioConfig {
                input_device: None,
                sample_rate: 16000,
            },
            hotkey: HotkeyConfig {
                key: Some("V".to_string()),
                modifiers: Some("Alt".to_string()),
                activation_mode: ActivationMode::Hold,
            },
            whisper: WhisperConfig {
                model_name: "tiny".to_string(),
                model_path: None,
                num_threads: 4,
            },
            llm: LlmConfig {
                enabled: false,
                provider_id: None,
                base_url: None,
                api_key: None,
                model: None,
                system_prompt: None,
            },
            processing: ProcessingConfig {
                fillers_enabled: true,
                dictionary: Vec::new(),
            },
        }
    }
}

impl Config {
    /// Load config from a path, or create default if missing.
    pub fn load(path: &PathBuf) -> Self {
        if path.exists() {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            toml::from_str(&content).unwrap_or_default()
        } else {
            let cfg = Config::default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(content) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(path, content);
            }
            cfg
        }
    }

    /// Default config directory for the platform.
    pub fn config_dir() -> PathBuf {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "fluidity", "fluidity") {
            proj_dirs.config_dir().to_path_buf()
        } else {
            PathBuf::from(".")
        }
    }

    /// Default config file path.
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Directory for model downloads.
    pub fn model_cache_dir() -> PathBuf {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "fluidity", "fluidity") {
            proj_dirs.cache_dir().join("models")
        } else {
            PathBuf::from("./models")
        }
    }
}
