use eframe::egui::{self, RichText};

use super::remote::Outgoing;
use super::widgets::{self, glyph};
use super::{pick_folder, theme, App, FolderTarget, Frame};
use crate::ipc::Command;
use crate::daemon::shell;
use crate::util;

fn joined(parts: &[String]) -> String {
    parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join("  ·  ")
}

fn mbit(kbps: f64) -> String {
    format!("{:.1} Мбит/с", kbps / 1000.0)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, frame: &Frame<'_>) {
    widgets::page_header(ui, "Сервис", "Диагностика", "Оборудование, кодировщики и подробные счётчики: сколько ресурсов уходит на запись прямо сейчас.");
    ui.horizontal(|ui| {
        if widgets::ghost_button(ui, glyph::REFRESH, "Обновить сведения").clicked() {
            app.remote.send(Outgoing::RefreshDevices);
        }
        if widgets::ghost_button(ui, glyph::FOLDER, "Журналы").clicked() {
            shell::open_path(&util::local_data_dir().join("logs").display().to_string());
        }
        if widgets::ghost_button(ui, glyph::FOLDER, "Папка настроек").clicked() {
            shell::open_path(&util::app_data_dir().display().to_string());
        }
        if frame.devices_loading {
            ui.spinner();
        }
    });
    ui.add_space(18.0);
    if let Some(status) = frame.status {
        let e = &status.engine;
        super::updates::card(app, ui, status);
        widgets::gap(ui);
        cache_card(app, ui, e);
        widgets::gap(ui);
        let p = &e.process;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Процесс", "Фоновая служба Kage Replay: захват, кодирование и буфер");
            ui.columns(4, |cols| {
                widgets::stat(&mut cols[0], "CPU сейчас", format!("{:.2} %", p.cpu_now), theme::INK);
                widgets::stat(&mut cols[1], "CPU в среднем", format!("{:.2} %", p.cpu_avg), theme::INK);
                widgets::stat(&mut cols[2], "CPU максимум", format!("{:.2} %", p.cpu_peak), if p.cpu_peak > 10.0 { theme::AMBER } else { theme::INK });
                widgets::stat(&mut cols[3], "Память", util::format_bytes(p.working_set), theme::INK);
            });
            ui.add_space(14.0);
            widgets::key_value(
                ui,
                "Загрузка CPU",
                joined(&[
                    format!("{:.2} % сейчас", p.cpu_now),
                    format!("в среднем {:.2} %", p.cpu_avg),
                    format!("макс {:.2} %", p.cpu_peak),
                    format!("{:.3} ядра из {}", p.cpu_cores, p.logical_cpus),
                ]),
            );
            widgets::key_value(ui, "Время CPU", joined(&[format!("пользователь {:.1} с", p.user_sec), format!("ядро системы {:.1} с", p.kernel_sec)]));
            widgets::key_value(
                ui,
                "Память",
                joined(&[
                    format!("рабочий набор {}", util::format_bytes(p.working_set)),
                    format!("пик {}", util::format_bytes(p.peak_working_set)),
                    format!("выделено {}", util::format_bytes(p.private_bytes)),
                ]),
            );
            widgets::key_value(ui, "Запись на диск", joined(&[format!("{}/с", util::format_bytes(p.disk_write_bps as u64)), format!("всего {}", util::format_bytes(p.disk_written))]));
            widgets::key_value(ui, "Потоки и дескрипторы", format!("{} потоков  ·  {} дескрипторов", p.threads, p.handles));
            widgets::key_value(ui, "Служба работает", util::format_duration(p.running_sec));
            widgets::key_value(ui, "Сессия захвата", if e.session_active { util::format_duration(e.uptime_sec) } else { "не активна".into() });
            widgets::key_value(
                ui,
                "Буфер",
                joined(&[
                    format!("{} из {}", util::format_duration(e.buffered_seconds), util::format_duration(e.buffer_target_sec as f64)),
                    util::format_bytes(e.buffer_bytes),
                    if e.buffer_on_disk { "на диске".into() } else { "в памяти".into() },
                ]),
            );
            if let Some(game) = &e.game_detected {
                widgets::key_value(ui, "Обнаруженная игра", game.clone());
            }
        });
        widgets::gap(ui);
        for v in &e.video {
            widgets::card(ui, |ui| {
                widgets::section(ui, &format!("Конвейер: {}", v.info.source), &v.info.status);
                ui.columns(4, |cols| {
                    widgets::stat(&mut cols[0], "Уникальные к/с", format!("{:.1}", v.unique_fps), theme::INK);
                    widgets::stat(&mut cols[1], "В файл к/с", format!("{:.1}", v.output_fps), theme::INK);
                    let target = if v.info.target_kbps > 0 { format!("{} / {}", mbit(v.bitrate_kbps), mbit(v.info.target_kbps as f64)) } else { mbit(v.bitrate_kbps) };
                    widgets::stat(&mut cols[2], "Битрейт / цель", target, theme::INK);
                    widgets::stat(&mut cols[3], "GPU на кадр", format!("{:.2} мс", v.render_ms), theme::INK);
                });
                ui.add_space(14.0);
                widgets::key_value(ui, "Метод захвата", v.info.backend.clone());
                widgets::key_value(ui, "Видеокарта", v.info.adapter.clone());
                widgets::key_value(ui, "Кодировщик", joined(&[v.info.encoder.clone(), v.info.rate_control.clone(), if v.info.gop > 0 { format!("GOP {}", v.info.gop) } else { String::new() }]));
                widgets::key_value(ui, "Разрешение", format!("{}×{} → {}×{} @ {:.3} к/с", v.info.source_width, v.info.source_height, v.info.width, v.info.height, v.info.fps));
                widgets::key_value(
                    ui,
                    "Кадры в секунду",
                    joined(&[
                        format!("с источника {:.1}", v.capture_fps),
                        format!("уникальных {:.1}", v.unique_fps),
                        format!("повторов {:.1}", v.duplicate_fps),
                        format!("пропусков {:.1}", v.drop_fps),
                        format!("закодировано {:.1}", v.encode_fps),
                    ]),
                );
                widgets::key_value(
                    ui,
                    "Всего кадров",
                    joined(&[
                        format!("в файл {}", v.total_emitted),
                        format!("повторов {}", v.total_duplicated),
                        format!("пропущено {} (кодировщик занят {}, задержка потока {})", v.total_dropped, v.starved_total, v.late_total),
                    ]),
                );
                if let Some(err) = &v.info.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(err).color(theme::RED));
                }
            });
            widgets::gap(ui);
        }
        if !e.audio.is_empty() {
            app.wants_levels();
            widgets::card(ui, |ui| {
                widgets::section(ui, "Звук", &format!("{} дорож. в файле", e.audio_tracks));
                for source in &e.audio {
                    let meter = frame.meters.get(&source.name).copied().unwrap_or_default();
                    let db = |v: f32| if v > -59.5 { format!("{v:.1} дБ") } else { "тишина".to_string() };
                    widgets::key_value(ui, &source.name, joined(&[source.status.clone(), format!("уровень {}", db(meter.level_db())), format!("пик {}", db(meter.peak_db()))]));
                }
            });
            widgets::gap(ui);
        }
        if !status.hotkey_errors.is_empty() {
            widgets::card(ui, |ui| {
                widgets::section(ui, "Горячие клавиши", "");
                for err in &status.hotkey_errors {
                    ui.label(RichText::new(err).color(theme::AMBER));
                }
            });
            widgets::gap(ui);
        }
    }
    let Some(devices) = frame.devices else { return };
    widgets::card(ui, |ui| {
        widgets::section(ui, "Система", "");
        widgets::key_value(ui, "Сборка Windows", devices.os_build.to_string());
        widgets::key_value(ui, "Windows Graphics Capture", if devices.graphics_capture { "поддерживается" } else { "недоступен" });
        widgets::key_value(ui, "Захват звука приложений", if devices.os_build >= 20348 { "поддерживается" } else { "требуется Windows 11" });
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Мониторы", "");
        for m in &devices.monitors {
            widgets::key_value(
                ui,
                &m.name,
                format!("{}×{}  ·  {:.3} Гц ({}/{})  ·  {}{}", m.width, m.height, m.refresh_hz(), m.refresh_num, m.refresh_den, m.adapter_name, if m.primary { "  ·  основной" } else { "" }),
            );
        }
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Видеокодировщики", "Найдены через Media Foundation для видеокарты основного монитора");
        if devices.encoders.is_empty() {
            ui.label(RichText::new("Кодировщики не найдены. Установите драйвер видеокарты или Media Feature Pack для Windows N.").color(theme::AMBER));
        }
        for enc in &devices.encoders {
            ui.horizontal(|ui| {
                widgets::chip(ui, enc.codec.short(), if enc.hardware { theme::GOOD } else { theme::INK_2 });
                ui.label(&enc.name);
                ui.label(widgets::faint(if enc.hardware { "аппаратный" } else { "программный" }));
            });
            ui.add_space(4.0);
        }
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Аудиоустройства", "");
        for d in &devices.outputs {
            widgets::key_value(ui, if d.is_default { "Вывод, по умолчанию" } else { "Вывод" }, d.name.clone());
        }
        for d in &devices.inputs {
            widgets::key_value(ui, if d.is_default { "Ввод, по умолчанию" } else { "Ввод" }, d.name.clone());
        }
    });
}

fn cache_card(app: &mut App, ui: &mut egui::Ui, e: &crate::engine::EngineStatus) {
    let confirm_id = egui::Id::new("kage_confirm_clear_cache");
    let mut confirm = ui.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false);
    let mut pick = false;
    let mut clear = false;
    let applied_dir = e.cache_dir.clone();
    let Some(draft) = app.draft.as_mut() else { return };
    let pending_dir = draft.general.cache_dir().display().to_string();
    let g = &mut draft.general;
    let limit = g.max_cache_gb as u64 * 1024 * 1024 * 1024;
    widgets::card(ui, |ui| {
        widgets::section(ui, "Кеш", "Дисковый буфер повторов и тестовые записи микрофона. Файлы временные и удаляются сами");
        widgets::framed(ui, 14.0, theme::white(6), Some(theme::LINE), egui::Margin::symmetric(14.0, 10.0), |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(widgets::icon(glyph::FOLDER, 15.0).color(theme::ACCENT));
                ui.add(egui::Label::new(widgets::medium(&applied_dir, 14.0).color(theme::INK)).truncate());
            });
            if !applied_dir.eq_ignore_ascii_case(&pending_dir) {
                ui.label(RichText::new(format!("После применения: {pending_dir}")).size(12.5).color(theme::AMBER));
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(widgets::tracked(&util::format_bytes(e.cache_bytes), egui::FontId::new(22.0, theme::bold()), theme::INK, -0.4));
            ui.label(widgets::faint(format!("из {} ГБ", g.max_cache_gb)));
        });
        ui.add_space(6.0);
        widgets::progress(ui, (e.cache_bytes as f64 / limit.max(1) as f64) as f32, theme::INK, 6.0);
        ui.add_space(12.0);
        widgets::setting_row(ui, "Максимальный размер кеша", "Если дисковый буфер упрётся в предел, самые старые секунды удаляются", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut g.max_cache_gb, 1..=500).logarithmic(true).suffix(" ГБ"));
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::ghost_button(ui, glyph::FOLDER, "Изменить папку").clicked() {
                pick = true;
            }
            if widgets::ghost_button(ui, glyph::OPEN, "Открыть").clicked() {
                let _ = std::fs::create_dir_all(&applied_dir);
                shell::open_path(&applied_dir);
            }
            if confirm {
                if widgets::button_sm(ui, widgets::Tone::Danger, glyph::DELETE, "Да, очистить", true).clicked() {
                    clear = true;
                    confirm = false;
                }
                if widgets::button_sm(ui, widgets::Tone::Ghost, "", "Отмена", true).clicked() {
                    confirm = false;
                }
                ui.label(widgets::faint("Буфер повторов начнётся заново"));
            } else if widgets::button_sm(ui, widgets::Tone::Ghost, glyph::BROOM, "Очистить кеш", true).clicked() {
                confirm = true;
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(confirm_id, confirm));
    if pick {
        app.folder_pick = Some((FolderTarget::DiskBuffer, pick_folder()));
    }
    if clear {
        app.remote.command(Command::ClearCache);
        app.flash("Кеш очищен", false);
    }
}
