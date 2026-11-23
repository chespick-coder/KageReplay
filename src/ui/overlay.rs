use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Id, Rect, Rounding, Sense, Stroke, Ui};
use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, RECT};

use super::theme;
use super::widgets::{self, glyph};
use crate::config::{AppConfig, HotkeyAction};
use crate::engine::EngineStatus;
use crate::ipc::{self, Command, Request, Response};
use crate::util::{self, WideString};

pub const TITLE: &str = "Kage Replay Overlay";
const PANEL_WIDTH: f32 = 440.0;
const PAD: f32 = 26.0;
const CONTENT_HEIGHT: f32 = 1010.0;

#[derive(Default)]
struct Live {
    status: Option<EngineStatus>,
    config: Option<AppConfig>,
    connected: bool,
    levels: Vec<(String, f32)>,
}

struct Overlay {
    live: Arc<Mutex<Live>>,
    monitor: Option<(RECT, f32)>,
    window: Option<(isize, i32, i32)>,
    revealed: i32,
    placed: bool,
    had_focus: bool,
    opened_at: f64,
    closing: Option<f64>,
    busy: bool,
    meters: [f32; 2],
}

pub fn close_existing() -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    let title = WideString::new(TITLE);
    unsafe {
        match FindWindowW(windows::core::PCWSTR::null(), title.pcwstr()) {
            Ok(hwnd) if !hwnd.0.is_null() => {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                true
            }
            _ => false,
        }
    }
}

fn foreground_monitor() -> Option<(RECT, f32)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        Some((info.rcMonitor, dpi_x.max(96) as f32 / 96.0))
    }
}

fn set_reveal(hwnd: isize, width: i32, height: i32, visible: i32) {
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn, HRGN};
    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        if visible >= width {
            SetWindowRgn(hwnd, Some(HRGN::default()), true);
        } else {
            SetWindowRgn(hwnd, Some(CreateRectRgn(0, 0, visible.max(1), height)), true);
        }
    }
}

fn place_window(rect: RECT, scale: f32) -> Option<(isize, i32, i32)> {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND};
    use windows::Win32::UI::WindowsAndMessaging::*;
    let title = WideString::new(TITLE);
    unsafe {
        let hwnd = FindWindowW(windows::core::PCWSTR::null(), title.pcwstr()).ok().filter(|h| !h.0.is_null())?;
        let corners = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corners as *const _ as *const _, std::mem::size_of_val(&corners) as u32);
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        let width = (PANEL_WIDTH * scale).round() as i32;
        let height = rect.bottom - rect.top;
        set_reveal(hwnd.0 as isize, width, height, 1);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), rect.left, rect.top, width, height, SWP_SHOWWINDOW);
        let _ = SetForegroundWindow(hwnd);
        Some((hwnd.0 as isize, width, height))
    }
}

fn poll(live: Arc<Mutex<Live>>, ctx: egui::Context) {
    let mut client: Option<ipc::Client> = None;
    let mut config_at = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
    let mut status_at = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
    loop {
        if client.is_none() {
            client = ipc::Client::connect(Duration::from_millis(500)).ok();
        }
        let Some(c) = client.as_mut() else {
            live.lock().connected = false;
            ctx.request_repaint();
            std::thread::sleep(Duration::from_millis(400));
            continue;
        };
        let mut ok = true;
        if status_at.elapsed() >= Duration::from_millis(250) {
            status_at = Instant::now();
            match c.request(&Request::Status) {
                Ok(Response::Status(status)) => {
                    let mut l = live.lock();
                    l.status = Some(status.engine);
                    l.connected = true;
                }
                _ => ok = false,
            }
        }
        if ok {
            match c.request(&Request::Levels) {
                Ok(Response::Levels(levels)) => live.lock().levels = levels,
                _ => ok = false,
            }
        }
        if ok && config_at.elapsed() > Duration::from_secs(2) {
            if let Ok(Response::Config(config, _)) = c.request(&Request::Config) {
                live.lock().config = Some(*config);
                config_at = Instant::now();
            }
        }
        if !ok {
            client = None;
        }
        ctx.request_repaint();
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn run() {
    util::init_logging("overlay");
    let name = WideString::new("Local\\KageReplay.Overlay");
    let mutex = unsafe { windows::Win32::System::Threading::CreateMutexW(None, true, name.pcwstr()) };
    if unsafe { windows::Win32::Foundation::GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
        close_existing();
        return;
    }
    let _mutex = mutex;
    let monitor = foreground_monitor();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size([PANEL_WIDTH, 720.0]),
        vsync: false,
        ..Default::default()
    };
    let result = eframe::run_native(TITLE, options, Box::new(move |cc| {
        theme::install(&cc.egui_ctx, super::reduced_motion());
        let live = Arc::new(Mutex::new(Live::default()));
        let thread_live = live.clone();
        let ctx = cc.egui_ctx.clone();
        std::thread::Builder::new().name("kage-overlay-poll".into()).spawn(move || poll(thread_live, ctx))?;
        Ok(Box::new(Overlay {
            live,
            monitor,
            window: None,
            revealed: 0,
            placed: false,
            had_focus: false,
            opened_at: 0.0,
            closing: None,
            busy: false,
            meters: [0.0; 2],
        }))
    }));
    if let Err(err) = result {
        crate::log_error!("overlay failed: {err}");
    }
}

enum Action {
    Send(Command),
    Toggle(Command),
    OpenUi(Option<&'static str>),
    Close,
}

impl Overlay {
    fn dispatch(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Close => self.begin_close(ctx),
            Action::OpenUi(page) => {
                if let Ok(exe) = std::env::current_exe() {
                    let mut command = std::process::Command::new(exe);
                    command.arg("--ui");
                    if let Some(page) = page {
                        command.arg(page);
                    }
                    let _ = command.spawn();
                }
                self.begin_close(ctx);
            }
            Action::Toggle(command) => {
                std::thread::spawn(move || {
                    let _ = ipc::send_command(command);
                });
            }
            Action::Send(command) => {
                if self.busy {
                    return;
                }
                self.busy = true;
                std::thread::spawn(move || {
                    let _ = ipc::send_command(command);
                });
                self.begin_close(ctx);
            }
        }
    }

    fn begin_close(&mut self, ctx: &egui::Context) {
        if self.closing.is_none() {
            self.closing = Some(ctx.input(|i| i.time));
        }
    }

    fn reveal(&mut self, progress: f32) {
        let Some((hwnd, width, height)) = self.window else { return };
        let visible = ((width as f32) * progress).round() as i32;
        if visible != self.revealed {
            self.revealed = visible;
            set_reveal(hwnd, width, height, visible);
        }
    }
}

fn hotkey_hint(config: Option<&AppConfig>, action: HotkeyAction) -> String {
    config.map(|c| c.hotkeys.get(action)).filter(|h| h.is_set()).map(|h| h.label()).unwrap_or_default()
}

fn section_title(ui: &Ui, top: f32, left: f32, text: &str) -> f32 {
    ui.painter().text(pos2(left, top), Align2::LEFT_TOP, text.to_uppercase(), FontId::new(11.5, theme::medium()), theme::INK_3);
    top + 26.0
}

struct Row<'a> {
    glyph: &'a str,
    title: &'a str,
    detail: &'a str,
    hotkey: &'a str,
    accent: Color32,
    enabled: bool,
}

fn action_row(ui: &mut Ui, rect: Rect, id: &str, row: Row<'_>) -> bool {
    let id = Id::new(("overlay_row", id));
    let response = ui.interact(rect, id, if row.enabled { Sense::click() } else { Sense::hover() });
    let hover = widgets::tween_bool(ui.ctx(), id.with("h"), row.enabled && response.hovered(), 0.16);
    let press = widgets::tween_bool(ui.ctx(), id.with("p"), row.enabled && response.is_pointer_button_down_on(), 0.1);
    let fade = if row.enabled { 1.0 } else { 0.4 };
    let rect = rect.shrink(1.5 * press);
    let painter = ui.painter();
    widgets::paint_box(painter, rect, 16.0, widgets::alpha(widgets::lerp_color(theme::white(8), theme::white(18), hover), fade), Some(widgets::lerp_color(theme::LINE, theme::LINE_3, hover)));
    let circle = pos2(rect.left() + 30.0, rect.center().y);
    painter.circle_filled(circle, 17.0, widgets::alpha(widgets::lerp_color(theme::white(14), row.accent, hover), fade));
    let ink = widgets::alpha(widgets::lerp_color(row.accent, theme::ON_LIGHT, hover), fade);
    painter.text(circle, Align2::CENTER_CENTER, row.glyph, FontId::new(15.0, theme::icons()), ink);
    let text_left = rect.left() + 58.0;
    let has_detail = !row.detail.is_empty();
    let title_y = if has_detail { rect.center().y - 9.0 } else { rect.center().y };
    painter.text(pos2(text_left, title_y), Align2::LEFT_CENTER, row.title, FontId::new(14.5, theme::medium()), widgets::alpha(theme::INK, fade));
    if has_detail {
        painter.text(pos2(text_left, rect.center().y + 10.0), Align2::LEFT_CENTER, row.detail, FontId::new(12.0, egui::FontFamily::Proportional), widgets::alpha(theme::INK_3, fade));
    }
    if !row.hotkey.is_empty() {
        let galley = painter.layout_no_wrap(row.hotkey.to_string(), FontId::new(11.5, theme::medium()), theme::INK_2);
        let chip = Rect::from_min_size(pos2(rect.right() - 14.0 - galley.size().x - 16.0, rect.center().y - 11.0), vec2(galley.size().x + 16.0, 22.0));
        widgets::paint_box(painter, chip, 11.0, widgets::alpha(theme::white(10), fade), Some(widgets::alpha(theme::LINE_2, fade)));
        painter.galley(chip.center() - galley.size() / 2.0, galley, widgets::alpha(theme::INK_2, fade));
    }
    if row.enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    } else {
        false
    }
}

fn audio_row(ui: &mut Ui, rect: Rect, title: &str, level: f32, muted: bool, available: bool) -> bool {
    let painter = ui.painter().clone();
    widgets::paint_box(&painter, rect, 16.0, theme::white(6), Some(theme::LINE));
    let icon = pos2(rect.left() + 30.0, rect.center().y);
    let glyph_code = if title.starts_with("Микро") { glyph::MIC } else { glyph::AUDIO };
    painter.circle_filled(icon, 17.0, theme::white(12));
    painter.text(icon, Align2::CENTER_CENTER, glyph_code, FontId::new(15.0, theme::icons()), if muted { theme::INK_4 } else { theme::INK });
    let text_left = rect.left() + 58.0;
    painter.text(pos2(text_left, rect.top() + 18.0), Align2::LEFT_CENTER, title, FontId::new(14.0, theme::medium()), theme::INK);
    let state = if !available { "Нет источника" } else if muted { "Выключен" } else { "" };
    if !state.is_empty() {
        painter.text(pos2(rect.right() - 76.0, rect.top() + 18.0), Align2::RIGHT_CENTER, state, FontId::new(12.0, egui::FontFamily::Proportional), theme::INK_3);
    }
    let bar = Rect::from_min_max(pos2(text_left, rect.bottom() - 19.0), pos2(rect.right() - 76.0, rect.bottom() - 14.0));
    painter.rect_filled(bar, Rounding::same(2.5), theme::white(16));
    let fraction = if muted { 0.0 } else { widgets::level_to_fraction(level) };
    if fraction > 0.0 {
        let mut fill = bar;
        fill.set_width(bar.width() * fraction.clamp(0.0, 1.0));
        let color = if fraction > 0.92 { theme::RED } else if fraction > 0.75 { theme::AMBER } else { theme::LIVE };
        painter.rect_filled(fill, Rounding::same(2.5), color);
    }
    let switch = Rect::from_min_size(pos2(rect.right() - 62.0, rect.center().y - 13.0), vec2(46.0, 26.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    let mut on = !muted;
    available && widgets::toggle(&mut child, &mut on).on_hover_text(if muted { "Включить" } else { "Выключить" }).clicked()
}

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = theme::PANEL;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if !self.placed {
            self.placed = true;
            match self.monitor.and_then(|(rect, scale)| place_window(rect, scale)) {
                Some(window) => {
                    self.window = Some(window);
                    self.revealed = 1;
                }
                None => ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true)),
            }
            self.opened_at = now;
            ctx.request_repaint();
        }
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
        } else if self.had_focus {
            self.begin_close(ctx);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.begin_close(ctx);
        }
        let appear = widgets::ease(((now - self.opened_at) as f32 / widgets::dur(0.26)).clamp(0.0, 1.0));
        let vanish = self.closing.map(|t| widgets::ease_in_out(((now - t) as f32 / widgets::dur(0.18)).clamp(0.0, 1.0))).unwrap_or(0.0);
        let progress = appear * (1.0 - vanish);
        self.reveal(progress);
        if self.closing.is_some() && vanish >= 1.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if appear < 1.0 || self.closing.is_some() {
            widgets::animate_frame(ctx);
        }
        let (status, config, connected, levels) = {
            let l = self.live.lock();
            (l.status.clone(), l.config.clone(), l.connected, l.levels.clone())
        };
        let e = status.as_ref();
        let mic_names: Vec<String> = e.map(|e| e.audio.iter().filter(|a| a.mic).map(|a| a.name.clone()).collect()).unwrap_or_default();
        let system_names: Vec<String> = e.map(|e| e.audio.iter().filter(|a| !a.mic).map(|a| a.name.clone()).collect()).unwrap_or_default();
        let peak_of = |names: &[String]| levels.iter().filter(|(n, _)| names.contains(n)).map(|(_, l)| *l).fold(0.0f32, f32::max);
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        for (meter, peak) in self.meters.iter_mut().zip([peak_of(&system_names), peak_of(&mic_names)]) {
            *meter = if peak > *meter { peak } else { *meter * (0.02f32).powf(dt) };
        }
        let mut action = None;
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let window = ui.max_rect();
            ui.painter().rect_filled(window, Rounding::ZERO, theme::PANEL);
            ui.painter().line_segment([pos2(window.right() - 0.5, window.top()), pos2(window.right() - 0.5, window.bottom())], Stroke::new(1.0_f32, theme::LINE_2));
            let shift = (progress - 1.0) * window.width();
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                let height = CONTENT_HEIGHT.max(window.height());
                let (area, _) = ui.allocate_exact_size(vec2(window.width(), height), Sense::hover());
                let left = area.left() + PAD + shift;
                let width = area.width() - PAD * 2.0;
                let top = area.top();
                let painter = ui.painter().clone();

                let logo = Rect::from_min_size(pos2(left, top + 26.0), vec2(40.0, 40.0));
                super::paint_logo(ui, logo);
                painter.text(pos2(logo.right() + 14.0, logo.top() + 1.0), Align2::LEFT_TOP, util::APP_NAME, FontId::new(17.0, theme::bold()), theme::INK);
                let recording = e.map(|e| e.recording.active).unwrap_or(false);
                let buffer = e.map(|e| e.buffer_active).unwrap_or(false);
                let (dot, live, line) = if !connected {
                    (theme::AMBER, false, "Подключение к службе…".to_string())
                } else if recording {
                    (theme::REC, true, format!("Идёт запись {}", util::format_duration(e.map(|e| e.recording.elapsed_sec).unwrap_or(0.0))))
                } else if buffer {
                    (theme::LIVE, true, format!("Буфер {}", util::format_duration(e.map(|e| e.buffered_seconds).unwrap_or(0.0))))
                } else {
                    (theme::INK_4, false, "Захват выключен".to_string())
                };
                widgets::paint_status_dot(ui, pos2(logo.right() + 20.0, logo.bottom() - 8.0), dot, live);
                painter.text(pos2(logo.right() + 34.0, logo.bottom() - 8.0), Align2::LEFT_CENTER, line, FontId::new(13.0, theme::medium()), theme::INK_2);

                let close = Rect::from_center_size(pos2(left + width - 17.0, logo.center().y), vec2(34.0, 34.0));
                let close_response = ui.interact(close, Id::new("overlay_close"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                let close_hover = widgets::tween_bool(ui.ctx(), Id::new("overlay_close_h"), close_response.hovered(), 0.16);
                painter.circle(close.center(), 17.0, widgets::lerp_color(theme::white(8), theme::white(24), close_hover), Stroke::new(1.0_f32, theme::LINE));
                painter.text(close.center(), Align2::CENTER_CENTER, glyph::CLOSE, FontId::new(12.0, theme::icons()), theme::INK);
                if close_response.clicked() {
                    action = Some(Action::Close);
                }

                let cfg = config.as_ref();
                let profile = cfg.map(|c| c.profile());
                let full = profile.map(|p| p.replay.duration_sec).unwrap_or(0);
                let short = profile.map(|p| p.replay.short_clip_sec).unwrap_or(30);
                let row_height = 58.0;
                let row_gap = 10.0;
                let row_rect = |y: f32| Rect::from_min_size(pos2(left, y), vec2(width, row_height));

                let mut y = section_title(ui, logo.bottom() + 30.0, left, "Мгновенный повтор");
                let switch = Rect::from_min_size(pos2(left, y), vec2(width, 26.0));
                let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(switch).layout(egui::Layout::left_to_right(egui::Align::Center)));
                let mut on = buffer;
                if widgets::toggle(&mut bar, &mut on).clicked() && connected {
                    action = Some(Action::Toggle(Command::ToggleBuffer));
                }
                bar.add_space(10.0);
                bar.label(widgets::medium(if buffer { "Буфер включён" } else { "Буфер выключен" }, 14.0).color(theme::INK));
                y += 26.0 + 14.0;
                let save_detail = if full > 0 { format!("Последние {}", util::format_duration(full as f64)) } else { String::new() };
                let save_hotkey = hotkey_hint(cfg, HotkeyAction::SaveReplay);
                if action_row(ui, row_rect(y), "save", Row { glyph: glyph::SAVE, title: "Сохранить повтор", detail: &save_detail, hotkey: &save_hotkey, accent: theme::INK, enabled: connected && buffer }) {
                    action = Some(Action::Send(Command::SaveReplay(None)));
                }
                y += row_height + row_gap;
                let short_detail = format!("Последние {short} с");
                let short_hotkey = hotkey_hint(cfg, HotkeyAction::SaveShortReplay);
                if action_row(ui, row_rect(y), "short", Row { glyph: glyph::REPLAY, title: "Короткий клип", detail: &short_detail, hotkey: &short_hotkey, accent: theme::INK, enabled: connected && buffer }) {
                    action = Some(Action::Send(Command::SaveReplay(Some(short))));
                }
                y += row_height + 26.0;

                y = section_title(ui, y, left, "Запись");
                let record_detail = if recording { format!("Идёт {}", util::format_duration(e.map(|e| e.recording.elapsed_sec).unwrap_or(0.0))) } else { "Запись экрана в файл".to_string() };
                let record_hotkey = hotkey_hint(cfg, HotkeyAction::ToggleRecording);
                let record_title = if recording { "Остановить запись" } else { "Начать запись" };
                if action_row(ui, row_rect(y), "record", Row { glyph: if recording { glyph::STOP } else { glyph::RECORD }, title: record_title, detail: &record_detail, hotkey: &record_hotkey, accent: theme::REC, enabled: connected }) {
                    action = Some(Action::Send(Command::ToggleRecording));
                }
                y += row_height + row_gap;
                let shot_hotkey = hotkey_hint(cfg, HotkeyAction::Screenshot);
                if action_row(ui, row_rect(y), "shot", Row { glyph: glyph::CAMERA, title: "Скриншот", detail: "PNG в исходном разрешении", hotkey: &shot_hotkey, accent: theme::INK, enabled: connected }) {
                    action = Some(Action::Send(Command::Screenshot));
                }
                y += row_height + 26.0;

                y = section_title(ui, y, left, "Нагрузка");
                let video = e.and_then(|e| e.video.first());
                let process = e.map(|e| &e.process);
                let peak = process.map(|p| p.cpu_peak).unwrap_or(0.0);
                let cells = [
                    (0.42, "Битрейт", video.map(|v| format!("{:.1} Мбит/с", v.bitrate_kbps / 1000.0)), theme::INK),
                    (0.29, "CPU сейчас", process.map(|p| format!("{:.2} %", p.cpu_now)), theme::INK),
                    (0.29, "CPU максимум", process.map(|p| format!("{:.2} %", p.cpu_peak)), if peak > 10.0 { theme::AMBER } else { theme::INK }),
                ];
                let mut x = left;
                for (share, label, value, color) in cells {
                    let cell = Rect::from_min_size(pos2(x, y), vec2(width * share, 52.0));
                    let mut column = ui.new_child(egui::UiBuilder::new().max_rect(cell));
                    widgets::stat(&mut column, label, value.unwrap_or_else(|| "-".into()), color);
                    x += width * share;
                }
                y += 52.0 + 26.0;

                y = section_title(ui, y, left, "Звук");
                let audio_height = 58.0;
                let muted_system = e.map(|e| e.muted_system).unwrap_or(false);
                let muted_mic = e.map(|e| e.muted_mic).unwrap_or(false);
                if audio_row(ui, Rect::from_min_size(pos2(left, y), vec2(width, audio_height)), "Звук системы", self.meters[0], muted_system, connected && !system_names.is_empty()) {
                    action = Some(Action::Toggle(Command::ToggleMute { mic: false }));
                }
                y += audio_height + row_gap;
                if audio_row(ui, Rect::from_min_size(pos2(left, y), vec2(width, audio_height)), "Микрофон", self.meters[1], muted_mic, connected && !mic_names.is_empty()) {
                    action = Some(Action::Toggle(Command::ToggleMute { mic: true }));
                }
                y += audio_height + 22.0;
                if let Some(name) = profile.map(|p| p.name.clone()) {
                    let game = e.and_then(|e| e.game_detected.clone()).map(|g| format!(" · игра: {g}")).unwrap_or_default();
                    painter.text(pos2(left, y), Align2::LEFT_TOP, format!("Профиль: {name}{game}"), FontId::new(12.5, egui::FontFamily::Proportional), theme::INK_3);
                }

                let bottom = area.bottom() - PAD;
                let hint = match hotkey_hint(cfg, HotkeyAction::Overlay) {
                    h if h.is_empty() => "Esc - закрыть".to_string(),
                    h => format!("Esc или {h} - закрыть"),
                };
                painter.text(pos2(left, bottom), Align2::LEFT_BOTTOM, hint, FontId::new(12.0, egui::FontFamily::Proportional), theme::INK_3);
                let buttons = Rect::from_min_max(pos2(left, bottom - 64.0), pos2(left + width, bottom - 26.0));
                let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::left_to_right(egui::Align::Center)));
                if widgets::ghost_button(&mut bar, glyph::CLIPS, "Клипы").clicked() {
                    action = Some(Action::OpenUi(Some("clips")));
                }
                if widgets::ghost_button(&mut bar, glyph::OPEN, "Открыть Kage Replay").clicked() {
                    action = Some(Action::OpenUi(None));
                }
            });
        });
        if let Some(action) = action {
            self.dispatch(ctx, action);
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}
