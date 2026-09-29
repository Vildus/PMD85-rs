//! The "Brigadier" theme system.
//!
//! A [`Theme`] is a small palette of colors (plus a name) that is
//! applied to egui's [`egui::Style`]. Themes are plain data: built-in
//! presets live in code, custom themes are JSON files in the themes
//! directory, and the active theme can be customized live in the
//! settings window and exported.
//!
//! The default theme, *Brigadier*, aims for a retro-futurist
//! socialist control-room feel: deep green-black surfaces, phosphor
//! green accents, amber warnings, hard edges, monospace everything.

use std::path::Path;

use egui::{Color32, CornerRadius, Stroke};
use serde::{Deserialize, Serialize};

/// Name of the default (built-in) theme.
pub const DEFAULT_THEME: &str = "Brigadier";

/// A named color palette.
///
/// Colors are sRGB `[r, g, b, a]`. Kept as plain arrays (not
/// `Color32`) so the struct serializes to readable JSON and can be
/// shared without egui.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Window background (the deepest layer).
    pub bg: [u8; 4],
    /// Panel surfaces (transport bar, status bar).
    pub panel: [u8; 4],
    /// Hairline border around panels and windows.
    pub panel_border: [u8; 4],
    /// Widget surface at rest.
    pub widget: [u8; 4],
    /// Widget surface hovered.
    pub widget_hover: [u8; 4],
    /// Widget surface active/selected.
    pub widget_active: [u8; 4],
    /// Primary text.
    pub text: [u8; 4],
    /// Dimmed text (status captions).
    pub text_weak: [u8; 4],
    /// Accent: phosphor highlight, sliders, focused controls.
    pub accent: [u8; 4],
    /// Accent at rest (selection fills, slider tracks).
    pub accent_dim: [u8; 4],
    /// Warnings.
    pub warn: [u8; 4],
    /// Errors, LEDs, record.
    pub danger: [u8; 4],
    /// CRT bezel around the emulated screen.
    pub screen_bezel: [u8; 4],
}

impl Default for Theme {
    fn default() -> Self {
        Self::brigadier()
    }
}

fn rgba(hex: u32) -> [u8; 4] {
    let [r, g, b, a] = hex.to_be_bytes();
    [r, g, b, a]
}

impl Theme {
    /// The default: green phosphor on green-black, amber warnings.
    pub fn brigadier() -> Self {
        Theme {
            name: DEFAULT_THEME.into(),
            bg: rgba(0x0A100CFF),
            panel: rgba(0x111A13FF),
            panel_border: rgba(0x2A4A34FF),
            widget: rgba(0x16231AFF),
            widget_hover: rgba(0x1E3325FF),
            widget_active: rgba(0x2A4A34FF),
            text: rgba(0xC9E6CEFF),
            text_weak: rgba(0x6E8C78FF),
            accent: rgba(0x40FF7AFF),
            accent_dim: rgba(0x1D5A32FF),
            warn: rgba(0xFFB000FF),
            danger: rgba(0xFF3B30FF),
            screen_bezel: rgba(0x050806FF),
        }
    }

    /// Warm amber terminal.
    pub fn amber() -> Self {
        Theme {
            name: "Amber Terminal".into(),
            bg: rgba(0x140E05FF),
            panel: rgba(0x1D160AFF),
            panel_border: rgba(0x4A3814FF),
            widget: rgba(0x251C0EFF),
            widget_hover: rgba(0x332612FF),
            widget_active: rgba(0x443116FF),
            text: rgba(0xF0D8A8FF),
            text_weak: rgba(0x97815AFF),
            accent: rgba(0xFFB000FF),
            accent_dim: rgba(0x5A4210FF),
            warn: rgba(0xFFD060FF),
            danger: rgba(0xFF5040FF),
            screen_bezel: rgba(0x0C0803FF),
        }
    }

    /// Engineering-blueprint blue with amber instrumentation.
    pub fn blueprint() -> Self {
        Theme {
            name: "Blueprint".into(),
            bg: rgba(0x070E1AFF),
            panel: rgba(0x0C1727FF),
            panel_border: rgba(0x24406AFF),
            widget: rgba(0x101F35FF),
            widget_hover: rgba(0x16294AFF),
            widget_active: rgba(0x1D3760FF),
            text: rgba(0xD6E4F5FF),
            text_weak: rgba(0x7186A8FF),
            accent: rgba(0x5B9DFFFF),
            accent_dim: rgba(0x1E3A66FF),
            warn: rgba(0xFFB000FF),
            danger: rgba(0xFF5040FF),
            screen_bezel: rgba(0x040810FF),
        }
    }

    /// Built-in presets, in menu order.
    pub fn presets() -> Vec<Theme> {
        vec![Self::brigadier(), Self::amber(), Self::blueprint()]
    }

    /// A palette field as an egui color.
    pub fn color(field: &[u8; 4]) -> Color32 {
        Color32::from_rgba_unmultiplied(field[0], field[1], field[2], field[3])
    }

    /// Apply the palette to an egui context (widgets, panels, text).
    /// Dark visuals for both the light and dark egui themes, so a
    /// system theme switch cannot wash the app out.
    pub fn apply(&self, ctx: &egui::Context) {
        let c = |f: &[u8; 4]| Self::color(f);
        let bg = c(&self.bg);
        let panel = c(&self.panel);
        let border = c(&self.panel_border);
        let widget = c(&self.widget);
        let widget_hover = c(&self.widget_hover);
        let widget_active = c(&self.widget_active);
        let text = c(&self.text);
        let accent = c(&self.accent);
        let accent_dim = c(&self.accent_dim);

        ctx.all_styles_mut(|style| {
            let v = &mut style.visuals;
            v.dark_mode = true;
            v.override_text_color = Some(text);
            v.weak_text_color = Some(c(&self.text_weak));
            v.hyperlink_color = accent;
            v.faint_bg_color = Color32::from_rgb(
                panel.r().saturating_add(6),
                panel.g().saturating_add(6),
                panel.b().saturating_add(6),
            );
            v.extreme_bg_color = bg;
            v.code_bg_color = widget;
            v.warn_fg_color = c(&self.warn);
            v.error_fg_color = c(&self.danger);
            v.panel_fill = panel;
            v.window_fill = panel;
            v.window_stroke = Stroke::new(1.0, border);
            v.window_corner_radius = CornerRadius::same(2);
            v.menu_corner_radius = CornerRadius::same(2);
            v.button_frame = true;

            let widget_visuals = |fill: Color32, fg: Color32| WidgetVisuals {
                bg_fill: fill,
                weak_bg_fill: fill,
                bg_stroke: Stroke::new(1.0, border),
                fg_stroke: Stroke::new(1.0, fg),
                corner_radius: CornerRadius::same(2),
                expansion: 0.0,
            };
            v.widgets.noninteractive = widget_visuals(panel, text);
            v.widgets.inactive = widget_visuals(widget, text);
            v.widgets.hovered = widget_visuals(widget_hover, accent);
            v.widgets.active = widget_visuals(widget_active, accent);
            v.widgets.open = widget_visuals(widget_active, text);
            v.selection.bg_fill = accent_dim;
            v.selection.stroke = Stroke::new(1.0, text);
        });
    }

    /// Install the bundled fonts (IBM Plex Mono + Phosphor icons).
    ///
    /// Must be called once per context before the first pass; uses
    /// `ctx.set_fonts`, which wakes up the font atlas on the next pass.
    pub fn install_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "ibm-plex-mono".into(),
            std::sync::Arc::new(egui::FontData::from_static(
                include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
            )),
        );
        fonts.font_data.insert(
            "ibm-plex-mono-semibold".into(),
            std::sync::Arc::new(egui::FontData::from_static(
                include_bytes!("../../assets/fonts/IBMPlexMono-SemiBold.ttf"),
            )),
        );
        // Monospace body font, and the proportional default too: this
        // app is all terminal.
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "ibm-plex-mono".into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "ibm-plex-mono".into());
        // A named family for headings (labels, section titles).
        fonts.families.insert(
            egui::FontFamily::Name("semibold".into()),
            vec!["ibm-plex-mono-semibold".into()],
        );
        // Phosphor icons: regular glyphs mixed into running text, the
        // filled variant selectable via the "phosphor-fill" family.
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::variants::Variant::Regular);
        egui_phosphor::add_font_bytes_as_family(
            &mut fonts,
            "phosphor-fill",
            egui_phosphor::variants::Variant::Fill.font_bytes(),
        );
        ctx.set_fonts(fonts);
    }

    /// Load custom themes (`*.json`) from `dir`, skipping unreadable
    /// files (logged). Built-ins are not duplicated: a custom file
    /// shadowing a preset name is still loaded (it wins by order).
    pub fn load_dir(dir: &Path) -> Vec<Theme> {
        let mut themes = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return themes,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| serde_json::from_str::<Theme>(&text).map_err(|e| e.to_string()))
            {
                Ok(theme) => themes.push(theme),
                Err(e) => log::warn!("cannot load theme {}: {e}", path.display()),
            }
        }
        themes
    }

    /// Serialize to pretty JSON (for the "export theme" action).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Shorthand for the egui widget-visuals struct we rebuild per state.
type WidgetVisuals = egui::style::WidgetVisuals;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_unique_names_and_valid_colors() {
        let presets = Theme::presets();
        let names: Vec<_> = presets.iter().map(|t| t.name.clone()).collect();
        for t in &presets {
            for field in [
                &t.bg, &t.panel, &t.panel_border, &t.widget, &t.widget_hover,
                &t.widget_active, &t.text, &t.text_weak, &t.accent, &t.accent_dim,
                &t.warn, &t.danger, &t.screen_bezel,
            ] {
                assert_eq!(field[3], 255, "{} has translucent color", t.name);
            }
        }
        assert_eq!(names.len(), 3);
        assert!(names.contains(&DEFAULT_THEME.to_string()));
    }

    #[test]
    fn theme_json_round_trip() {
        let theme = Theme::amber();
        let json = theme.to_json();
        let back: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(theme, back);
        // Missing fields fall back to the default palette (serde default).
        let partial: Theme = serde_json::from_str("{\"name\":\"X\"}").unwrap();
        assert_eq!(partial.name, "X");
        assert_eq!(partial.bg, Theme::brigadier().bg);
    }

    #[test]
    fn load_dir_reads_and_skips_bad_files() {
        let dir = std::env::temp_dir().join(format!("pmd85-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("custom.json"), Theme::amber().to_json()).unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        std::fs::write(dir.join("not-a-theme.txt"), "hello").unwrap();
        let themes = Theme::load_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].name, "Amber Terminal");
    }

    #[test]
    fn apply_sets_style_and_fonts_install() {
        let ctx = egui::Context::default();
        Theme::install_fonts(&ctx);
        let theme = Theme::brigadier();
        theme.apply(&ctx);
        let style = ctx.style_of(egui::Theme::Dark);
        assert_eq!(style.visuals.panel_fill, Theme::color(&theme.panel));
        assert_eq!(
            style.visuals.override_text_color,
            Some(Theme::color(&theme.text))
        );
        // Fonts: a pass must be able to lay out text with the new fonts.
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("BRIGADIER 123");
            ui.heading("heading");
        });
        assert!(!output.shapes.is_empty());
        output.drop_without_applying_deltas();
    }
}
