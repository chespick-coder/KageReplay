use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use std::collections::HashSet;

use crossbeam_channel::Receiver;
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, Rounding, Sense, Shape, Stroke};
use eframe::epaint::{RectShape, Shadow};

use super::widgets::{self, glyph, Tone};
use super::{theme, App};
use crate::daemon::shell;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Replays,
    Recordings,
    Screenshots,
    Favorites,
    Backup,
}

#[derive(Clone)]
pub struct ClipItem {
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub size: u64,
    pub modified: SystemTime,
    pub stamp: String,
    pub duration: Option<f64>,
    pub screenshot: bool,
    pub recording: bool,
    pub backup: bool,
}

pub struct ClipsState {
    pub items: Vec<ClipItem>,
    pub loading: bool,
    receiver: Option<Receiver<Vec<ClipItem>>>,
    pub filter: Filter,
    pub search: String,
    last_scan: Option<Instant>,
    seen_activity: Option<u64>,
    scanned_dir: Option<PathBuf>,
    confirm_delete: Option<PathBuf>,
    shown: usize,
    favorites: Favorites,
}

#[derive(Default)]
struct Favorites {
    paths: HashSet<String>,
}

impl Favorites {
    fn file() -> PathBuf {
        util::app_data_dir().join("favorites.json")
    }

    fn key(path: &Path) -> String {
        path.to_string_lossy().to_lowercase()
    }

    fn load() -> Self {
        let paths = std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default();
        Self { paths: paths.iter().map(|p| Self::key(Path::new(p))).collect() }
    }

    fn save(&self) {
        let mut list: Vec<&String> = self.paths.iter().collect();
        list.sort();
        let Ok(text) = serde_json::to_vec_pretty(&list) else { return };
        let file = Self::file();
        let temp = file.with_extension("tmp");
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &file);
        }
    }

    fn contains(&self, path: &Path) -> bool {
        self.paths.contains(&Self::key(path))
    }

    fn toggle(&mut self, path: &Path) {
        let key = Self::key(path);
        if !self.paths.remove(&key) {
            self.paths.insert(key);
        }
        self.save();
    }

    fn forget(&mut self, path: &Path) {
        if self.paths.remove(&Self::key(path)) {
            self.save();
        }
    }
}

impl Default for ClipsState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            loading: false,
            receiver: None,
            filter: Filter::All,
            search: String::new(),
            last_scan: None,
            seen_activity: None,
            scanned_dir: None,
            confirm_delete: None,
            shown: PAGE_SIZE,
            favorites: Favorites::load(),
        }
    }
}

const PAGE_SIZE: usize = 36;

fn written_by_kage(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else { return false };
    let mut head = vec![0u8; 64 * 1024];
    let read = file.read(&mut head).unwrap_or(0);
    head[..read].windows(4).any(|w| w == b"Kage")
}

fn has_stamp(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.windows(19).any(|w| {
        let digit = |i: usize| w[i].is_ascii_digit();
        (0..4).all(digit)
            && w[4] == b'-'
            && digit(5)
            && digit(6)
            && w[7] == b'-'
            && digit(8)
            && digit(9)
            && w[10] == b' '
            && digit(11)
            && digit(12)
            && w[13] == b'-'
            && digit(14)
            && digit(15)
            && w[16] == b'-'
            && digit(17)
            && digit(18)
    })
}

fn read_box_header(file: &mut File) -> Option<(u64, [u8; 4], u64)> {
    let mut header = [0u8; 8];
    file.read_exact(&mut header).ok()?;
    let mut size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let kind = [header[4], header[5], header[6], header[7]];
    let mut header_len = 8;
    if size == 1 {
        let mut large = [0u8; 8];
        file.read_exact(&mut large).ok()?;
        size = u64::from_be_bytes(large);
        header_len = 16;
    }
    Some((size, kind, header_len))
}

pub fn probe_duration(path: &Path) -> Option<f64> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut pos = 0u64;
    while pos + 8 <= length {
        file.seek(SeekFrom::Start(pos)).ok()?;
        let (size, kind, header_len) = read_box_header(&mut file)?;
        if size < header_len {
            return None;
        }
        if &kind == b"moov" {
            let end = pos + size;
            let mut inner = pos + header_len;
            while inner + 8 <= end {
                file.seek(SeekFrom::Start(inner)).ok()?;
                let (isize, ikind, ihl) = read_box_header(&mut file)?;
                if &ikind == b"mvhd" {
                    let mut v = [0u8; 4];
                    file.read_exact(&mut v).ok()?;
                    if v[0] == 1 {
                        let mut buf = [0u8; 28];
                        file.read_exact(&mut buf).ok()?;
                        let timescale = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
                        let duration = u64::from_be_bytes([buf[20], buf[21], buf[22], buf[23], buf[24], buf[25], buf[26], buf[27]]);
                        return (timescale > 0).then(|| duration as f64 / timescale as f64);
                    } else {
                        let mut buf = [0u8; 16];
                        file.read_exact(&mut buf).ok()?;
                        let timescale = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
                        let duration = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
                        return (timescale > 0).then(|| duration as f64 / timescale as f64);
                    }
                }
                if isize < ihl {
                    return None;
                }
                inner += isize;
            }
            return None;
        }
        if size == 0 {
            return None;
        }
        pos += size;
    }
    None
}

fn scan(dir: PathBuf) -> Vec<ClipItem> {
    let backup_dir = dir.join(crate::config::BACKUP_FOLDER);
    let mut items = Vec::new();
    let mut stack = vec![(dir.clone(), 0)];
    while let Some((current, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let hidden = path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false);
            if meta.is_dir() {
                if depth < 3 && !hidden {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !matches!(ext.as_str(), "mp4" | "mkv" | "mov" | "png") {
                continue;
            }
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if !has_stamp(&name) || (ext != "png" && !written_by_kage(&path)) {
                continue;
            }
            let folder = path
                .parent()
                .filter(|p| *p != dir.as_path())
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let screenshot = ext == "png";
            items.push(ClipItem {
                duration: match ext.as_str() {
                    "png" => None,
                    "mkv" => crate::engine::mkv::probe_duration(&path),
                    _ => probe_duration(&path),
                },
                recording: name.starts_with("Запись"),
                backup: path.starts_with(&backup_dir),
                stamp: util::local_stamp(modified),
                path,
                name,
                folder,
                size: meta.len(),
                modified,
                screenshot,
            });
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.modified));
    items
}

fn recycle(path: &Path) -> bool {
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    let mut from: Vec<u16> = path.as_os_str().encode_wide_vec();
    from.push(0);
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    unsafe { SHFileOperationW(&mut op) == 0 }
}

trait EncodeWideVec {
    fn encode_wide_vec(&self) -> Vec<u16>;
}

impl EncodeWideVec for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::page_header(ui, "Библиотека", "Клипы", "Сохранённые повторы, записи и скриншоты. Нажмите на обложку, чтобы открыть.");
    let dir = app.base.as_ref().map(|(c, _)| c.general.output_dir.clone());
    let Some(dir) = dir else { return };
    let state = &mut app.clips;
    if let Some(rx) = &state.receiver {
        if let Ok(items) = rx.try_recv() {
            state.items = items;
            state.loading = false;
            state.receiver = None;
        }
    }
    let newest = app.remote.state.lock().status.as_ref().and_then(|s| s.activity.iter().find(|a| a.path.is_some()).map(|a| a.id));
    let fresh_file = newest.is_some() && newest != state.seen_activity;
    let stale = fresh_file || state.last_scan.map(|t| t.elapsed().as_secs() >= 3).unwrap_or(true) || state.scanned_dir.as_ref() != Some(&dir);
    if stale && !state.loading {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let scan_dir = dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(scan(scan_dir));
        });
        state.receiver = Some(rx);
        state.loading = true;
        state.last_scan = Some(Instant::now());
        state.seen_activity = newest;
        state.scanned_dir = Some(dir.clone());
    }
    if state.loading {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    ui.horizontal(|ui| {
        let options = [
            (Filter::All, "Все".to_string()),
            (Filter::Replays, "Повторы".to_string()),
            (Filter::Recordings, "Записи".to_string()),
            (Filter::Screenshots, "Скриншоты".to_string()),
            (Filter::Favorites, "Избранное".to_string()),
            (Filter::Backup, "Запаска".to_string()),
        ];
        if widgets::segmented(ui, "clip_filter", &mut state.filter, &options, widgets::SegStyle::Solid) {
            state.shown = PAGE_SIZE;
        }
        ui.add_space(8.0);
        widgets::text_field(ui, egui::TextEdit::singleline(&mut state.search).hint_text("Поиск по названию или игре").desired_width(230.0).margin(vec2(14.0, 9.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::ghost_button(ui, glyph::FOLDER, "Открыть папку").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                shell::open_path(&dir.display().to_string());
            }
            if state.loading {
                ui.spinner();
            }
        });
    });
    ui.add_space(14.0);
    let query = state.search.to_lowercase();
    let filtered: Vec<ClipItem> = state
        .items
        .iter()
        .filter(|c| match state.filter {
            Filter::All => !c.backup,
            Filter::Replays => !c.backup && !c.screenshot && !c.recording,
            Filter::Recordings => !c.backup && c.recording,
            Filter::Screenshots => !c.backup && c.screenshot,
            Filter::Favorites => state.favorites.contains(&c.path),
            Filter::Backup => c.backup,
        })
        .filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query) || c.folder.to_lowercase().contains(&query))
        .cloned()
        .collect();
    let total: u64 = filtered.iter().map(|c| c.size).sum();
    let count = filtered.len();
    let noun = match (count % 10, count % 100) {
        (1, n) if n != 11 => "файл",
        (2..=4, n) if !(12..=14).contains(&n) => "файла",
        _ => "файлов",
    };
    widgets::caps(ui, &format!("{count} {noun} · {}", util::format_bytes(total)));
    ui.add_space(10.0);
    if filtered.is_empty() && !state.loading {
        let favorites = state.filter == Filter::Favorites;
        widgets::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(widgets::icon(if favorites { glyph::STAR } else { glyph::CLIPS }, 36.0).color(theme::INK_3));
                ui.add_space(6.0);
                if favorites {
                    ui.label(widgets::tracked("В избранном пока пусто", FontId::new(20.0, theme::bold()), theme::INK, -0.4));
                    ui.label(widgets::faint("Нажмите на звёздочку в углу клипа или скриншота, чтобы добавить его сюда"));
                } else {
                    ui.label(widgets::tracked("Здесь появятся ваши клипы", FontId::new(20.0, theme::bold()), theme::INK, -0.4));
                    ui.label(widgets::faint("Сохраните повтор горячей клавишей прямо в игре"));
                }
                ui.add_space(24.0);
            });
        });
        return;
    }
    let width = ui.available_width();
    let gap = 16.0;
    let columns = ((width + gap) / (250.0 + gap)).floor().clamp(2.0, 4.0) as usize;
    let tile_width = (width - gap * (columns as f32 - 1.0)) / columns as f32;
    let visible: Vec<&ClipItem> = filtered.iter().take(state.shown).collect();
    let mut keep = HashSet::new();
    let mut removed = None;
    let mut open = None;
    let mut reveal = None;
    let mut star = None;
    let mut confirm = state.confirm_delete.clone();
    for row in visible.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for clip in row {
                keep.insert(clip.path.clone());
                let texture = app.thumbs.get(ui.ctx(), &clip.path);
                let favorite = app.clips.favorites.contains(&clip.path);
                match tile(ui, clip, texture.as_ref(), tile_width, confirm.as_ref() == Some(&clip.path), favorite) {
                    TileAction::Open => open = Some(clip.path.clone()),
                    TileAction::Star => star = Some(clip.path.clone()),
                    TileAction::Reveal => reveal = Some(clip.path.clone()),
                    TileAction::AskDelete => confirm = Some(clip.path.clone()),
                    TileAction::CancelDelete => confirm = None,
                    TileAction::Delete => {
                        if recycle(&clip.path) {
                            removed = Some(clip.path.clone());
                        }
                        confirm = None;
                    }
                    TileAction::None => {}
                }
            }
        });
        ui.add_space(gap + 4.0);
    }
    app.clips.confirm_delete = confirm;
    if filtered.len() > app.clips.shown {
        ui.vertical_centered(|ui| {
            if widgets::button(ui, Tone::Ghost, "", &format!("Показать ещё · осталось {}", filtered.len() - app.clips.shown), true).clicked() {
                app.clips.shown += PAGE_SIZE;
            }
        });
    }
    app.thumbs.retain(&keep);
    if let Some(path) = open {
        let is_image = path.extension().map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false);
        if is_image {
            shell::open_path(&path.display().to_string());
        } else {
            let playlist: Vec<PathBuf> = filtered.iter().filter(|c| !c.screenshot).map(|c| c.path.clone()).collect();
            let index = playlist.iter().position(|p| *p == path).unwrap_or(0);
            let ctx = ui.ctx().clone();
            app.open_player(&ctx, playlist, index);
        }
    }
    if let Some(path) = reveal {
        shell::reveal_file(&path.display().to_string());
    }
    if let Some(path) = star {
        app.clips.favorites.toggle(&path);
    }
    if let Some(path) = removed {
        app.clips.favorites.forget(&path);
        app.thumbs.forget(&path);
        app.clips.items.retain(|c| c.path != path);
    }
}

fn paint_stroke(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32) {
    let (_, stroke) = widgets::box_shapes(painter, rect, radius, Color32::TRANSPARENT, Some(color));
    painter.add(stroke);
}

enum TileAction {
    None,
    Open,
    Star,
    Reveal,
    AskDelete,
    CancelDelete,
    Delete,
}

fn overlay_button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, glyph_code: &str, hint: &str, visible: f32, danger: bool) -> bool {
    let response = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint);
    let hover = if response.hovered() { 1.0 } else { 0.0 };
    if visible > 0.01 {
        let fill = if danger && hover > 0.0 {
            widgets::lerp_color(Color32::from_rgba_unmultiplied(10, 10, 10, 170), Color32::from_rgb(255, 84, 92), hover)
        } else {
            widgets::lerp_color(Color32::from_rgba_unmultiplied(10, 10, 10, 170), theme::INK, hover)
        };
        let color = if danger { widgets::lerp_color(theme::INK, Color32::WHITE, hover) } else { widgets::lerp_color(theme::INK, theme::ON_LIGHT, hover) };
        ui.painter().circle(rect.center(), rect.width() / 2.0, widgets::alpha(fill, visible), Stroke::new(1.0_f32, widgets::alpha(theme::white(31), visible)));
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph_code, FontId::new(13.0, theme::icons()), widgets::alpha(color, visible));
    }
    visible > 0.5 && response.clicked()
}

fn paint_star(painter: &egui::Painter, center: egui::Pos2, radius: f32, filled: bool, color: Color32) {
    let inner = radius * 0.45;
    let points: Vec<egui::Pos2> = (0..10)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let r = if i % 2 == 0 { radius } else { inner };
            center + vec2(angle.cos(), angle.sin()) * r
        })
        .collect();
    let shift = vec2(0.0, radius * (1.0 - 36f32.to_radians().cos()) / 2.0);
    let points: Vec<egui::Pos2> = points.into_iter().map(|p| p + shift).collect();
    if filled {
        let mut mesh = eframe::epaint::Mesh::default();
        mesh.colored_vertex(center + shift, color);
        for p in &points {
            mesh.colored_vertex(*p, color);
        }
        for i in 0..10u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
        }
        painter.add(Shape::mesh(mesh));
    } else {
        painter.add(Shape::closed_line(points, Stroke::new(1.4_f32, color)));
    }
}

fn star_button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, favorite: bool, visible: f32) -> bool {
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if favorite { "Убрать из избранного" } else { "В избранное" });
    let hover = widgets::tween_bool(ui.ctx(), id.with("h"), response.hovered(), 0.2);
    let shown = if favorite { 1.0 } else { visible };
    if shown > 0.01 {
        let fill = widgets::lerp_color(Color32::from_rgba_unmultiplied(10, 10, 10, 170), Color32::from_rgba_unmultiplied(30, 30, 30, 220), hover);
        ui.painter().circle(rect.center(), rect.width() / 2.0, widgets::alpha(fill, shown), Stroke::new(1.0_f32, widgets::alpha(theme::white(31), shown)));
        let color = if favorite { theme::AMBER } else { widgets::lerp_color(theme::INK_2, theme::INK, hover) };
        paint_star(ui.painter(), rect.center(), 7.5, favorite, widgets::alpha(color, shown));
    }
    shown > 0.5 && response.clicked()
}

fn tile(ui: &mut egui::Ui, clip: &ClipItem, texture: Option<&egui::TextureHandle>, width: f32, confirming: bool, favorite: bool) -> TileAction {
    let thumb_height = (width * 9.0 / 16.0).round();
    let (rect, response) = ui.allocate_exact_size(vec2(width, thumb_height + 54.0), Sense::click());
    let id = response.id;
    let pointer_inside = ui.rect_contains_pointer(rect);
    let hover = widgets::tween_bool(ui.ctx(), id.with("hover"), pointer_inside || confirming, 0.3);
    let mut action = TileAction::None;
    if !ui.is_rect_visible(rect) {
        return action;
    }
    let thumb = widgets::snap(ui.painter(), Rect::from_min_size(rect.min, vec2(width, thumb_height)));
    let radius = 20.0;
    let painter = ui.painter().clone();
    if hover > 0.01 {
        let shadow = Shadow { offset: vec2(0.0, 22.0), blur: 50.0, spread: 0.0, color: Color32::from_black_alpha((140.0 * hover) as u8) };
        painter.add(shadow.as_shape(thumb, Rounding::same(radius)));
    }
    let base = if clip.screenshot { Color32::from_rgb(16, 18, 24) } else if clip.recording { Color32::from_rgb(24, 14, 16) } else { Color32::from_rgb(14, 14, 14) };
    painter.rect_filled(thumb, Rounding::same(radius), base);
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let image_aspect = size.x / size.y.max(1.0);
        let box_aspect = thumb.width() / thumb.height();
        let mut uv = if image_aspect > box_aspect {
            let w = box_aspect / image_aspect;
            Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
        } else {
            let h = image_aspect / box_aspect;
            Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
        };
        let zoom = 1.0 + 0.05 * hover;
        uv = Rect::from_center_size(uv.center(), uv.size() / zoom);
        let brightness = (225.0 + 30.0 * hover) as u8;
        let mut shape = RectShape::filled(thumb, Rounding::same(radius), Color32::from_gray(brightness));
        shape.fill_texture_id = texture.id();
        shape.uv = uv;
        painter.add(shape);
    } else {
        let glyph_code = if clip.screenshot { glyph::CAMERA } else { glyph::VIDEO };
        painter.text(thumb.center(), Align2::CENTER_CENTER, glyph_code, FontId::new(26.0, theme::icons()), theme::INK_4);
    }
    let shade = widgets::rounded_mesh(thumb, radius, 30.0, |p| {
        let t = (p.y - thumb.top()) / thumb.height();
        let a = if t < 0.3 { (0.3 - t) / 0.3 * 0.3 } else if t > 0.55 { (t - 0.55) / 0.45 * 0.65 } else { 0.0 };
        Color32::from_black_alpha((a * 255.0) as u8)
    });
    painter.add(Shape::mesh(shade));
    paint_stroke(&painter, thumb, radius, widgets::lerp_color(theme::LINE, theme::LINE_2, hover));
    let kind = if clip.screenshot {
        "Скриншот".to_string()
    } else {
        let label = if clip.recording { "Запись" } else { "Повтор" };
        match clip.duration {
            Some(d) => format!("{label} · {}", util::format_duration(d)),
            None => label.to_string(),
        }
    };
    let chip_galley = painter.layout_no_wrap(kind, FontId::new(12.0, theme::medium()), theme::INK);
    let chip = Rect::from_min_size(thumb.min + vec2(12.0, 12.0), vec2(chip_galley.size().x + 20.0, 26.0));
    widgets::paint_box(&painter, chip, 13.0, Color32::from_rgba_unmultiplied(10, 10, 10, 150), Some(theme::white(31)));
    painter.galley(pos2(chip.left() + 10.0, chip.center().y - chip_galley.size().y / 2.0), chip_galley, theme::INK);
    if !confirming {
        let play = widgets::ease(hover);
        if play > 0.01 {
            let r = 28.0 * (0.7 + 0.3 * play);
            painter.circle_filled(thumb.center(), r, widgets::alpha(Color32::from_rgba_unmultiplied(255, 255, 255, 235), play));
            let scale = 0.7 + 0.3 * play;
            if clip.screenshot {
                painter.text(thumb.center(), Align2::CENTER_CENTER, glyph::OPEN, FontId::new(17.0 * scale, theme::icons()), widgets::alpha(theme::ON_LIGHT, play));
            } else {
                widgets::play_icon(&painter, thumb.center(), 18.0 * scale, widgets::alpha(theme::ON_LIGHT, play));
            }
        }
        let delete_rect = Rect::from_center_size(pos2(thumb.right() - 26.0, thumb.top() + 26.0), vec2(30.0, 30.0));
        let folder_rect = delete_rect.translate(vec2(-38.0, 0.0));
        if overlay_button(ui, folder_rect, id.with("reveal"), glyph::FOLDER, "Показать в папке", hover, false) {
            action = TileAction::Reveal;
        }
        if overlay_button(ui, delete_rect, id.with("delete"), glyph::DELETE, "Удалить", hover, true) {
            action = TileAction::AskDelete;
        }
        let star_rect = Rect::from_center_size(pos2(thumb.right() - 26.0, thumb.bottom() - 26.0), vec2(30.0, 30.0));
        if star_button(ui, star_rect, id.with("star"), favorite, hover) {
            action = TileAction::Star;
        }
    } else {
        painter.rect_filled(thumb, Rounding::same(radius), Color32::from_black_alpha(170));
        painter.text(thumb.center() - vec2(0.0, 22.0), Align2::CENTER_CENTER, "Удалить в корзину?", FontId::new(15.0, theme::medium()), theme::INK);
        let yes = Rect::from_center_size(thumb.center() + vec2(-52.0, 18.0), vec2(92.0, 34.0));
        let no = Rect::from_center_size(thumb.center() + vec2(52.0, 18.0), vec2(92.0, 34.0));
        let yes_response = ui.interact(yes, id.with("yes"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let no_response = ui.interact(no, id.with("no"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let yes_hover = widgets::tween_bool(ui.ctx(), id.with("yes_h"), yes_response.hovered(), 0.25);
        let no_hover = widgets::tween_bool(ui.ctx(), id.with("no_h"), no_response.hovered(), 0.25);
        painter.rect_filled(yes, Rounding::same(17.0), widgets::lerp_color(Color32::from_rgb(220, 64, 72), Color32::from_rgb(255, 84, 92), yes_hover));
        painter.text(yes.center(), Align2::CENTER_CENTER, "Удалить", FontId::new(13.5, theme::medium()), Color32::WHITE);
        widgets::paint_box(&painter, no, 17.0, widgets::lerp_color(theme::white(20), theme::white(40), no_hover), Some(theme::LINE_2));
        painter.text(no.center(), Align2::CENTER_CENTER, "Отмена", FontId::new(13.5, theme::medium()), theme::INK);
        if yes_response.clicked() {
            action = TileAction::Delete;
        } else if no_response.clicked() {
            action = TileAction::CancelDelete;
        }
    }
    let name_pos = pos2(rect.left() + 6.0, thumb.bottom() + 10.0);
    let name_galley = painter.layout(clip.name.clone(), FontId::new(14.0, theme::medium()), theme::INK, width - 12.0);
    let first_row = name_galley.rows.first().map(|r| r.rect.height()).unwrap_or(16.0);
    let clip_rect = Rect::from_min_size(name_pos, vec2(width - 12.0, first_row + 1.0));
    ui.painter().with_clip_rect(clip_rect).galley(name_pos, name_galley, theme::INK);
    let mut meta = Vec::new();
    if !clip.folder.is_empty() {
        meta.push(clip.folder.clone());
    }
    meta.push(util::format_bytes(clip.size));
    meta.push(clip.stamp.clone());
    let meta_galley = painter.layout_no_wrap(meta.join(" · "), FontId::new(12.5, egui::FontFamily::Proportional), theme::INK_3);
    let meta_pos = pos2(rect.left() + 6.0, thumb.bottom() + 12.0 + first_row);
    ui.painter().with_clip_rect(Rect::from_min_size(meta_pos, vec2(width - 12.0, 18.0))).galley(meta_pos, meta_galley, theme::INK_3);
    if matches!(action, TileAction::None) && !confirming && response.clicked() {
        action = TileAction::Open;
    }
    action
}
