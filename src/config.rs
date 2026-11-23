use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    pub const ALL: [Codec; 3] = [Codec::H264, Codec::Hevc, Codec::Av1];

    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264 / AVC",
            Codec::Hevc => "H.265 / HEVC",
            Codec::Av1 => "AV1",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Container {
    #[default]
    Mp4,
    Mkv,
    Mov,
}

impl Container {
    pub const ALL: [Container; 3] = [Container::Mp4, Container::Mkv, Container::Mov];

    pub fn extension(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Mov => "mov",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::Mkv => "MKV",
            Container::Mov => "MOV",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderPreference {
    Auto,
    Hardware,
    Software,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateControl {
    Cbr,
    Vbr,
    Quality,
}

impl RateControl {
    pub fn short(self) -> &'static str {
        match self {
            RateControl::Cbr => "CBR",
            RateControl::Vbr => "VBR",
            RateControl::Quality => "CQ",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RateControl::Cbr => "CBR - постоянный битрейт",
            RateControl::Vbr => "VBR - переменный битрейт",
            RateControl::Quality => "CQ - постоянное качество",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum FpsMode {
    Fixed(u32),
    MonitorRefresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionMode {
    Native,
    Height(u32),
    Custom(u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureBackend {
    Auto,
    DesktopDuplication,
    GraphicsCapture,
}

impl CaptureBackend {
    pub fn label(self) -> &'static str {
        match self {
            CaptureBackend::Auto => "Автоматически",
            CaptureBackend::DesktopDuplication => "DXGI Desktop Duplication",
            CaptureBackend::GraphicsCapture => "Windows Graphics Capture",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoSource {
    Monitor { id: String, name: String },
    Window { exe: String, title: String },
}

impl VideoSource {
    pub fn display_name(&self) -> String {
        match self {
            VideoSource::Monitor { name, .. } => name.clone(),
            VideoSource::Window { exe, title } => {
                if title.is_empty() {
                    format!("Окно: {exe}")
                } else {
                    format!("Окно: {title} ({exe})")
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub sources: Vec<VideoSource>,
    pub codec: Codec,
    pub encoder: EncoderPreference,
    pub rate_control: RateControl,
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub quality: u32,
    pub fps: FpsMode,
    pub resolution: ResolutionMode,
    pub keyframe_interval_sec: f32,
    pub speed_quality: u32,
    pub capture_cursor: bool,
    #[serde(default)]
    pub container: Container,
    pub backend: CaptureBackend,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            sources: vec![VideoSource::Monitor { id: "primary".into(), name: "Основной монитор".into() }],
            codec: Codec::H264,
            encoder: EncoderPreference::Auto,
            rate_control: RateControl::Cbr,
            bitrate_kbps: 30_000,
            max_bitrate_kbps: 150_000,
            quality: 70,
            fps: FpsMode::Fixed(60),
            resolution: ResolutionMode::Native,
            keyframe_interval_sec: 1.0,
            speed_quality: 100,
            capture_cursor: true,
            container: Container::Mkv,
            backend: CaptureBackend::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioSourceKind {
    Output,
    Input,
    Process,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioFilters {
    pub noise_suppression: bool,
    pub suppression_strength: u32,
    pub gate: bool,
    pub gate_threshold_db: f32,
    pub high_pass: bool,
    pub high_pass_hz: u32,
    pub compressor: bool,
    pub compressor_threshold_db: f32,
    pub compressor_ratio: f32,
    pub mono: bool,
}

impl Default for AudioFilters {
    fn default() -> Self {
        Self {
            noise_suppression: false,
            suppression_strength: 100,
            gate: false,
            gate_threshold_db: -45.0,
            high_pass: false,
            high_pass_hz: 80,
            compressor: false,
            compressor_threshold_db: -18.0,
            compressor_ratio: 3.0,
            mono: false,
        }
    }
}

impl AudioFilters {
    pub fn microphone() -> Self {
        Self {
            noise_suppression: true,
            suppression_strength: 75,
            gate: false,
            gate_threshold_db: -50.0,
            high_pass: true,
            high_pass_hz: 100,
            compressor: false,
            compressor_threshold_db: -21.0,
            compressor_ratio: 2.5,
            mono: false,
        }
    }

    pub fn recommended() -> Self {
        Self {
            noise_suppression: true,
            suppression_strength: 85,
            gate: false,
            gate_threshold_db: -50.0,
            high_pass: true,
            high_pass_hz: 90,
            compressor: true,
            compressor_threshold_db: -22.0,
            compressor_ratio: 2.5,
            mono: true,
        }
    }

    pub fn any_enabled(&self) -> bool {
        self.noise_suppression || self.gate || self.high_pass || self.compressor || self.mono
    }

    fn normalize(&mut self) {
        self.suppression_strength = self.suppression_strength.clamp(10, 100);
        self.gate_threshold_db = self.gate_threshold_db.clamp(-80.0, -10.0);
        self.high_pass_hz = self.high_pass_hz.clamp(40, 300);
        self.compressor_threshold_db = self.compressor_threshold_db.clamp(-50.0, -1.0);
        self.compressor_ratio = self.compressor_ratio.clamp(1.5, 20.0);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSource {
    pub enabled: bool,
    pub kind: AudioSourceKind,
    pub device_id: String,
    pub name: String,
    pub process_exe: String,
    pub volume: f32,
    pub tracks: u8,
    #[serde(default)]
    pub exclude_from_system: bool,
    #[serde(default)]
    pub filters: AudioFilters,
}

impl Default for AudioSource {
    fn default() -> Self {
        AudioSource::default_output()
    }
}

impl AudioSource {
    pub fn default_output() -> Self {
        Self {
            enabled: true,
            kind: AudioSourceKind::Output,
            device_id: "default".into(),
            name: "Звук системы".into(),
            process_exe: String::new(),
            volume: 0.9,
            tracks: 0b001,
            exclude_from_system: false,
            filters: AudioFilters::default(),
        }
    }

    pub fn default_input() -> Self {
        Self {
            enabled: true,
            kind: AudioSourceKind::Input,
            device_id: "default".into(),
            name: "Микрофон".into(),
            process_exe: String::new(),
            volume: 1.2,
            tracks: 0b001,
            exclude_from_system: false,
            filters: AudioFilters::microphone(),
        }
    }
}

pub const MAX_AUDIO_TRACKS: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub sources: Vec<AudioSource>,
    pub bitrate_kbps: u32,
    pub track_names: Vec<String>,
    #[serde(default)]
    pub default_track: usize,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sources: vec![AudioSource::default_output(), AudioSource::default_input()],
            bitrate_kbps: 320,
            track_names: vec![
                "Общий микс".into(),
                "Звук системы".into(),
                "Микрофон".into(),
                "Дорожка 4".into(),
                "Дорожка 5".into(),
                "Дорожка 6".into(),
            ],
            default_track: 0,
        }
    }
}

impl AudioSettings {
    pub fn track_count(&self) -> usize {
        let mask = self.sources.iter().filter(|s| s.enabled).fold(0u8, |acc, s| acc | s.tracks);
        if mask == 0 {
            return 0;
        }
        (8 - mask.leading_zeros() as usize).min(MAX_AUDIO_TRACKS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BufferStorage {
    Memory,
    Disk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BufferActivation {
    Always,
    GamesOnly,
    Manual,
}

impl BufferActivation {
    pub fn label(self) -> &'static str {
        match self {
            BufferActivation::Always => "Всегда",
            BufferActivation::GamesOnly => "Только во время игр",
            BufferActivation::Manual => "Вручную",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplaySettings {
    pub duration_sec: u32,
    pub short_clip_sec: u32,
    pub storage: BufferStorage,
    pub max_memory_mb: u32,
    pub activation: BufferActivation,
    pub game_exes: Vec<String>,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            duration_sec: 180,
            short_clip_sec: 30,
            storage: BufferStorage::Disk,
            max_memory_mb: 850,
            activation: BufferActivation::Always,
            game_exes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct RecordingSettings {
    pub split_minutes: u32,
    pub include_buffer: bool,
    pub mp4_backup: bool,
}


#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub builtin: bool,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub replay: ReplaySettings,
    pub recording: RecordingSettings,
}

pub const BUILTIN_PROFILE: &str = "Встроенный";

impl Default for Profile {
    fn default() -> Self {
        Profile::named("Мой профиль")
    }
}

impl Profile {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.into(),
            builtin: false,
            video: VideoSettings::default(),
            audio: AudioSettings::default(),
            replay: ReplaySettings::default(),
            recording: RecordingSettings::default(),
        }
    }

    pub fn builtin() -> Self {
        Self { builtin: true, ..Profile::named(BUILTIN_PROFILE) }
    }

    pub fn custom_copy(&self, name: String) -> Self {
        Self { name, builtin: false, ..self.clone() }
    }

    fn same_settings(&self, other: &Profile) -> bool {
        let comparable = |profile: &Profile| {
            let mut profile = profile.clone();
            profile.name.clear();
            profile.builtin = false;
            for source in &mut profile.audio.sources {
                source.device_id.clear();
            }
            profile
        };
        comparable(self) == comparable(other)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Hotkey {
    pub vk: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Hotkey {
    pub const fn alt(vk: u32) -> Self {
        Self { vk, ctrl: false, alt: true, shift: false, win: false }
    }

    pub fn is_set(&self) -> bool {
        self.vk != 0
    }

    pub fn label(&self) -> String {
        if !self.is_set() {
            return "Не назначено".into();
        }
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.win {
            parts.push("Win".to_string());
        }
        parts.push(crate::keys::vk_name(self.vk));
        parts.join(" + ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    SaveReplay,
    SaveShortReplay,
    ToggleRecording,
    ToggleBuffer,
    Screenshot,
    NextProfile,
    Overlay,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 7] = [
        HotkeyAction::SaveReplay,
        HotkeyAction::SaveShortReplay,
        HotkeyAction::ToggleRecording,
        HotkeyAction::ToggleBuffer,
        HotkeyAction::Screenshot,
        HotkeyAction::NextProfile,
        HotkeyAction::Overlay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::SaveReplay => "Сохранить повтор",
            HotkeyAction::SaveShortReplay => "Сохранить короткий клип",
            HotkeyAction::ToggleRecording => "Начать / остановить запись",
            HotkeyAction::ToggleBuffer => "Включить / выключить буфер",
            HotkeyAction::Screenshot => "Скриншот",
            HotkeyAction::NextProfile => "Следующий профиль",
            HotkeyAction::Overlay => "Открыть оверлей",
        }
    }

    pub fn id(self) -> i32 {
        match self {
            HotkeyAction::SaveReplay => 1,
            HotkeyAction::SaveShortReplay => 2,
            HotkeyAction::ToggleRecording => 3,
            HotkeyAction::ToggleBuffer => 4,
            HotkeyAction::Screenshot => 5,
            HotkeyAction::NextProfile => 6,
            HotkeyAction::Overlay => 7,
        }
    }

    pub fn from_id(id: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub bindings: Vec<(HotkeyAction, Hotkey)>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            bindings: vec![
                (HotkeyAction::SaveReplay, Hotkey { vk: 0x76, ctrl: false, alt: false, shift: false, win: false }),
                (HotkeyAction::SaveShortReplay, Hotkey::alt(0x77)),
                (HotkeyAction::ToggleRecording, Hotkey::alt(0x75)),
                (HotkeyAction::ToggleBuffer, Hotkey { vk: 0x76, ctrl: false, alt: true, shift: true, win: false }),
                (HotkeyAction::Screenshot, Hotkey::alt(0x7A)),
                (HotkeyAction::NextProfile, Hotkey::default()),
                (HotkeyAction::Overlay, Hotkey::alt(0x5A)),
            ],
        }
    }
}

impl HotkeySettings {
    pub fn get(&self, action: HotkeyAction) -> Hotkey {
        self.bindings.iter().find(|(a, _)| *a == action).map(|(_, h)| *h).unwrap_or_default()
    }

    pub fn set(&mut self, action: HotkeyAction, hotkey: Hotkey) {
        if let Some(entry) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
            entry.1 = hotkey;
        } else {
            self.bindings.push((action, hotkey));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessPriority {
    Normal,
    AboveNormal,
    High,
}

impl ProcessPriority {
    pub fn label(self) -> &'static str {
        match self {
            ProcessPriority::Normal => "Обычный",
            ProcessPriority::AboveNormal => "Выше среднего",
            ProcessPriority::High => "Высокий",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub autostart: bool,
    pub output_dir: PathBuf,
    pub organize_by_app: bool,
    pub notifications: bool,
    pub sounds: bool,
    pub start_buffer_on_launch: bool,
    pub process_priority: ProcessPriority,
    pub gpu_priority: bool,
    pub disk_buffer_dir: PathBuf,
    pub min_free_space_gb: u32,
    #[serde(default = "default_cache_gb")]
    pub max_cache_gb: u32,
}

fn default_cache_gb() -> u32 {
    1
}

pub const BACKUP_FOLDER: &str = "Запаска";

impl GeneralSettings {
    pub fn cache_dir(&self) -> PathBuf {
        self.disk_buffer_dir.join(".kage-buffer")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.output_dir.join(BACKUP_FOLDER)
    }

    pub fn backup_for(&self, dir: &std::path::Path) -> PathBuf {
        match dir.strip_prefix(&self.output_dir) {
            Ok(relative) => self.backup_dir().join(relative),
            Err(_) => self.backup_dir(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            output_dir: util::default_videos_dir(),
            organize_by_app: true,
            notifications: true,
            sounds: true,
            start_buffer_on_launch: true,
            process_priority: ProcessPriority::High,
            gpu_priority: true,
            disk_buffer_dir: util::default_disk_buffer_dir(),
            min_free_space_gb: 10,
            max_cache_gb: default_cache_gb(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub active_profile: usize,
    pub profiles: Vec<Profile>,
    pub general: GeneralSettings,
    pub hotkeys: HotkeySettings,
    #[serde(default)]
    pub first_run_done: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 3,
            active_profile: 0,
            profiles: vec![Profile::builtin()],
            general: GeneralSettings::default(),
            hotkeys: HotkeySettings::default(),
            first_run_done: false,
        }
    }
}

fn salvage(text: &str) -> Option<AppConfig> {
    let saved: Value = serde_json::from_str(text).ok()?;
    let mut merged = serde_json::to_value(AppConfig::default()).ok()?;
    graft(&mut merged, String::new(), &saved);
    serde_json::from_value(merged).ok()
}

fn readable(root: &Value) -> bool {
    AppConfig::deserialize(root).is_ok()
}

fn graft(root: &mut Value, pointer: String, saved: &Value) {
    let Some(current) = root.pointer(&pointer).cloned() else { return };
    match (current, saved) {
        (Value::Object(_), Value::Object(fields)) => {
            for (key, value) in fields {
                let child = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                if root.pointer(&child).is_some() {
                    place(root, child, value);
                }
            }
        }
        (Value::Array(template), Value::Array(items)) => {
            let Some(blank) = template.first() else { return };
            let original = Value::Array(template.clone());
            if let Some(slot) = root.pointer_mut(&pointer) {
                *slot = Value::Array(vec![blank.clone(); items.len()]);
            }
            if !readable(root) {
                if let Some(slot) = root.pointer_mut(&pointer) {
                    *slot = original;
                }
                return;
            }
            for (index, item) in items.iter().enumerate() {
                place(root, format!("{pointer}/{index}"), item);
            }
        }
        _ => {}
    }
}

fn place(root: &mut Value, pointer: String, value: &Value) {
    let Some(slot) = root.pointer_mut(&pointer) else { return };
    let previous = std::mem::replace(slot, value.clone());
    if readable(root) {
        return;
    }
    if let Some(slot) = root.pointer_mut(&pointer) {
        *slot = previous;
    }
    graft(root, pointer, value);
}

impl AppConfig {
    pub fn path() -> PathBuf {
        util::app_data_dir().join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let mut attempt = 0;
        let bytes = loop {
            match std::fs::read(&path) {
                Ok(bytes) => break bytes,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return AppConfig::default(),
                Err(err) if attempt < 20 => {
                    attempt += 1;
                    crate::log_warn!("config read failed, retrying: {err}");
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                Err(err) => {
                    crate::log_error!("config unreadable, using defaults without overwriting it: {err}");
                    return AppConfig { first_run_done: true, ..AppConfig::default() };
                }
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        match serde_json::from_str::<AppConfig>(&text) {
            Ok(mut config) => {
                let loaded = config.clone();
                config.normalize();
                if config != loaded {
                    let _ = config.save();
                }
                config
            }
            Err(err) => {
                let _ = std::fs::copy(&path, path.with_extension("broken.json"));
                let mut config = salvage(&text).unwrap_or_else(|| AppConfig { first_run_done: true, ..AppConfig::default() });
                crate::log_warn!("config parse failed ({err}), kept every readable setting");
                config.normalize();
                let _ = config.save();
                config
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        static SAVING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        let _guard = SAVING.lock();
        let path = Self::path();
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn next_custom_name(&self) -> String {
        (1..).map(|n| format!("Мой профиль {n}")).find(|name| !self.profiles.iter().any(|p| &p.name == name)).unwrap_or_default()
    }

    fn ensure_builtin(&mut self) {
        let builtin = Profile::builtin();
        let migrating = !self.profiles.iter().any(|p| p.builtin);
        let active = self.active_profile;
        let mut active_is_builtin = self.profiles.get(active).is_none_or(|p| p.builtin);
        let mut custom = Vec::new();
        let mut custom_active = None;
        for (index, profile) in std::mem::take(&mut self.profiles).into_iter().enumerate() {
            if profile.builtin || (migrating && index == 0 && profile.same_settings(&builtin)) {
                if index == active {
                    active_is_builtin = true;
                }
                continue;
            }
            if index == active {
                custom_active = Some(custom.len() + 1);
            }
            custom.push(profile);
        }
        self.profiles = vec![builtin];
        for mut profile in custom {
            let name = profile.name.trim();
            if name.is_empty() || name == BUILTIN_PROFILE || self.profiles.iter().any(|p| p.name == name) {
                profile.name = self.next_custom_name();
            }
            self.profiles.push(profile);
        }
        self.active_profile = if active_is_builtin { 0 } else { custom_active.unwrap_or(0) };
    }

    pub fn normalize(&mut self) {
        self.ensure_builtin();
        let g = &mut self.general;
        if g.output_dir.as_os_str().is_empty() {
            g.output_dir = util::default_videos_dir();
        }
        if g.disk_buffer_dir.as_os_str().is_empty() || util::same_dir(&g.disk_buffer_dir, &g.output_dir) {
            g.disk_buffer_dir = util::default_disk_buffer_dir();
        }
        g.max_cache_gb = g.max_cache_gb.clamp(1, 500);
        let defaults = HotkeySettings::default();
        for action in HotkeyAction::ALL {
            if !self.hotkeys.bindings.iter().any(|(a, _)| *a == action) {
                self.hotkeys.bindings.push((action, defaults.get(action)));
            }
        }
        if self.version < 2 {
            for profile in &mut self.profiles {
                for source in profile.audio.sources.iter_mut().filter(|s| s.kind == AudioSourceKind::Input) {
                    if !source.filters.any_enabled() {
                        source.filters = AudioFilters::recommended();
                    }
                }
            }
            self.version = 2;
        }
        if self.version < 3 {
            self.general.max_cache_gb = default_cache_gb();
            self.version = 3;
        }
        for profile in &mut self.profiles {
            let v = &mut profile.video;
            v.bitrate_kbps = v.bitrate_kbps.clamp(1_000, 500_000);
            v.max_bitrate_kbps = v.max_bitrate_kbps.clamp(v.bitrate_kbps, 600_000);
            v.quality = v.quality.clamp(1, 100);
            v.speed_quality = v.speed_quality.min(100);
            v.keyframe_interval_sec = v.keyframe_interval_sec.clamp(0.25, 10.0);
            if v.resolution == ResolutionMode::Height(900) {
                v.resolution = ResolutionMode::Height(1080);
            }
            if let FpsMode::Fixed(fps) = &mut v.fps {
                *fps = (*fps).clamp(1, 360);
            }
            if v.sources.is_empty() {
                v.sources.push(VideoSource::Monitor { id: "primary".into(), name: "Основной монитор".into() });
            }
            let a = &mut profile.audio;
            if !matches!(a.bitrate_kbps, 96 | 128 | 160 | 192 | 256 | 320) {
                a.bitrate_kbps = 192;
            }
            while a.track_names.len() < MAX_AUDIO_TRACKS {
                a.track_names.push(format!("Дорожка {}", a.track_names.len() + 1));
            }
            a.default_track = a.default_track.min(a.track_count().saturating_sub(1));
            for s in &mut a.sources {
                s.volume = s.volume.clamp(0.0, 4.0);
                s.tracks &= (1 << MAX_AUDIO_TRACKS) - 1;
                s.filters.normalize();
            }
            let r = &mut profile.replay;
            r.duration_sec = r.duration_sec.clamp(5, 3600);
            r.short_clip_sec = r.short_clip_sec.clamp(5, r.duration_sec);
            r.max_memory_mb = r.max_memory_mb.clamp(128, 32_768);
        }
    }

    pub fn profile(&self) -> &Profile {
        &self.profiles[self.active_profile.min(self.profiles.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_known_values() {
        let text = r#"{ "general": { "max_cache_gb": 7 }, "profiles": [ { "name": "Игры", "video": { "bitrate_kbps": 25000 }, "audio": { "sources": [ { "name": "Микрофон" } ] } } ] }"#;
        let mut config: AppConfig = serde_json::from_str(text).expect("partial config must parse");
        config.normalize();
        assert_eq!(config.general.max_cache_gb, 7);
        assert_eq!(config.profiles[0], Profile::builtin());
        assert_eq!(config.profiles[1].name, "Игры");
        assert_eq!(config.profiles[1].video.bitrate_kbps, 25000);
        assert_eq!(config.profiles[1].audio.sources[0].name, "Микрофон");
        assert_eq!(config.active_profile, 1);
    }

    #[test]
    fn broken_fields_do_not_reset_other_settings() {
        let mut saved = AppConfig::default();
        saved.general.max_cache_gb = 42;
        saved.general.output_dir = PathBuf::from("D:/Clips");
        saved.profiles[0].name = "Игры".into();
        saved.profiles[0].video.bitrate_kbps = 12345;
        saved.profiles.push(Profile::named("Второй"));
        let mut value = serde_json::to_value(&saved).unwrap();
        value["profiles"][0]["video"]["codec"] = Value::String("FutureCodec".into());
        value["profiles"][1]["video"]["bitrate_kbps"] = Value::String("много".into());
        value["general"]["autostart"] = Value::from(5);
        let text = value.to_string();
        assert!(serde_json::from_str::<AppConfig>(&text).is_err());
        let config = salvage(&text).expect("salvage");
        assert_eq!(config.general.max_cache_gb, 42);
        assert_eq!(config.general.output_dir, PathBuf::from("D:/Clips"));
        assert_eq!(config.profiles.len(), 2);
        assert_eq!(config.profiles[0].name, "Игры");
        assert_eq!(config.profiles[0].video.bitrate_kbps, 12345);
        assert_eq!(config.profiles[0].video.codec, AppConfig::default().profiles[0].video.codec);
        assert_eq!(config.profiles[1].name, "Второй");
        assert_eq!(config.general.autostart, AppConfig::default().general.autostart);
    }

    #[test]
    fn builtin_profile_is_restored_and_kept_first() {
        let mut old = AppConfig {
            profiles: vec![Profile { builtin: false, name: "Основной".into(), ..Profile::builtin() }, Profile::named("Стрим")],
            ..AppConfig::default()
        };
        old.profiles[0].audio.sources[0].device_id = "{0.0.0.00000000}.{device}".into();
        old.active_profile = 1;
        old.normalize();
        assert_eq!(old.profiles.len(), 2);
        assert_eq!(old.profiles[0], Profile::builtin());
        assert_eq!(old.profiles[1].name, "Стрим");
        assert_eq!(old.active_profile, 1);

        let mut tampered = AppConfig::default();
        tampered.profiles[0].video.bitrate_kbps = 1;
        tampered.profiles[0].name = "Взлом".into();
        tampered.profiles.push(Profile { name: BUILTIN_PROFILE.into(), ..Profile::named("x") });
        tampered.normalize();
        assert_eq!(tampered.profiles[0], Profile::builtin());
        assert_eq!(tampered.profiles[1].name, "Мой профиль 1");
        assert_eq!(tampered.active_profile, 0);

        let mut changed = AppConfig::default();
        changed.profiles[0] = Profile { builtin: false, ..Profile::builtin() };
        changed.profiles[0].replay.duration_sec = 60;
        changed.normalize();
        assert_eq!(changed.profiles[0], Profile::builtin());
        assert_eq!(changed.profiles[1].replay.duration_sec, 60);
        assert_eq!(changed.active_profile, 1);
    }

    #[test]
    fn empty_config_is_default() {
        let config: AppConfig = serde_json::from_str("{}").expect("empty config must parse");
        assert_eq!(config.general.max_cache_gb, AppConfig::default().general.max_cache_gb);
    }

    #[test]
    fn current_config_roundtrips() {
        let config = AppConfig::default();
        let text = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), text);
    }
}
