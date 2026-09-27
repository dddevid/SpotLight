
use souvlaki::{MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone)]
pub enum PendingMediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    SeekForward,
    SeekBackward,
}
pub struct MediaControlsManager {
    pub controls: Option<MediaControls>,
    pub pending: Arc<Mutex<Option<PendingMediaAction>>>,
    last_uri: String,
}

impl MediaControlsManager {
    pub fn new_uninit(pending: Arc<Mutex<Option<PendingMediaAction>>>) -> Self {
        Self { controls: None, pending, last_uri: String::new() }
    }
    pub fn init(&mut self, hwnd: Option<*mut std::ffi::c_void>, ctx: eframe::egui::Context) {
        let config = PlatformConfig {
            dbus_name: "com.devid.spotlight",
            display_name: "SpotLight",
            hwnd,
        };

        match MediaControls::new(config) {
            Ok(mut controls) => {
                let pending = Arc::clone(&self.pending);
                let attach_result = controls.attach(move |evt| {
                    use souvlaki::MediaControlEvent::*;
                    let action = match evt {
                        Play     => Some(PendingMediaAction::Play),
                        Pause    => Some(PendingMediaAction::Pause),
                        Toggle   => Some(PendingMediaAction::Toggle),
                        Next     => Some(PendingMediaAction::Next),
                        Previous => Some(PendingMediaAction::Previous),
                        Seek(SeekDirection::Forward)  => Some(PendingMediaAction::SeekForward),
                        Seek(SeekDirection::Backward) => Some(PendingMediaAction::SeekBackward),
                        _ => None,
                    };
                    if let Some(a) = action {
                        if let Ok(mut lock) = pending.lock() {
                            *lock = Some(a);
                        }
                        ctx.request_repaint(); // Wake up the UI immediately!
                    }
                });
                if let Err(e) = attach_result {
                    eprintln!("[SpotLight] SMTC/MPRIS attach error: {e}");
                }
                self.controls = Some(controls);
                println!("[SpotLight] Media controls initialized.");
            }
            Err(e) => {
                eprintln!("[SpotLight] Could not initialize media controls: {e}");
            }
        }
    }
    pub fn update(&mut self, track: &crate::app::Track, is_playing: bool, progress_ms: u64) {
        let controls = match &mut self.controls {
            Some(c) => c,
            None    => return,
        };
        if self.last_uri != track.uri {
            self.last_uri = track.uri.clone();
            let _ = controls.set_metadata(MediaMetadata {
                title:    Some(&track.name),
                artist:   Some(&track.artist),
                album:    Some(&track.album),
                duration: Some(std::time::Duration::from_millis(track.duration_ms)),
                cover_url: None,
            });
        }
        let pos = MediaPosition(std::time::Duration::from_millis(progress_ms));
        let playback = if is_playing {
            MediaPlayback::Playing { progress: Some(pos) }
        } else {
            MediaPlayback::Paused  { progress: Some(pos) }
        };
        let _ = controls.set_playback(playback);
    }
    pub fn set_stopped(&mut self) {
        if let Some(c) = &mut self.controls {
            let _ = c.set_playback(MediaPlayback::Stopped);
        }
    }
}
