mod clips;
mod dashboard;
pub mod overlay;
mod diagnostics;
mod remote;
mod settings;
mod player;
mod theme;
mod thumbs;
mod updates;
mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use eframe::egui::{self, pos2, vec2, Align, Color32, Layout, Margin, Pos2, Rect, Rounding, Stroke};
use eframe::epaint::{Mesh, Shadow};

use crate::config::{AppConfig, Hotkey, HotkeyAction};
use crate::ipc::{DaemonStatus, DeviceList};
use crate::util::{self, WideString};
use remote::{Outgoing, Remote};
use widgets::glyph;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Clips,
    Video,
    Audio,
    Replay,
    Hotkeys,
    Profiles,
    General,
    Diagnostics,
}

impl Page {
    fn from_name(name: &str) -> Page {
        match name {
            "clips" => Page::Clips,
            "video" => Page::Video,
            "audio" => Page::Audio,
            "replay" => Page::Replay,
            "hotkeys" => Page::Hotkeys,
            "profiles" => Page::Profiles,
            "general" => Page::General,
            "diagnostics" => Page::Diagnostics,
            _ => Page::Dashboard,
        }
    }

    fn is_settings(self) -> bool {
        matches!(self, Page::Video | Page::Audio | Page::Replay | Page::Hotkeys | Page::Profiles | Page::General)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Home,
    Clips,
    Settings,
    Diagnostics,
}

pub enum FolderTarget {
    Output,
    DiskBuffer,
}

pub struct App {
    remote: Remote,
    page: Page,
    base: Option<(AppConfig, u64)>,
    draft: Option<AppConfig>,
    editing: usize,
    capture: Option<HotkeyAction>,
    folder_pick: Option<(FolderTarget, Receiver<Option<PathBuf>>)>,
    clips: clips::ClipsState,
    flash: Option<(String, Instant, bool)>,
    rename: Option<(usize, String)>,
    new_game: String,
    reduced_motion: bool,
    page_epoch: f64,
    last_settings: Page,
    thumbs: thumbs::Thumbs,
    meters: HashMap<String, Meter>,
    pointer: Pos2,
    scrolled: f32,
    stars: Vec<Star>,
    title_styled: bool,
    pub mic_test: Option<settings::MicTest>,
    player: Option<player::PlayerView>,
    player_volume: f32,
    ui_request: Option<u64>,
    settle_layout: bool,
    update_prompt: Option<updates::Prompt>,
    update_armed: bool,
    update_manual: Option<u64>,
}

#[derive(Clone, Copy, Default)]
pub struct Meter {
    pub level: f32,
    pub peak: f32,
    peak_at: f64,
}

impl Meter {
    pub fn level_db(&self) -> f32 {
        widgets::fraction_to_db(self.level)
    }

    pub fn peak_db(&self) -> f32 {
        widgets::fraction_to_db(self.peak)
    }
}

#[derive(Clone, Copy)]
struct Star {
    x: f32,
    y: f32,
    depth: f32,
    size: f32,
    phase: f32,
    speed: f32,
}

fn make_stars() -> Vec<Star> {
    let mut seed = 0x9E37_79B9u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32) / (u32::MAX as f32)
    };
    (0..120)
        .map(|_| Star {
            x: next(),
            y: next(),
            depth: 0.15 + next() * 0.85,
            size: 0.5 + next() * 0.9,
            phase: next() * std::f32::consts::TAU,
            speed: 0.6 + next() * 0.8,
        })
        .collect()
}

pub struct Frame<'a> {
    pub status: Option<&'a DaemonStatus>,
    pub devices: Option<&'a DeviceList>,
    pub devices_loading: bool,
    pub history: &'a [std::collections::VecDeque<(f64, f64)>],
    pub connected: bool,
    pub meters: &'a HashMap<String, Meter>,
}

fn reduced_motion() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut enabled = windows::core::BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut enabled as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    !enabled.as_bool()
}

fn style_title_bar() -> bool {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    let title = WideString::new(util::APP_NAME);
    unsafe {
        let Ok(hwnd) = FindWindowW(windows::core::PCWSTR::null(), title.pcwstr()) else { return false };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if hwnd.0.is_null() || pid != std::process::id() {
            return false;
        }
        let dark = windows::core::BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, 4);
        let caption = 0x0005_0505u32;
        let border = 0x0018_1818u32;
        let text = 0x00AA_AAAAu32;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &caption as *const _ as *const _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as *const _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text as *const _ as *const _, 4);
    }
    true
}

fn focus_existing() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    let title = WideString::new(util::APP_NAME);
    unsafe {
        if let Ok(hwnd) = FindWindowW(windows::core::PCWSTR::null(), title.pcwstr()) {
            if !hwnd.0.is_null() {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                } else {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                }
                let _ = SetForegroundWindow(hwnd);
                return true;
            }
        }
    }
    false
}

pub fn run(start_page: Option<&str>, play: Option<PathBuf>) {
    util::init_logging("ui");
    let name = WideString::new("Local\\KageReplay.UI");
    let mutex = unsafe { windows::Win32::System::Threading::CreateMutexW(None, true, name.pcwstr()) };
    if unsafe { windows::Win32::Foundation::GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
        if let Some(page) = start_page {
            let _ = crate::ipc::send_command(crate::ipc::Command::ShowPage(page.to_string()));
        }
        focus_existing();
        return;
    }
    let _mutex = mutex;
    let size = 128;
    let icon = egui::IconData { rgba: crate::icon_art::render_icon_rgba(size), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(util::APP_NAME)
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([980.0, 640.0])
            .with_icon(Arc::new(icon)),
        vsync: false,
        centered: true,
        ..Default::default()
    };
    let page = Page::from_name(start_page.unwrap_or(""));
    let result = eframe::run_native(util::APP_NAME, options, Box::new(move |cc| {
        let mut app = App::new(cc);
        app.page = page;
        if let Some(path) = play {
            app.open_player(&cc.egui_ctx, vec![path], 0);
        }
        Ok(Box::new(app))
    }));
    if let Err(err) = result {
        crate::log_error!("ui failed: {err}");
        let text = WideString::new(&format!("Не удалось открыть окно Kage Replay: {err}"));
        let caption = WideString::new(util::APP_NAME);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::MessageBoxW(None, text.pcwstr(), caption.pcwstr(), windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
        }
    }
}

pub fn pick_folder() -> Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
        let result = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            (|| -> windows::core::Result<Option<PathBuf>> {
                let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
                dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
                let title = WideString::new("Выберите папку");
                dialog.SetTitle(title.pcwstr())?;
                if dialog.Show(None).is_err() {
                    return Ok(None);
                }
                let item = dialog.GetResult()?;
                let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                let s = util::from_pwstr(path.0);
                CoTaskMemFree(Some(path.0 as *const _));
                Ok(Some(PathBuf::from(s)))
            })()
        };
        let _ = tx.send(result.unwrap_or(None));
    });
    rx
}

pub fn egui_key_to_vk(key: egui::Key) -> Option<u32> {
    use egui::Key::*;
    let vk = match key {
        A => 0x41, B => 0x42, C => 0x43, D => 0x44, E => 0x45, F => 0x46, G => 0x47, H => 0x48, I => 0x49, J => 0x4A,
        K => 0x4B, L => 0x4C, M => 0x4D, N => 0x4E, O => 0x4F, P => 0x50, Q => 0x51, R => 0x52, S => 0x53, T => 0x54,
        U => 0x55, V => 0x56, W => 0x57, X => 0x58, Y => 0x59, Z => 0x5A,
        Num0 => 0x30, Num1 => 0x31, Num2 => 0x32, Num3 => 0x33, Num4 => 0x34, Num5 => 0x35, Num6 => 0x36, Num7 => 0x37, Num8 => 0x38, Num9 => 0x39,
        F1 => 0x70, F2 => 0x71, F3 => 0x72, F4 => 0x73, F5 => 0x74, F6 => 0x75, F7 => 0x76, F8 => 0x77, F9 => 0x78, F10 => 0x79,
        F11 => 0x7A, F12 => 0x7B, F13 => 0x7C, F14 => 0x7D, F15 => 0x7E, F16 => 0x7F, F17 => 0x80, F18 => 0x81, F19 => 0x82, F20 => 0x83,
        ArrowLeft => 0x25, ArrowUp => 0x26, ArrowRight => 0x27, ArrowDown => 0x28,
        Insert => 0x2D, Delete => 0x2E, Home => 0x24, End => 0x23, PageUp => 0x21, PageDown => 0x22,
        Space => 0x20, Tab => 0x09, Enter => 0x0D,
        Minus => 0xBD, Plus | Equals => 0xBB, Comma => 0xBC, Period => 0xBE, Semicolon => 0xBA, Slash => 0xBF,
        Backslash => 0xDC, OpenBracket => 0xDB, CloseBracket => 0xDD, Backtick => 0xC0, Quote => 0xDE,
        _ => return None,
    };
    Some(vk)
}


impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let reduced = reduced_motion();
        theme::install(&cc.egui_ctx, reduced);
        let remote = Remote::start(cc.egui_ctx.clone());
        Self {
            remote,
            page: Page::Dashboard,
            base: None,
            draft: None,
            editing: 0,
            capture: None,
            folder_pick: None,
            clips: clips::ClipsState::default(),
            flash: None,
            rename: None,
            new_game: String::new(),
            reduced_motion: reduced,
            page_epoch: 0.0,
            last_settings: Page::Video,
            thumbs: thumbs::Thumbs::new(cc.egui_ctx.clone()),
            meters: HashMap::new(),
            pointer: pos2(0.5, 0.5),
            scrolled: 0.0,
            stars: make_stars(),
            title_styled: false,
            mic_test: None,
            player: None,
            player_volume: 0.8,
            ui_request: None,
            settle_layout: true,
            update_prompt: None,
            update_armed: true,
            update_manual: None,
        }
    }

    fn sync_config(&mut self, incoming: Option<(AppConfig, u64)>) {
        let Some((config, revision)) = incoming else { return };
        let changed = self.base.as_ref().map(|(_, r)| *r != revision).unwrap_or(true);
        if !changed {
            return;
        }
        let clean = match (&self.draft, &self.base) {
            (Some(d), Some((b, _))) => d == b,
            _ => true,
        };
        if clean {
            self.draft = Some(config.clone());
            if self.base.is_none() {
                self.editing = config.active_profile;
            }
            self.editing = self.editing.min(config.profiles.len().saturating_sub(1));
        } else if let (Some(draft), Some((old, _))) = (self.draft.as_mut(), self.base.as_ref()) {
            let switched_elsewhere = old.active_profile != config.active_profile && draft.active_profile == old.active_profile;
            if switched_elsewhere && config.active_profile < draft.profiles.len() {
                draft.active_profile = config.active_profile;
            }
        }
        self.base = Some((config, revision));
    }

    fn fork_builtin_edits(&mut self) {
        let (Some(draft), Some((base, _))) = (self.draft.as_mut(), self.base.as_ref()) else { return };
        let editing = self.editing;
        let Some(edited) = draft.profiles.get(editing).filter(|p| p.builtin) else { return };
        let Some(original) = base.profiles.iter().find(|p| p.builtin) else { return };
        if edited == original {
            return;
        }
        let name = draft.next_custom_name();
        let fork = edited.custom_copy(name.clone());
        draft.profiles[editing] = original.clone();
        let insert_at = editing + 1;
        let was_active = draft.active_profile == editing;
        draft.profiles.insert(insert_at, fork);
        if was_active {
            draft.active_profile = insert_at;
        } else if draft.active_profile >= insert_at {
            draft.active_profile += 1;
        }
        self.editing = insert_at;
        self.flash(format!("Встроенный профиль не меняется, изменения сохранены в «{name}»"), false);
    }

    fn dirty(&self) -> bool {
        match (&self.draft, &self.base) {
            (Some(d), Some((b, _))) => d != b,
            _ => false,
        }
    }

    pub fn flash(&mut self, text: impl Into<String>, error: bool) {
        self.flash = Some((text.into(), Instant::now(), error));
    }

    pub fn open_player(&mut self, ctx: &egui::Context, playlist: Vec<PathBuf>, index: usize) {
        if playlist.is_empty() {
            return;
        }
        self.player = Some(player::PlayerView::new(ctx, playlist, index, self.player_volume));
    }

    pub fn wants_levels(&self) {
        self.remote.want_levels();
    }

    fn set_page(&mut self, ctx: &egui::Context, page: Page) {
        if page != self.page {
            self.page = page;
            self.settle_layout = true;
            self.page_epoch = ctx.input(|i| i.time);
            if page.is_settings() {
                self.last_settings = page;
            }
        }
    }

    fn handle_hotkey_capture(&mut self, ctx: &egui::Context) {
        let Some(action) = self.capture else { return };
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                if key == egui::Key::Escape {
                    self.capture = None;
                    return;
                }
                if key == egui::Key::Backspace && !modifiers.any() {
                    if let Some(d) = self.draft.as_mut() {
                        d.hotkeys.set(action, Hotkey::default());
                    }
                    self.capture = None;
                    return;
                }
                if let Some(vk) = egui_key_to_vk(key) {
                    let is_function = (0x70..=0x87).contains(&vk);
                    if !is_function && !modifiers.ctrl && !modifiers.alt && !modifiers.shift {
                        self.flash("Добавьте Ctrl, Alt или Shift: одиночные клавиши мешают играм", true);
                        continue;
                    }
                    let hotkey = Hotkey { vk, ctrl: modifiers.ctrl, alt: modifiers.alt, shift: modifiers.shift, win: false };
                    if let Some(d) = self.draft.as_mut() {
                        for other in HotkeyAction::ALL {
                            if other != action && d.hotkeys.get(other) == hotkey {
                                d.hotkeys.set(other, Hotkey::default());
                            }
                        }
                        d.hotkeys.set(action, hotkey);
                    }
                    self.capture = None;
                    return;
                }
            }
        }
    }

    fn poll_folder_pick(&mut self) {
        let Some((_, rx)) = &self.folder_pick else { return };
        let Ok(result) = rx.try_recv() else { return };
        let (target, _) = self.folder_pick.take().unwrap();
        if let (Some(path), Some(d)) = (result, self.draft.as_mut()) {
            match target {
                FolderTarget::Output => d.general.output_dir = path,
                FolderTarget::DiskBuffer => d.general.disk_buffer_dir = path,
            }
        }
    }

    fn update_meters(&mut self, ctx: &egui::Context, levels: &[(String, f32)]) {
        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05);
        let attack = 1.0 - (-dt / 0.045).exp();
        let release = 1.0 - (-dt / 0.22).exp();
        let mut moving = false;
        for (name, peak) in levels {
            let target = widgets::level_to_fraction(*peak);
            let meter = self.meters.entry(name.clone()).or_default();
            let k = if target > meter.level { attack } else { release };
            meter.level += (target - meter.level) * k;
            if meter.level >= meter.peak {
                meter.peak = meter.level;
                meter.peak_at = now;
            } else if now - meter.peak_at > 1.0 {
                meter.peak = (meter.peak - dt * 0.5).max(meter.level);
            }
            moving |= meter.level > 0.002 || (target - meter.level).abs() > 0.002 || meter.peak > meter.level + 0.002;
        }
        self.meters.retain(|name, _| levels.iter().any(|(n, _)| n == name));
        if moving {
            let focused = ctx.input(|i| i.focused);
            ctx.request_repaint_after(Duration::from_millis(if focused { 15 } else { 50 }));
        }
    }

    fn paint_background(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(screen, Rounding::ZERO, theme::BG);
        let target = ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| pos2((p.x - screen.left()) / screen.width().max(1.0), (p.y - screen.top()) / screen.height().max(1.0)))
            .unwrap_or(pos2(0.5, 0.5));
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        let follow = 1.0 - 0.94f32.powf(dt * 60.0);
        self.pointer += (target - self.pointer) * follow;
        if (target - self.pointer).length() > 0.002 {
            widgets::animate_frame(ctx);
        }
        let glow_center = pos2(screen.center().x, screen.top() - 40.0);
        let mut mesh = Mesh::default();
        let rings = 18u32;
        let segments = 64u32;
        for ring in 0..=rings {
            let r = 560.0 * ring as f32 / rings as f32;
            let k = 1.0 - ring as f32 / rings as f32;
            let color = Color32::from_rgba_unmultiplied(140, 165, 255, (k * k * 30.0) as u8);
            for s in 0..segments {
                let a = s as f32 / segments as f32 * std::f32::consts::TAU;
                mesh.colored_vertex(glow_center + vec2(a.cos() * r * 1.35, a.sin() * r * 0.75), color);
            }
        }
        for ring in 0..rings {
            for s in 0..segments {
                let a = ring * segments + s;
                let b = ring * segments + (s + 1) % segments;
                let c = a + segments;
                let d = b + segments;
                mesh.add_triangle(a, b, c);
                mesh.add_triangle(b, d, c);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        let shift = widgets::dist(18.0);
        let offset = vec2((self.pointer.x - 0.5) * shift, (self.pointer.y - 0.5) * shift);
        let (tempo, sway) = if self.reduced_motion { (0.5, 0.5) } else { (1.0, 1.0) };
        let t = ctx.input(|i| i.time) as f32 * tempo;
        for star in &self.stars {
            let angle = t * 0.11 * star.speed + star.phase;
            let drift = vec2(angle.cos(), (angle * 1.3 + star.phase).sin()) * (5.0 + 9.0 * star.depth) * sway;
            let p = pos2(screen.left() + star.x * screen.width(), screen.top() + star.y * screen.height()) - offset * star.depth + drift;
            let twinkle = 0.8 + 0.2 * (t * 0.7 * star.speed + star.phase * 2.0).sin();
            let a = ((40.0 + 150.0 * star.depth * star.depth) * twinkle) as u8;
            painter.circle_filled(p, star.size * (0.6 + star.depth * 0.6), Color32::from_rgba_unmultiplied(220, 228, 255, a));
        }
        let (minimized, focused) = ctx.input(|i| (i.viewport().minimized.unwrap_or(false), i.focused));
        if !minimized {
            ctx.request_repaint_after(Duration::from_millis(if focused { 33 } else { 100 }));
        }
    }

    fn header(&mut self, ctx: &egui::Context, frame: &Frame<'_>) {
        let line = widgets::tween(ctx, egui::Id::new("header_line"), if self.scrolled > 4.0 { 1.0 } else { 0.0 }, 0.4);
        egui::TopBottomPanel::top("header")
            .exact_height(72.0)
            .frame(egui::Frame::none().fill(Color32::from_rgba_unmultiplied(5, 5, 5, (190.0 * line) as u8)).inner_margin(Margin::symmetric(28.0, 0.0)))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                let y = painter.round_to_pixel_center(rect.bottom() - 1.0);
                painter.hline(rect.x_range().expand(28.0), y, Stroke::new(widgets::hair(ctx), widgets::alpha(theme::LINE, line)));
                ui.horizontal_centered(|ui| {
                    let (logo, _) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::hover());
                    paint_logo(ui, logo);
                    ui.add_space(2.0);
                    ui.label(widgets::tracked("Kage Replay", egui::FontId::new(16.5, theme::bold()), theme::INK, -0.2));
                    ui.add_space(18.0);
                    let mut section = match self.page {
                        Page::Dashboard => Section::Home,
                        Page::Clips => Section::Clips,
                        Page::Diagnostics => Section::Diagnostics,
                        _ => Section::Settings,
                    };
                    let options = [
                        (Section::Home, "Главная".to_string()),
                        (Section::Clips, "Клипы".to_string()),
                        (Section::Settings, "Настройки".to_string()),
                        (Section::Diagnostics, "Диагностика".to_string()),
                    ];
                    if widgets::segmented(ui, "main_nav", &mut section, &options, widgets::SegStyle::Nav) {
                        let page = match section {
                            Section::Home => Page::Dashboard,
                            Section::Clips => Page::Clips,
                            Section::Settings => self.last_settings,
                            Section::Diagnostics => Page::Diagnostics,
                        };
                        self.set_page(ctx, page);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let engine = frame.status.map(|s| &s.engine);
                        let buffer = engine.map(|e| e.buffer_active).unwrap_or(false);
                        let can_save = buffer && engine.map(|e| e.buffered_seconds > 0.5).unwrap_or(false);
                        let hint = self.base.as_ref().map(|(c, _)| c.hotkeys.get(HotkeyAction::SaveReplay)).filter(|h| h.is_set()).map(|h| h.label()).unwrap_or_default();
                        let response = widgets::button_sm(ui, widgets::Tone::Light, glyph::SAVE, "Сохранить повтор", can_save);
                        if response.clicked() {
                            self.remote.command(crate::ipc::Command::SaveReplay(None));
                        }
                        if !hint.is_empty() {
                            response.on_hover_text(format!("В игре: {hint}"));
                        }
                        ui.add_space(6.0);
                        status_chip(ui, frame);
                    });
                });
            });
    }

    fn save_bar(&mut self, ctx: &egui::Context) {
        let dirty = self.dirty();
        let t = widgets::tween_bool(ctx, egui::Id::new("save_bar"), dirty, 0.45);
        if t < 0.01 {
            return;
        }
        let offset = (1.0 - t) * widgets::dist(28.0);
        egui::Area::new(egui::Id::new("save_bar_area"))
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -22.0 + offset))
            .order(egui::Order::Foreground)
            .interactable(dirty)
            .show(ctx, |ui| {
                ui.multiply_opacity(t);
                let shadow_idx = ui.painter().add(egui::Shape::Noop);
                let inner = widgets::framed(ui, 26.0, Color32::from_rgb(16, 16, 16), Some(theme::LINE_2), Margin { left: 22.0, right: 8.0, top: 8.0, bottom: 8.0 }, |ui| {
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 36.0), egui::Sense::hover());
                            widgets::paint_status_dot(ui, dot.center(), theme::ACCENT, false);
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(widgets::medium("Есть неприменённые изменения", 14.5));
                                ui.label(widgets::faint("Захват перезапустится, буфер начнётся заново"));
                            });
                            ui.add_space(18.0);
                            if widgets::button_sm(ui, widgets::Tone::Ghost, "", "Отменить", true).clicked() {
                                self.draft = self.base.as_ref().map(|(c, _)| c.clone());
                            }
                            if widgets::button_sm(ui, widgets::Tone::Light, glyph::CHECK, "Применить", true).clicked() {
                                if let Some(d) = self.draft.clone() {
                                    self.remote.send(Outgoing::SetConfig(Box::new(d)));
                                }
                            }
                        });
                    });
                let shadow = Shadow { offset: vec2(0.0, 18.0), blur: 44.0, spread: 0.0, color: Color32::from_black_alpha(190) };
                ui.painter().set(shadow_idx, shadow.as_shape(inner.response.rect, Rounding::same(26.0)));
            });
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some((text, at, error)) = self.flash.clone() else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 3.4 {
            self.flash = None;
            return;
        }
        let enter = widgets::ease(age / widgets::dur(0.4));
        let leave = if age > 3.0 { 1.0 - (age - 3.0) / 0.4 } else { 1.0 };
        let opacity = (enter * leave).clamp(0.0, 1.0);
        let lift = (1.0 - enter) * widgets::dist(10.0);
        let base = if self.dirty() { -96.0 } else { -28.0 };
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, base + lift))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                let (fill, color, stroke) = if error { (Color32::from_rgb(28, 22, 12), theme::AMBER, widgets::alpha(theme::AMBER, 0.5)) } else { (theme::INK, theme::ON_LIGHT, theme::INK) };
                widgets::framed(ui, 20.0, fill, Some(stroke), Margin::symmetric(18.0, 10.0), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(widgets::icon(if error { glyph::WARNING } else { glyph::CHECK }, 13.0).color(color));
                        ui.label(widgets::medium(text, 14.0).color(color));
                    });
                });
            });
        ctx.request_repaint_after(Duration::from_millis(if self.reduced_motion { 50 } else { 16 }));
    }

    fn settings_nav(&mut self, ui: &mut egui::Ui) {
        let mut page = self.page;
        let options = [
            (Page::Video, "Видео".to_string()),
            (Page::Audio, "Звук".to_string()),
            (Page::Replay, "Повторы и запись".to_string()),
            (Page::Hotkeys, "Клавиши".to_string()),
            (Page::Profiles, "Профили".to_string()),
            (Page::General, "Общие".to_string()),
        ];
        if widgets::segmented(ui, "settings_nav", &mut page, &options, widgets::SegStyle::Solid) {
            let ctx = ui.ctx().clone();
            self.set_page(&ctx, page);
        }
        ui.add_space(26.0);
    }
}

fn status_chip(ui: &mut egui::Ui, frame: &Frame<'_>) {
    let engine = frame.status.map(|s| &s.engine);
    let recording = engine.map(|e| e.recording.active).unwrap_or(false);
    let buffer = engine.map(|e| e.buffer_active).unwrap_or(false);
    let (color, live, text) = if !frame.connected {
        (theme::AMBER, false, "Подключение…".to_string())
    } else if recording {
        (theme::REC, true, format!("Запись {}", util::format_duration(engine.map(|e| e.recording.elapsed_sec).unwrap_or(0.0))))
    } else if buffer {
        (theme::LIVE, true, format!("Буфер {}", util::format_duration(engine.map(|e| e.buffered_seconds).unwrap_or(0.0))))
    } else {
        (theme::INK_4, false, "Захват выключен".to_string())
    };
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::new(13.0, theme::medium()), theme::INK_2);
    let size = vec2(galley.size().x + 44.0, 36.0);
    let (alloc, _) = ui.allocate_exact_size(vec2(size.x.round(), size.y), egui::Sense::hover());
    widgets::paint_box(ui.painter(), alloc, 18.0, theme::white(8), Some(theme::LINE));
    let rect = widgets::snap(ui.painter(), alloc);
    widgets::paint_status_dot(ui, pos2(rect.left() + 19.0, rect.center().y), color, live);
    let pos = ui.painter().round_pos_to_pixels(pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0));
    ui.painter().galley(pos, galley, theme::INK_2);
}

pub fn paint_logo(ui: &egui::Ui, rect: Rect) {
    let painter = ui.painter();
    widgets::paint_box(painter, rect, rect.width() * 0.3, Color32::from_rgb(14, 14, 14), Some(theme::LINE_2));
    let rect = widgets::snap(painter, rect);
    let height = rect.height() * 0.56;
    let width = height * crate::icon_art::mark_aspect();
    let origin = (rect.center().x - width / 2.0 + rect.width() * 0.01, rect.center().y - height / 2.0);
    for polygon in crate::icon_art::MARK_POLYGONS {
        let points: Vec<Pos2> = polygon
            .iter()
            .map(|&(x, y)| {
                let (px, py) = crate::icon_art::mark_point(x, y, origin, height);
                pos2(px, py)
            })
            .collect();
        painter.add(egui::Shape::convex_polygon(points, theme::INK, Stroke::NONE));
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.02, 0.02, 0.02, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (status, devices, devices_loading, history, connected, config, saved_ok, remote_error, levels) = {
            let s = self.remote.state.lock();
            (s.status.clone(), s.devices.clone(), s.devices_loading, s.fps_history.clone(), s.connected, s.config.clone(), s.saved_ok, s.last_error.clone(), s.levels.clone())
        };
        let command_error = self.remote.state.lock().command_error.take();
        if let Some(error) = command_error {
            let mut chars = error.chars();
            let text: String = chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
            self.flash(text, true);
        }
        if self.page_epoch == 0.0 {
            self.page_epoch = ctx.input(|i| i.time).max(0.001);
        }
        if !self.title_styled {
            self.title_styled = style_title_bar();
        }
        self.sync_config(config);
        updates::watch(self, ctx, status.as_ref());
        if let Some((id, page)) = status.as_ref().and_then(|s| s.ui_request.clone()) {
            if self.ui_request.is_some_and(|seen| seen != id) {
                self.set_page(ctx, Page::from_name(&page));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.ui_request = Some(id);
        } else if status.is_some() && self.ui_request.is_none() {
            self.ui_request = Some(0);
        }
        if let Some(t) = saved_ok {
            if t.elapsed() < Duration::from_millis(600) && self.flash.as_ref().map(|f| f.1 < t).unwrap_or(true) {
                self.flash("Настройки применены", false);
            }
        }
        self.handle_hotkey_capture(ctx);
        self.poll_folder_pick();
        self.update_meters(ctx, &levels);
        widgets::begin_frame(ctx, self.page_epoch);
        self.paint_background(ctx);
        let meters = self.meters.clone();
        let frame = Frame { status: status.as_ref(), devices: devices.as_ref(), devices_loading, history: &history, connected, meters: &meters };

        self.header(ctx, &frame);

        let bottom_pad = if self.dirty() { 120.0 } else { 48.0 };
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            if !connected && status.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.36);
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(widgets::strong("Запускаем службу Kage Replay…", 18.0));
                    if let Some(err) = &remote_error {
                        ui.label(widgets::faint(err));
                    }
                });
                return;
            }
            let mut scroll = egui::ScrollArea::vertical().id_salt(self.page as u8).auto_shrink([false; 2]);
            if self.settle_layout {
                scroll = scroll.vertical_scroll_offset(0.0);
            }
            let output = scroll.show(ui, |ui| {
                let max_width = 1040.0f32;
                let width = ui.available_width();
                let pad = ((width - max_width) / 2.0).max(32.0).round();
                let opacity = widgets::page_fade(ui.ctx());
                egui::Frame::none().inner_margin(Margin { left: pad, right: pad, top: 26.0, bottom: bottom_pad }).show(ui, |ui| {
                    ui.set_max_width(max_width.min(width - 64.0).round());
                    if self.page.is_settings() {
                        self.settings_nav(ui);
                    }
                    ui.multiply_opacity(opacity);
                    match self.page {
                        Page::Dashboard => dashboard::show(self, ui, &frame),
                        Page::Clips => clips::show(self, ui),
                        Page::Video => settings::video(self, ui, &frame),
                        Page::Audio => settings::audio(self, ui, &frame),
                        Page::Replay => settings::replay(self, ui, &frame),
                        Page::Hotkeys => settings::hotkeys(self, ui, &frame),
                        Page::Profiles => settings::profiles(self, ui),
                        Page::General => settings::general(self, ui),
                        Page::Diagnostics => diagnostics::show(self, ui, &frame),
                    }
                });
            });
            self.scrolled = output.state.offset.y;
        });

        if self.settle_layout {
            self.settle_layout = false;
            ctx.request_discard("page layout");
        }
        self.fork_builtin_edits();
        self.save_bar(ctx);
        if let Some(view) = self.player.as_mut() {
            if !view.show(ctx) {
                self.player_volume = view.volume();
                self.player = None;
            }
        }
        updates::show_prompt(self, ctx);
        self.toast(ctx);
        if self.capture.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}
