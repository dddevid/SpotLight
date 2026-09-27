use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(skip)]
    pub spotify_username: Option<String>,
    #[serde(skip)]
    pub spotify_password: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub saved_volume: f32,
    pub theme: ThemeConfig,
    #[serde(default)]
    pub repeat_mode: u8,
    #[serde(default)]
    pub automix: bool,
    #[serde(default = "default_crossfade")]
    pub crossfade_seconds: f32,
    #[serde(default)]
    pub gapless_playback: bool,
    #[serde(default)]
    pub smart_shuffle: bool,
    #[serde(default)]
    pub auto_queue: bool,
    #[serde(default = "default_auto_queue_limit")]
    pub auto_queue_limit: u8,
    #[serde(default = "default_accent_color")]
    pub accent_color: [u8; 3],
    #[serde(default)]
    pub mock_mode: bool,
}

fn default_volume() -> f32 { 1.0 }
fn default_crossfade() -> f32 { 5.0 }
fn default_auto_queue_limit() -> u8 { 5 }
fn default_accent_color() -> [u8; 3] { [29, 185, 84] }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub primary_color: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            client_id: "".to_string(),
            client_secret: "".to_string(),
            spotify_username: None,
            spotify_password: None,
            volume: default_volume(),
            saved_volume: 0.0,
            theme: ThemeConfig {
                primary_color: "green".to_string(),
            },
            repeat_mode: 0,
            automix: false,
            crossfade_seconds: default_crossfade(),
            gapless_playback: false,
            smart_shuffle: false,
            auto_queue: false,
            auto_queue_limit: default_auto_queue_limit(),
            accent_color: default_accent_color(),
            mock_mode: false,
        }
    }
}

pub fn get_config_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("spotlight");
    path
}



pub fn load_api_credentials() -> (String, String) {
    let client_id = keyring::Entry::new("SpotLightApp", "SpotifyClientID").and_then(|e| e.get_password()).unwrap_or_default();
    let client_secret = keyring::Entry::new("SpotLightApp", "SpotifyClientSecret").and_then(|e| e.get_password()).unwrap_or_default();
    (client_id, client_secret)
}

pub fn save_api_credentials(client_id: &str, client_secret: &str) {
    if let Ok(e) = keyring::Entry::new("SpotLightApp", "SpotifyClientID") {
        let _ = e.set_password(client_id);
    }
    if let Ok(e) = keyring::Entry::new("SpotLightApp", "SpotifyClientSecret") {
        let _ = e.set_password(client_secret);
    }
}

pub fn load_config() -> AppConfig {
    let mut path = get_config_dir();
    std::fs::create_dir_all(&path).ok();
    path.push("config.toml");

    let mut config = if let Ok(contents) = std::fs::read_to_string(&path) {
        toml::from_str(&contents).unwrap_or_else(|_| AppConfig::default())
    } else {
        AppConfig::default()
    };

    // If client_id or client_secret are not yet in config.toml, check keyring fallback
    if config.client_id.is_empty() || config.client_secret.is_empty() {
        let (k_id, k_secret) = load_api_credentials();
        if config.client_id.is_empty() && !k_id.is_empty() {
            config.client_id = k_id;
        }
        if config.client_secret.is_empty() && !k_secret.is_empty() {
            config.client_secret = k_secret;
        }
    }

    config
}

pub fn save_config(config: &AppConfig) {
    let mut path = get_config_dir();
    std::fs::create_dir_all(&path).ok();
    path.push("config.toml");
    
    // Save to keyring as backup
    if !config.client_id.is_empty() || !config.client_secret.is_empty() {
        save_api_credentials(&config.client_id, &config.client_secret);
    }
    
    if let Ok(toml_str) = toml::to_string_pretty(config) {
        std::fs::write(&path, toml_str).ok();
    }
}

