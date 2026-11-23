use eframe::egui::{self, vec2, Align, Layout, RichText};

use super::remote::Outgoing;
use super::widgets::{self, glyph, SegStyle, Tone};
use super::{pick_folder, theme, App, FolderTarget, Frame};
use crate::config::*;
use crate::ipc::Command;
use crate::util;

fn profile_bar(app: &mut App, ui: &mut egui::Ui) {
    let Some(draft) = app.draft.as_ref() else { return };
    let names: Vec<(usize, String)> = draft.profiles.iter().enumerate().map(|(i, p)| (i, p.name.clone())).collect();
    let active = draft.active_profile;
    widgets::framed(ui, 22.0, theme::PANEL, Some(theme::LINE), egui::Margin { left: 18.0, right: 8.0, top: 6.0, bottom: 6.0 }, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(widgets::icon(glyph::PROFILE, 14.0).color(theme::INK_2));
                ui.label(widgets::medium("Профиль", 14.0));
                let mut editing = app.editing.min(names.len().saturating_sub(1));
                let options: Vec<(usize, String)> = names
                    .iter()
                    .map(|(i, n)| (*i, if *i == active { format!("{n}  · активный") } else { n.clone() }))
                    .collect();
                if widgets::combo(ui, "profile_bar", &mut editing, &options, 220.0) {
                    app.editing = editing;
                }
                if app.editing != active {
                    ui.label(widgets::faint("изменения вступят в силу, когда профиль станет активным"));
                }
            });
        });
    ui.add_space(18.0);
}

fn profile(app: &mut App) -> Option<&mut Profile> {
    let editing = app.editing;
    app.draft.as_mut().and_then(|d| d.profiles.get_mut(editing))
}

fn codec_available(frame: &Frame<'_>, codec: Codec) -> Option<bool> {
    let devices = frame.devices?;
    Some(devices.encoders.iter().any(|e| e.codec == codec && e.hardware) || (codec == Codec::H264 && devices.encoders.iter().any(|e| e.codec == codec)))
}

fn estimate_mb_per_minute(video: &VideoSettings, audio: &AudioSettings) -> f64 {
    let kbps = match video.rate_control {
        RateControl::Cbr => video.bitrate_kbps as f64,
        RateControl::Vbr => video.bitrate_kbps as f64 * 0.85,
        RateControl::Quality => video.max_bitrate_kbps as f64 * 0.6,
    };
    let audio_kbps = audio.bitrate_kbps as f64 * audio.track_count() as f64;
    (kbps + audio_kbps) * 60.0 / 8.0 / 1024.0
}

fn auto_tune(video: &mut VideoSettings, frame: &Frame<'_>) {
    let monitor = frame.devices.and_then(|d| d.monitors.iter().find(|m| m.primary).or(d.monitors.first()).cloned());
    let (w, h, hz) = monitor.map(|m| (m.width, m.height, m.refresh_hz())).unwrap_or((1920, 1080, 60.0));
    let hevc = codec_available(frame, Codec::Hevc).unwrap_or(false);
    let pixels = w as f64 * h as f64;
    let fps = if hz >= 100.0 && pixels <= 2_100_000.0 { 120.0f64.min(hz.round()) } else { 60.0f64.min(hz.round()) };
    video.fps = FpsMode::Fixed(fps as u32);
    video.resolution = if h > 1440 { ResolutionMode::Height(1440) } else { ResolutionMode::Native };
    let out_pixels = match video.resolution {
        ResolutionMode::Height(t) => pixels * (t as f64 / h as f64).powi(2),
        _ => pixels,
    };
    video.codec = if hevc { Codec::Hevc } else { Codec::H264 };
    let bpp = if hevc { 0.075 } else { 0.11 };
    let kbps = (out_pixels * fps * bpp / 1000.0).round().clamp(8_000.0, 120_000.0) as u32;
    video.rate_control = RateControl::Vbr;
    video.bitrate_kbps = kbps / 1000 * 1000;
    video.max_bitrate_kbps = (video.bitrate_kbps as f64 * 1.5) as u32;
    video.keyframe_interval_sec = 1.0;
    video.speed_quality = 50;
    video.encoder = EncoderPreference::Auto;
    video.backend = CaptureBackend::Auto;
}

pub fn video(app: &mut App, ui: &mut egui::Ui, frame: &Frame<'_>) {
    widgets::page_header(ui, "Настройки", "Видео", "Источники, кодек, частота кадров и качество изображения.");
    profile_bar(app, ui);
    let devices = frame.devices.cloned();
    let mut refresh = false;
    let mut flash: Option<(String, bool)> = None;
    {
        let Some(profile) = profile(app) else { return };
        let audio_settings = profile.audio.clone();
        let buffer_sec = profile.replay.duration_sec as f64;
        let backup = &mut profile.recording.mp4_backup;
        let v = &mut profile.video;

        widgets::card(ui, |ui| {
            widgets::section_with_action(ui, "Источники", "Каждый источник записывается в отдельный файл с общими звуковыми дорожками", |ui| {
                if widgets::ghost_button(ui, glyph::REFRESH, "Обновить").clicked() {
                    refresh = true;
                }
            });
            let mut remove = None;
            for (i, source) in v.sources.iter().enumerate() {
                ui.horizontal(|ui| {
                    let icon = match source {
                        VideoSource::Monitor { .. } => glyph::MONITOR,
                        VideoSource::Window { .. } => glyph::WINDOW,
                    };
                    ui.label(widgets::icon(icon, 16.0).color(theme::INK_2));
                    let mut label = source.display_name();
                    if let (VideoSource::Monitor { id, .. }, Some(d)) = (source, &devices) {
                        let found = if id == "primary" { d.monitors.iter().find(|m| m.primary) } else { d.monitors.iter().find(|m| &m.id == id) };
                        match found {
                            Some(m) => label = format!("{} · {}×{} @ {:.2} Гц", if id == "primary" { "Основной монитор".to_string() } else { m.name.clone() }, m.width, m.height, m.refresh_hz()),
                            None => label.push_str(" · не подключён, будет использован основной"),
                        }
                    }
                    ui.label(widgets::medium(label, 14.5));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if v.sources.len() > 1 && widgets::icon_button(ui, glyph::DELETE, "Убрать источник").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                v.sources.remove(i);
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(d) = &devices {
                    ui.menu_button(RichText::new(format!("{}  Добавить монитор", glyph::ADD)).family(theme::icons()), |ui| {
                        let primary = VideoSource::Monitor { id: "primary".into(), name: "Основной монитор".into() };
                        if !v.sources.contains(&primary) && ui.button("Основной монитор (следовать за основным)").clicked() {
                            v.sources.push(primary);
                            ui.close_menu();
                        }
                        for m in &d.monitors {
                            let source = VideoSource::Monitor { id: m.id.clone(), name: m.name.clone() };
                            let exists = v.sources.iter().any(|s| matches!(s, VideoSource::Monitor { id, .. } if id == &m.id));
                            if !exists && ui.button(m.label()).clicked() {
                                v.sources.push(source);
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button(RichText::new(format!("{}  Добавить окно", glyph::ADD)).family(theme::icons()), |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for w in &d.windows {
                                let text = format!("{} - {}", w.exe, w.title.chars().take(60).collect::<String>());
                                if ui.button(text).clicked() {
                                    v.sources.push(VideoSource::Window { exe: w.exe.clone(), title: w.title.clone() });
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    if ui.button("Заменить на основной монитор").clicked() {
                        v.sources = vec![VideoSource::Monitor { id: "primary".into(), name: "Основной монитор".into() }];
                    }
                } else {
                    ui.spinner();
                    ui.label(widgets::faint("Загружаем список мониторов и окон…"));
                }
            });
            ui.add_space(4.0);
            ui.label(widgets::faint("Окно захватывается через Windows Graphics Capture и подключается автоматически, когда приложение запускается."));
        });
        widgets::gap(ui);

        widgets::card(ui, |ui| {
            widgets::section_with_action(ui, "Кодирование", "NVIDIA NVENC напрямую, AMD AMF и Intel Quick Sync через Media Foundation - без копирования кадров в память", |ui| {
                if widgets::ghost_button(ui, glyph::BOLT, "Подобрать автоматически").clicked() {
                    let no_meters = std::collections::HashMap::new();
                    let fake = Frame { status: None, devices: devices.as_ref(), devices_loading: false, history: &[], connected: true, meters: &no_meters };
                    auto_tune(v, &fake);
                    flash = Some(("Параметры подобраны под ваш монитор и видеокарту".into(), false));
                }
            });
            let codec_options: Vec<(Codec, String)> = Codec::ALL
                .iter()
                .map(|&c| {
                    let suffix = match codec_available(frame, c) {
                        Some(true) => "",
                        Some(false) => " - нет на этой видеокарте",
                        None => "",
                    };
                    let note = match c {
                        Codec::H264 => " · максимальная совместимость",
                        Codec::Hevc => " · лучше качество при том же размере",
                        Codec::Av1 => " · самый эффективный, RTX 40+/RX 7000+/Arc",
                    };
                    (c, format!("{}{}{}", c.label(), note, suffix))
                })
                .collect();
            widgets::setting_row(ui, "Кодек", "H.264 откроется где угодно; HEVC и AV1 дают меньший размер файла", |ui| {
                widgets::combo(ui, "codec", &mut v.codec, &codec_options, 380.0);
            });
            if codec_available(frame, v.codec) == Some(false) {
                ui.label(RichText::new("Этот кодек не поддерживается видеокартой - запись не запустится. Выберите H.264.").color(theme::AMBER));
                ui.add_space(6.0);
            }
            widgets::setting_row(
                ui,
                "Контейнер файла",
                match v.container {
                    Container::Mp4 => "MP4: открывается везде, в том числе в Telegram, Discord и на телефонах",
                    Container::Mkv => "MKV: файл не повреждается, даже если запись оборвалась. Telegram и iPhone его не воспроизводят",
                    Container::Mov => "MOV: для Premiere, DaVinci и Final Cut. AV1 в MOV поддерживают не все программы",
                },
                |ui| {
                    let options: Vec<(Container, String)> = Container::ALL.iter().map(|c| (*c, c.label().to_string())).collect();
                    widgets::segmented(ui, "container", &mut v.container, &options, SegStyle::Solid);
                },
            );
            if v.container != Container::Mp4 {
                widgets::setting_row(ui, "Запасной вариант в MP4", "Каждый повтор и запись дополнительно сохраняются в MP4 в папку «Запаска». Файлы займут вдвое больше места", |ui| {
                    widgets::toggle(ui, backup);
                });
            }
            widgets::setting_row(ui, "Кодировщик", "Авто выбирает аппаратный, а при его отсутствии - программный H.264", |ui| {
                widgets::combo(
                    ui,
                    "encoder_pref",
                    &mut v.encoder,
                    &[
                        (EncoderPreference::Auto, "Автоматически".into()),
                        (EncoderPreference::Hardware, "Только аппаратный".into()),
                        (EncoderPreference::Software, "Программный (нагружает CPU)".into()),
                    ],
                    260.0,
                );
            });
            widgets::setting_row(ui, "Управление битрейтом", "CBR держит битрейт ровно как задано. VBR берёт столько, сколько нужно сцене, но не больше пика", |ui| {
                widgets::combo(
                    ui,
                    "rc",
                    &mut v.rate_control,
                    &[
                        (RateControl::Vbr, RateControl::Vbr.label().into()),
                        (RateControl::Cbr, RateControl::Cbr.label().into()),
                        (RateControl::Quality, RateControl::Quality.label().into()),
                    ],
                    260.0,
                );
            });
            match v.rate_control {
                RateControl::Quality => {
                    widgets::setting_row(ui, "Качество", "Чем выше, тем лучше картинка и больше файл", |ui| {
                        widgets::slider(ui, egui::Slider::new(&mut v.quality, 1..=100).suffix(" %"));
                    });
                    widgets::setting_row(ui, "Предел битрейта", "Ограничение для сложных сцен", |ui| {
                        let mut mbps = v.max_bitrate_kbps as f32 / 1000.0;
                        if widgets::slider(ui, egui::Slider::new(&mut mbps, 5.0..=200.0).suffix(" Мбит/с").max_decimals(0)).changed() {
                            v.max_bitrate_kbps = (mbps * 1000.0) as u32;
                        }
                    });
                }
                _ => {
                    widgets::setting_row(ui, "Битрейт", "Для 1080p60: 20-50 Мбит/с, для 1440p/4K или 120+ FPS - больше", |ui| {
                        let mut mbps = v.bitrate_kbps as f32 / 1000.0;
                        if widgets::slider(ui, egui::Slider::new(&mut mbps, 2.0..=150.0).suffix(" Мбит/с").max_decimals(0)).changed() {
                            v.bitrate_kbps = (mbps * 1000.0) as u32;
                            v.max_bitrate_kbps = v.max_bitrate_kbps.max(v.bitrate_kbps);
                        }
                    });
                    if v.rate_control == RateControl::Vbr {
                        widgets::setting_row(ui, "Пиковый битрейт", "Сколько кодировщик может взять на динамичных сценах", |ui| {
                            let mut mbps = v.max_bitrate_kbps as f32 / 1000.0;
                            let min = v.bitrate_kbps as f32 / 1000.0;
                            if widgets::slider(ui, egui::Slider::new(&mut mbps, min..=200.0).suffix(" Мбит/с").max_decimals(0)).changed() {
                                v.max_bitrate_kbps = (mbps * 1000.0) as u32;
                            }
                        });
                    }
                }
            }
            widgets::setting_row(ui, "Пресет: скорость ↔ качество", "Левее - меньше нагрузка на видеокарту, правее - лучше детализация", |ui| {
                widgets::slider(ui, egui::Slider::new(&mut v.speed_quality, 0..=100));
            });
            widgets::setting_row(ui, "Интервал ключевых кадров", "Клип начинается с ключевого кадра: меньший интервал - точнее начало", |ui| {
                widgets::slider(ui, egui::Slider::new(&mut v.keyframe_interval_sec, 0.5..=5.0).suffix(" с").step_by(0.5));
            });
        });
        widgets::gap(ui);

        widgets::card(ui, |ui| {
            widgets::section(ui, "Кадры и разрешение", "Kage Replay выдаёт ровно заданную частоту и честно показывает, сколько кадров уникальных");
            let refresh_hz = devices.as_ref().and_then(|d| d.monitors.iter().find(|m| m.primary).map(|m| m.refresh_hz()));
            let mut fps_options: Vec<(FpsMode, String)> = [30u32, 60, 90, 120, 144, 165, 240].iter().map(|f| (FpsMode::Fixed(*f), format!("{f}"))).collect();
            fps_options.push((
                FpsMode::MonitorRefresh,
                match refresh_hz {
                    Some(hz) => format!("Монитор · {hz:.2} Гц"),
                    None => "Как у монитора".into(),
                },
            ));
            if !fps_options.iter().any(|(m, _)| *m == v.fps) {
                if let FpsMode::Fixed(f) = v.fps {
                    fps_options.insert(0, (v.fps, format!("{f}")));
                }
            }
            widgets::caps(ui, "Частота кадров");
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                widgets::segmented(ui, "fps_mode", &mut v.fps, &fps_options, SegStyle::Solid);
                ui.add_space(6.0);
                if let FpsMode::Fixed(fps) = &mut v.fps {
                    ui.add(egui::DragValue::new(fps).range(1..=360).suffix(" к/с"));
                }
            });
            ui.label(widgets::faint("Режим «Как у монитора» использует точную частоту дисплея (например 143,98 Гц), поэтому кадры не теряются и не дублируются из-за расхождения таймингов."));
            ui.add_space(10.0);
            ui.add_space(6.0);
            widgets::caps(ui, "Разрешение");
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let custom = matches!(v.resolution, ResolutionMode::Custom(..));
                let mut options = vec![
                    (ResolutionMode::Native, "Исходное".to_string()),
                    (ResolutionMode::Height(2160), "2160p".into()),
                    (ResolutionMode::Height(1440), "1440p".into()),
                    (ResolutionMode::Height(1080), "1080p".into()),
                    (ResolutionMode::Height(720), "720p".into()),
                ];
                options.push((if custom { v.resolution } else { ResolutionMode::Custom(1920, 1080) }, "Своё".into()));
                widgets::segmented(ui, "resolution_mode", &mut v.resolution, &options, SegStyle::Solid);
                if let ResolutionMode::Custom(w, h) = &mut v.resolution {
                    ui.add_space(6.0);
                    ui.add(egui::DragValue::new(w).range(128..=7680));
                    ui.label("×");
                    ui.add(egui::DragValue::new(h).range(128..=4320));
                }
            });
            ui.label(widgets::faint("Уменьшение выполняется на GPU фильтром с 16 выборками - мелкий текст остаётся читаемым."));
            ui.add_space(10.0);
            widgets::setting_row(ui, "Показывать курсор", "", |ui| {
                widgets::toggle(ui, &mut v.capture_cursor);
            });
            widgets::setting_row(ui, "Метод захвата экрана", "Desktop Duplication - минимальная задержка для игр, Graphics Capture - совместимость", |ui| {
                widgets::combo(
                    ui,
                    "backend",
                    &mut v.backend,
                    &[
                        (CaptureBackend::Auto, CaptureBackend::Auto.label().into()),
                        (CaptureBackend::DesktopDuplication, CaptureBackend::DesktopDuplication.label().into()),
                        (CaptureBackend::GraphicsCapture, CaptureBackend::GraphicsCapture.label().into()),
                    ],
                    260.0,
                );
            });
        });
        widgets::gap(ui);
        let per_minute = estimate_mb_per_minute(v, &audio_settings);
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(widgets::icon(glyph::INFO, 16.0).color(theme::ACCENT));
                ui.label(format!(
                    "Примерно {:.0} МБ на минуту видео · буфер {} займёт ≈ {:.0} МБ",
                    per_minute,
                    util::format_duration(buffer_sec),
                    per_minute * buffer_sec / 60.0
                ));
            });
        });
    }

    if refresh {
        app.remote.send(Outgoing::RefreshDevices);
    }
    if let Some((text, error)) = flash {
        app.flash(text, error);
    }
}


pub fn audio(app: &mut App, ui: &mut egui::Ui, frame: &Frame<'_>) {
    widgets::page_header(ui, "Настройки", "Звук", "Источники, фильтры микрофона, громкость и дорожки для монтажа.");
    profile_bar(app, ui);
    app.wants_levels();
    let devices = frame.devices.cloned();
    let mut refresh = false;
    let mut mic_test = app.mic_test.take();
    let cache_dir = app.draft.as_ref().map(|d| d.general.cache_dir()).unwrap_or_else(|| crate::util::default_disk_buffer_dir().join(".kage-buffer"));
    let Some(profile) = profile(app) else {
        app.mic_test = mic_test;
        return;
    };
    let a = &mut profile.audio;
    widgets::card(ui, |ui| {
        widgets::section_with_action(ui, "Источники звука", "Отметьте, в какие дорожки попадает каждый источник. Полоска показывает уровень после фильтров", |ui| {
            if widgets::ghost_button(ui, glyph::REFRESH, "Обновить устройства").clicked() {
                refresh = true;
            }
        });
        let mut remove = None;
        let count = a.sources.len();
        for (i, s) in a.sources.iter_mut().enumerate() {
            widgets::framed(ui, 20.0, theme::white(6), Some(theme::LINE), egui::Margin::same(16.0), |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        widgets::toggle(ui, &mut s.enabled);
                        ui.add_space(4.0);
                        let icon = match s.kind {
                            AudioSourceKind::Output => glyph::AUDIO,
                            AudioSourceKind::Input => glyph::MIC,
                            AudioSourceKind::Process => glyph::APP,
                        };
                        ui.label(widgets::icon(icon, 16.0).color(theme::INK_2));
                        widgets::text_field(ui, egui::TextEdit::singleline(&mut s.name).desired_width(200.0).margin(vec2(12.0, 7.0)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if count > 1 && widgets::icon_button(ui, glyph::DELETE, "Удалить источник").clicked() {
                                remove = Some(i);
                            }
                            if let Some(meter) = frame.meters.get(&s.name) {
                                ui.add_space(6.0);
                                widgets::meter(ui, meter.level, meter.peak, (ui.available_width() - 12.0).clamp(60.0, 220.0));
                            }
                        });
                    });
                    ui.add_space(10.0);
                    match s.kind {
                        AudioSourceKind::Output | AudioSourceKind::Input => {
                            let list = devices.as_ref().map(|d| if s.kind == AudioSourceKind::Output { d.outputs.clone() } else { d.inputs.clone() }).unwrap_or_default();
                            let mut options = vec![("default".to_string(), "По умолчанию (следовать за Windows)".to_string())];
                            options.extend(list.iter().map(|d| (d.id.clone(), d.name.clone())));
                            if !options.iter().any(|(id, _)| id == &s.device_id) {
                                options.push((s.device_id.clone(), "Устройство отключено".into()));
                            }
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(vec2(96.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    ui.label(widgets::faint("Устройство"));
                                });
                                widgets::combo(ui, &format!("dev{i}"), &mut s.device_id, &options, 380.0);
                            });
                        }
                        AudioSourceKind::Process => {
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(vec2(96.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    ui.label(widgets::faint("Процесс"));
                                });
                                widgets::text_field(ui, egui::TextEdit::singleline(&mut s.process_exe).hint_text("game.exe").desired_width(200.0).margin(vec2(12.0, 7.0)));
                                if let Some(d) = &devices {
                                    ui.menu_button("Выбрать из запущенных", |ui| {
                                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                            let mut seen = std::collections::BTreeSet::new();
                                            for w in &d.windows {
                                                if seen.insert(w.exe.to_lowercase()) && ui.button(format!("{} · {}", w.exe, w.title.chars().take(40).collect::<String>())).clicked() {
                                                    s.process_exe = w.exe.clone();
                                                    ui.close_menu();
                                                }
                                            }
                                            ui.separator();
                                            for p in &d.processes {
                                                if seen.insert(p.to_lowercase()) && ui.button(p).clicked() {
                                                    s.process_exe = p.clone();
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                            ui.horizontal(|ui| {
                                widgets::toggle(ui, &mut s.exclude_from_system);
                                ui.label(if s.exclude_from_system { "Весь звук системы, кроме этого процесса" } else { "Только звук этого процесса и его дочерних" });
                            });
                            if devices.as_ref().map(|d| d.os_build < 20348).unwrap_or(false) {
                                ui.label(RichText::new("Захват звука отдельных приложений требует Windows 11 (или Windows 10 сборки 20348+).").color(theme::AMBER));
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(vec2(96.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.label(widgets::faint("Громкость"));
                        });
                        let mut percent = s.volume * 100.0;
                        if widgets::slider(ui, egui::Slider::new(&mut percent, 0.0..=300.0).suffix(" %").max_decimals(0)).changed() {
                            s.volume = percent / 100.0;
                        }
                        ui.add_space(14.0);
                        ui.label(widgets::faint("Дорожки"));
                        for t in 0..MAX_AUDIO_TRACKS {
                            let mut on = s.tracks & (1 << t) != 0;
                            if widgets::chip_toggle(ui, &format!("{}", t + 1), &mut on).changed() {
                                if on {
                                    s.tracks |= 1 << t;
                                } else {
                                    s.tracks &= !(1 << t);
                                }
                            }
                        }
                    });
                    if s.kind == AudioSourceKind::Input || s.filters.any_enabled() {
                        ui.add_space(12.0);
                        filters_block(ui, i, &mut s.filters);
                    }
                    if s.kind == AudioSourceKind::Input && s.enabled {
                        ui.add_space(8.0);
                        mic_test_block(ui, i, s, &mut mic_test, &cache_dir);
                    }
                });
            ui.add_space(10.0);
        }
        if let Some(i) = remove {
            a.sources.remove(i);
        }
        ui.horizontal_wrapped(|ui| {
            if widgets::ghost_button(ui, glyph::AUDIO, "Звук устройства вывода").clicked() {
                let mut s = AudioSource::default_output();
                s.name = "Звук системы".into();
                a.sources.push(s);
            }
            if widgets::ghost_button(ui, glyph::MIC, "Микрофон").clicked() {
                let mut s = AudioSource::default_input();
                s.enabled = true;
                a.sources.push(s);
            }
            if widgets::ghost_button(ui, glyph::APP, "Звук приложения").clicked() {
                a.sources.push(AudioSource {
                    enabled: true,
                    kind: AudioSourceKind::Process,
                    device_id: String::new(),
                    name: "Игра".into(),
                    process_exe: String::new(),
                    volume: 1.0,
                    tracks: 0b1001,
                    exclude_from_system: false,
                    filters: Default::default(),
                });
            }
        });
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Дорожки", "Как раскладывать звук по дорожкам файла. Имена дорожек видны в видеоредакторах");
        let scheme = track_scheme(a);
        let mut picked = scheme.unwrap_or(TrackScheme::Custom);
        let mut options = vec![
            (TrackScheme::Combined, "Одна общая дорожка".to_string()),
            (TrackScheme::MixAndSeparate, "Общая + раздельные".to_string()),
            (TrackScheme::Separate, "Только раздельные".to_string()),
        ];
        if scheme.is_none() {
            options.push((TrackScheme::Custom, "Своя схема".to_string()));
        }
        if widgets::segmented(ui, "track_scheme", &mut picked, &options, SegStyle::Solid) && picked != TrackScheme::Custom {
            apply_track_scheme(a, picked);
        }
        ui.add_space(6.0);
        ui.label(widgets::faint(match track_scheme(a) {
            Some(TrackScheme::Combined) => "Весь звук смешан в одну дорожку: любой плеер и мессенджер воспроизведут всё сразу.",
            Some(TrackScheme::MixAndSeparate) => "Дорожка 1 содержит общий микс, дальше каждый источник отдельно: при монтаже можно убрать или приглушить микрофон.",
            Some(TrackScheme::Separate) => "Каждый источник на своей дорожке, без общего микса.",
            _ => "Дорожки выбраны вручную у источников.",
        }));
        ui.add_space(12.0);
        let used = a.track_count();
        for t in 0..used {
            ui.horizontal(|ui| {
                let (badge, _) = ui.allocate_exact_size(vec2(28.0, 28.0), egui::Sense::hover());
                ui.painter().circle(badge.center(), 14.0, theme::white(10), egui::Stroke::new(1.0_f32, theme::LINE_2));
                ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, format!("{}", t + 1), egui::FontId::new(13.0, theme::bold()), theme::INK);
                widgets::text_field(ui, egui::TextEdit::singleline(&mut a.track_names[t]).desired_width(240.0).margin(vec2(12.0, 7.0)));
                let members: Vec<String> = a.sources.iter().filter(|s| s.enabled && s.tracks & (1 << t) != 0).map(|s| s.name.clone()).collect();
                ui.label(widgets::faint(if members.is_empty() { "тишина".to_string() } else { members.join(", ") }));
                if a.default_track == t && used > 1 {
                    widgets::chip(ui, "по умолчанию", theme::GOOD);
                }
            });
            ui.add_space(4.0);
        }
        if used == 0 {
            ui.label(widgets::faint("Звук не записывается: ни один источник не назначен на дорожку."));
        }
        ui.add_space(8.0);
        if used > 1 {
            let options: Vec<(usize, String)> = (0..used).map(|t| (t, format!("{} · {}", t + 1, a.track_names[t]))).collect();
            widgets::setting_row(ui, "Дорожка по умолчанию", "Её услышат плееры, Telegram, Discord и браузеры. Остальные переключаются в плеере (VLC: Аудио → Аудиодорожка)", |ui| {
                widgets::combo(ui, "default_track", &mut a.default_track, &options, 260.0);
            });
        }
        widgets::setting_row(ui, "Битрейт AAC на дорожку", "320 кбит/с: максимальное качество AAC. 192 кбит/с уже прозрачно для большинства", |ui| {
            widgets::combo(ui, "abitrate", &mut a.bitrate_kbps, &[(96, "96 кбит/с".into()), (128, "128 кбит/с".into()), (160, "160 кбит/с".into()), (192, "192 кбит/с".into()), (256, "256 кбит/с".into()), (320, "320 кбит/с".into())], 160.0);
        });
    });
    app.mic_test = mic_test;
    if refresh {
        app.remote.send(Outgoing::RefreshDevices);
    }
}

pub struct MicTest {
    source: usize,
    started: std::time::Instant,
    seconds: f32,
    level: std::sync::Arc<std::sync::atomic::AtomicU32>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    receiver: Option<crossbeam_channel::Receiver<Result<Vec<f32>, String>>>,
    raw: Option<std::sync::Arc<Vec<f32>>>,
    error: Option<String>,
    meter: f32,
    playing: Option<(bool, std::time::Instant, f32)>,
}

fn play_wav(path: Option<&std::path::Path>) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    unsafe {
        match path {
            Some(path) => {
                let wide = crate::util::WideString::new(&path.display().to_string());
                let _ = PlaySoundW(wide.pcwstr(), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
            }
            None => {
                let _ = PlaySoundW(windows::core::PCWSTR::null(), None, SND_ASYNC);
            }
        }
    }
}

fn render_test(raw: &[f32], source: &AudioSource, filtered: bool, path: &std::path::Path) -> Result<f32, String> {
    use crate::engine::audio_filters::FilterChain;
    let filters = if filtered { source.filters.clone() } else { AudioFilters::default() };
    let mut chain = FilterChain::new(&filters, source.volume);
    let mut out = Vec::new();
    let mut tail = raw.to_vec();
    tail.extend(std::iter::repeat_n(0.0, chain.latency_frames() as usize * 2));
    chain.process(&tail, &mut out);
    let skip = (chain.latency_frames() as usize * 2).min(out.len());
    let samples = &out[skip..];
    crate::engine::audio_capture::write_wav(path, samples).map_err(|e| e.to_string())?;
    Ok(samples.len() as f32 / 2.0 / crate::engine::audio_capture::SAMPLE_RATE as f32)
}

fn mic_test_block(ui: &mut egui::Ui, index: usize, source: &AudioSource, state: &mut Option<MicTest>, cache_dir: &std::path::Path) {
    use std::sync::atomic::Ordering;
    let ctx = ui.ctx().clone();
    if let Some(test) = state.as_mut() {
        if let Some(rx) = &test.receiver {
            if let Ok(result) = rx.try_recv() {
                test.receiver = None;
                match result {
                    Ok(raw) if !raw.is_empty() => test.raw = Some(std::sync::Arc::new(raw)),
                    Ok(_) => test.error = Some("Микрофон ничего не прислал".into()),
                    Err(err) => test.error = Some(err),
                }
            }
        }
    }
    let mine = state.as_ref().map(|t| t.source == index).unwrap_or(false);
    let recording = mine && state.as_ref().map(|t| t.receiver.is_some()).unwrap_or(false);
    ui.horizontal(|ui| {
        widgets::caps(ui, "Проверка микрофона");
        ui.add_space(6.0);
        ui.label(widgets::faint("Скажите пару фраз, пощёлкайте мышью и клавиатурой, затем сравните звук"));
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if recording {
            let test = state.as_mut().unwrap();
            let elapsed = test.started.elapsed().as_secs_f32();
            let peak = f32::from_bits(test.level.swap(0, Ordering::Relaxed));
            let target = widgets::level_to_fraction(peak);
            test.meter += (target - test.meter) * if target > test.meter { 0.5 } else { 0.12 };
            if widgets::button_sm(ui, widgets::Tone::Danger, glyph::STOP, &format!("Стоп · {:.0} с", (test.seconds - elapsed).max(0.0).ceil()), true).clicked() {
                test.stop.store(true, Ordering::SeqCst);
            }
            ui.add_space(8.0);
            widgets::meter(ui, test.meter, test.meter, 220.0);
            widgets::animate_frame(&ctx);
            return;
        }
        if widgets::button_sm(ui, widgets::Tone::Ghost, glyph::MIC, if mine && state.as_ref().map(|t| t.raw.is_some()).unwrap_or(false) { "Записать заново" } else { "Записать 5 секунд" }, true).clicked() {
            play_wav(None);
            let level = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (tx, rx) = crossbeam_channel::bounded(1);
            let config = source.clone();
            let thread_level = level.clone();
            let thread_stop = stop.clone();
            let notify = ctx.clone();
            std::thread::spawn(move || {
                let result = crate::engine::audio_capture::record_source(&config, 5.0, &thread_level, &thread_stop).map_err(|e| format!("{e:#}"));
                let _ = tx.send(result);
                notify.request_repaint();
            });
            *state = Some(MicTest {
                source: index,
                started: std::time::Instant::now(),
                seconds: 5.0,
                level,
                stop,
                receiver: Some(rx),
                raw: None,
                error: None,
                meter: 0.0,
                playing: None,
            });
            return;
        }
        let Some(test) = state.as_mut().filter(|t| t.source == index) else { return };
        if let Some(err) = &test.error {
            ui.label(RichText::new(err).color(theme::AMBER));
            return;
        }
        let Some(raw) = test.raw.clone() else { return };
        ui.add_space(6.0);
        for (filtered, label, tone) in [(true, "С фильтрами", widgets::Tone::Light), (false, "Без фильтров", widgets::Tone::Ghost)] {
            let active = test.playing.map(|(f, at, len)| f == filtered && at.elapsed().as_secs_f32() < len).unwrap_or(false);
            let glyph_code = if active { glyph::STOP } else { glyph::PLAY };
            if widgets::button_sm(ui, tone, glyph_code, label, true).clicked() {
                if active {
                    play_wav(None);
                    test.playing = None;
                } else {
                    let path = cache_dir.join(if filtered { "mic-test-filtered.wav" } else { "mic-test-raw.wav" });
                    match render_test(&raw, source, filtered, &path) {
                        Ok(len) => {
                            play_wav(Some(&path));
                            test.playing = Some((filtered, std::time::Instant::now(), len));
                        }
                        Err(err) => test.error = Some(err),
                    }
                }
            }
        }
        if let Some((_, at, len)) = test.playing {
            if at.elapsed().as_secs_f32() < len {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else {
                test.playing = None;
            }
        }
        ui.label(widgets::faint("Запись лежит в кеше. Меняйте фильтры и слушайте снова: та же запись обработается заново"));
    });
}

fn filter_row(ui: &mut egui::Ui, on: &mut bool, title: &str, hint: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        widgets::toggle(ui, on);
        ui.add_space(4.0);
        ui.allocate_ui_with_layout(vec2(230.0, 30.0), Layout::top_down(Align::Min), |ui| {
            ui.label(widgets::medium(title, 14.0));
            ui.label(RichText::new(hint).size(12.0).color(theme::INK_3));
        });
        ui.spacing_mut().slider_width = 170.0;
        ui.add_enabled_ui(*on, |ui| control(ui));
    });
    ui.add_space(6.0);
}

fn filters_block(ui: &mut egui::Ui, index: usize, f: &mut AudioFilters) {
    ui.horizontal(|ui| {
        widgets::caps(ui, "Фильтры");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::button_sm(ui, Tone::Ghost, glyph::FILTER, "Рекомендуемые", true).on_hover_text("Шумоподавление, гейт, срез низких, компрессор и моно").clicked() {
                *f = AudioFilters::recommended();
            }
            if f.any_enabled() && widgets::button_sm(ui, Tone::Ghost, "", "Выключить все", true).clicked() {
                *f = AudioFilters::default();
            }
        });
    });
    ui.add_space(6.0);
    ui.push_id(("filters", index), |ui| {
        filter_row(ui, &mut f.noise_suppression, "Шумоподавление", "RNNoise: убирает вентиляторы, клавиатуру, гул", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut f.suppression_strength, 10..=100).suffix(" %").text("сила"));
        });
        filter_row(ui, &mut f.gate, "Шумовой гейт", "Глушит микрофон, пока вы молчите", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut f.gate_threshold_db, -80.0..=-10.0).suffix(" дБ").max_decimals(0).text("порог"));
        });
        filter_row(ui, &mut f.high_pass, "Срез низких частот", "Убирает гул, удары по столу и шум ветра", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut f.high_pass_hz, 40..=300).suffix(" Гц").text("частота"));
        });
        filter_row(ui, &mut f.compressor, "Компрессор", "Выравнивает громкость: тихое громче, крик тише", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut f.compressor_threshold_db, -50.0..=-1.0).suffix(" дБ").max_decimals(0).text("порог"));
            widgets::slider(ui, egui::Slider::new(&mut f.compressor_ratio, 1.5..=20.0).prefix("1:").max_decimals(1).logarithmic(true));
        });
        filter_row(ui, &mut f.mono, "Моно", "Если микрофон слышно только в одном ухе", |_| {});
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackScheme {
    Combined,
    MixAndSeparate,
    Separate,
    Custom,
}

fn scheme_masks(sources: &[AudioSource], scheme: TrackScheme) -> Vec<u8> {
    let mut next = 0u8;
    sources
        .iter()
        .map(|s| {
            if !s.enabled {
                return s.tracks;
            }
            match scheme {
                TrackScheme::Combined => 0b1,
                TrackScheme::MixAndSeparate => {
                    next += 1;
                    0b1 | (1 << next.min(MAX_AUDIO_TRACKS as u8 - 1))
                }
                TrackScheme::Separate => {
                    let mask = 1 << next.min(MAX_AUDIO_TRACKS as u8 - 1);
                    next += 1;
                    mask
                }
                TrackScheme::Custom => s.tracks,
            }
        })
        .collect()
}

fn track_scheme(a: &AudioSettings) -> Option<TrackScheme> {
    let current: Vec<u8> = a.sources.iter().map(|s| s.tracks).collect();
    [TrackScheme::Combined, TrackScheme::MixAndSeparate, TrackScheme::Separate]
        .into_iter()
        .find(|&scheme| scheme_masks(&a.sources, scheme) == current)
}

fn apply_track_scheme(a: &mut AudioSettings, scheme: TrackScheme) {
    let masks = scheme_masks(&a.sources, scheme);
    for (s, mask) in a.sources.iter_mut().zip(masks) {
        s.tracks = mask;
    }
    let enabled: Vec<String> = a.sources.iter().filter(|s| s.enabled).map(|s| s.name.clone()).collect();
    match scheme {
        TrackScheme::Combined => a.track_names[0] = "Общий микс".into(),
        TrackScheme::MixAndSeparate => {
            a.track_names[0] = "Общий микс".into();
            for (i, name) in enabled.iter().enumerate().take(MAX_AUDIO_TRACKS - 1) {
                a.track_names[i + 1] = name.clone();
            }
        }
        TrackScheme::Separate => {
            for (i, name) in enabled.iter().enumerate().take(MAX_AUDIO_TRACKS) {
                a.track_names[i] = name.clone();
            }
        }
        TrackScheme::Custom => {}
    }
    a.default_track = 0;
}

pub fn replay(app: &mut App, ui: &mut egui::Ui, _frame: &Frame<'_>) {
    widgets::page_header(ui, "Настройки", "Повторы и запись", "Длительность мгновенного повтора, хранение буфера и параметры длинной записи.");
    profile_bar(app, ui);
    let mut new_game = std::mem::take(&mut app.new_game);
    let Some(profile) = profile(app) else { return };
    let per_minute = estimate_mb_per_minute(&profile.video, &profile.audio);
    let r = &mut profile.replay;
    widgets::card(ui, |ui| {
        widgets::section(ui, "Мгновенный повтор", "Последние минуты постоянно хранятся в буфере и сохраняются по горячей клавише");
        widgets::setting_row(ui, "Длительность повтора", "Сколько последних секунд попадает в клип", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut r.duration_sec, 10..=1200).logarithmic(true).custom_formatter(|v, _| util::format_duration(v)));
        });
        widgets::setting_row(ui, "Короткий клип", "Отдельная горячая клавиша для быстрых моментов", |ui| {
            let max = r.duration_sec;
            widgets::slider(ui, egui::Slider::new(&mut r.short_clip_sec, 5..=max).custom_formatter(|v, _| util::format_duration(v)));
        });
        widgets::setting_row(ui, "Когда работает буфер", "«Только во время игр» включает запись при полноэкранном приложении и выключает после его закрытия", |ui| {
            widgets::combo(
                ui,
                "activation",
                &mut r.activation,
                &[
                    (BufferActivation::Always, BufferActivation::Always.label().into()),
                    (BufferActivation::GamesOnly, BufferActivation::GamesOnly.label().into()),
                    (BufferActivation::Manual, BufferActivation::Manual.label().into()),
                ],
                240.0,
            );
        });
        if r.activation == BufferActivation::GamesOnly {
            ui.label(widgets::faint("Игры определяются автоматически. Добавьте исполняемые файлы, чтобы буфер включался и для оконных игр:"));
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, exe) in r.game_exes.iter().enumerate() {
                    if widgets::button_sm(ui, Tone::Ghost, glyph::CLOSE, exe, true).on_hover_text("Убрать").clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    r.game_exes.remove(i);
                }
                widgets::text_field(ui, egui::TextEdit::singleline(&mut new_game).hint_text("game.exe").desired_width(160.0).margin(vec2(12.0, 8.0)));
                if widgets::ghost_button(ui, glyph::ADD, "Добавить").clicked() && !new_game.trim().is_empty() {
                    r.game_exes.push(new_game.trim().to_string());
                    new_game.clear();
                }
            });
            ui.add_space(6.0);
        }
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Хранение буфера", "В памяти быстрее всего. На диске можно держать длинные повторы без расхода ОЗУ");
        widgets::segmented(ui, "storage", &mut r.storage, &[(BufferStorage::Memory, "В оперативной памяти".to_string()), (BufferStorage::Disk, "На диске".to_string())], SegStyle::Solid);
        ui.add_space(12.0);
        let need = per_minute * r.duration_sec as f64 / 60.0;
        if r.storage == BufferStorage::Memory {
            widgets::setting_row(ui, "Предел памяти", "Если буфер не помещается, самые старые секунды отбрасываются", |ui| {
                widgets::slider(ui, egui::Slider::new(&mut r.max_memory_mb, 256..=16384).logarithmic(true).suffix(" МБ"));
            });
            let color = if need > r.max_memory_mb as f64 { theme::AMBER } else { theme::INK_3 };
            ui.label(RichText::new(format!("Для {} при текущем битрейте нужно ≈ {:.0} МБ", util::format_duration(r.duration_sec as f64), need)).color(color).size(13.0));
        } else {
            ui.label(widgets::faint(format!("Буфер займёт ≈ {:.0} МБ на диске. Данные пишутся кусками по 64 МБ в скрытую служебную папку и удаляются автоматически. Папка задаётся в разделе «Общие».", need)));
        }
    });
    widgets::gap(ui);
    let rec = &mut profile.recording;
    widgets::card(ui, |ui| {
        widgets::section(ui, "Полная запись", "Файл пишется в устойчивом к сбоям формате и превращается в обычный MP4 при остановке");
        widgets::setting_row(ui, "Начинать запись с содержимого буфера", "Запись включит в себя последние минуты до нажатия кнопки", |ui| {
            widgets::toggle(ui, &mut rec.include_buffer);
        });
        widgets::setting_row(ui, "Делить запись на части", "0 - одним файлом", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut rec.split_minutes, 0..=240).suffix(" мин"));
        });
    });
    app.new_game = new_game;
}

pub fn hotkeys(app: &mut App, ui: &mut egui::Ui, frame: &Frame<'_>) {
    widgets::page_header(ui, "Настройки", "Горячие клавиши", "Работают в любой игре и приложении, даже когда окно Kage Replay закрыто.");
    if let Some(status) = frame.status {
        for err in &status.hotkey_errors {
            widgets::notice(ui, glyph::WARNING, theme::AMBER, err);
        }
    }
    let capture = app.capture;
    let Some(draft) = app.draft.as_mut() else { return };
    let mut start_capture = None;
    widgets::card(ui, |ui| {
        for action in HotkeyAction::ALL {
            let hotkey = draft.hotkeys.get(action);
            widgets::setting_row(ui, action.label(), "", |ui| {
                let capturing = capture == Some(action);
                let text = if capturing { "Нажмите сочетание…".to_string() } else { hotkey.label() };
                let fill = if capturing { widgets::alpha(theme::ACCENT, 0.18) } else { theme::white(10) };
                let stroke = if capturing { theme::ACCENT } else { theme::LINE_2 };
                let color = if hotkey.is_set() || capturing { theme::INK } else { theme::INK_3 };
                let button = egui::Button::new(widgets::medium(text, 14.0).color(color))
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0_f32, stroke))
                    .rounding(egui::Rounding::same(18.0))
                    .min_size(vec2(230.0, 36.0));
                if ui.add(button).clicked() {
                    start_capture = Some(action);
                }
            });
        }
        ui.add_space(4.0);
        ui.label(widgets::faint("Нажмите на кнопку и введите сочетание. Esc - отмена, Backspace - убрать назначение."));
    });
    if let Some(action) = start_capture {
        app.capture = Some(action);
    }
}

pub fn profiles(app: &mut App, ui: &mut egui::Ui) {
    widgets::page_header(
        ui,
        "Настройки",
        "Профили",
        "Наборы настроек под разные задачи. Встроенный профиль не меняется: если изменить в нём что-то, рядом появится «Мой профиль» с вашими изменениями.",
    );
    let dirty = app.dirty();
    let mut activate = None;
    let mut rename_start = None;
    let mut rename_commit = None;
    let mut duplicate = None;
    let mut delete = None;
    let rename = app.rename.clone();
    let Some(draft) = app.draft.as_mut() else { return };
    let active = draft.active_profile;
    let count = draft.profiles.len();
    for (i, p) in draft.profiles.iter().enumerate() {
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::icon_tile(ui, if i == active { glyph::STAR } else { glyph::PROFILE }, if i == active { 1.0 } else { 0.0 }, theme::INK_2);
                ui.add_space(4.0);
                match &rename {
                    Some((index, text)) if *index == i => {
                        let mut value = text.clone();
                        let response = widgets::text_field(ui, egui::TextEdit::singleline(&mut value).desired_width(240.0).margin(vec2(12.0, 7.0)));
                        if response.changed() {
                            rename_start = Some((i, value.clone()));
                        }
                        if response.lost_focus() || ui.button("Готово").clicked() {
                            rename_commit = Some((i, value));
                        }
                    }
                    _ => {
                        ui.label(widgets::tracked(&p.name, egui::FontId::new(17.0, theme::bold()), theme::INK, -0.3));
                    }
                }
                if i == active {
                    widgets::chip(ui, "Активный", theme::GOOD);
                }
                if p.builtin {
                    widgets::chip(ui, "Не меняется", theme::INK_3);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !p.builtin && count > 1 && widgets::icon_button(ui, glyph::DELETE, "Удалить").clicked() {
                        delete = Some(i);
                    }
                    if widgets::icon_button(ui, glyph::COPY, "Дублировать").clicked() {
                        duplicate = Some(i);
                    }
                    if !p.builtin && widgets::icon_button(ui, glyph::EDIT, "Переименовать").clicked() {
                        rename_start = Some((i, p.name.clone()));
                    }
                    if i != active && widgets::button_sm(ui, Tone::Light, glyph::CHECK, "Сделать активным", true).clicked() {
                        activate = Some(i);
                    }
                });
            });
            let fps = match p.video.fps {
                FpsMode::Fixed(f) => format!("{f} к/с"),
                FpsMode::MonitorRefresh => "частота монитора".into(),
            };
            let res = match p.video.resolution {
                ResolutionMode::Native => "исходное разрешение".to_string(),
                ResolutionMode::Height(h) => format!("{h}p"),
                ResolutionMode::Custom(w, h) => format!("{w}×{h}"),
            };
            ui.label(widgets::faint(format!(
                "{} · {} · {} · {:.0} Мбит/с · повтор {} · {} дорож.",
                p.video.codec.short(),
                fps,
                res,
                p.video.bitrate_kbps as f64 / 1000.0,
                util::format_duration(p.replay.duration_sec as f64),
                p.audio.track_count()
            )));
        });
        widgets::gap(ui);
    }
    if widgets::button(ui, Tone::Ghost, glyph::ADD, "Новый профиль", true).clicked() {
        let name = draft.next_custom_name();
        draft.profiles.push(Profile::builtin().custom_copy(name));
    }
    if let Some((i, text)) = rename_start {
        app.rename = Some((i, text));
    }
    if let Some((i, text)) = rename_commit {
        let name = text.trim().to_string();
        let taken = name == crate::config::BUILTIN_PROFILE || draft.profiles.iter().enumerate().any(|(j, p)| j != i && p.name == name);
        if let Some(p) = draft.profiles.get_mut(i).filter(|p| !p.builtin) {
            if !name.is_empty() && !taken {
                p.name = name;
            }
        }
        app.rename = None;
    }
    if let Some(i) = duplicate {
        let name = if draft.profiles[i].builtin { draft.next_custom_name() } else { format!("{} (копия)", draft.profiles[i].name) };
        let copy = draft.profiles[i].custom_copy(name);
        draft.profiles.insert(i + 1, copy);
        if draft.active_profile > i {
            draft.active_profile += 1;
        }
        if app.editing > i {
            app.editing += 1;
        }
    }
    if let Some(i) = delete {
        draft.profiles.remove(i);
        if draft.active_profile >= draft.profiles.len() || draft.active_profile > i {
            draft.active_profile = draft.active_profile.saturating_sub(1);
        }
        if app.editing > i {
            app.editing -= 1;
        }
        app.editing = app.editing.min(draft.profiles.len() - 1);
    }
    if let Some(i) = activate {
        if dirty {
            draft.active_profile = i;
        } else {
            app.remote.command(Command::SetProfile(i));
        }
        app.editing = i;
    }
}

pub fn general(app: &mut App, ui: &mut egui::Ui) {
    widgets::page_header(ui, "Настройки", "Общие", "Запуск, уведомления, папки и приоритет.");
    let mut pick = None;
    let Some(draft) = app.draft.as_mut() else { return };
    let g = &mut draft.general;
    widgets::card(ui, |ui| {
        widgets::section(ui, "Запуск", "");
        widgets::setting_row(ui, "Запускать вместе с Windows", "Kage Replay стартует в фоне и сидит в трее", |ui| {
            widgets::toggle(ui, &mut g.autostart);
        });
        widgets::setting_row(ui, "Включать буфер при запуске", "Работает в режиме «Всегда»", |ui| {
            widgets::toggle(ui, &mut g.start_buffer_on_launch);
        });
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Уведомления", "");
        widgets::setting_row(ui, "Всплывающие уведомления", "Небольшая плашка в углу экрана, не перехватывает фокус игры", |ui| {
            widgets::toggle(ui, &mut g.notifications);
        });
        widgets::setting_row(ui, "Звуковые сигналы", "Короткий сигнал при сохранении, старте и остановке записи", |ui| {
            widgets::toggle(ui, &mut g.sounds);
        });
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Файлы", "");
        widgets::setting_row(ui, "Папка для клипов и записей", &g.output_dir.display().to_string(), |ui| {
            if widgets::ghost_button(ui, glyph::FOLDER, "Изменить").clicked() {
                pick = Some(FolderTarget::Output);
            }
        });
        widgets::setting_row(ui, "Раскладывать по папкам игр", "Клипы попадают в подпапку с названием игры или приложения", |ui| {
            widgets::toggle(ui, &mut g.organize_by_app);
        });
        widgets::setting_row(ui, "Папка дискового буфера", &format!("{}\nВременные куски буфера лежат в скрытой подпапке .kage-buffer и удаляются сами", g.disk_buffer_dir.display()), |ui| {
            if widgets::ghost_button(ui, glyph::FOLDER, "Изменить").clicked() {
                pick = Some(FolderTarget::DiskBuffer);
            }
        });
        widgets::setting_row(ui, "Остановить запись, если свободно меньше", "Защита от переполнения диска", |ui| {
            widgets::slider(ui, egui::Slider::new(&mut g.min_free_space_gb, 1..=100).suffix(" ГБ"));
        });
    });
    widgets::gap(ui);
    widgets::card(ui, |ui| {
        widgets::section(ui, "Производительность", "");
        widgets::setting_row(ui, "Приоритет процесса", "«Выше среднего» защищает запись от подтормаживаний при загрузке CPU", |ui| {
            widgets::combo(
                ui,
                "prio",
                &mut g.process_priority,
                &[
                    (ProcessPriority::Normal, ProcessPriority::Normal.label().into()),
                    (ProcessPriority::AboveNormal, ProcessPriority::AboveNormal.label().into()),
                    (ProcessPriority::High, ProcessPriority::High.label().into()),
                ],
                200.0,
            );
        });
        widgets::setting_row(ui, "Повышенный приоритет GPU", "Захват не теряет кадры, когда игра нагружает видеокарту на 100%", |ui| {
            widgets::toggle(ui, &mut g.gpu_priority);
        });
    });
    if let Some(target) = pick {
        app.folder_pick = Some((target, pick_folder()));
    }
    ui.add_space(24.0);
    ui.horizontal(|ui| {
        if widgets::button_sm(ui, Tone::Danger, glyph::POWER, "Закрыть Kage Replay полностью", true).clicked() {
            app.remote.command(Command::Quit);
        }
        ui.label(widgets::faint("Фоновая служба завершится, горячие клавиши перестанут работать до следующего запуска."));
    });
}
