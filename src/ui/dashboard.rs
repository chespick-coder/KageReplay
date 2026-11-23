use eframe::egui::{self, vec2, Align, Layout, RichText};

use super::widgets::{self, glyph, Tone};
use super::{theme, App, Frame};
use crate::config::HotkeyAction;
use crate::daemon::shell;
use crate::engine::stats::VideoRates;
use crate::ipc::Command;
use crate::util;

fn hotkey_hint(app: &App, action: HotkeyAction) -> String {
    app.base
        .as_ref()
        .map(|(c, _)| c.hotkeys.get(action))
        .filter(|h| h.is_set())
        .map(|h| h.label())
        .unwrap_or_default()
}

pub fn show(app: &mut App, ui: &mut egui::Ui, frame: &Frame<'_>) {
    let Some(status) = frame.status else { return };
    let engine = &status.engine;
    let config = app.base.as_ref().map(|(c, _)| c.clone());
    let replay = config.as_ref().map(|c| c.profile().replay.clone()).unwrap_or_default();
    let rec = &engine.recording;
    let save_hint = hotkey_hint(app, HotkeyAction::SaveReplay);

    for err in &status.hotkey_errors {
        widgets::notice(ui, glyph::WARNING, theme::AMBER, err);
    }
    if let Some(err) = &engine.last_error {
        widgets::notice(ui, glyph::WARNING, theme::AMBER, err);
    }

    let (label, title, lead) = if rec.active {
        ("Запись", "Идёт запись".to_string(), format!("Файл растёт: {} · {}. Буфер повторов продолжает работать параллельно.", util::format_duration(rec.elapsed_sec), util::format_bytes(rec.bytes)))
    } else if engine.buffer_active {
        (
            "Мгновенный повтор",
            "Буфер пишет".to_string(),
            if save_hint.is_empty() {
                format!("Последние {} всегда под рукой. Сохраните их кнопкой ниже.", util::format_duration(replay.duration_sec as f64))
            } else {
                format!("Последние {} всегда под рукой. Нажмите {} в игре, чтобы сохранить повтор.", util::format_duration(replay.duration_sec as f64), save_hint)
            },
        )
    } else if let Some(game) = &engine.game_detected {
        ("Мгновенный повтор", "Игра найдена".to_string(), format!("{game}: буфер включится автоматически."))
    } else {
        ("Мгновенный повтор", "Захват выключен".to_string(), "Включите буфер, чтобы сохранять удачные моменты задним числом.".to_string())
    };

    widgets::card(ui, |ui| {
        widgets::caps(ui, label);
        ui.add_space(2.0);
        ui.label(widgets::tracked(&title, egui::FontId::new(44.0, theme::bold()), egui::Color32::WHITE, -1.8));
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(lead).size(16.0).color(theme::INK_2)).wrap());
        ui.add_space(22.0);
        let can_save = engine.buffer_active && engine.buffered_seconds > 0.5;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if widgets::button(ui, Tone::Light, glyph::SAVE, &format!("Сохранить повтор · {}", util::format_duration(replay.duration_sec as f64)), can_save).clicked() {
                app.remote.command(Command::SaveReplay(None));
            }
            if widgets::button(ui, Tone::Ghost, "", &format!("Последние {} с", replay.short_clip_sec), can_save).clicked() {
                app.remote.command(Command::SaveReplay(Some(replay.short_clip_sec)));
            }
            if widgets::button(ui, Tone::Ghost, glyph::CAMERA, "Скриншот", engine.session_active).clicked() {
                app.remote.command(Command::Screenshot);
            }
            if widgets::button(ui, Tone::Ghost, glyph::FOLDER, "Папка", true).clicked() {
                app.remote.command(Command::OpenOutputFolder);
            }
        });
        ui.add_space(22.0);
        let target = engine.buffer_target_sec.max(1) as f64;
        widgets::progress(ui, (engine.buffered_seconds / target) as f32, theme::INK, 6.0);
        ui.add_space(8.0);
        let place = if engine.buffer_on_disk { "на диске" } else { "в памяти" };
        ui.label(widgets::faint(format!(
            "В буфере {} из {} · {} {}",
            util::format_duration(engine.buffered_seconds),
            util::format_duration(target),
            util::format_bytes(engine.buffer_bytes),
            place
        )));
    });
    widgets::gap(ui);

    let full = ui.available_width();
    let gap = 16.0;
    let col = (full - gap) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.allocate_ui_with_layout(vec2(col, 0.0), Layout::top_down(Align::Min), |ui| {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::icon_tile(ui, glyph::REPLAY, if engine.buffer_active { 1.0 } else { 0.0 }, theme::INK);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(widgets::tracked("Буфер повторов", egui::FontId::new(17.0, theme::bold()), theme::INK, -0.3));
                        let state = if engine.buffer_active {
                            format!("работает · {}", util::format_duration(engine.buffered_seconds))
                        } else if let Some(game) = &engine.game_detected {
                            format!("ждёт: {game}")
                        } else {
                            "выключен".to_string()
                        };
                        ui.label(widgets::faint(state));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut on = engine.buffer_active;
                        if widgets::toggle(ui, &mut on).changed() {
                            app.remote.command(Command::ToggleBuffer);
                        }
                    });
                });
            });
        });
        ui.allocate_ui_with_layout(vec2(col, 0.0), Layout::top_down(Align::Min), |ui| {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::icon_tile(ui, glyph::RECORD, if rec.active { 1.0 } else { 0.0 }, theme::REC);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(widgets::tracked("Запись", egui::FontId::new(17.0, theme::bold()), theme::INK, -0.3));
                        let hint = hotkey_hint(app, HotkeyAction::ToggleRecording);
                        let state = if rec.active {
                            format!("{} · {}", util::format_duration(rec.elapsed_sec), util::format_bytes(rec.bytes))
                        } else if hint.is_empty() {
                            "остановлена".to_string()
                        } else {
                            format!("остановлена · {hint}")
                        };
                        ui.label(widgets::faint(state));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (tone, icon, text) = if rec.active { (Tone::Danger, glyph::STOP, "Стоп") } else { (Tone::Ghost, glyph::RECORD, "Начать") };
                        if widgets::button_sm(ui, tone, icon, text, frame.connected).clicked() {
                            app.remote.command(Command::ToggleRecording);
                        }
                    });
                });
            });
        });
    });
    widgets::gap(ui);

    if engine.video.is_empty() {
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::icon_tile(ui, glyph::POWER, 0.0, theme::INK_3);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(widgets::tracked("Захват сейчас не идёт", egui::FontId::new(17.0, theme::bold()), theme::INK, -0.3));
                    ui.label(widgets::faint("Включите буфер или начните запись: здесь появятся кадры, битрейт и кодировщик."));
                });
            });
        });
        widgets::gap(ui);
    }
    for (i, video) in engine.video.iter().enumerate() {
        video_card(ui, video, frame.history.get(i));
        widgets::gap(ui);
    }

    if !engine.audio.is_empty() {
        app.wants_levels();
        widgets::card(ui, |ui| {
            widgets::section(ui, "Звук", &format!("{} дорож. · AAC 48 кГц", engine.audio_tracks));
            for source in &engine.audio {
                let meter = frame.meters.get(&source.name).copied().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(200.0, 22.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.label(widgets::medium(&source.name, 14.0));
                    });
                    let ok = source.status == "Активен";
                    let width = (ui.available_width() - 150.0).max(80.0);
                    widgets::meter(ui, meter.level, meter.peak, width);
                    ui.add_space(8.0);
                    ui.label(RichText::new(&source.status).size(12.5).color(if ok { theme::INK_3 } else { theme::AMBER }));
                });
                ui.add_space(4.0);
            }
        });
        widgets::gap(ui);
    }

    let mut open_clip: Option<String> = None;
    widgets::card(ui, |ui| {
        widgets::section(ui, "Последние события", "");
        if status.activity.is_empty() {
            ui.label(widgets::faint("Пока пусто. Сохранённые повторы и записи появятся здесь."));
        }
        for item in status.activity.iter().take(10) {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(vec2(64.0, 30.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(&item.time).size(13.0).color(theme::INK_3));
                });
                let color = if item.error { theme::AMBER } else { theme::INK };
                ui.label(RichText::new(&item.text).color(color));
                if let Some(path) = &item.path {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, glyph::FOLDER, "Показать в папке").clicked() {
                            shell::reveal_file(path);
                        }
                        if widgets::icon_button(ui, glyph::PLAY, "Смотреть").clicked() {
                            open_clip = Some(path.clone());
                        }
                    });
                }
            });
        }
    });
    if let Some(path) = open_clip {
        let path = std::path::PathBuf::from(path);
        if path.extension().map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false) {
            shell::open_path(&path.display().to_string());
        } else {
            let ctx = ui.ctx().clone();
            app.open_player(&ctx, vec![path], 0);
        }
    }
    ui.add_space(14.0);
    ui.label(widgets::faint(format!("Kage Replay сейчас использует {:.1} % CPU и {} памяти", engine.cpu_percent, util::format_bytes(engine.memory_bytes))));
}

fn video_card(ui: &mut egui::Ui, v: &VideoRates, history: Option<&std::collections::VecDeque<(f64, f64)>>) {
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::icon_tile(ui, glyph::MONITOR, 0.0, theme::INK);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(widgets::tracked(&v.info.source, egui::FontId::new(17.0, theme::bold()), theme::INK, -0.3));
                ui.label(widgets::faint(format!("{}×{} · {:.2} к/с · {} · {}", v.info.width, v.info.height, v.info.fps, v.info.encoder, v.info.backend)));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let color = if v.info.error.is_some() { theme::RED } else if v.info.status == "Запись" { theme::GOOD } else { theme::AMBER };
                let status = if v.info.status == "Запись" { "Захват идёт".to_string() } else { v.info.status.clone() };
                widgets::chip(ui, &status, color);
            });
        });
        if let Some(err) = &v.info.error {
            ui.add_space(6.0);
            ui.label(RichText::new(err).color(theme::RED));
        }
        ui.add_space(14.0);
        let target = v.info.fps;
        ui.columns(5, |cols| {
            let unique_color = if v.unique_fps >= target * 0.97 { theme::GOOD } else { theme::INK };
            widgets::stat(&mut cols[0], "Уникальные", format!("{:.1}", v.unique_fps), unique_color);
            let out_color = if (v.output_fps - target).abs() <= target * 0.03 + 0.5 { theme::INK } else { theme::AMBER };
            widgets::stat(&mut cols[1], "В файл", format!("{:.1} / {:.0}", v.output_fps, target), out_color);
            widgets::stat(&mut cols[2], "Повторы", format!("{:.1}", v.duplicate_fps), theme::INK_2);
            let drop_color = if v.drop_fps > 0.0 { theme::RED } else { theme::INK_2 };
            widgets::stat(&mut cols[3], "Пропуски", format!("{:.1}", v.drop_fps), drop_color);
            widgets::stat(&mut cols[4], "Битрейт", format!("{:.1} Мбит/с", v.bitrate_kbps / 1000.0), theme::INK);
        });
        ui.add_space(12.0);
        if let Some(history) = history {
            widgets::sparkline(ui, history, target, 76.0);
        }
        ui.add_space(10.0);
        let (icon, color, text) = health(v);
        ui.horizontal(|ui| {
            ui.label(widgets::icon(icon, 14.0).color(color));
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(theme::INK_2)).wrap());
        });
    });
}

fn health(v: &VideoRates) -> (&'static str, egui::Color32, String) {
    let target = v.info.fps;
    if v.info.error.is_some() {
        return (glyph::WARNING, theme::RED, "Захват остановлен из-за ошибки. Проверьте настройки видео или откройте диагностику.".into());
    }
    if v.drop_fps > 0.5 {
        return (
            glyph::WARNING,
            theme::RED,
            format!("Кодировщик не успевает за {:.0} к/с: пропущено {:.1} кадров/с. Снизьте FPS или разрешение, выберите H.264 или более быстрый пресет.", target, v.drop_fps),
        );
    }
    if v.output_fps > 0.0 && v.unique_fps < target * 0.95 && v.duplicate_fps > 0.5 {
        return (
            glyph::INFO,
            theme::ACCENT,
            format!(
                "Экран обновлялся {:.0} раз/с, меньше цели {:.0}. Видео всё равно идёт ровно {:.0} к/с, недостающие кадры повторяются: так и должно быть на статичном экране или в игре с низким FPS.",
                v.unique_fps, target, target
            ),
        );
    }
    if v.output_fps > 0.0 {
        return (glyph::CHECK, theme::GOOD, format!("Частота соблюдается: {:.1} уникальных кадров в секунду без пропусков.", v.unique_fps));
    }
    (glyph::INFO, theme::INK_3, "Ожидание первых кадров…".into())
}
