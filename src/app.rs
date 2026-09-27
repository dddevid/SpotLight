use crate::config::AppConfig;
use eframe::egui;
use tokio::runtime::Runtime;
use std::sync::{Arc, Mutex};

#[derive(PartialEq, Clone)]
pub enum View {
    Home,
    Search,
    Playlists(String), // Playlist ID
    Queue,
    Lyrics,
    Settings,
}

#[derive(Clone, Default, Debug)]
pub struct Track {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub uri: String,
    pub duration_ms: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub token: Option<String>,
    pub user_name: Option<String>,
    pub playlists: Vec<(String, String)>, // (ID, Name)
    pub loaded_playlist_tracks: std::collections::HashMap<String, Vec<Track>>,
    pub recent_tracks: Vec<Track>,
    pub current_track: Option<Track>,
    pub search_query: String,
    pub search_results: Vec<Track>,
    pub top_tracks: Vec<Track>,
    pub connect_status: Option<String>,
    pub player: Option<std::sync::Arc<crate::player::backend::PlayerBackend>>,
    pub volume: f32,
    pub is_auto_login_attempted: bool,
    pub is_playing: bool,
    pub progress_ms: u64,
    pub queue: crate::player::queue::PlaybackQueue,
    pub queue_fill_in_progress: bool,
    pub preloaded_uri: Option<String>,
    pub lyrics: Option<Vec<(u64, String)>>,
    pub lyrics_loading: bool,
    pub current_lyrics_uri: Option<String>,
    pub pending_media_action: Arc<Mutex<Option<crate::media_controls::PendingMediaAction>>>,
}

pub struct SpotLightApp {
    pub config: AppConfig,
    pub rt: Runtime,
    pub current_view: View,
    pub state: Arc<Mutex<AppState>>,
    pub last_frame_time: Option<std::time::Instant>,
    pub media_controls: crate::media_controls::MediaControlsManager,
    pub media_controls_initialized: bool,
    pub needs_onboarding: bool,
    pub onboarding_client_id: String,
    pub onboarding_client_secret: String,
}

impl SpotLightApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, config: AppConfig, rt: Runtime) -> Self {
        let initial_volume = config.volume;
        let pending = Arc::new(Mutex::new(None));
        let media_controls = crate::media_controls::MediaControlsManager::new_uninit(Arc::clone(&pending));
        let needs_onboarding = config.client_id.is_empty() || config.client_secret.is_empty();
        let onboarding_client_id = config.client_id.clone();
        let onboarding_client_secret = config.client_secret.clone();

        Self {
            config,
            rt,
            current_view: View::Home,
            media_controls,
            media_controls_initialized: false,
            needs_onboarding,
            onboarding_client_id,
            onboarding_client_secret,
            state: Arc::new(Mutex::new(AppState {
                token: None,
                user_name: None,
                playlists: vec![],
                loaded_playlist_tracks: std::collections::HashMap::new(),
                recent_tracks: vec![],
                current_track: None,
                search_query: String::new(),
                search_results: vec![],
                top_tracks: Vec::new(),
                connect_status: None,
                player: None,
                volume: initial_volume,
                is_auto_login_attempted: false,
                is_playing: false,
                progress_ms: 0,
                queue: crate::player::queue::PlaybackQueue::default(),
                queue_fill_in_progress: false,
                preloaded_uri: None,
                lyrics: None,
                lyrics_loading: false,
                current_lyrics_uri: None,
                pending_media_action: pending,
            })),
            last_frame_time: None,
        }
    }
}

impl eframe::App for SpotLightApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.media_controls_initialized {
            self.media_controls_initialized = true;

            #[cfg(target_os = "windows")]
            {
                use raw_window_handle::{RawWindowHandle, HasWindowHandle};
                if let Ok(handle) = frame.window_handle() {
                    if let RawWindowHandle::Win32(h) = handle.as_raw() {
                        let hwnd = h.hwnd.get() as *mut std::ffi::c_void;
                        self.media_controls.init(Some(hwnd), ctx.clone());
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = frame;
                self.media_controls.init(None, ctx.clone());
            }
        }
        {
            let st = self.state.lock().unwrap();
            if let Some(track) = &st.current_track {
                self.media_controls.update(track, st.is_playing, st.progress_ms);
            } else {
                self.media_controls.set_stopped();
            }
        }

        crate::ui::draw(ctx, self);
    }
}

