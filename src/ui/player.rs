use std::path::PathBuf;
use std::time::Duration;

use crossbeam_channel::Receiver;
use eframe::egui::{self, pos2, vec2, Align2, Color32, ColorImage, FontId, Id, Key, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Ui};
use eframe::epaint::RectShape;

use super::theme;
use super::widgets::{self, glyph};
use crate::daemon::shell;
use crate::engine::playback::Playback;
use crate::util;

pub struct PlayerView {
    playlist: Vec<PathBuf>,
    index: usize,
    playback: Option<Playback>,
    loading: Option<Receiver<Result<Playback, String>>>,
    error: Option<String>,
    texture: Option<TextureHandle>,
    volume: f32,
    muted: bool,
    fullscreen: bool,
    last_activity: f64,
    opened_at: f64,
    drag_was_playing: Option<bool>,
    last_seek_sent: f64,
    pending_track: Option<usize>,
    closing: bool,
}

const SKIP: &str = "\u{E893}";
const PREVIOUS: &str = "\u{E892}";
const PAUSE: &str = "\u{E769}";
const VOLUME: &str = "\u{E767}";
const MUTED: &str = "\u{E74F}";
const FULL: &str = "\u{E740}";
const EXIT_FULL: &str = "\u{E73F}";

impl PlayerView {
    pub fn new(ctx: &egui::Context, playlist: Vec<PathBuf>, index: usize, volume: f32) -> Self {
        let mut view = Self {
            playlist,
            index,
            playback: None,
            loading: None,
            error: None,
            texture: None,
            volume,
            muted: false,
            fullscreen: false,
            last_activity: ctx.input(|i| i.time),
            opened_at: ctx.input(|i| i.time),
            drag_was_playing: None,
            last_seek_sent: 0.0,
            pending_track: None,
            closing: false,
        };
        view.load(ctx);
        view
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    fn path(&self) -> &PathBuf {
        &self.playlist[self.index]
    }

    fn load(&mut self, ctx: &egui::Context) {
        self.playback = None;
        self.texture = None;
        self.error = None;
        let path = self.path().clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let notify = ctx.clone();
        std::thread::spawn(move || {
            let result = Playback::open(&path, 1920, 1080).map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            notify.request_repaint();
        });
        self.loading = Some(rx);
    }

    fn step(&mut self, ctx: &egui::Context, delta: isize) {
        let next = self.index as isize + delta;
        if next >= 0 && (next as usize) < self.playlist.len() {
            self.index = next as usize;
            self.load(ctx);
        }
    }

    fn apply_volume(&self) {
        if let Some(p) = &self.playback {
            p.set_volume(if self.muted { 0.0 } else { self.volume });
        }
    }

    pub fn exit_fullscreen(&mut self, ctx: &egui::Context) {
        if self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        if pressed(Key::Escape) {
            if self.fullscreen {
                self.exit_fullscreen(ctx);
            } else {
                self.closing = true;
            }
        }
        if pressed(Key::F) {
            self.toggle_fullscreen(ctx);
        }
        if pressed(Key::M) {
            self.muted = !self.muted;
            self.apply_volume();
        }
        if pressed(Key::PageDown) || pressed(Key::N) {
            self.step(ctx, 1);
        }
        if pressed(Key::PageUp) || pressed(Key::P) {
            self.step(ctx, -1);
        }
        if pressed(Key::ArrowUp) {
            self.volume = (self.volume + 0.05).min(1.0);
            self.muted = false;
            self.apply_volume();
        }
        if pressed(Key::ArrowDown) {
            self.volume = (self.volume - 0.05).max(0.0);
            self.apply_volume();
        }
        let Some(p) = &self.playback else { return };
        if pressed(Key::Space) || pressed(Key::K) {
            p.toggle();
        }
        let jump = |seconds: f64| p.seek(p.position() + seconds);
        if pressed(Key::ArrowLeft) {
            jump(-5.0);
        }
        if pressed(Key::ArrowRight) {
            jump(5.0);
        }
        if pressed(Key::J) {
            jump(-10.0);
        }
        if pressed(Key::L) {
            jump(10.0);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.loading {
            if let Ok(result) = rx.try_recv() {
                self.loading = None;
                match result {
                    Ok(playback) => {
                        playback.set_volume(if self.muted { 0.0 } else { self.volume });
                        playback.seek(0.0);
                        playback.play();
                        self.playback = Some(playback);
                    }
                    Err(err) => self.error = Some(err),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        if let Some(track) = self.pending_track.take() {
            if let Some(p) = &self.playback {
                p.set_track(track);
            }
        }
        let Some(p) = &self.playback else { return };
        if let Some(err) = p.error() {
            self.error = Some(err);
        }
        if let Some(frame) = p.take_frame() {
            let pixels = frame.pixels.into_boxed_slice();
            let count = pixels.len() / 4;
            let raw = Box::into_raw(pixels) as *mut Color32;
            let colors = unsafe { Vec::from_raw_parts(raw, count, count) };
            let image = ColorImage { size: [frame.width, frame.height], pixels: colors };
            match &mut self.texture {
                Some(texture) if texture.size() == [frame.width, frame.height] => texture.set(image, TextureOptions::LINEAR),
                _ => self.texture = Some(ctx.load_texture("kage-player", image, TextureOptions::LINEAR)),
            }
        }
        if p.playing() {
            let wait = p.next_frame_in().unwrap_or(0.008).clamp(0.002, 0.016);
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        } else if p.awaiting_frame() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
        if p.ended() && p.playing() {
            p.pause();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        self.poll(ctx);
        self.handle_keys(ctx);
        let now = ctx.input(|i| i.time);
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_down()) {
            self.last_activity = now;
        }
        let screen = ctx.screen_rect();
        let appear = widgets::ease(((now - self.opened_at) as f32 / widgets::dur(0.25)).clamp(0.0, 1.0));
        if appear < 1.0 {
            widgets::animate_frame(ctx);
        }
        egui::Area::new(Id::new("kage_player"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                let painter = ui.painter();
                let backdrop = if self.fullscreen { Color32::BLACK } else { Color32::from_rgba_unmultiplied(3, 3, 3, (232.0 * appear) as u8) };
                painter.rect_filled(screen, Rounding::ZERO, backdrop);
                ui.interact(screen, Id::new("kage_player_block"), Sense::click());
                ui.multiply_opacity(appear);
                if self.fullscreen {
                    self.fullscreen_layout(ui, screen, now);
                } else {
                    self.window_layout(ui, screen);
                }
            });
        if self.closing {
            self.exit_fullscreen(ctx);
        }
        !self.closing
    }

    fn title(&self) -> (String, String) {
        let path = self.path();
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let folder = path.parent().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| util::format_bytes(m.len())).unwrap_or_default();
        let mut meta = vec![folder, size];
        if let Some(p) = &self.playback {
            meta.push(format!("{} · {}×{} · {:.0} к/с{}", p.info.codec, p.info.source_width, p.info.source_height, p.info.fps, if p.hardware() { " · декодирует GPU" } else { "" }));
        }
        (name, meta.into_iter().filter(|m| !m.is_empty()).collect::<Vec<_>>().join("  ·  "))
    }

    fn window_layout(&mut self, ui: &mut Ui, screen: Rect) {
        let width = (screen.width() - 80.0).clamp(400.0, 1280.0);
        let video_height = (width * 9.0 / 16.0).min(screen.height() - 220.0).max(160.0);
        let video_width = (video_height * 16.0 / 9.0).min(width);
        let panel_height = video_height + 150.0;
        let panel = Rect::from_center_size(screen.center(), vec2(video_width + 32.0, panel_height));
        let panel = widgets::snap(ui.painter(), panel);
        widgets::paint_box(ui.painter(), panel, 26.0, theme::PANEL, Some(theme::LINE_2));
        let top = Rect::from_min_size(panel.min + vec2(20.0, 14.0), vec2(panel.width() - 40.0, 48.0));
        let (name, meta) = self.title();
        let painter = ui.painter().clone();
        let title_galley = painter.layout_no_wrap(name, FontId::new(17.0, theme::bold()), theme::INK);
        painter.with_clip_rect(Rect::from_min_max(top.min, pos2(top.right() - 170.0, top.bottom()))).galley(painter.round_pos_to_pixels(top.min + vec2(0.0, 4.0)), title_galley, theme::INK);
        let meta_galley = painter.layout_no_wrap(meta, FontId::new(12.5, egui::FontFamily::Proportional), theme::INK_3);
        painter.with_clip_rect(Rect::from_min_max(top.min, pos2(top.right() - 170.0, top.bottom()))).galley(painter.round_pos_to_pixels(top.min + vec2(0.0, 27.0)), meta_galley, theme::INK_3);
        let mut buttons = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(top.right() - 170.0, top.top()), top.max)).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if widgets::icon_button(&mut buttons, glyph::CLOSE, "Закрыть (Esc)").clicked() {
            self.closing = true;
        }
        if widgets::icon_button(&mut buttons, glyph::OPEN, "Открыть во внешнем плеере").clicked() {
            if let Some(p) = &self.playback {
                p.pause();
            }
            shell::open_path(&self.path().display().to_string());
        }
        if widgets::icon_button(&mut buttons, glyph::FOLDER, "Показать в папке").clicked() {
            shell::reveal_file(&self.path().display().to_string());
        }
        let video = Rect::from_min_size(pos2(panel.left() + 16.0, top.bottom() + 6.0), vec2(video_width, video_height));
        self.paint_video(ui, video, 18.0);
        let controls = Rect::from_min_max(pos2(panel.left() + 20.0, video.bottom() + 12.0), pos2(panel.right() - 20.0, panel.bottom() - 12.0));
        self.controls(ui, controls, false);
    }

    fn fullscreen_layout(&mut self, ui: &mut Ui, screen: Rect, now: f64) {
        self.paint_video(ui, screen, 0.0);
        let playing = self.playback.as_ref().map(|p| p.playing()).unwrap_or(false);
        let bar = Rect::from_min_max(pos2(screen.left(), screen.bottom() - 150.0), screen.max);
        let pointer_over_bar = ui.input(|i| i.pointer.hover_pos()).map(|p| bar.contains(p)).unwrap_or(false);
        let active = now - self.last_activity < 2.5 || !playing || pointer_over_bar || self.drag_was_playing.is_some();
        let visible = widgets::tween_bool(ui.ctx(), Id::new("kage_player_controls"), active, 0.3);
        if !active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        } else if playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if visible < 0.01 {
            return;
        }
        let shade = widgets::rounded_mesh(bar, 0.0, 40.0, |p| {
            let t = ((p.y - bar.top()) / bar.height()).clamp(0.0, 1.0);
            Color32::from_black_alpha((t * t * 210.0 * visible) as u8)
        });
        ui.painter().add(egui::Shape::mesh(shade));
        let (name, meta) = self.title();
        let top_shade = Rect::from_min_max(screen.min, pos2(screen.right(), screen.top() + 110.0));
        let mesh = widgets::rounded_mesh(top_shade, 0.0, 40.0, |p| {
            let t = 1.0 - ((p.y - top_shade.top()) / top_shade.height()).clamp(0.0, 1.0);
            Color32::from_black_alpha((t * t * 170.0 * visible) as u8)
        });
        ui.painter().add(egui::Shape::mesh(mesh));
        let painter = ui.painter();
        painter.text(screen.min + vec2(32.0, 30.0), Align2::LEFT_TOP, name, FontId::new(18.0, theme::bold()), widgets::alpha(theme::INK, visible));
        painter.text(screen.min + vec2(32.0, 56.0), Align2::LEFT_TOP, meta, FontId::new(12.5, egui::FontFamily::Proportional), widgets::alpha(theme::INK_2, visible));
        let controls = Rect::from_min_max(pos2(screen.left() + 32.0, screen.bottom() - 96.0), pos2(screen.right() - 32.0, screen.bottom() - 24.0));
        ui.scope(|ui| {
            ui.multiply_opacity(visible);
            self.controls(ui, controls, true);
        });
    }

    fn paint_video(&mut self, ui: &mut Ui, rect: Rect, radius: f32) {
        let response = ui.interact(rect, Id::new("kage_player_video"), Sense::click());
        let painter = ui.painter();
        if radius > 0.0 {
            painter.rect_filled(rect, Rounding::same(radius), Color32::BLACK);
        }
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fitted = Rect::from_center_size(rect.center(), size * scale);
            let fitted = painter.round_rect_to_pixels(fitted);
            let mut shape = RectShape::filled(fitted, Rounding::same(if (fitted.width() - rect.width()).abs() < 2.0 && (fitted.height() - rect.height()).abs() < 2.0 { radius } else { radius.min(6.0) }), Color32::WHITE);
            shape.fill_texture_id = texture.id();
            shape.uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.add(shape);
        }
        if let Some(err) = &self.error {
            painter.text(rect.center() - vec2(0.0, 12.0), Align2::CENTER_CENTER, err, FontId::new(15.0, theme::medium()), theme::AMBER);
            painter.text(rect.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, "Попробуйте открыть файл во внешнем плеере", FontId::new(13.0, egui::FontFamily::Proportional), theme::INK_3);
        } else if self.loading.is_some() || (self.playback.is_some() && self.texture.is_none()) {
            let t = ui.input(|i| i.time) as f32;
            for k in 0..3 {
                let phase = ((t * 2.2 - k as f32 * 0.25).sin() * 0.5 + 0.5).clamp(0.2, 1.0);
                painter.circle_filled(rect.center() + vec2((k as f32 - 1.0) * 16.0, 0.0), 4.0, widgets::alpha(theme::INK, phase));
            }
            widgets::animate_frame(ui.ctx());
        }
        if let Some(p) = &self.playback {
            if !p.playing() && self.texture.is_some() && self.error.is_none() {
                painter.circle_filled(rect.center(), 34.0, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
                widgets::play_icon(painter, rect.center(), 22.0, theme::ON_LIGHT);
            }
            if response.double_clicked() {
                p.toggle();
                self.toggle_fullscreen(ui.ctx());
            } else if response.clicked() {
                p.toggle();
            }
        }
    }

    fn round_button(ui: &mut Ui, center: Pos2, radius: f32, glyph_code: &str, light: bool, id: Id, enabled: bool) -> bool {
        let rect = Rect::from_center_size(center, vec2(radius * 2.0, radius * 2.0));
        let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
        let press = widgets::tween_bool(ui.ctx(), id.with("press"), enabled && response.is_pointer_button_down_on(), 0.14);
        let r = radius * (1.0 + 0.05 * press);
        let painter = ui.painter();
        let fade = if enabled { 1.0 } else { 0.35 };
        if light {
            painter.circle_filled(center, r, widgets::alpha(theme::INK, fade));
        } else {
            painter.circle(center, r, widgets::alpha(theme::white(14), fade), Stroke::new(widgets::hair(ui.ctx()), widgets::alpha(theme::LINE_2, fade)));
        }
        let ink = widgets::alpha(if light { theme::ON_LIGHT } else { theme::INK }, fade);
        if glyph_code == glyph::PLAY {
            widgets::play_icon(painter, center, radius * 0.72 * (1.0 + 0.05 * press), ink);
        } else {
            painter.text(center, Align2::CENTER_CENTER, glyph_code, FontId::new(radius * 0.75 * (1.0 + 0.05 * press), theme::icons()), ink);
        }
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        } else {
            false
        }
    }

    fn controls(&mut self, ui: &mut Ui, rect: Rect, overlay: bool) {
        let ctx = ui.ctx().clone();
        let (position, duration, playing, tracks, track) = match &self.playback {
            Some(p) => (p.position(), p.info.duration, p.playing(), p.info.tracks.clone(), p.track()),
            None => (0.0, 0.0, false, Vec::new(), 0),
        };
        let seek_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 18.0));
        self.seek_bar(ui, seek_rect, position, duration);
        let row_y = rect.top() + 48.0;
        let mut x = rect.left() + 18.0;
        if Self::round_button(ui, pos2(x, row_y), 17.0, PREVIOUS, false, Id::new("kp_prev"), self.index > 0) {
            self.step(&ctx, -1);
        }
        x += 46.0;
        if Self::round_button(ui, pos2(x, row_y), 22.0, if playing { PAUSE } else { glyph::PLAY }, true, Id::new("kp_play"), self.playback.is_some()) {
            if let Some(p) = &self.playback {
                p.toggle();
            }
        }
        x += 46.0;
        if Self::round_button(ui, pos2(x, row_y), 17.0, SKIP, false, Id::new("kp_next"), self.index + 1 < self.playlist.len()) {
            self.step(&ctx, 1);
        }
        x += 32.0;
        let time = format!("{} / {}", util::format_duration(position), util::format_duration(duration));
        ui.painter().text(pos2(x, row_y), Align2::LEFT_CENTER, time, FontId::new(13.5, theme::medium()), theme::INK_2);
        let mut right = rect.right() - 18.0;
        if Self::round_button(ui, pos2(right, row_y), 17.0, if self.fullscreen { EXIT_FULL } else { FULL }, false, Id::new("kp_full"), true) {
            self.toggle_fullscreen(&ctx);
        }
        right -= 26.0;
        let slider = Rect::from_min_max(pos2(right - 96.0, row_y - 10.0), pos2(right, row_y + 10.0));
        self.volume_slider(ui, slider);
        right -= 96.0 + 22.0;
        if Self::round_button(ui, pos2(right, row_y), 15.0, if self.muted || self.volume <= 0.001 { MUTED } else { VOLUME }, false, Id::new("kp_mute"), true) {
            self.muted = !self.muted;
            self.apply_volume();
        }
        right -= 24.0;
        if tracks.len() > 1 {
            let width = 210.0;
            let area = Rect::from_min_max(pos2(right - width, row_y - 18.0), pos2(right, row_y + 18.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let mut chosen = track;
            let options: Vec<(usize, String)> = tracks.iter().enumerate().map(|(i, n)| (i, n.clone())).collect();
            if widgets::combo(&mut child, "kp_track", &mut chosen, &options, width) {
                self.pending_track = Some(chosen);
            }
        }
        let _ = overlay;
    }

    fn seek_bar(&mut self, ui: &mut Ui, rect: Rect, position: f64, duration: f64) {
        let id = Id::new("kp_seek");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let hover = widgets::tween_bool(ui.ctx(), id.with("h"), response.hovered() || response.dragged(), 0.15);
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0 + 2.0 * hover));
        let painter = ui.painter();
        painter.rect_filled(track, Rounding::same(3.0), theme::white(36));
        let fraction = if duration > 0.0 { (position / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let mut played = track;
        played.set_width(track.width() * fraction);
        painter.rect_filled(played, Rounding::same(3.0), theme::INK);
        if hover > 0.01 || response.dragged() {
            painter.circle_filled(pos2(played.right(), track.center().y), 6.0 * hover.max(0.6), theme::INK);
        }
        if let (Some(pointer), true) = (response.hover_pos(), duration > 0.0) {
            let t = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64 * duration;
            let label = util::format_duration(t);
            let galley = painter.layout_no_wrap(label, FontId::new(12.0, theme::medium()), theme::ON_LIGHT);
            let bubble = Rect::from_center_size(pos2(pointer.x, track.top() - 18.0), galley.size() + vec2(14.0, 8.0));
            painter.rect_filled(bubble, Rounding::same(8.0), theme::INK);
            painter.galley(bubble.center() - galley.size() / 2.0, galley, theme::ON_LIGHT);
        }
        let Some(p) = &self.playback else { return };
        if duration <= 0.0 {
            return;
        }
        let target_at = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0) as f64 * duration;
        if response.drag_started() {
            self.drag_was_playing = Some(p.playing());
            p.pause();
        }
        let now = ui.input(|i| i.time);
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if now - self.last_seek_sent > 0.03 {
                    self.last_seek_sent = now;
                    p.scrub(target_at(pointer.x));
                }
            }
        }
        if response.drag_stopped() {
            if let Some(pointer) = response.interact_pointer_pos() {
                p.seek(target_at(pointer.x));
            }
            if self.drag_was_playing.take() == Some(true) {
                p.play();
            }
        } else if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                p.seek(target_at(pointer.x));
            }
        }
    }

    fn volume_slider(&mut self, ui: &mut Ui, rect: Rect) {
        let id = Id::new("kp_volume");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        if response.dragged() || response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                self.volume = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                self.muted = false;
                self.apply_volume();
            }
        }
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
        let painter = ui.painter();
        painter.rect_filled(track, Rounding::same(2.0), theme::white(36));
        let level = if self.muted { 0.0 } else { self.volume };
        let mut fill = track;
        fill.set_width(track.width() * level);
        painter.rect_filled(fill, Rounding::same(2.0), theme::INK);
        painter.circle_filled(pos2(fill.right(), track.center().y), 6.0, theme::INK);
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("Громкость {:.0} %", level * 100.0));
    }
}
