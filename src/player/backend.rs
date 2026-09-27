use anyhow::Result;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_playback::audio_backend;
use librespot_playback::config::PlayerConfig;
use librespot_playback::player::Player;
use std::sync::Arc;
use std::time::Duration;

pub struct PlayerBackend {
    #[allow(dead_code)]
    pub session: Session,
    pub player: Arc<Player>,
    #[allow(dead_code)]
    pub device_id: String,
    #[allow(dead_code)]
    pub is_spirc_active: bool,
    #[allow(dead_code)]
    pub rt: tokio::runtime::Handle,
    pub spirc: Option<librespot_connect::Spirc>,
    pub mixer: std::sync::Arc<dyn librespot_playback::mixer::Mixer>,
}

impl PlayerBackend {
    pub async fn connect(token: &str, username: &str, config: &crate::config::AppConfig, state: Arc<std::sync::Mutex<crate::app::AppState>>) -> Result<Self> {
        let credentials = if let (Some(u), Some(p)) = (&config.spotify_username, &config.spotify_password) {
            println!("Utilizzo username e password per librespot (Login4)...");
            librespot_core::authentication::Credentials::with_password(u, p)
        } else {
            println!("Utilizzo Access Token OAuth per librespot (Spirc bypass)...");
            librespot_core::authentication::Credentials {
                username: Some(username.to_string()),
                auth_type: librespot_protocol::authentication::AuthenticationType::AUTHENTICATION_SPOTIFY_TOKEN,
                auth_data: token.as_bytes().to_vec(),
            }
        };
        
        let session_config = SessionConfig::default();
        let device_id = session_config.device_id.clone();
        let session = Session::new(session_config, None);
        session.connect(credentials.clone(), false).await?;
        let mixer = librespot_playback::mixer::find(Some("softvol"))
            .unwrap()(librespot_playback::mixer::MixerConfig::default()).unwrap();
        
        let volume_getter = mixer.get_soft_volume();
        let mut player_config = PlayerConfig::default();
        player_config.bitrate = librespot_playback::config::Bitrate::Bitrate160;
        player_config.gapless = config.gapless_playback;
        player_config.position_update_interval = Some(Duration::from_millis(250));
        
        let audio_backend = audio_backend::find(None).unwrap();
        
        let player = Player::new(
            player_config,
            session.clone(),
            volume_getter,
            move || audio_backend(None, librespot_playback::config::AudioFormat::default()),
        );

        let mut event_channel = player.get_player_event_channel();
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(event) = event_channel.recv().await {
                match event {
                    librespot_playback::player::PlayerEvent::Playing { position_ms, .. } |
                    librespot_playback::player::PlayerEvent::Paused { position_ms, .. } |
                    librespot_playback::player::PlayerEvent::Loading { position_ms, .. } => {
                        println!("PLAYER_EVENT Playing/Paused/Loading: position_ms={}", position_ms);
                        if let Ok(mut st) = state_clone.lock() {
                            st.progress_ms = position_ms as u64;
                        }
                    },
                    librespot_playback::player::PlayerEvent::PositionChanged { position_ms, .. } => {
                        println!("PLAYER_EVENT PositionChanged: position_ms={}", position_ms);
                        if let Ok(mut st) = state_clone.lock() {
                            st.progress_ms = position_ms as u64;
                        }
                    },
                    _ => {}
                }
            }
        });

        let mut is_spirc_active = false;
        let mut spirc_handle = None;
        if config.spotify_username.is_none() || config.spotify_password.is_none() {
            is_spirc_active = true;
            println!("Registrazione del dispositivo come Spotify Connect (Spirc) per sbloccare l'OAuth...");
            let connect_config = librespot_connect::ConnectConfig {
                name: "SpotLight Player".to_string(),
                device_type: librespot_core::config::DeviceType::Computer,
                initial_volume: 65535, // Massimo default
                ..Default::default()
            };
            let creds_for_spirc = credentials.clone();
            let session_clone = session.clone();
            let player_clone = player.clone();
            
            if let Ok((_spirc, spirc_task)) = librespot_connect::Spirc::new(
                connect_config,
                session_clone,
                creds_for_spirc,
                player_clone,
                mixer.clone(),
            ).await {
                tokio::spawn(spirc_task);
                spirc_handle = Some(_spirc);
            }
        }
        
        let rt = tokio::runtime::Handle::current();
        Ok(Self { session, player, device_id, is_spirc_active, rt, spirc: spirc_handle, mixer })
    }

    pub fn play_track(&self, uri: &str, _token: &str) {
        if let Some(spirc) = &self.spirc {
            let request = librespot_connect::LoadRequest::from_tracks(
                vec![uri.to_string()],
                librespot_connect::LoadRequestOptions {
                    start_playing: true,
                    ..Default::default()
                }
            );
            let _ = spirc.load(request);
        } else {
            if let Ok(track_id) = librespot_core::SpotifyUri::from_uri(uri) {
                self.player.load(track_id, true, 0);
            }
        }
    }

    pub fn pause(&self, _token: &str) {
        if let Some(spirc) = &self.spirc {
            let _ = spirc.pause();
        } else {
            self.player.pause();
        }
    }

    pub fn preload_track(&self, uri: &str) {
        if let Ok(track_id) = librespot_core::SpotifyUri::from_uri(uri) {
            self.player.preload(track_id);
        }
    }

    pub fn play(&self, _token: &str) {
        if let Some(spirc) = &self.spirc {
            let _ = spirc.play();
        } else {
            self.player.play();
        }
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub fn set_volume(&self, volume: u16) {
        if let Some(spirc) = &self.spirc {
            let _ = spirc.set_volume(volume);
        } else {
            self.mixer.set_volume(volume);
        }
    }

    pub fn shutdown(&self) {
        if let Some(spirc) = &self.spirc {
            let _ = spirc.shutdown();
        }
        self.player.stop();
    }
}
