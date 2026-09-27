use eframe::egui;
use crate::app::{SpotLightApp, View};
use std::sync::Arc;

pub fn draw(ctx: &egui::Context, app: &mut SpotLightApp) {
    let now = std::time::Instant::now();
    let delta = app.last_frame_time.map(|t| now.duration_since(t)).unwrap_or_default();
    app.last_frame_time = Some(now);


    let mut fill_queue_seed = None;
    let mut next_track_to_play = None;
    {
        let (
            key_space, key_right, key_left,
            key_alt_right, key_alt_left,
            key_vol_up, key_vol_down,
        ) = ctx.input(|i| (
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::ArrowRight) && !i.modifiers.alt,
            i.key_pressed(egui::Key::ArrowLeft)  && !i.modifiers.alt,
            i.key_pressed(egui::Key::ArrowRight) && i.modifiers.alt,
            i.key_pressed(egui::Key::ArrowLeft)  && i.modifiers.alt,
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
        ));

        if !ctx.wants_keyboard_input() {
            let mut state = app.state.lock().unwrap();

            if key_space {
                toggle_play_pause(&mut state, app);
            }
            if key_right {
                let new_pos = (state.progress_ms + 5_000).min(state.current_track.as_ref().map(|t| t.duration_ms).unwrap_or(0));
                state.progress_ms = new_pos;
                if let Some(player) = &state.player {
                    player.seek(new_pos.min(u32::MAX as u64) as u32);
                }
            }
            if key_left {
                let new_pos = state.progress_ms.saturating_sub(5_000);
                state.progress_ms = new_pos;
                if let Some(player) = &state.player {
                    player.seek(new_pos.min(u32::MAX as u64) as u32);
                }
            }
            if key_alt_right {
                advance_queue(&mut state, app, 1);
            }
            if key_alt_left {
                advance_queue(&mut state, app, -1);
            }
            if key_vol_up {
                let v = (state.volume + 0.05).min(1.0);
                set_volume(&mut state, app, v);
            }
            if key_vol_down {
                let v = (state.volume - 0.05).max(0.0);
                set_volume(&mut state, app, v);
            }
        }
    }
    {
        let action = {
            let st = app.state.lock().unwrap();
            let val = st.pending_media_action.lock().unwrap().take();
            val
        };
        if let Some(action) = action {
            use crate::media_controls::PendingMediaAction::*;
            let mut state = app.state.lock().unwrap();
            match action {
                Play        => { if !state.is_playing { toggle_play_pause(&mut state, app); } }
                Pause       => { if  state.is_playing { toggle_play_pause(&mut state, app); } }
                Toggle      => { toggle_play_pause(&mut state, app); }
                Next        => { advance_queue(&mut state, app, 1); }
                Previous    => { advance_queue(&mut state, app, -1); }
                SeekForward => {
                    let p = (state.progress_ms + 5_000).min(state.current_track.as_ref().map(|t| t.duration_ms).unwrap_or(0));
                    state.progress_ms = p;
                    if let Some(pl) = &state.player { pl.seek(p as u32); }
                }
                SeekBackward => {
                    let p = state.progress_ms.saturating_sub(5_000);
                    state.progress_ms = p;
                    if let Some(pl) = &state.player { pl.seek(p as u32); }
                }
            }
        }
    }

    {
        let mut state = app.state.lock().unwrap();
        if state.is_playing {
            state.progress_ms += delta.as_millis() as u64;
            if let Some(track) = state.current_track.clone() {
                if track.duration_ms > 0 && state.progress_ms >= track.duration_ms {
                    state.progress_ms = track.duration_ms;
                    state.is_playing = false;

                    if app.config.repeat_mode == 2 {
                        state.progress_ms = 0;
                        state.is_playing = true;
                        next_track_to_play = Some(track.clone());
                    } else {
                        let mut next_idx = state.queue.next_index();
                        if next_idx.is_none() && app.config.repeat_mode == 1 && !state.queue.items().is_empty() {
                            next_idx = Some(0); // Repeat All → wrap
                        }
                        if let Some(ni) = next_idx {
                            next_track_to_play = state.queue.set_current(ni).map(|item| item.track.clone());
                            state.current_track = next_track_to_play.clone();
                            state.progress_ms = 0;
                            state.is_playing = next_track_to_play.is_some();
                            state.preloaded_uri = None;
                        } else if app.config.auto_queue && !state.queue_fill_in_progress {
                            state.queue_fill_in_progress = true;
                            fill_queue_seed = Some(track.clone());
                        }
                    }
                } else if app.config.automix && track.duration_ms > state.progress_ms {
                    let remaining_ms = track.duration_ms - state.progress_ms;
                    let overlap_ms = (app.config.crossfade_seconds.max(1.0) * 1000.0) as u64;
                    if remaining_ms <= overlap_ms {
                        let mut preload_idx = state.queue.next_index();
                        if preload_idx.is_none() && app.config.repeat_mode == 1 && !state.queue.items().is_empty() {
                            preload_idx = Some(0);
                        } else if app.config.repeat_mode == 2 {
                            preload_idx = state.queue.current_index();
                        }
                        if let Some(ni) = preload_idx {
                            let next_uri = state.queue.items().get(ni).map(|item| item.track.uri.clone());
                            if let Some(next_uri) = next_uri {
                                if state.preloaded_uri.as_deref() != Some(next_uri.as_str()) {
                                    if let Some(player) = &state.player {
                                        player.preload_track(&next_uri);
                                        state.preloaded_uri = Some(next_uri);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ctx.request_repaint();
        }
        if !app.needs_onboarding && !state.is_auto_login_attempted && !app.config.client_id.is_empty() {
            state.is_auto_login_attempted = true;
            state.connect_status = Some("Authenticating...".to_string());
            
            let state_clone = Arc::clone(&app.state);
            let ctx_clone = ctx.clone();
            let config_clone = app.config.clone();
            
            app.rt.spawn(async move {
                if let Ok(tokens) = crate::api::auth::try_refresh_tokens(&config_clone).await {
                    handle_login_success(tokens, state_clone, ctx_clone, config_clone).await;
                } else {
                    state_clone.lock().unwrap().connect_status = None;
                    ctx_clone.request_repaint();
                }
            });
        }
    }
    let lyrics_needed = {
        let mut state = app.state.lock().unwrap();
        let info = state.current_track.as_ref().map(|t| (t.uri.clone(), t.name.clone(), t.artist.clone(), t.duration_ms));
        if let Some((uri, name, artist, duration_ms)) = info {
            if state.current_lyrics_uri.as_deref() != Some(uri.as_str()) {
                state.current_lyrics_uri = Some(uri);
                state.lyrics = None;
                state.lyrics_loading = true;
                Some((name, artist, duration_ms))
            } else { None }
        } else { None }
    };
    if let Some((name, artist, duration_ms)) = lyrics_needed {
        let state_cl = Arc::clone(&app.state);
        let ctx_cl = ctx.clone();
        app.rt.spawn(async move {
            let result = crate::api::web::fetch_lyrics(&name, &artist, duration_ms).await;
            let mut st = state_cl.lock().unwrap();
            st.lyrics = result.ok();
            st.lyrics_loading = false;
            ctx_cl.request_repaint();
        });
    }

    if let Some(track) = next_track_to_play {

        let state = app.state.lock().unwrap();
        if let (Some(player), Some(token)) = (state.player.clone(), state.token.clone()) {
            app.rt.spawn_blocking(move || player.play_track(&track.uri, &token));
        }
    }

    if let Some(seed) = fill_queue_seed {
        let state_clone = Arc::clone(&app.state);
        let ctx_clone = ctx.clone();
        let (token, player, seed_tracks) = {
            let state = app.state.lock().unwrap();
            let mut seeds = state
                .queue
                .items()
                .iter()
                .rev()
                .take(5)
                .map(|item| item.track.clone())
                .collect::<Vec<_>>();
            if !seeds.iter().any(|track| track.uri == seed.uri) {
                seeds.insert(0, seed.clone());
            }
            (state.token.clone(), state.player.clone(), seeds)
        };
        let limit = app.config.auto_queue_limit;
        app.rt.spawn(async move {
            let (recommendations, error) = match token.clone() {
                Some(token) => match crate::api::web::fetch_recommendations(&token, &seed_tracks, limit).await {
                    Ok(tracks) => (tracks, None),
                    Err(error) => (Vec::new(), Some(error.to_string())),
                },
                None => (Vec::new(), Some("Token Spotify mancante".to_string())),
            };
            let mut state = state_clone.lock().unwrap();
            state.queue_fill_in_progress = false;
            let added = state.queue.extend_unique(
                recommendations,
                crate::player::queue::QueueSource::Recommended,
            );
            if added > 0 {
                if let Some(next_index) = state.queue.next_index() {
                    if let Some(next) = state.queue.set_current(next_index).map(|item| item.track.clone()) {
                        state.current_track = Some(next.clone());
                        state.progress_ms = 0;
                        state.is_playing = true;
                        if let (Some(player), Some(token)) = (player, token) {
                            player.play_track(&next.uri, &token);
                        }
                    }
                }
            }
            if let Some(error) = error {
                eprintln!("Cannot add similar tracks: {}", error);
                state.connect_status = Some(format!("Similar tracks unavailable: {}", error));
            } else if added == 0 {
                state.connect_status = Some("No similar tracks available".to_string());
            }
            ctx_clone.request_repaint();
        });
    }

    let mut state = app.state.lock().unwrap();

    let bg_color = egui::Color32::from_rgb(24, 24, 24); // Dark gray for main
    let top_color = egui::Color32::from_rgb(32, 32, 32); // Slightly lighter for top bar
    let side_color = egui::Color32::from_rgb(12, 12, 12); // Almost black for sidebar
    let accent_color = egui::Color32::from_rgb(app.config.accent_color[0], app.config.accent_color[1], app.config.accent_color[2]);
    let text_normal = egui::Color32::from_rgb(220, 220, 220); // Light gray text
    let text_muted = egui::Color32::from_rgb(140, 140, 140); // Darker gray text
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = bg_color;
    style.visuals.panel_fill = bg_color;
    style.visuals.widgets.noninteractive.fg_stroke.color = text_normal;
    style.visuals.widgets.inactive.fg_stroke.color = text_normal;
    style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    style.visuals.widgets.active.fg_stroke.color = accent_color;
    ctx.set_style(style);

    if app.needs_onboarding {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg_color)).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(egui::RichText::new("Welcome to SpotLight").size(32.0).strong().color(accent_color));
                ui.add_space(20.0);
                ui.label(egui::RichText::new("To proceed, you must configure your Spotify Developer credentials.").size(16.0).color(text_normal));
                ui.add_space(30.0);
                
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 250.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("1. Go to").size(14.0).color(text_muted));
                            if ui.link(egui::RichText::new("developer.spotify.com/dashboard").size(14.0)).clicked() {
                                let _ = open::that("https://developer.spotify.com/dashboard");
                            }
                            ui.label(egui::RichText::new("and create a new App.").size(14.0).color(text_muted));
                        });
                        
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("2. In the app settings, set the Redirect URI to exactly:").size(14.0).color(text_muted));
                        ui.label(egui::RichText::new("http://127.0.0.1:8888/callback").size(16.0).strong().color(egui::Color32::WHITE));
                        
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("3. Paste your Client ID and Client Secret below.").size(14.0).color(text_muted));
                        
                        ui.add_space(30.0);
                        
                        ui.label(egui::RichText::new("Client ID").size(14.0).color(text_normal));
                        let mut client_id = app.onboarding_client_id.clone();
                        let id_response = ui.add_sized(egui::vec2(400.0, 30.0), egui::TextEdit::singleline(&mut client_id));
                        id_response.context_menu(|ui| {
                            if ui.button("Paste").clicked() {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    if let Ok(text) = cb.get_text() {
                                        client_id = text;
                                    }
                                }
                                ui.close_menu();
                            }
                        });
                        if id_response.changed() || client_id != app.onboarding_client_id {
                            app.onboarding_client_id = client_id;
                        }
                        
                        ui.add_space(16.0);
                        
                        ui.label(egui::RichText::new("Client Secret").size(14.0).color(text_normal));
                        let mut client_secret = app.onboarding_client_secret.clone();
                        let secret_response = ui.add_sized(egui::vec2(400.0, 30.0), egui::TextEdit::singleline(&mut client_secret).password(true));
                        secret_response.context_menu(|ui| {
                            if ui.button("Paste").clicked() {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    if let Ok(text) = cb.get_text() {
                                        client_secret = text;
                                    }
                                }
                                ui.close_menu();
                            }
                        });
                        if secret_response.changed() || client_secret != app.onboarding_client_secret {
                            app.onboarding_client_secret = client_secret;
                        }
                        
                        ui.add_space(32.0);
                        
                        let can_save = !app.onboarding_client_id.is_empty() && !app.onboarding_client_secret.is_empty();
                        
                        if ui.add_enabled(can_save, egui::Button::new(egui::RichText::new("Save & Start").size(16.0).color(bg_color)).fill(accent_color)).clicked() {
                            app.config.client_id = app.onboarding_client_id.clone();
                            app.config.client_secret = app.onboarding_client_secret.clone();
                            crate::config::save_config(&app.config);
                            app.needs_onboarding = false;

                            let state_clone = Arc::clone(&app.state);
                            let ctx_clone = ctx.clone();
                            let config_clone = app.config.clone();
                            app.rt.spawn(async move {
                                state_clone.lock().unwrap().connect_status = Some("Connecting to Spotify...".to_string());
                                ctx_clone.request_repaint();
                                if let Ok(tokens) = crate::api::auth::authenticate(&config_clone).await {
                                    handle_login_success(tokens, state_clone, ctx_clone, config_clone).await;
                                } else {
                                    state_clone.lock().unwrap().connect_status = None;
                                    ctx_clone.request_repaint();
                                }
                            });
                        }
                    });
                });
            });
        });
        return;
    }
    let mut top_frame = egui::Frame::none();
    top_frame.fill = top_color;
    top_frame.inner_margin = egui::Margin::symmetric(8.0, 4.0);
    
    egui::TopBottomPanel::top("top_panel")
        .frame(top_frame)
        .exact_height(32.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let btn_size = egui::vec2(24.0, 24.0);
                if ui.add_sized(btn_size, egui::Button::new("🔍").frame(false)).clicked() {
                    app.current_view = View::Search;
                }
                
                ui.add_space(16.0);
                if ui.add_sized(btn_size, egui::Button::new("⏮").frame(false))
                    .on_hover_text("Previous (or restart if >3s)")
                    .clicked()
                {
                    if state.progress_ms > 3000 || state.queue.current_index().unwrap_or(0) == 0 {
                        state.progress_ms = 0;
                        if let Some(pl) = &state.player { pl.seek(0); }
                    } else if let Some(curr) = state.queue.current_index() {
                        if curr > 0 {
                            if let Some(item) = state.queue.set_current(curr - 1) {
                                let track = item.track.clone();
                                state.current_track = Some(track.clone());
                                state.progress_ms = 0;
                                state.is_playing = true;
                                if let (Some(pl), Some(tok)) = (&state.player, &state.token) {
                                    let (pl, tok) = (Arc::clone(pl), tok.clone());
                                    app.rt.spawn_blocking(move || { pl.play_track(&track.uri, &tok); });
                                }
                            }
                        }
                    }
                }
                
                let is_playing = state.is_playing;
                let play_pause_icon = if is_playing { "⏸" } else { "▶" };
                let play_btn = ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(play_pause_icon).size(16.0)).frame(false));
                if play_btn.clicked() {
                    if let Some(player) = &state.player {
                        if let Some(token) = &state.token {
                            let player_clone = Arc::clone(player);
                            let token_clone = token.clone();
                            let is_p = is_playing;
                            app.rt.spawn_blocking(move || {
                                if is_p { player_clone.pause(&token_clone); }
                                else    { player_clone.play(&token_clone); }
                            });
                            state.is_playing = !is_playing;
                        }
                    }
                }
                
                if ui.add_sized(btn_size, egui::Button::new("⏭").frame(false))
                    .on_hover_text("Next track")
                    .clicked()
                {
                    let ni = state.queue.next_index().or_else(|| {
                        if app.config.repeat_mode == 1 && !state.queue.items().is_empty() { Some(0) } else { None }
                    });
                    if let Some(idx) = ni {
                        if let Some(item) = state.queue.set_current(idx) {
                            let track = item.track.clone();
                            state.current_track = Some(track.clone());
                            state.progress_ms = 0;
                            state.is_playing = true;
                            if let (Some(pl), Some(tok)) = (&state.player, &state.token) {
                                let (pl, tok) = (Arc::clone(pl), tok.clone());
                                app.rt.spawn_blocking(move || { pl.play_track(&track.uri, &tok); });
                            }
                        }
                    }
                }
                
                ui.add_space(16.0);
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut volume = state.volume;
                    let slider = ui.add_sized(egui::vec2(60.0, 20.0), egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false));
                    if slider.changed() {
                        state.volume = volume;
                        app.config.volume = volume;
                        crate::config::save_config(&app.config);
                        if let Some(player) = &state.player {
                            player.set_volume((volume * 65535.0) as u16);
                        }
                    }
                    let vol_icon = if state.volume == 0.0 { "🔇" } else if state.volume < 0.5 { "🔉" } else { "🔊" };
                    if ui.add(egui::Button::new(vol_icon).frame(false)).on_hover_text("Mute / Unmute").clicked() {
                        if state.volume > 0.0 {
                            app.config.saved_volume = state.volume;
                            state.volume = 0.0;
                        } else {
                            state.volume = if app.config.saved_volume > 0.0 { app.config.saved_volume } else { 1.0 };
                        }
                        app.config.volume = state.volume;
                        crate::config::save_config(&app.config);
                        if let Some(player) = &state.player {
                            player.set_volume((state.volume * 65535.0) as u16);
                        }
                    }
                    let (repeat_icon, repeat_tip) = match app.config.repeat_mode {
                        1 => ("🔁", "Repeat All — click for Single"),
                        2 => ("🔂", "Repeat Single — click to Disable"),
                        _ => ("🔁", "Repeat Off — click for All"),
                    };
                    let repeat_color = if app.config.repeat_mode > 0 { accent_color } else { text_muted };
                    if ui.add(egui::Button::new(egui::RichText::new(repeat_icon).color(repeat_color)).frame(false))
                        .on_hover_text(repeat_tip).clicked()
                    {
                        app.config.repeat_mode = (app.config.repeat_mode + 1) % 3;
                        crate::config::save_config(&app.config);
                    }
                    let shuffle_label = if app.config.smart_shuffle { "🔀" } else { "🔀" };
                    let shuffle_color = if app.config.smart_shuffle { accent_color } else { text_muted };
                    if ui.add(egui::Button::new(egui::RichText::new(shuffle_label).color(shuffle_color)).frame(false))
                        .on_hover_text(if app.config.smart_shuffle { "Smart Shuffle active" } else { "Smart Shuffle off" })
                        .clicked()
                    {
                        app.config.smart_shuffle = !app.config.smart_shuffle;
                        if app.config.smart_shuffle {
                            state.queue.smart_shuffle(now.elapsed().as_nanos() as u64);
                        }
                        crate::config::save_config(&app.config);
                    }
                    if app.config.automix {
                        ui.label(egui::RichText::new("⚡").size(14.0).color(accent_color))
                          .on_hover_text("Auto Mix active");
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new("🎤").color(if matches!(app.current_view, View::Lyrics) { accent_color } else { text_muted })).frame(false))
                        .on_hover_text("Synchronized Lyrics")
                        .clicked()
                    {
                        app.current_view = View::Lyrics;
                    }
                    ui.add_space(8.0);

                    let duration_ms = state.current_track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
                    let progress_ms = state.progress_ms;

                    let progress_sec = progress_ms / 1000;
                    let duration_sec = duration_ms / 1000;
                    let progress_str = format!("{}:{:02}", progress_sec / 60, progress_sec % 60);
                    let duration_str = format!("{}:{:02}", duration_sec / 60, duration_sec % 60);

                    ui.label(egui::RichText::new(format!("{}/{}", progress_str, duration_str)).size(12.0).color(text_normal));

                    ui.add_space(8.0);

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let available_w = ui.available_width();
                        if available_w > 0.0 {
                            let fraction = if duration_ms > 0 {
                                (progress_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };

                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(available_w, 8.0),
                                egui::Sense::click_and_drag(),
                            );
                            if duration_ms > 0 && (response.clicked() || response.dragged()) {
                                if let Some(pointer) = response.interact_pointer_pos() {
                                    let fraction = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                    let seek_ms = (fraction * duration_ms as f32) as u64;
                                    state.progress_ms = seek_ms;
                                    if let Some(player) = &state.player {
                                        player.seek(seek_ms.min(u32::MAX as u64) as u32);
                                    }
                                }
                            }
                            ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(50, 50, 50));
                            let mut filled_rect = rect;
                            filled_rect.max.x = rect.min.x + (rect.width() * fraction);
                            ui.painter().rect_filled(filled_rect, 2.0, accent_color);

                            if duration_ms > 0 {
                                let center = egui::pos2(filled_rect.max.x, rect.center().y);
                                ui.painter().circle_filled(center, 5.0, egui::Color32::WHITE);
                            }
                        }
                    });
                });
            });
        });
    let mut side_frame = egui::Frame::none();
    side_frame.fill = side_color;
    side_frame.inner_margin = egui::Margin::symmetric(16.0, 8.0);
    
    egui::SidePanel::left("sidebar_panel")
        .frame(side_frame)
        .resizable(true)
        .exact_width(220.0)
        .show(ctx, |ui| {
            let now_playing_height = 82.0;
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - now_playing_height).max(0.0))
                .show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Library").color(accent_color).size(14.0));
                ui.add_space(4.0);
                        
                        let mut nav_item = |ui: &mut egui::Ui, view: View, text: &str| {
                            let is_selected = app.current_view == view;
                            let color = if is_selected { text_normal } else { text_muted };
                            if ui.add(egui::Button::new(egui::RichText::new(text).color(color).size(13.0)).frame(false)).clicked() {
                                app.current_view = view;
                            }
                        };
                        
                        nav_item(ui, View::Home, "Home");

                        ui.add_space(8.0);
                        nav_item(ui, View::Queue, "🎵 Queue");
                        nav_item(ui, View::Lyrics, "🎤 Lyrics");
                        nav_item(ui, View::Settings, "⚙ Settings");
                        
                        ui.add_space(16.0);
                        
                        ui.label(egui::RichText::new("Playlists").color(accent_color).size(14.0));
                        ui.add_space(4.0);
                        
                        if state.playlists.is_empty() {
                            ui.label(egui::RichText::new("Loading...").italics().color(text_muted));
                        } else {
                            for (id, name) in &state.playlists {
                                let is_selected = matches!(&app.current_view, View::Playlists(pid) if pid == id);
                                let color = if is_selected { text_normal } else { text_muted };
                                
                                let bg = if is_selected { egui::Color32::from_rgb(50, 50, 50) } else { egui::Color32::TRANSPARENT };
                                let btn = egui::Button::new(egui::RichText::new(name).size(13.0).color(color)).fill(bg).frame(is_selected);
                                
                                let is_mock = cfg!(debug_assertions) && app.config.mock_mode;
                                
                                if ui.add_sized(egui::vec2(ui.available_width(), 24.0), btn).clicked() {
                                    app.current_view = View::Playlists(id.clone());
                                    if !state.loaded_playlist_tracks.contains_key(id) {
                                        if let Some(token) = &state.token {
                                            let token_clone = token.clone();
                                            let id_clone = id.clone();
                                            let state_clone = Arc::clone(&app.state);
                                            let ctx_clone = ctx.clone();
                                            app.rt.spawn(async move {
                                                if is_mock {
                                                    let mock_tracks = vec![
                                                        crate::app::Track {
                                                            name: "Mock Playlist Track 1".into(),
                                                            artist: "Mock Artist".into(),
                                                            album: "Mock Album".into(),
                                                            duration_ms: 210000,
                                                            uri: format!("spotify:track:mock_p_t1_{}", id_clone),
                                                        },
                                                    ];
                                                    state_clone.lock().unwrap().loaded_playlist_tracks.insert(id_clone, mock_tracks);
                                                    ctx_clone.request_repaint();
                                                } else {
                                                    if let Ok(tracks) = crate::api::web::fetch_playlist_tracks(&token_clone, &id_clone).await {
                                                        state_clone.lock().unwrap().loaded_playlist_tracks.insert(id_clone, tracks);
                                                        ctx_clone.request_repaint();
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    });

            ui.separator();
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Now Playing").color(accent_color).size(12.0));
            if let Some(track) = &state.current_track {
                ui.add(
                    egui::Label::new(egui::RichText::new(&track.name).color(text_normal).size(13.0))
                        .truncate(),
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(&track.artist).color(text_muted).size(11.0))
                        .truncate(),
                );
            } else {
                ui.label(egui::RichText::new("No track").color(text_muted).size(11.0));
            }
        });

    egui::TopBottomPanel::bottom("bottom_user_panel")
        .frame(egui::Frame::none().fill(bg_color).inner_margin(egui::Margin::symmetric(16.0, 8.0)))
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(name) = &state.user_name {
                    ui.menu_button(egui::RichText::new(name).size(14.0).strong().color(text_normal), |ui| {
                        if ui.button("Logout").clicked() {
                            if let Some(player) = state.player.as_ref() {
                                player.shutdown();
                            }
                            state.token = None;
                            state.user_name = None;
                            state.player = None;
                            state.is_playing = false;
                            state.current_track = None;
                            state.progress_ms = 0;
                            state.queue.clear();
                            state.loaded_playlist_tracks.clear();
                            state.search_results.clear();
                            state.preloaded_uri = None;
                            state.queue_fill_in_progress = false;
                            state.connect_status = None;
                            app.current_view = View::Home;
                        }
                    });
                }
            });
        });
    let mut central_frame = egui::Frame::none();
    central_frame.fill = bg_color;
    central_frame.inner_margin = egui::Margin::symmetric(0.0, 0.0);
    
    egui::CentralPanel::default()
        .frame(central_frame)
        .show(ctx, |ui| {
            
            macro_rules! render_tracks {
                ($id_source:expr, $tracks:expr) => {
                    let header_h = 24.0;
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.allocate_ui_with_layout(egui::vec2(30.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("#").color(text_muted).size(12.0)); });
                        ui.allocate_ui_with_layout(egui::vec2(300.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Title").color(text_muted).size(12.0)); });
                        ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Artist").color(text_muted).size(12.0)); });
                        ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Album").color(text_muted).size(12.0)); });
                        ui.allocate_ui_with_layout(egui::vec2(80.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Length").color(text_muted).size(12.0)); });
                        ui.allocate_ui_with_layout(egui::vec2(100.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Added").color(text_muted).size(12.0)); });
                    });
                    
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

                    let mut track_to_play = None;
                    let row_height = 32.0;

                    egui::ScrollArea::vertical().id_salt($id_source).show_rows(ui, row_height, $tracks.len(), |ui, row_range| {
                        for i in row_range {
                            let track = &$tracks[i];
                            let mut is_playing_this = false;
                            if let Some(current) = &state.current_track {
                                if current.uri == track.uri {
                                    is_playing_this = true;
                                }
                            }
                            
                            let (id, rect) = ui.allocate_space(egui::vec2(ui.available_width(), row_height));
                            let is_hovered = ui.rect_contains_pointer(rect);
                            
                            let mut bg_row_color = egui::Color32::TRANSPARENT;
                            if is_hovered {
                                bg_row_color = egui::Color32::from_rgb(45, 45, 45);
                                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                            }
                            ui.painter().rect_filled(rect, 0.0, bg_row_color);
                            
                            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.add_space(16.0);
                                    let row_text_color = if is_playing_this { accent_color } else { text_normal };
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(30.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        if is_playing_this {
                                            ui.label(egui::RichText::new("▶").color(accent_color).size(12.0));
                                        } else {
                                            ui.label(egui::RichText::new(format!("{}", i + 1)).color(text_muted).size(13.0));
                                        }
                                    });
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(300.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.add(egui::Label::new(egui::RichText::new(&track.name).color(row_text_color).size(13.0)).truncate());
                                    });
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(200.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.add(egui::Label::new(egui::RichText::new(&track.artist).color(text_muted).size(13.0)).truncate());
                                    });
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(200.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.add(egui::Label::new(egui::RichText::new(&track.album).color(text_muted).size(13.0)).truncate());
                                    });
                                    
                                    let duration_sec = track.duration_ms / 1000;
                                    let duration_str = format!("{}:{:02}", duration_sec / 60, duration_sec % 60);
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(80.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(duration_str).color(text_muted).size(13.0));
                                    });
                                    
                                    ui.allocate_ui_with_layout(egui::vec2(100.0, row_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new("1 day ago").color(text_muted).size(13.0));
                                    });
                                });
                            });
                            
                            let response = ui.interact(rect, id, egui::Sense::click());
                            if response.clicked() {
                                track_to_play = Some(track.clone());
                            }
                        }
                    });
                    
                    if let Some(track) = track_to_play {
                        let tracks_for_queue = $tracks.to_vec();
                        replace_queue_with_tracks(&mut state, &tracks_for_queue, &track);
                        state.current_track = Some(track.clone());
                        state.is_playing = true;
                        state.progress_ms = 0;
                        if let Some(player) = &state.player {
                            if let Some(token) = &state.token {
                                let player_clone = Arc::clone(player);
                                let token_clone = token.clone();
                                let uri_clone = track.uri.clone();
                                app.rt.spawn_blocking(move || {
                                    player_clone.play_track(&uri_clone, &token_clone);
                                });
                            }
                        }
                    }
                }
            }

            match &app.current_view {
                View::Home => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("Home").size(32.0).strong().color(text_normal));
                        });
                        ui.add_space(24.0);
                        
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.vertical(|ui| {
                                if state.token.is_none() {
                                    if ui.add_sized(egui::vec2(200.0, 36.0), egui::Button::new(egui::RichText::new("Login with Spotify").size(14.0).color(bg_color)).fill(accent_color)).clicked() {
                                        let is_loading = if state.connect_status.as_deref() == Some("Connecting...") {
                                            true
                                        } else {
                                            state.connect_status = Some("Connecting...".to_string());
                                            false
                                        };
                                        
                                        if !is_loading {
                                            let state_clone = Arc::clone(&app.state);
                                            let ctx_clone = ctx.clone();
                                            let config_clone = app.config.clone();
                                            
                                            app.rt.spawn(async move {
                                                if let Ok(tokens) = crate::api::auth::authenticate(&config_clone).await {
                                                    handle_login_success(tokens, state_clone, ctx_clone, config_clone).await;
                                                } else {
                                                    state_clone.lock().unwrap().connect_status = None;
                                                    ctx_clone.request_repaint();
                                                }
                                            });
                                        }
                                    }
                                } else {

                                    if state.connect_status.as_deref() != Some("Player Active!") {
                                        if let Some(status) = &state.connect_status {
                                            ui.add_space(8.0);
                                            ui.label(egui::RichText::new(status).strong().color(accent_color));
                                        }
                                    }
                                    
                                    ui.add_space(32.0);
                                    
                                    if !state.top_tracks.is_empty() {
                                        ui.label(egui::RichText::new("Your Top Tracks").size(24.0).strong().color(text_normal));
                                        ui.add_space(16.0);
                                        
                                        let header_h = 24.0;
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(egui::vec2(30.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("#").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(300.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Title").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Artist").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Album").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(80.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Length").color(text_muted).size(12.0)); });
                                        });
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width() - 32.0, 1.0), egui::Sense::hover());
                                        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

                                        let mut track_to_play = None;
                                        for (i, track) in state.top_tracks.iter().enumerate() {
                                            let mut is_playing_this = false;
                                            if let Some(current) = &state.current_track {
                                                if current.uri == track.uri { is_playing_this = true; }
                                            }
                                            let color = if is_playing_this { accent_color } else { text_normal };
                                            
                                            ui.horizontal(|ui| {
                                                let row_rect = ui.min_rect().expand2(egui::vec2(ui.available_width() / 2.0, 16.0));
                                                let response = ui.interact(row_rect, ui.id().with(format!("top_track_{}", i)), egui::Sense::click());
                                                if response.clicked() {
                                                    track_to_play = Some(track.clone());
                                                }
                                                if response.hovered() {
                                                    ui.painter().rect_filled(row_rect, 4.0, egui::Color32::from_rgb(40, 40, 40));
                                                }
                                                
                                                ui.allocate_ui_with_layout(egui::vec2(30.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if is_playing_this {
                                                        ui.label(egui::RichText::new("▶").color(accent_color).size(12.0));
                                                    } else {
                                                        ui.label(egui::RichText::new(format!("{}", i + 1)).color(text_muted).size(12.0));
                                                    }
                                                });
                                                ui.allocate_ui_with_layout(egui::vec2(300.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.name).color(color).size(14.0)); });
                                                ui.allocate_ui_with_layout(egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.artist).color(text_muted).size(13.0)); });
                                                ui.allocate_ui_with_layout(egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.album).color(text_muted).size(13.0)); });
                                                
                                                let duration_sec = track.duration_ms / 1000;
                                                let duration_str = format!("{}:{:02}", duration_sec / 60, duration_sec % 60);
                                                ui.allocate_ui_with_layout(egui::vec2(80.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(duration_str).color(text_muted).size(13.0)); });
                                            });
                                        }
                                        if let Some(track) = track_to_play {
                                            start_queue_with_track(&mut state, track.clone(), crate::player::queue::QueueSource::User);
                                            state.current_track = Some(track.clone());
                                            state.is_playing = true;
                                            state.progress_ms = 0;
                                            if let Some(player) = &state.player {
                                                if let Some(token) = &state.token {
                                                    let player_clone = Arc::clone(player);
                                                    let token_clone = token.clone();
                                                    let uri_clone = track.uri.clone();
                                                    app.rt.spawn_blocking(move || { player_clone.play_track(&uri_clone, &token_clone); });
                                                }
                                            }
                                        }
                                        
                                        ui.add_space(32.0);
                                    }
                                    
                                    if !state.recent_tracks.is_empty() {
                                        ui.label(egui::RichText::new("Recently played").size(24.0).strong().color(text_normal));
                                        ui.add_space(16.0);
                                        
                                        let header_h = 24.0;
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(egui::vec2(30.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("#").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(300.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Title").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Artist").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(200.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Album").color(text_muted).size(12.0)); });
                                            ui.allocate_ui_with_layout(egui::vec2(80.0, header_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new("Length").color(text_muted).size(12.0)); });
                                        });
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width() - 32.0, 1.0), egui::Sense::hover());
                                        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

                                        let mut track_to_play = None;
                                        for (i, track) in state.recent_tracks.iter().enumerate() {
                                            let mut is_playing_this = false;
                                            if let Some(current) = &state.current_track {
                                                if current.uri == track.uri { is_playing_this = true; }
                                            }
                                            let color = if is_playing_this { accent_color } else { text_normal };
                                            
                                            ui.horizontal(|ui| {
                                                let row_rect = ui.min_rect().expand2(egui::vec2(ui.available_width() / 2.0, 16.0));
                                                let response = ui.interact(row_rect, ui.id().with(format!("recent_track_{}", i)), egui::Sense::click());
                                                if response.clicked() {
                                                    track_to_play = Some(track.clone());
                                                }
                                                if response.hovered() {
                                                    ui.painter().rect_filled(row_rect, 4.0, egui::Color32::from_rgb(40, 40, 40));
                                                }
                                                
                                                ui.allocate_ui_with_layout(egui::vec2(30.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if is_playing_this {
                                                        ui.label(egui::RichText::new("▶").color(accent_color).size(12.0));
                                                    } else {
                                                        ui.label(egui::RichText::new(format!("{}", i + 1)).color(text_muted).size(12.0));
                                                    }
                                                });
                                                ui.allocate_ui_with_layout(egui::vec2(300.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.name).color(color).size(14.0)); });
                                                ui.allocate_ui_with_layout(egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.artist).color(text_muted).size(13.0)); });
                                                ui.allocate_ui_with_layout(egui::vec2(200.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(&track.album).color(text_muted).size(13.0)); });
                                                
                                                let duration_sec = track.duration_ms / 1000;
                                                let duration_str = format!("{}:{:02}", duration_sec / 60, duration_sec % 60);
                                                ui.allocate_ui_with_layout(egui::vec2(80.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.label(egui::RichText::new(duration_str).color(text_muted).size(13.0)); });
                                            });
                                        }
                                        if let Some(track) = track_to_play {
                                            start_queue_with_track(&mut state, track.clone(), crate::player::queue::QueueSource::User);
                                            state.current_track = Some(track.clone());
                                            state.is_playing = true;
                                            state.progress_ms = 0;
                                            if let Some(player) = &state.player {
                                                if let Some(token) = &state.token {
                                                    let player_clone = Arc::clone(player);
                                                    let token_clone = token.clone();
                                                    let uri_clone = track.uri.clone();
                                                    app.rt.spawn_blocking(move || { player_clone.play_track(&uri_clone, &token_clone); });
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    });
                }
                View::Search => {
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        
                        let mut search_query = state.search_query.clone();
                        let search_response = ui.add(
                            egui::TextEdit::singleline(&mut search_query)
                                .hint_text("Search tracks...")
                                .desired_width(320.0)
                        );
                        if search_response.changed() {
                            state.search_query = search_query.clone();
                        }
                        
                        let btn_clicked = ui.add_sized(egui::vec2(80.0, 24.0), egui::Button::new("Search").fill(accent_color)).clicked();
                        let enter_pressed = search_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        
                        if btn_clicked || enter_pressed {
                            if let Some(token) = &state.token {
                                let token_clone = token.clone();
                                let query = search_query.clone();
                                let state_clone = Arc::clone(&app.state);
                                let ctx_clone = ctx.clone();
                                app.rt.spawn(async move {
                                    if let Ok(tracks) = crate::api::web::fetch_search(&token_clone, &query).await {
                                        state_clone.lock().unwrap().search_results = tracks;
                                        ctx_clone.request_repaint();
                                    }
                                });
                            }
                        }
                    });
                    
                    if state.search_results.is_empty() {
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new("No results or type to search.").size(14.0).color(text_muted));
                        });
                    } else {
                        ui.add_space(16.0);
                        render_tracks!("search_results_scroll", state.search_results);
                    }
                }

                View::Queue => {
                    let queue_len = state.queue.items().len();
                    let current_idx = state.queue.current_index().unwrap_or(0);
                    let played = if queue_len > 0 { current_idx } else { 0 };
                    let remaining = queue_len.saturating_sub(current_idx.saturating_add(1));

                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(egui::RichText::new("Queue").size(28.0).strong().color(text_normal));
                        ui.add_space(16.0);
                        if ui.small_button("Clear all").clicked() {
                            state.queue.clear();
                        }
                        ui.add_space(16.0);
                        if queue_len > 0 {
                            ui.label(egui::RichText::new(
                                format!("{} in queue  •  {} played", remaining, played)
                            ).size(12.0).color(text_muted));
                        }
                    });
                    ui.add_space(8.0);

                    if state.queue.items().is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("Queue is empty.").color(text_muted));
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.allocate_ui_with_layout(egui::vec2(20.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new("#").color(text_muted).size(11.0));
                            });
                            ui.add_space(4.0);
                            ui.allocate_ui_with_layout(egui::vec2(260.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new("Title").color(text_muted).size(11.0));
                            });
                            ui.allocate_ui_with_layout(egui::vec2(180.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new("Artist").color(text_muted).size(11.0));
                            });
                            ui.allocate_ui_with_layout(egui::vec2(72.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new("Source").color(text_muted).size(11.0));
                            });
                        });
                        let (sep_rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 1.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(sep_rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

                        let mut remove_index = None;
                        let mut play_index = None;
                        let row_h = 34.0;

                        egui::ScrollArea::vertical().show_rows(ui, row_h, state.queue.items().len(), |ui, rows| {
                            for index in rows {
                                let item = &state.queue.items()[index];
                                let is_current = state.queue.current_index() == Some(index);
                                let is_played = state.queue.current_index().map(|c| index < c).unwrap_or(false);

                                let row_text_color = if is_current { accent_color } else if is_played { text_muted } else { text_normal };

                                let (row_id, row_rect) = ui.allocate_space(egui::vec2(ui.available_width(), row_h));
                                let row_resp = ui.interact(row_rect, row_id, egui::Sense::click());
                                let bg = if is_current {
                                    egui::Color32::from_rgb(50, 40, 20)
                                } else if row_resp.hovered() {
                                    egui::Color32::from_rgb(40, 40, 40)
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                ui.painter().rect_filled(row_rect, 2.0, bg);

                                if row_resp.double_clicked() {
                                    play_index = Some(index);
                                }
                                if row_resp.hovered() {
                                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                                }

                                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row_rect), |ui| {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.add_space(24.0);
                                        ui.allocate_ui_with_layout(egui::vec2(20.0, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            if is_current {
                                                ui.label(egui::RichText::new("▶").color(accent_color).size(11.0));
                                            } else {
                                                ui.label(egui::RichText::new(format!("{}", index + 1)).color(text_muted).size(11.0));
                                            }
                                        });
                                        ui.add_space(4.0);
                                        ui.allocate_ui_with_layout(egui::vec2(260.0, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.add(egui::Label::new(egui::RichText::new(&item.track.name).color(row_text_color).size(13.0)).truncate());
                                        });
                                        ui.allocate_ui_with_layout(egui::vec2(180.0, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.add(egui::Label::new(egui::RichText::new(&item.track.artist).color(text_muted).size(12.0)).truncate());
                                        });
                                        let (badge_label, badge_color) = match item.source {
                                            crate::player::queue::QueueSource::Playlist =>
                                                ("Playlist", egui::Color32::from_rgb(59, 130, 200)),
                                            crate::player::queue::QueueSource::Recommended =>
                                                ("Similar", egui::Color32::from_rgb(29, 185, 84)),
                                            crate::player::queue::QueueSource::User =>
                                                ("User", egui::Color32::from_rgb(230, 168, 34)),
                                            crate::player::queue::QueueSource::Search =>
                                                ("Search", egui::Color32::from_rgb(100, 100, 120)),
                                        };
                                        ui.allocate_ui_with_layout(egui::vec2(72.0, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let badge_text = egui::RichText::new(badge_label).size(10.0).color(egui::Color32::WHITE);
                                            let btn = egui::Button::new(badge_text)
                                                .fill(badge_color)
                                                .rounding(6.0);
                                            ui.add(btn);
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.add_space(8.0);
                                            if ui.small_button("×").on_hover_text("Remove from queue").clicked() {
                                                remove_index = Some(index);
                                            }
                                        });
                                    });
                                });
                            }
                        });

                        if let Some(index) = remove_index {
                            state.queue.remove(index);
                        }
                        if let Some(index) = play_index {
                            if let Some(item) = state.queue.set_current(index) {
                                let track = item.track.clone();
                                state.current_track = Some(track.clone());
                                state.progress_ms = 0;
                                state.is_playing = true;
                                if let Some(player) = &state.player {
                                    if let Some(token) = &state.token {
                                        let player_clone = Arc::clone(player);
                                        let token_clone = token.clone();
                                        let uri_clone = track.uri.clone();
                                        app.rt.spawn_blocking(move || {
                                            player_clone.play_track(&uri_clone, &token_clone);
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                View::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("Theme Settings").size(28.0).strong().color(text_normal));
                        });
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label("Accent Color:");
                            ui.add_space(8.0);
                            let mut c = app.config.accent_color;
                            if ui.color_edit_button_srgb(&mut c).changed() {
                                app.config.accent_color = c;
                                crate::config::save_config(&app.config);
                            }
                        });
                        ui.add_space(40.0);

                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("Playback Settings").size(28.0).strong().color(text_normal));
                        });
                        ui.add_space(20.0);
                        ui.vertical(|ui| {
                            ui.add_space(24.0);
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.checkbox(&mut app.config.automix, "Auto Mix").changed() {
                                    crate::config::save_config(&app.config);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(48.0);
                                ui.label("Crossfade");
                                let slider = ui.add_enabled(
                                    app.config.automix,
                                    egui::Slider::new(&mut app.config.crossfade_seconds, 1.0..=12.0).suffix(" s"),
                                );
                                if slider.changed() {
                                    crate::config::save_config(&app.config);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.checkbox(&mut app.config.gapless_playback, "Gapless playback").changed() {
                                    crate::config::save_config(&app.config);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.checkbox(&mut app.config.smart_shuffle, "Smart shuffle").changed() {
                                    crate::config::save_config(&app.config);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.checkbox(&mut app.config.auto_queue, "Add similar tracks to end of queue").changed() {
                                    crate::config::save_config(&app.config);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(48.0);
                                ui.label("Similar tracks to add");
                                let mut limit = app.config.auto_queue_limit as i32;
                                if ui.add(egui::Slider::new(&mut limit, 1..=20)).changed() {
                                    app.config.auto_queue_limit = limit as u8;
                                    crate::config::save_config(&app.config);
                                }
                            });

                            if cfg!(debug_assertions) {
                                ui.add_space(20.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    ui.label(egui::RichText::new("Developer Tools").strong().color(egui::Color32::from_rgb(255, 100, 100)));
                                });
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    if ui.checkbox(&mut app.config.mock_mode, "Mock Mode").changed() {
                                        crate::config::save_config(&app.config);
                                        
                                        // Trigger reload
                                        let state_clone = Arc::clone(&app.state);
                                        let ctx_clone = ctx.clone();
                                        let config_clone = app.config.clone();
                                        app.rt.spawn(async move {
                                            if let Ok(tokens) = crate::api::auth::try_refresh_tokens(&config_clone).await {
                                                handle_login_success(tokens, state_clone, ctx_clone, config_clone).await;
                                            } else {
                                                state_clone.lock().unwrap().connect_status = None;
                                                ctx_clone.request_repaint();
                                            }
                                        });
                                    }
                                });
                            }
                        });
                        
                        ui.add_space(40.0);

                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            if ui.add_sized(egui::vec2(200.0, 30.0), egui::Button::new("Reset to Default Settings")).clicked() {
                                let old_id = app.config.client_id.clone();
                                let old_secret = app.config.client_secret.clone();
                                app.config = crate::config::AppConfig::default();
                                app.config.client_id = old_id;
                                app.config.client_secret = old_secret;
                                crate::config::save_config(&app.config);
                            }
                        });
                        
                        ui.add_space(40.0);
                        
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("About").size(28.0).strong().color(text_normal));
                        });
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("SpotLight Version: ").color(text_normal));
                            ui.label(egui::RichText::new("1.0.0").strong().color(accent_color));
                        });
                        ui.add_space(24.0);
                    });
                }
                View::Lyrics => {
                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(egui::RichText::new("🎤 Lyrics").size(28.0).strong().color(text_normal));
                    });
                    ui.add_space(16.0);
                    if let Some(track) = &state.current_track {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new(format!("{} — {}", track.name, track.artist)).size(16.0).color(text_muted));
                        });
                        ui.add_space(24.0);
                        if state.lyrics_loading {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new("Loading lyrics...").italics().color(text_muted));
                            });
                        } else if let Some(lyrics) = &state.lyrics {
                            if lyrics.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    ui.label(egui::RichText::new("Lyrics not available.").color(text_muted));
                                });
                            } else {
                                let progress = state.progress_ms;
                                let dur = state.current_track.as_ref().map(|t| t.duration_ms).unwrap_or(u64::MAX);
                                
                                let mem_id = egui::Id::new("lyrics_last_scroll_index");
                                let last_scrolled: Option<usize> = ui.ctx().memory(|mem| mem.data.get_temp(mem_id).unwrap_or(None));
                                let mut new_scrolled = last_scrolled;

                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        ui.add_space(24.0);
                                        ui.vertical(|ui| {
                                            for i in 0..lyrics.len() {
                                                let (time, ref text) = lyrics[i];
                                                let next_time = lyrics.get(i + 1).map(|l| l.0).unwrap_or(dur);
                                                let is_active = progress >= time && progress < next_time;
                                                
                                                let (color, size) = if is_active {
                                                    (egui::Color32::WHITE, 22.0_f32)
                                                } else {
                                                    (text_muted, 16.0_f32)
                                                };
                                                
                                                ui.add_space(4.0);
                                                let response = ui.label(egui::RichText::new(text).color(color).size(size));
                                                ui.add_space(4.0);
                                                
                                                if is_active && last_scrolled != Some(i) {
                                                    response.scroll_to_me(Some(egui::Align::Center));
                                                    new_scrolled = Some(i);
                                                }
                                            }
                                        });
                                    });
                                });

                                if new_scrolled != last_scrolled {
                                    ui.ctx().memory_mut(|mem| mem.data.insert_temp(mem_id, new_scrolled));
                                }
                            }
                        } else {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new("No lyrics found.").color(text_muted));
                            });
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("No track playing.").color(text_muted));
                        });
                    }
                }
                View::Playlists(id) => {
                    ui.add_space(8.0);
                    if let Some(tracks) = state.loaded_playlist_tracks.get(id) {
                        render_tracks!(id, tracks);
                    } else {
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new("Loading tracks...").italics().color(text_muted));
                        });
                    }
                }
            }
        });
}

    fn start_queue_with_track(
        state: &mut crate::app::AppState,
        track: crate::app::Track,
        source: crate::player::queue::QueueSource,
    ) {
        state.queue.clear();
        state.queue.push(track, source);
        state.queue.set_current(0);
    }

    fn replace_queue_with_tracks(
        state: &mut crate::app::AppState,
        tracks: &[crate::app::Track],
        selected: &crate::app::Track,
    ) {
        state.queue.clear();
        state.queue.extend_unique(
            tracks.iter().cloned(),
            crate::player::queue::QueueSource::Playlist,
        );
        if let Some(index) = state.queue.items().iter().position(|item| item.track.uri == selected.uri) {
            state.queue.set_current(index);
        }
    }

async fn handle_login_success(tokens: crate::api::auth::AuthTokens, state: Arc<std::sync::Mutex<crate::app::AppState>>, ctx: egui::Context, config: crate::config::AppConfig) {
    let web_access_token = tokens.web_token.access_token;
    let play_access_token = tokens.playback_token.access_token;

    {
        let mut s = state.lock().unwrap();
        s.token = Some(web_access_token.clone());
        s.connect_status = Some("Initializing internal player...".to_string());
    }
    ctx.request_repaint();

    if let Ok(name) = crate::api::web::fetch_user_profile(&web_access_token).await {
        state.lock().unwrap().user_name = Some(name.clone());
        ctx.request_repaint();
    }

    let username = crate::api::web::fetch_user_profile(&web_access_token).await.unwrap_or_else(|_| "User".to_string());

    {
        let mut s = state.lock().unwrap();
        s.connect_status = Some("Initializing internal player...".to_string());
    }
    ctx.request_repaint();

    if let Ok(player) = crate::player::backend::PlayerBackend::connect(&play_access_token, &username, &config, state.clone()).await {
        let mut s = state.lock().unwrap();
        let current_vol = s.volume;
        s.player = Some(Arc::new(player));
        s.connect_status = Some("Player Active!".to_string());
        if let Some(p) = &s.player {
            p.set_volume((current_vol * 65535.0) as u16);
        }
        ctx.request_repaint();
    }

    let is_mock = cfg!(debug_assertions) && config.mock_mode;
    
    if is_mock {
        let playlists = vec![
            ("mock_playlist_1".to_string(), "Mock Playlist 1".to_string()),
            ("mock_playlist_2".to_string(), "Mock Playlist 2".to_string()),
        ];
        
        let mock_tracks = vec![
            crate::app::Track {
                name: "Mock Track 1".into(),
                artist: "Mock Artist".into(),
                album: "Mock Album".into(),
                duration_ms: 180000,
                uri: "spotify:track:mock_track_1".into(),
            },
            crate::app::Track {
                name: "Mock Track 2".into(),
                artist: "Mock Artist".into(),
                album: "Mock Album".into(),
                duration_ms: 200000,
                uri: "spotify:track:mock_track_2".into(),
            }
        ];
        
        let mut st = state.lock().unwrap();
        st.playlists = playlists;
        st.top_tracks = mock_tracks.clone();
        st.recent_tracks = mock_tracks.clone();
        st.current_track = Some(mock_tracks[0].clone());
        st.is_playing = true;
        ctx.request_repaint();
    } else {
        match crate::api::web::fetch_playlists(&web_access_token).await {
            Ok(playlists) => {
                state.lock().unwrap().playlists = playlists;
                ctx.request_repaint();
            }
            Err(e) => {
                println!("Failed to fetch playlists: {:?}", e);
            }
        }
        
        if let Ok(top) = crate::api::web::fetch_top_tracks(&web_access_token).await {
            state.lock().unwrap().top_tracks = top;
            ctx.request_repaint();
        }
        
        if let Ok(recent) = crate::api::web::fetch_recent_tracks(&web_access_token).await {
            state.lock().unwrap().recent_tracks = recent;
            ctx.request_repaint();
        }
    }
}
fn toggle_play_pause(state: &mut crate::app::AppState, app: &SpotLightApp) {
    if let (Some(player), Some(token)) = (state.player.clone(), state.token.clone()) {
        let is_playing = state.is_playing;
        let player = Arc::clone(&player);
        app.rt.spawn_blocking(move || {
            if is_playing { player.pause(&token); }
            else          { player.play(&token);  }
        });
        state.is_playing = !is_playing;
    }
}
fn advance_queue(state: &mut crate::app::AppState, app: &SpotLightApp, delta: i32) {
    let new_idx = if delta > 0 {
        state.queue.next_index().or_else(|| {
            if app.config.repeat_mode >= 1 && !state.queue.items().is_empty() { Some(0) } else { None }
        })
    } else {
        state.queue.current_index().and_then(|c| {
            if c == 0 { None } else { Some(c - 1) }
        })
    };

    if let Some(idx) = new_idx {
        if let Some(item) = state.queue.set_current(idx) {
            let track = item.track.clone();
            state.current_track = Some(track.clone());
            state.progress_ms = 0;
            state.is_playing = true;
            state.preloaded_uri = None;
            if let (Some(pl), Some(tok)) = (state.player.clone(), state.token.clone()) {
                let pl = Arc::clone(&pl);
                app.rt.spawn_blocking(move || { pl.play_track(&track.uri, &tok); });
            }
        }
    }
}
fn set_volume(state: &mut crate::app::AppState, _app: &SpotLightApp, v: f32) {
    state.volume = v;
    if let Some(player) = &state.player {
        player.set_volume((v * 65535.0) as u16);
    }
}
