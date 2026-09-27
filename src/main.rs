#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod app;
mod events;
mod api;
mod player;
mod ui;
mod media_controls;

use app::SpotLightApp;
use eframe::egui;

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::new(28.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Name("Subheading".into()), egui::FontId::new(20.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Body, egui::FontId::new(15.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Button, egui::FontId::new(15.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Small, egui::FontId::new(12.0, egui::FontFamily::Proportional)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(16.0, 16.0);
    style.spacing.window_margin = egui::Margin::same(24.0);
    style.spacing.button_padding = egui::vec2(16.0, 10.0);
    let mut visuals = egui::Visuals::dark();
    let bg_color = egui::Color32::from_rgb(15, 23, 42); // slate-900
    let panel_color = egui::Color32::from_rgb(30, 41, 59); // slate-800
    let accent_color = egui::Color32::from_rgb(78, 205, 196); // Cyan
    let text_color = egui::Color32::from_rgb(248, 250, 252); // slate-50
    let text_muted = egui::Color32::from_rgb(148, 163, 184); // slate-400

    visuals.window_fill = bg_color;
    visuals.panel_fill = bg_color;
    visuals.override_text_color = Some(text_color);
    visuals.widgets.noninteractive.bg_fill = panel_color;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text_color);
    
    visuals.widgets.inactive.bg_fill = panel_color;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text_muted);
    visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(51, 65, 85); // slate-700
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, text_color);
    visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);

    visuals.widgets.active.bg_fill = accent_color;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, bg_color);
    visuals.widgets.active.rounding = egui::Rounding::same(8.0);
    
    visuals.selection.bg_fill = accent_color;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent_color);
    
    visuals.window_stroke = egui::Stroke::NONE;
    
    ctx.set_style(style);
    ctx.set_visuals(visuals);
}

#[cfg(target_os = "windows")]
fn ensure_windows_shortcut() {
    std::thread::spawn(|| {
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(appdata) = dirs::data_dir() {
                let programs_dir = appdata.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
                let shortcut_path = programs_dir.join("SpotLight.lnk");
                let exe_str = current_exe.to_string_lossy();
                let sc_str = shortcut_path.to_string_lossy();
                
                let script = format!(
                    "$ws=New-Object -ComObject WScript.Shell;$s=$ws.CreateShortcut('{}');$s.TargetPath='{}';$s.IconLocation='{},0';$s.Save()",
                    sc_str.replace('\'', "''"),
                    exe_str.replace('\'', "''"),
                    exe_str.replace('\'', "''"),
                );
                
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                let _ = std::process::Command::new("powershell")
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-Command")
                    .arg(&script)
                    .creation_flags(CREATE_NO_WINDOW)
                    .status();
            }
        }
    });
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    #[cfg(target_os = "windows")]
    ensure_windows_shortcut();
    
    let config = config::load_config();

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let icon_data = {
        let png_bytes = include_bytes!("../assets/icon.png");
        if let Ok(img) = image::load_from_memory(png_bytes) {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            })
        } else {
            None
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([600.0, 500.0]);

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "SpotLight",
        native_options,
        Box::new(|cc| {
            configure_style(&cc.egui_ctx);
            Ok(Box::new(SpotLightApp::new(cc, config, rt)))
        }),
    )
}
