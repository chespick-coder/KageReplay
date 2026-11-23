use std::collections::VecDeque;
use std::hash::Hash;

use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Id, Layout, Margin, Pos2, Rect, Response, RichText, Rounding, Sense, Shape, Stroke, Ui};
use eframe::epaint::{Mesh, RectShape, Shadow};

use super::theme::{self, FILL, FILL_2, INK, INK_2, INK_3, INK_4, LINE, LINE_2, ON_LIGHT};

pub mod glyph {
    pub const VIDEO: &str = "\u{E714}";
    pub const AUDIO: &str = "\u{E767}";
    pub const REPLAY: &str = "\u{E81C}";
    pub const RECORD: &str = "\u{E7C8}";
    pub const PROFILE: &str = "\u{E77B}";
    pub const CLIPS: &str = "\u{E8B2}";
    pub const SAVE: &str = "\u{E74E}";
    pub const CAMERA: &str = "\u{E722}";
    pub const PLAY: &str = "\u{E768}";
    pub const STOP: &str = "\u{E71A}";
    pub const DELETE: &str = "\u{E74D}";
    pub const FOLDER: &str = "\u{E838}";
    pub const ADD: &str = "\u{E710}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const MONITOR: &str = "\u{E7F4}";
    pub const WINDOW: &str = "\u{E737}";
    pub const MIC: &str = "\u{E720}";
    pub const APP: &str = "\u{E71D}";
    pub const CHECK: &str = "\u{E73E}";
    pub const WARNING: &str = "\u{E7BA}";
    pub const INFO: &str = "\u{E946}";
    pub const BOLT: &str = "\u{E945}";
    pub const CLOSE: &str = "\u{E711}";
    pub const COPY: &str = "\u{E8C8}";
    pub const EDIT: &str = "\u{E70F}";
    pub const STAR: &str = "\u{E734}";
    pub const POWER: &str = "\u{E7E8}";
    pub const OPEN: &str = "\u{E8A7}";
    pub const FILTER: &str = "\u{E9E9}";
    pub const BROOM: &str = "\u{EA99}";
    pub const DOWNLOAD: &str = "\u{E896}";
}

pub fn animate_frame(ctx: &egui::Context) {
    ctx.request_repaint_after(std::time::Duration::from_millis(15));
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn dur(seconds: f32) -> f32 {
    if theme::reduced_motion() {
        seconds * 0.6
    } else {
        seconds
    }
}

pub fn dist(pixels: f32) -> f32 {
    if theme::reduced_motion() {
        pixels * 0.4
    } else {
        pixels
    }
}

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    duration: f32,
}

impl Tween {
    fn value(&self, now: f64) -> f32 {
        let t = ((now - self.start) as f32 / self.duration.max(0.001)).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * ease(t)
    }
}

pub fn tween(ctx: &egui::Context, id: Id, target: f32, seconds: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let duration = dur(seconds);
    let (value, running) = ctx.data_mut(|d| {
        let t = d.get_temp_mut_or_insert_with(id, || Tween { from: target, to: target, start: now - 100.0, duration });
        if (t.to - target).abs() > f32::EPSILON {
            t.from = t.value(now);
            t.to = target;
            t.start = now;
            t.duration = duration;
        }
        (t.value(now), now - t.start < t.duration as f64)
    });
    if running {
        animate_frame(ctx);
    }
    value
}

pub fn tween_bool(ctx: &egui::Context, id: Id, on: bool, seconds: f32) -> f32 {
    tween(ctx, id, if on { 1.0 } else { 0.0 }, seconds)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn alpha(color: Color32, factor: f32) -> Color32 {
    color.gamma_multiply(factor.clamp(0.0, 1.0))
}

pub fn hair(ctx: &egui::Context) -> f32 {
    1.0 / ctx.pixels_per_point()
}

pub fn play_icon(painter: &egui::Painter, center: Pos2, height: f32, color: Color32) {
    let width = height * 0.9;
    let corners = [
        pos2(center.x - width / 3.0, center.y - height / 2.0),
        pos2(center.x + width * 2.0 / 3.0, center.y),
        pos2(center.x - width / 3.0, center.y + height / 2.0),
    ];
    let radius = height * 0.13;
    let steps = 6;
    let mut outline = Vec::with_capacity(3 * (steps + 1));
    for i in 0..3 {
        let corner = corners[i];
        let to_prev = (corners[(i + 2) % 3] - corner).normalized();
        let to_next = (corners[(i + 1) % 3] - corner).normalized();
        let half_angle = to_prev.dot(to_next).clamp(-1.0, 1.0).acos() / 2.0;
        let arc_center = corner + (to_prev + to_next).normalized() * (radius / half_angle.sin());
        let start = (corner + to_prev * (radius / half_angle.tan()) - arc_center).angle();
        let end = (corner + to_next * (radius / half_angle.tan()) - arc_center).angle();
        let mut sweep = end - start;
        if sweep > std::f32::consts::PI {
            sweep -= std::f32::consts::TAU;
        } else if sweep < -std::f32::consts::PI {
            sweep += std::f32::consts::TAU;
        }
        for k in 0..=steps {
            let angle = start + sweep * k as f32 / steps as f32;
            outline.push(arc_center + vec2(angle.cos(), angle.sin()) * radius);
        }
    }
    painter.add(Shape::convex_polygon(outline, color, Stroke::NONE));
}

pub fn snap(painter: &egui::Painter, rect: Rect) -> Rect {
    painter.round_rect_to_pixels(rect)
}

pub fn box_shapes(painter: &egui::Painter, rect: Rect, radius: f32, fill: Color32, stroke: Option<Color32>) -> (Shape, Shape) {
    let r = snap(painter, rect);
    let radius = radius.min(r.height() / 2.0).min(r.width() / 2.0);
    let fill_shape = Shape::Rect(RectShape::filled(r, Rounding::same(radius), fill));
    let stroke_shape = match stroke {
        Some(color) => {
            let w = 1.0 / painter.ctx().pixels_per_point();
            Shape::Rect(RectShape::stroke(r.shrink(w / 2.0), Rounding::same((radius - w / 2.0).max(0.0)), Stroke::new(w, color)))
        }
        None => Shape::Noop,
    };
    (fill_shape, stroke_shape)
}

pub fn paint_box(painter: &egui::Painter, rect: Rect, radius: f32, fill: Color32, stroke: Option<Color32>) {
    let (a, b) = box_shapes(painter, rect, radius, fill, stroke);
    painter.add(a);
    painter.add(b);
}

pub fn framed<R>(ui: &mut Ui, radius: f32, fill: Color32, stroke: Option<Color32>, margin: Margin, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let fill_idx = ui.painter().add(Shape::Noop);
    let stroke_idx = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::none().inner_margin(margin).show(ui, add);
    let (a, b) = box_shapes(ui.painter(), inner.response.rect, radius, fill, stroke);
    ui.painter().set(fill_idx, a);
    ui.painter().set(stroke_idx, b);
    inner
}

pub fn rounded_mesh(rect: Rect, radius: f32, step: f32, color: impl Fn(Pos2) -> Color32) -> Mesh {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let mut xs = Vec::new();
    let corner_steps = 12;
    for i in 0..=corner_steps {
        xs.push(rect.left() + r * i as f32 / corner_steps as f32);
    }
    let middle = (rect.width() - 2.0 * r).max(0.0);
    let middle_steps = (middle / step.max(4.0)).ceil().max(1.0) as usize;
    for i in 1..middle_steps {
        xs.push(rect.left() + r + middle * i as f32 / middle_steps as f32);
    }
    for i in 0..=corner_steps {
        xs.push(rect.right() - r + r * i as f32 / corner_steps as f32);
    }
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    let rows = ((rect.height() / step.max(4.0)).ceil() as usize).clamp(1, 24);
    let extent = |x: f32| -> f32 {
        let dx = if x < rect.left() + r {
            rect.left() + r - x
        } else if x > rect.right() - r {
            x - (rect.right() - r)
        } else {
            0.0
        };
        if r <= 0.0 {
            0.0
        } else {
            r - (r * r - dx.min(r) * dx.min(r)).max(0.0).sqrt()
        }
    };
    let mut mesh = Mesh::default();
    for &x in &xs {
        let inset = extent(x);
        let top = rect.top() + inset;
        let bottom = rect.bottom() - inset;
        for row in 0..=rows {
            let y = top + (bottom - top) * row as f32 / rows as f32;
            let p = pos2(x, y);
            mesh.colored_vertex(p, color(p));
        }
    }
    let per_column = (rows + 1) as u32;
    for c in 0..xs.len().saturating_sub(1) as u32 {
        for row in 0..rows as u32 {
            let a = c * per_column + row;
            let b = a + per_column;
            mesh.add_triangle(a, b, a + 1);
            mesh.add_triangle(b, b + 1, a + 1);
        }
    }
    mesh
}

pub fn begin_frame(ctx: &egui::Context, page_epoch: f64) {
    ctx.data_mut(|d| d.insert_temp(Id::new("kage_page_epoch"), page_epoch));
}

pub fn page_fade(ctx: &egui::Context) -> f32 {
    let now = ctx.input(|i| i.time);
    let epoch = ctx.data(|d| d.get_temp::<f64>(Id::new("kage_page_epoch")).unwrap_or(0.0));
    let duration = if theme::reduced_motion() { 0.22 } else { 0.32 };
    let t = ((now - epoch) as f32 / duration).clamp(0.0, 1.0);
    if t < 1.0 {
        animate_frame(ctx);
    }
    ease_in_out(t)
}

fn reset_rows(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new("kage_row_index"), 0usize));
}

fn next_row(ctx: &egui::Context) -> usize {
    ctx.data_mut(|d| {
        let counter = d.get_temp_mut_or_default::<usize>(Id::new("kage_row_index"));
        let index = *counter;
        *counter += 1;
        index
    })
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    framed(ui, 24.0, theme::PANEL, Some(LINE), Margin::same(24.0), |ui| {
        ui.set_width(ui.available_width());
        reset_rows(ui.ctx());
        add(ui)
    })
    .inner
}

pub fn gap(ui: &mut Ui) {
    ui.add_space(16.0);
}

pub fn icon(code: &str, size: f32) -> RichText {
    RichText::new(code).family(theme::icons()).size(size)
}

pub fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(theme::bold()).size(size)
}

pub fn medium(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(theme::medium()).size(size)
}

pub fn faint(text: impl Into<String>) -> RichText {
    RichText::new(text).color(INK_3).size(13.0)
}

pub fn tracked(text: &str, font: FontId, color: Color32, spacing: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, egui::TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    job
}

pub fn caps(ui: &mut Ui, text: &str) {
    ui.label(tracked(&text.to_uppercase(), FontId::new(12.0, theme::medium()), INK_3, 0.7));
}

pub fn page_header(ui: &mut Ui, label: &str, title: &str, lead: &str) {
    ui.add_space(4.0);
    caps(ui, label);
    ui.add_space(4.0);
    ui.label(tracked(title, FontId::new(38.0, theme::bold()), Color32::WHITE, -1.4));
    if !lead.is_empty() {
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(lead).size(15.5).color(INK_2)).wrap());
    }
    ui.add_space(22.0);
}

pub fn section(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(tracked(title, FontId::new(18.0, theme::bold()), INK, -0.3));
    if !subtitle.is_empty() {
        ui.add(egui::Label::new(RichText::new(subtitle).size(13.5).color(INK_3)).wrap());
    }
    ui.add_space(10.0);
}

pub fn section_with_action(ui: &mut Ui, title: &str, subtitle: &str, action: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - 280.0).max(220.0);
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(width);
            ui.label(tracked(title, FontId::new(18.0, theme::bold()), INK, -0.3));
            if !subtitle.is_empty() {
                ui.add(egui::Label::new(RichText::new(subtitle).size(13.5).color(INK_3)).wrap());
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            action(ui);
        });
    });
    ui.add_space(12.0);
}

pub fn hairline(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    let painter = ui.painter();
    let y = painter.round_to_pixel_center(rect.center().y);
    let w = hair(ui.ctx());
    painter.hline(painter.round_to_pixel(rect.left())..=painter.round_to_pixel(rect.right()), y, Stroke::new(w, LINE));
}

pub fn setting_row(ui: &mut Ui, title: &str, description: &str, control: impl FnOnce(&mut Ui)) {
    if next_row(ui.ctx()) > 0 {
        ui.add_space(4.0);
        hairline(ui);
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        let width = ui.available_width();
        let text_width = (width * 0.48).max(220.0).min(width - 140.0);
        ui.allocate_ui_with_layout(vec2(text_width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(text_width);
            ui.label(medium(title, 14.5).color(INK));
            if !description.is_empty() {
                ui.add(egui::Label::new(RichText::new(description).size(13.0).color(INK_3)).wrap());
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            control(ui);
        });
    });
    ui.add_space(4.0);
}

pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(46.0, 26.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = tween_bool(ui.ctx(), response.id, *on, 0.3);
    let hover = tween_bool(ui.ctx(), response.id.with("h"), response.hovered(), 0.2);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track = lerp_color(FILL_2, INK, t);
        let stroke = lerp_color(lerp_color(LINE_2, theme::LINE_3, hover), INK, t);
        paint_box(painter, rect, 13.0, track, Some(stroke));
        let r = snap(painter, rect);
        let x = egui::lerp(r.left() + 13.0..=r.right() - 13.0, t);
        let knob = lerp_color(lerp_color(INK_3, INK_2, hover), ON_LIGHT, t);
        painter.circle_filled(pos2(x, r.center().y), 9.0, knob);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Light,
    Ghost,
    Danger,
}

fn button_impl(ui: &mut Ui, tone: Tone, glyph_code: Option<&str>, label: &str, enabled: bool, small: bool) -> Response {
    let height = if small { 36.0 } else { 46.0 };
    let font_size = if small { 13.5 } else { 15.0 };
    let icon_size = if small { 14.0 } else { 16.0 };
    let pad = if small { 16.0 } else { 24.0 };
    let text_color = match tone {
        Tone::Light => ON_LIGHT,
        Tone::Ghost => INK,
        Tone::Danger => Color32::from_rgb(255, 214, 216),
    };
    let base_galley = ui.painter().layout_no_wrap(label.to_string(), FontId::new(font_size, theme::medium()), text_color);
    let icon_width = if glyph_code.is_some() { if label.is_empty() { 16.0 } else { 26.0 } } else { 0.0 };
    let width = if label.is_empty() { height } else { (base_galley.size().x + pad * 2.0 + icon_width).round() };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
    let press = tween_bool(ui.ctx(), response.id.with("press"), enabled && response.is_pointer_button_down_on(), 0.16);
    if ui.is_rect_visible(rect) {
        let scale = 1.0 + 0.035 * press;
        let painter = ui.painter();
        let fade = if enabled { 1.0 } else { 0.4 };
        let shape_rect = if press > 0.001 { Rect::from_center_size(rect.center(), rect.size() * scale) } else { snap(painter, rect) };
        let radius = shape_rect.height() / 2.0;
        let (fill, stroke) = match tone {
            Tone::Light => (INK, None),
            Tone::Ghost => (theme::white(12), Some(LINE_2)),
            Tone::Danger => (Color32::from_rgba_unmultiplied(255, 84, 92, 46), Some(Color32::from_rgba_unmultiplied(255, 84, 92, 130))),
        };
        if press > 0.001 {
            painter.rect_filled(shape_rect, Rounding::same(radius), alpha(fill, fade));
            if let Some(stroke) = stroke {
                painter.rect_stroke(shape_rect, Rounding::same(radius), Stroke::new(hair(ui.ctx()), alpha(stroke, fade)));
            }
        } else {
            paint_box(painter, shape_rect, radius, alpha(fill, fade), stroke.map(|s| alpha(s, fade)));
        }
        let color = alpha(text_color, fade);
        let galley = if press > 0.001 { painter.layout_no_wrap(label.to_string(), FontId::new(font_size * scale, theme::medium()), text_color) } else { base_galley };
        let scaled_icon = icon_width * scale;
        let content_width = galley.size().x + if label.is_empty() { 0.0 } else { scaled_icon };
        let center = shape_rect.center();
        let mut x = center.x - content_width / 2.0;
        if let Some(code) = glyph_code {
            let icon_center = if label.is_empty() { center } else { pos2(x + 8.0 * scale, center.y) };
            painter.text(icon_center, Align2::CENTER_CENTER, code, FontId::new(icon_size * scale, theme::icons()), color);
            if !label.is_empty() {
                x += scaled_icon;
            }
        }
        if !label.is_empty() {
            let y = center.y - galley.size().y / 2.0;
            let pos = if press > 0.001 { pos2(x, y) } else { painter.round_pos_to_pixels(pos2(x, y)) };
            painter.galley_with_override_text_color(pos, galley, color);
        }
    }
    response.on_hover_cursor(if enabled { egui::CursorIcon::PointingHand } else { egui::CursorIcon::NotAllowed })
}

pub fn button(ui: &mut Ui, tone: Tone, glyph_code: &str, label: &str, enabled: bool) -> Response {
    button_impl(ui, tone, Some(glyph_code).filter(|g| !g.is_empty()), label, enabled, false)
}

pub fn button_sm(ui: &mut Ui, tone: Tone, glyph_code: &str, label: &str, enabled: bool) -> Response {
    button_impl(ui, tone, Some(glyph_code).filter(|g| !g.is_empty()), label, enabled, true)
}

pub fn ghost_button(ui: &mut Ui, glyph_code: &str, label: &str) -> Response {
    button_impl(ui, Tone::Ghost, Some(glyph_code).filter(|g| !g.is_empty()), label, true, true)
}

pub fn icon_button(ui: &mut Ui, glyph_code: &str, hint: &str) -> Response {
    button_impl(ui, Tone::Ghost, Some(glyph_code), "", true, true).on_hover_text(hint)
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::new(12.0, theme::medium()), color);
    let size = vec2((galley.size().x + 22.0).round(), 26.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let tinted = color != INK && color != INK_2 && color != INK_3;
    let fill = if tinted { alpha(color, 0.12) } else { Color32::from_rgba_unmultiplied(10, 10, 10, 140) };
    let stroke = if tinted { alpha(color, 0.35) } else { theme::white(31) };
    let painter = ui.painter();
    paint_box(painter, rect, 13.0, fill, Some(stroke));
    let r = snap(painter, rect);
    painter.galley(painter.round_pos_to_pixels(pos2(r.left() + 11.0, r.center().y - galley.size().y / 2.0)), galley, color);
}

pub fn paint_status_dot(ui: &Ui, center: Pos2, color: Color32, live: bool) {
    let painter = ui.painter();
    if live && ui.input(|i| i.focused) {
        let period = if theme::reduced_motion() { 3.2 } else { 2.4 };
        let t = ((ui.input(|i| i.time) % period) / period) as f32;
        let scale = 0.8 + t * if theme::reduced_motion() { 0.7 } else { 1.2 };
        let fade = (1.0 - t) * 0.8;
        painter.circle_stroke(center, 5.0 * scale + 1.0, Stroke::new(1.5_f32, alpha(color, fade)));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
    painter.circle_filled(center, 4.5, color);
}

pub fn progress(ui: &mut Ui, fraction: f32, color: Color32, height: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    let r = snap(painter, rect);
    painter.rect_filled(r, Rounding::same(height / 2.0), FILL_2);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut fill = r;
        fill.set_width((r.width() * f).max(height).round());
        painter.rect_filled(fill, Rounding::same(height / 2.0), color);
    }
}

pub fn level_to_fraction(level: f32) -> f32 {
    let db = if level > 0.0 { 20.0 * level.log10() } else { -90.0 };
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

pub fn fraction_to_db(fraction: f32) -> f32 {
    fraction * 60.0 - 60.0
}

pub fn meter(ui: &mut Ui, fraction: f32, peak: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    let painter = ui.painter();
    let r = snap(painter, rect);
    painter.rect_filled(r, Rounding::same(3.0), FILL_2);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.002 {
        let mut fill = r;
        fill.set_width((r.width() * f).max(6.0));
        let hot = peak.max(f);
        let color = if hot > 0.95 {
            theme::RED
        } else if hot > 0.85 {
            lerp_color(theme::GOOD, theme::AMBER, (hot - 0.85) / 0.1)
        } else {
            theme::GOOD
        };
        painter.rect_filled(fill, Rounding::same(3.0), color);
    }
    if peak > 0.02 {
        let x = r.left() + r.width() * peak.clamp(0.0, 1.0);
        painter.rect_filled(Rect::from_center_size(pos2(x, r.center().y), vec2(2.0, 10.0)), Rounding::same(1.0), alpha(INK, 0.85));
    }
}

pub fn sparkline(ui: &mut Ui, data: &VecDeque<(f64, f64)>, target: f64, height: f32) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    paint_box(ui.painter(), rect, 16.0, theme::white(5), Some(LINE));
    let painter = ui.painter_at(rect);
    if data.len() < 2 || target <= 0.0 {
        return;
    }
    let inner = rect.shrink2(vec2(10.0, 10.0));
    let max = data.iter().map(|d| d.0.max(d.1)).fold(target, f64::max) * 1.1;
    let to_y = |v: f64| inner.bottom() - (v / max) as f32 * inner.height();
    let n = 120usize;
    let step = inner.width() / (n - 1) as f32;
    let offset = n.saturating_sub(data.len());
    let target_y = painter.round_to_pixel_center(to_y(target));
    let mut x = inner.left();
    while x < inner.right() {
        painter.line_segment([pos2(x, target_y), pos2((x + 4.0).min(inner.right()), target_y)], Stroke::new(hair(ui.ctx()), LINE_2));
        x += 9.0;
    }
    let unique: Vec<Pos2> = data.iter().enumerate().map(|(i, d)| pos2(inner.left() + (offset + i) as f32 * step, to_y(d.0))).collect();
    let output: Vec<Pos2> = data.iter().enumerate().map(|(i, d)| pos2(inner.left() + (offset + i) as f32 * step, to_y(d.1))).collect();
    let mut mesh = Mesh::default();
    for p in &unique {
        mesh.colored_vertex(*p, alpha(theme::ACCENT, 0.22));
        mesh.colored_vertex(pos2(p.x, inner.bottom()), Color32::TRANSPARENT);
    }
    for i in 0..unique.len().saturating_sub(1) as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(output, Stroke::new(1.0_f32, INK_4)));
    painter.add(Shape::line(unique.clone(), Stroke::new(2.0_f32, theme::ACCENT)));
    if let Some(last) = unique.last() {
        painter.circle_filled(*last, 3.5, theme::ACCENT);
    }
    if let Some(last) = data.back() {
        response.on_hover_text(format!("Уникальные кадры: {:.1}/с\nВ файл: {:.1}/с\nЦель: {:.2}", last.0, last.1, target));
    }
}

pub fn stat(ui: &mut Ui, label: &str, value: String, color: Color32) {
    ui.vertical(|ui| {
        ui.label(tracked(&label.to_uppercase(), FontId::new(11.0, theme::medium()), INK_3, 0.6));
        ui.label(tracked(&value, FontId::new(22.0, theme::bold()), color, -0.4));
    });
}

pub fn slider(ui: &mut Ui, slider: egui::Slider<'_>) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 22.0;
        let visuals = &mut ui.visuals_mut().widgets;
        visuals.inactive.fg_stroke = Stroke::new(2.0_f32, INK);
        visuals.hovered.fg_stroke = Stroke::new(2.5_f32, Color32::WHITE);
        visuals.active.fg_stroke = Stroke::new(2.5_f32, theme::ACCENT);
        visuals.inactive.bg_fill = Color32::from_rgb(34, 34, 34);
        visuals.hovered.bg_fill = Color32::from_rgb(34, 34, 34);
        visuals.active.bg_fill = Color32::from_rgb(34, 34, 34);
        ui.add(slider)
    })
    .inner
}

pub fn text_field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    let fill_idx = ui.painter().add(Shape::Noop);
    let stroke_idx = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::none().inner_margin(Margin::symmetric(12.0, 8.0)).show(ui, |ui| ui.add(edit.frame(false).margin(Margin::ZERO)));
    let response = inner.inner;
    let focused = response.has_focus();
    let stroke = if focused { theme::ACCENT } else if response.hovered() { theme::LINE_3 } else { LINE_2 };
    let (a, b) = box_shapes(ui.painter(), inner.response.rect, 12.0, Color32::from_rgb(12, 12, 12), Some(stroke));
    ui.painter().set(fill_idx, a);
    ui.painter().set(stroke_idx, b);
    response
}

#[derive(Clone, Copy)]
struct PopupState {
    open: bool,
    opened_at: f64,
}

pub fn combo<T: PartialEq + Clone>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, String)], width: f32) -> bool {
    let popup_id = ui.make_persistent_id(("kage_combo", id));
    let current = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_else(|| "-".into());
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let now = ui.input(|i| i.time);
    let mut state = ui.data(|d| d.get_temp::<PopupState>(popup_id)).unwrap_or(PopupState { open: false, opened_at: 0.0 });
    if response.clicked() {
        state.open = !state.open;
        state.opened_at = now;
    }
    let painter = ui.painter();
    let stroke = if state.open || response.hovered() { theme::LINE_3 } else { LINE_2 };
    paint_box(painter, rect, 12.0, Color32::from_rgb(12, 12, 12), Some(stroke));
    let r = snap(painter, rect);
    let text_rect = Rect::from_min_max(pos2(r.left() + 14.0, r.top()), pos2(r.right() - 34.0, r.bottom()));
    let galley = painter.layout_no_wrap(current, FontId::new(14.0, theme::medium()), INK);
    painter.with_clip_rect(text_rect).galley(painter.round_pos_to_pixels(pos2(text_rect.left(), r.center().y - galley.size().y / 2.0)), galley, INK);
    let turn = tween_bool(ui.ctx(), popup_id.with("turn"), state.open, 0.2);
    let chevron_center = pos2(r.right() - 18.0, r.center().y);
    let a = turn * std::f32::consts::PI;
    let points: Vec<Pos2> = [vec2(-4.0, -2.0), vec2(0.0, 2.0), vec2(4.0, -2.0)]
        .iter()
        .map(|v| chevron_center + vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos()))
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.5_f32, INK_2)));
    let mut changed = false;
    if state.open {
        let t = ease(((now - state.opened_at) as f32 / dur(0.2)).clamp(0.0, 1.0));
        if t < 1.0 {
            animate_frame(ui.ctx());
        }
        let row_height = 34.0;
        let list_height = (options.len() as f32 * (row_height + 2.0) + 12.0).min(340.0);
        let below = ui.ctx().screen_rect().bottom() - r.bottom() > list_height + 20.0;
        let origin = if below { r.left_bottom() + vec2(0.0, 6.0) } else { r.left_top() - vec2(0.0, list_height + 6.0) };
        let slide = (1.0 - t) * dist(8.0) * if below { -1.0 } else { 1.0 };
        let area = egui::Area::new(popup_id.with("area"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(origin.x.round(), (origin.y + slide).round()))
            .show(ui.ctx(), |ui| {
                ui.multiply_opacity(t);
                let shadow = Shadow { offset: vec2(0.0, 14.0), blur: 36.0, spread: 0.0, color: Color32::from_black_alpha(160) };
                let shadow_idx = ui.painter().add(Shape::Noop);
                let inner = framed(ui, 16.0, theme::POPUP, Some(LINE_2), Margin::same(6.0), |ui| {
                    ui.set_width(width - 12.0);
                    egui::ScrollArea::vertical().max_height(328.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (option, label) in options {
                            let (row, row_response) = ui.allocate_exact_size(vec2(width - 12.0, row_height), Sense::click());
                            let selected = option == value;
                            let hover = row_response.hovered();
                            let painter = ui.painter();
                            if selected || hover {
                                let fill = if selected { theme::white(20) } else { theme::white(10) };
                                painter.rect_filled(snap(painter, row), Rounding::same(10.0), fill);
                            }
                            let label_rect = Rect::from_min_max(pos2(row.left() + 12.0, row.top()), pos2(row.right() - 32.0, row.bottom()));
                            let g = painter.layout_no_wrap(label.clone(), FontId::new(14.0, if selected { theme::medium() } else { egui::FontFamily::Proportional }), INK);
                            painter.with_clip_rect(label_rect).galley(painter.round_pos_to_pixels(pos2(label_rect.left(), row.center().y - g.size().y / 2.0)), g, if selected { INK } else { INK_2 });
                            if selected {
                                painter.text(pos2(row.right() - 18.0, row.center().y), Align2::CENTER_CENTER, glyph::CHECK, FontId::new(12.0, theme::icons()), INK);
                            }
                            if row_response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                if !selected {
                                    *value = option.clone();
                                    changed = true;
                                }
                                state.open = false;
                            }
                        }
                    });
                });
                ui.painter().set(shadow_idx, shadow.as_shape(inner.response.rect, Rounding::same(16.0)));
            });
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let pressed = ui.input(|i| i.pointer.any_pressed());
        if pressed {
            if let Some(p) = pointer {
                if !area.response.rect.contains(p) && !r.contains(p) {
                    state.open = false;
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(popup_id, state));
    changed
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegStyle {
    Solid,
    Nav,
}

pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, id_salt: impl Hash, value: &mut T, options: &[(T, String)], style: SegStyle) -> bool {
    let id = ui.make_persistent_id(id_salt);
    let font = FontId::new(13.5, theme::medium());
    let height = if style == SegStyle::Nav { 38.0 } else { 36.0 };
    let pad = 4.0;
    let galleys: Vec<_> = options.iter().map(|(_, label)| ui.painter().layout_no_wrap(label.clone(), font.clone(), INK)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 28.0).round()).collect();
    let total = widths.iter().sum::<f32>() + pad * 2.0;
    let (alloc, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    let rect = snap(ui.painter(), alloc);
    let mut changed = false;
    let selected = options.iter().position(|(v, _)| v == value);
    let mut x = rect.left() + pad;
    let mut item_rects = Vec::new();
    for w in &widths {
        item_rects.push(Rect::from_min_size(pos2(x, rect.top() + pad), vec2(*w, height - pad * 2.0)));
        x += w;
    }
    let mut responses = Vec::new();
    for (i, item) in item_rects.iter().enumerate() {
        let response = ui.interact(*item, id.with(i), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() && Some(i) != selected {
            *value = options[i].0.clone();
            changed = true;
        }
        responses.push(response);
    }
    let selected = options.iter().position(|(v, _)| v == value);
    let painter = ui.painter();
    if style == SegStyle::Solid {
        paint_box(painter, rect, height / 2.0, theme::white(10), Some(LINE));
    }
    if let Some(index) = selected {
        let target = item_rects[index];
        let left = tween(ui.ctx(), id.with("thumb_x"), target.left() - rect.left(), 0.4);
        let width = tween(ui.ctx(), id.with("thumb_w"), target.width(), 0.4);
        let thumb = Rect::from_min_size(pos2(rect.left() + left, target.top()), vec2(width, target.height()));
        match style {
            SegStyle::Solid => {
                painter.rect_filled(thumb, Rounding::same(thumb.height() / 2.0), INK);
            }
            SegStyle::Nav => {
                let settled = (left - (target.left() - rect.left())).abs() < 0.05;
                if settled {
                    paint_box(painter, thumb, thumb.height() / 2.0, theme::white(16), Some(LINE_2));
                } else {
                    painter.rect_filled(thumb, Rounding::same(thumb.height() / 2.0), theme::white(16));
                }
            }
        }
    }
    for (i, (galley, item)) in galleys.into_iter().zip(item_rects.iter()).enumerate() {
        let hover = tween_bool(ui.ctx(), id.with(("hover", i)), responses[i].hovered(), 0.2);
        let is_selected = Some(i) == selected;
        let color = if is_selected {
            if style == SegStyle::Solid {
                ON_LIGHT
            } else {
                INK
            }
        } else {
            lerp_color(if style == SegStyle::Nav { INK_3 } else { INK_2 }, INK, hover)
        };
        let pos = painter.round_pos_to_pixels(pos2(item.center().x - galley.size().x / 2.0, item.center().y - galley.size().y / 2.0));
        painter.galley_with_override_text_color(pos, galley, color);
    }
    changed
}

pub fn chip_toggle(ui: &mut Ui, label: &str, on: &mut bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::new(12.5, theme::medium()), INK);
    let size = vec2((galley.size().x + 20.0).max(30.0).round(), 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = tween_bool(ui.ctx(), response.id, *on, 0.25);
    let painter = ui.painter();
    let fill = lerp_color(FILL, INK, t);
    let stroke = lerp_color(LINE_2, INK, t);
    paint_box(painter, rect, 14.0, fill, Some(stroke));
    let r = snap(painter, rect);
    let color = lerp_color(INK_2, ON_LIGHT, t);
    painter.galley_with_override_text_color(painter.round_pos_to_pixels(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0)), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_tile(ui: &mut Ui, glyph_code: &str, active: f32, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(42.0, 42.0), Sense::hover());
    let painter = ui.painter();
    let r = snap(painter, rect);
    painter.rect_filled(r, Rounding::same(14.0), lerp_color(FILL_2, INK, active));
    painter.text(r.center(), Align2::CENTER_CENTER, glyph_code, FontId::new(18.0, theme::icons()), lerp_color(accent, ON_LIGHT, active));
}

pub fn notice(ui: &mut Ui, glyph_code: &str, color: Color32, text: &str) {
    framed(ui, 18.0, alpha(color, 0.07), Some(alpha(color, 0.3)), Margin::symmetric(18.0, 12.0), |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(icon(glyph_code, 15.0).color(color));
            ui.add(egui::Label::new(RichText::new(text).color(INK)).wrap());
        });
    });
    ui.add_space(12.0);
}

pub fn key_value(ui: &mut Ui, label: &str, value: impl Into<egui::WidgetText>) {
    if next_row(ui.ctx()) > 0 {
        hairline(ui);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(250.0, 18.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(label).color(INK_3).size(13.5));
        });
        ui.add(egui::Label::new(value).wrap());
    });
    ui.add_space(6.0);
}
