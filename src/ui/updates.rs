use eframe::egui::{self, pos2, vec2, Color32, Id, Margin, Rect, RichText, Rounding, Sense};

use super::widgets::{self, glyph, Tone};
use super::{theme, App};
use crate::engine::playback;
use crate::ipc::{Command, DaemonStatus};
use crate::update::{self, Download, DownloadStage, Release, CURRENT_VERSION};
use crate::util;

const PROMPT_WIDTH: f32 = 460.0;

pub struct Prompt {
    release: Release,
    notes: String,
    notes_open: bool,
    download: Option<Download>,
    opened_at: f64,
    height: f32,
}

impl Prompt {
    fn new(release: Release) -> Self {
        let notes = update::readable_notes(&release.notes);
        Self { release, notes, notes_open: false, download: None, opened_at: 0.0, height: 0.0 }
    }
}

enum Action {
    ToggleNotes,
    Install,
    Cancel,
    Later,
}

impl App {
    pub fn open_update(&mut self, release: Release) {
        if self.update_prompt.as_ref().is_some_and(|p| p.release == release) {
            return;
        }
        self.update_prompt = Some(Prompt::new(release));
    }
}

fn stamp(unix: u64) -> String {
    if unix == 0 {
        return "-".into();
    }
    util::local_stamp(update::unix_to_time(unix))
}

pub fn watch(app: &mut App, ctx: &egui::Context, status: Option<&DaemonStatus>) {
    let (focused, minimized) = ctx.input(|i| (i.viewport().focused.unwrap_or(true), i.viewport().minimized.unwrap_or(false)));
    if !focused || minimized {
        app.update_armed = true;
        return;
    }
    let Some(state) = status.map(|s| &s.update) else { return };
    if let Some(asked) = app.update_manual {
        if state.check_id > asked && !state.checking {
            app.update_manual = None;
            match (&state.available, &state.error) {
                (Some(release), _) => app.open_update(release.clone()),
                (None, Some(error)) => app.flash(error.clone(), true),
                (None, None) => app.flash("Последняя версия уже установлена", false),
            }
        }
    }
    if app.update_armed {
        app.update_armed = false;
        if let Some(release) = state.available.as_ref().filter(|_| !state.snoozed) {
            app.open_update(release.clone());
        }
    }
}

pub fn show_prompt(app: &mut App, ctx: &egui::Context) {
    let Some(prompt) = app.update_prompt.as_mut() else { return };
    let now = ctx.input(|i| i.time);
    if prompt.opened_at == 0.0 {
        prompt.opened_at = now;
    }
    let appear = widgets::ease(((now - prompt.opened_at) as f32 / widgets::dur(0.3)).clamp(0.0, 1.0));
    if appear < 1.0 || prompt.height == 0.0 {
        widgets::animate_frame(ctx);
    }
    let stage = prompt.download.as_ref().map(|d| d.stage.lock().clone());
    let downloading = matches!(stage, Some(DownloadStage::Downloading { .. }));
    let busy = matches!(stage, Some(DownloadStage::Downloading { .. } | DownloadStage::Verifying | DownloadStage::Launching | DownloadStage::Started));
    let failed = matches!(stage, Some(DownloadStage::Failed(_)));
    let screen = ctx.screen_rect();
    let mut action = None;
    egui::Area::new(Id::new("kage_update_prompt")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_rgba_unmultiplied(3, 3, 3, (210.0 * appear) as u8));
        ui.interact(screen, Id::new("kage_update_block"), Sense::click());
        ui.multiply_opacity(appear);
        let height = if prompt.height > 0.0 { prompt.height } else { 240.0 };
        let top = (screen.center().y - height / 2.0 + (1.0 - appear) * widgets::dist(14.0)).max(screen.top() + 24.0);
        let area = Rect::from_min_size(pos2(screen.center().x - PROMPT_WIDTH / 2.0, top), vec2(PROMPT_WIDTH, screen.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let card = widgets::framed(&mut child, 22.0, Color32::from_rgb(14, 14, 14), Some(theme::LINE_2), Margin::same(26.0), |ui| {
            ui.set_width(PROMPT_WIDTH - 52.0);
            widgets::caps(ui, "Обновление");
            ui.add_space(4.0);
            ui.label(widgets::strong(format!("Вышло новое обновление v{}", prompt.release.version), 22.0));
            ui.add_space(2.0);
            let mut meta = format!("Сейчас установлена версия {CURRENT_VERSION}");
            if prompt.release.asset_size > 0 {
                meta.push_str(&format!("  ·  установщик {}", util::format_bytes(prompt.release.asset_size)));
            }
            ui.label(widgets::faint(meta));
            if prompt.notes_open {
                ui.add_space(14.0);
                widgets::framed(ui, 14.0, theme::white(6), Some(theme::LINE), Margin::symmetric(16.0, 12.0), |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        let text = if prompt.notes.is_empty() { "Описание к этой версии не добавлено." } else { prompt.notes.as_str() };
                        ui.add(egui::Label::new(RichText::new(text).size(14.0).color(theme::INK_2)).wrap());
                    });
                });
            }
            if let Some(stage) = &stage {
                ui.add_space(16.0);
                match stage {
                    DownloadStage::Downloading { done, total } => {
                        let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                        widgets::progress(ui, fraction, theme::ACCENT, 6.0);
                        ui.add_space(6.0);
                        let size = if *total > 0 { format!("{} из {}", util::format_bytes(*done), util::format_bytes(*total)) } else { util::format_bytes(*done) };
                        ui.label(widgets::faint(format!("Скачивание  ·  {size}")));
                    }
                    DownloadStage::Verifying => {
                        ui.label(widgets::faint("Проверка файла…"));
                    }
                    DownloadStage::Launching => {
                        ui.label(widgets::faint("Запуск установщика…"));
                    }
                    DownloadStage::Started => {
                        widgets::notice(ui, glyph::CHECK, theme::GOOD, "Установщик запущен. После установки Kage Replay откроется сам.");
                    }
                    DownloadStage::Failed(error) => {
                        widgets::notice(ui, glyph::WARNING, theme::AMBER, &format!("Не удалось обновить: {error}"));
                    }
                }
            }
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if widgets::button_sm(ui, Tone::Ghost, glyph::INFO, if prompt.notes_open { "Скрыть описание" } else { "Что нового" }, true).clicked() {
                    action = Some(Action::ToggleNotes);
                }
                if widgets::button_sm(ui, Tone::Light, glyph::DOWNLOAD, if failed { "Повторить" } else { "Скачать и установить" }, !busy).clicked() {
                    action = Some(Action::Install);
                }
                if downloading {
                    if widgets::button_sm(ui, Tone::Ghost, "", "Отменить", true).clicked() {
                        action = Some(Action::Cancel);
                    }
                } else if widgets::button_sm(ui, Tone::Ghost, "", "Напомнить позже", !busy).clicked() {
                    action = Some(Action::Later);
                }
            });
        });
        prompt.height = card.response.rect.height();
    });
    match action {
        Some(Action::ToggleNotes) => prompt.notes_open = !prompt.notes_open,
        Some(Action::Install) => {
            let repaint = ctx.clone();
            prompt.download = Some(Download::start(prompt.release.clone(), move || repaint.request_repaint()));
        }
        Some(Action::Cancel) => {
            if let Some(download) = prompt.download.take() {
                download.cancel();
            }
        }
        Some(Action::Later) => {
            app.update_prompt = None;
            app.remote.command(Command::SnoozeUpdate);
            app.flash("Напомним через 12 часов", false);
        }
        None => {}
    }
}

pub fn card(app: &mut App, ui: &mut egui::Ui, status: &DaemonStatus) {
    let state = &status.update;
    let waiting = state.checking || app.update_manual.is_some();
    widgets::card(ui, |ui| {
        widgets::section(ui, "Программа", "Версии компонентов и обновления. Новые версии проверяются раз в час");
        ui.columns(3, |cols| {
            widgets::stat(&mut cols[0], "Версия приложения", status.version.clone(), theme::INK);
            widgets::stat(&mut cols[1], "Версия FFmpeg", playback::ffmpeg_version(), theme::INK);
            widgets::stat(&mut cols[2], "Последнее обновление", stamp(state.updated_at), theme::INK);
        });
        ui.add_space(14.0);
        if let Some(previous) = &state.updated_from {
            widgets::key_value(ui, "Предыдущая версия", previous.clone());
        }
        widgets::key_value(
            ui,
            "Последняя проверка",
            if state.checking {
                "Идёт проверка…".to_string()
            } else if state.last_check == 0 {
                "Ещё не проверялось".to_string()
            } else {
                stamp(state.last_check)
            },
        );
        let result = match (&state.available, &state.error) {
            (Some(release), _) => RichText::new(format!("Доступна версия {}", release.version)).color(theme::GOOD),
            (None, Some(error)) => RichText::new(error).color(theme::AMBER),
            (None, None) if state.last_check > 0 => RichText::new("Последняя версия уже установлена"),
            (None, None) => RichText::new("-"),
        };
        widgets::key_value(ui, "Состояние", result);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::button_sm(ui, Tone::Ghost, glyph::REFRESH, "Проверить наличие обновлений", !waiting).clicked() {
                app.update_manual = Some(state.check_id);
                app.remote.command(Command::CheckUpdates);
            }
            if let Some(release) = &state.available {
                if widgets::button_sm(ui, Tone::Light, glyph::DOWNLOAD, &format!("Установить {}", release.version), true).clicked() {
                    app.update_prompt = None;
                    app.open_update(release.clone());
                }
            }
            if waiting {
                ui.spinner();
            }
        });
    });
}
