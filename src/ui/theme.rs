use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Margin, Rounding, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(5, 5, 5);
pub const PANEL_MID: Color32 = Color32::from_rgb(12, 12, 12);
pub const POPUP: Color32 = Color32::from_rgb(16, 16, 16);
pub const PANEL: Color32 = Color32::from_rgb(13, 13, 13);
pub const INK: Color32 = Color32::from_rgb(243, 243, 243);
pub const INK_2: Color32 = Color32::from_rgb(170, 170, 170);
pub const INK_3: Color32 = Color32::from_rgb(121, 121, 121);
pub const INK_4: Color32 = Color32::from_rgb(80, 80, 80);
pub const ON_LIGHT: Color32 = Color32::from_rgb(10, 10, 10);
pub const ACCENT: Color32 = Color32::from_rgb(159, 182, 255);
pub const GOOD: Color32 = Color32::from_rgb(142, 240, 181);
pub const LIVE: Color32 = Color32::from_rgb(61, 220, 132);
pub const AMBER: Color32 = Color32::from_rgb(245, 196, 112);
pub const RED: Color32 = Color32::from_rgb(255, 107, 107);
pub const REC: Color32 = Color32::from_rgb(255, 84, 92);

pub const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

pub const LINE: Color32 = white(23);
pub const LINE_2: Color32 = white(41);
pub const LINE_3: Color32 = white(82);
pub const FILL: Color32 = white(9);
pub const FILL_2: Color32 = white(15);
pub const FILL_3: Color32 = white(26);

static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn reduced_motion() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

fn system_font(name: &str) -> Option<Vec<u8>> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    std::fs::read(format!("{windir}\\Fonts\\{name}")).ok()
}

pub fn install(ctx: &egui::Context, reduced_motion: bool) {
    REDUCED.store(reduced_motion, Ordering::Relaxed);
    let mut fonts = FontDefinitions::default();
    let embedded: [(&str, &'static [u8]); 6] = [
        ("roboto_400_latin", include_bytes!("../../assets/fonts/roboto-latin-400.ttf")),
        ("roboto_400_cyr", include_bytes!("../../assets/fonts/roboto-cyrillic-400.ttf")),
        ("roboto_500_latin", include_bytes!("../../assets/fonts/roboto-latin-500.ttf")),
        ("roboto_500_cyr", include_bytes!("../../assets/fonts/roboto-cyrillic-500.ttf")),
        ("roboto_700_latin", include_bytes!("../../assets/fonts/roboto-latin-700.ttf")),
        ("roboto_700_cyr", include_bytes!("../../assets/fonts/roboto-cyrillic-700.ttf")),
    ];
    for (name, data) in embedded {
        fonts.font_data.insert(name.into(), FontData::from_static(data));
    }
    let mut fallback = Vec::new();
    if let Some(data) = system_font("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), FontData::from_owned(data));
        fallback.push("segoe".to_string());
    }
    if let Some(data) = system_font("seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), FontData::from_owned(data));
        fallback.push("symbols".to_string());
    }
    if let Some(data) = system_font("seguiemj.ttf") {
        fonts.font_data.insert("emoji".into(), FontData::from_owned(data));
        fallback.push("emoji".to_string());
    }
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |weight: &str| -> Vec<String> {
        let mut list = vec![format!("roboto_{weight}_latin"), format!("roboto_{weight}_cyr")];
        list.extend(fallback.iter().cloned());
        list.extend(defaults.iter().cloned());
        list
    };
    let regular = family("400");
    fonts.families.insert(FontFamily::Proportional, regular.clone());
    fonts.families.insert(medium(), family("500"));
    fonts.families.insert(bold(), family("700"));
    let mut icon_list = Vec::new();
    if let Some(data) = system_font("SegoeIcons.ttf").or_else(|| system_font("segmdl2.ttf")) {
        fonts.font_data.insert("icons".into(), FontData::from_owned(data));
        icon_list.push("icons".to_string());
    }
    icon_list.extend(regular.iter().cloned());
    fonts.families.insert(icons(), icon_list);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, medium())),
        (TextStyle::Heading, FontId::new(32.0, bold())),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.interact_size = egui::vec2(40.0, 34.0);
    style.spacing.slider_width = 260.0;
    style.spacing.slider_rail_height = 4.0;
    style.spacing.combo_width = 260.0;
    style.spacing.menu_margin = Margin::same(8.0);
    style.spacing.window_margin = Margin::same(16.0);
    style.spacing.scroll.bar_width = 6.0;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_inner_margin = 6.0;
    style.animation_time = if reduced_motion { 0.12 } else { 0.22 };
    style.interaction.tooltip_delay = 0.25;
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG;
    v.window_fill = POPUP;
    v.extreme_bg_color = Color32::from_rgb(11, 11, 11);
    v.faint_bg_color = FILL;
    v.code_bg_color = FILL_2;
    v.override_text_color = Some(INK);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(159, 182, 255, 120);
    v.selection.stroke = Stroke::NONE;
    v.window_rounding = Rounding::same(18.0);
    v.menu_rounding = Rounding::same(14.0);
    v.window_stroke = Stroke::NONE;
    v.popup_shadow = egui::epaint::Shadow { offset: egui::vec2(0.0, 16.0), blur: 40.0, spread: 0.0, color: Color32::from_black_alpha(170) };
    v.window_shadow = v.popup_shadow;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.slider_trailing_fill = true;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT);
    let round = Rounding::same(12.0);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL_MID;
    w.noninteractive.weak_bg_fill = PANEL_MID;
    w.noninteractive.bg_stroke = Stroke::NONE;
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK_2);
    w.noninteractive.rounding = round;
    w.inactive.bg_fill = Color32::from_rgb(38, 38, 38);
    w.inactive.weak_bg_fill = FILL_2;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.5_f32, INK);
    w.inactive.rounding = round;
    w.inactive.expansion = 0.0;
    w.hovered.bg_fill = Color32::from_rgb(52, 52, 52);
    w.hovered.weak_bg_fill = FILL_3;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.hovered.rounding = round;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = Color32::from_rgb(64, 64, 64);
    w.active.weak_bg_fill = FILL_3;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.active.rounding = round;
    w.active.expansion = 0.0;
    w.open = w.hovered;
    ctx.set_style(style);
}
