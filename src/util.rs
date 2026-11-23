use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use parking_lot::Mutex;
use windows::core::PCWSTR;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub const APP_NAME: &str = "Kage Replay";
pub const APP_ID: &str = "KageReplay";

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct WideString(Vec<u16>);

impl WideString {
    pub fn new(text: &str) -> Self {
        Self(wide(text))
    }

    pub fn pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

pub fn from_wide(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

pub unsafe fn from_pwstr(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
}

fn qpc_frequency() -> i64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    *FREQUENCY.get_or_init(|| {
        let mut value = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut value);
        }
        value.max(1)
    })
}

pub fn qpc_now() -> i64 {
    let mut value = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut value);
    }
    value
}

pub fn qpc_to_hns(qpc: i64) -> i64 {
    let freq = qpc_frequency() as i128;
    ((qpc as i128) * 10_000_000 / freq) as i64
}

pub fn now_hns() -> i64 {
    qpc_to_hns(qpc_now())
}

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join(APP_ID);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn local_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join(APP_ID);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn default_videos_dir() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        if let Ok(path) = SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) {
            let text = from_pwstr(path.0);
            windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
            if !text.is_empty() {
                return PathBuf::from(text);
            }
        }
    }
    let profile = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    profile.join("Videos")
}

pub fn default_disk_buffer_dir() -> PathBuf {
    local_data_dir().join("buffer")
}

pub fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

static LOGGER: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init_logging(tag: &str) {
    let dir = local_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{tag}.log"));
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 4 * 1024 * 1024 {
            let _ = std::fs::rename(&path, dir.join(format!("{tag}.old.log")));
        }
    }
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    let _ = LOGGER.set(Mutex::new(file));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_line("PANIC", &format!("{info}"));
        default_hook(info);
    }));
}

pub fn log_line(level: &str, message: &str) {
    let stamp = timestamp_string();
    let line = format!("[{stamp}] {level} {message}\n");
    if let Some(lock) = LOGGER.get() {
        if let Some(file) = lock.lock().as_mut() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::util::log_line("INFO", &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::util::log_line("WARN", &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::util::log_line("ERROR", &format!($($arg)*)) };
}

pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millis: u16,
}

pub fn local_time() -> LocalTime {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    LocalTime {
        year: st.wYear,
        month: st.wMonth,
        day: st.wDay,
        hour: st.wHour,
        minute: st.wMinute,
        second: st.wSecond,
        millis: st.wMilliseconds,
    }
}

pub fn timestamp_string() -> String {
    let t = local_time();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.year, t.month, t.day, t.hour, t.minute, t.second, t.millis
    )
}

pub fn file_stamp() -> String {
    let t = local_time();
    format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return "Clip".to_string();
    }
    let name: String = trimmed.chars().take(80).collect::<String>().trim_end_matches(['.', ' ']).to_string();
    let base = name.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT")) && base.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("{name}_")
    } else {
        name
    }
}

pub fn unique_path(dir: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut index = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({index}).{ext}"));
        index += 1;
    }
    candidate
}

pub fn format_bytes(bytes: u64) -> String {
    let value = bytes as f64;
    if value >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} ГБ", value / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024.0 * 1024.0 {
        format!("{:.1} МБ", value / (1024.0 * 1024.0))
    } else if value >= 1024.0 {
        format!("{:.0} КБ", value / 1024.0)
    } else {
        format!("{bytes} Б")
    }
}

pub fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub struct QuitSignal(windows::Win32::Foundation::HANDLE);

unsafe impl Send for QuitSignal {}
unsafe impl Sync for QuitSignal {}

impl QuitSignal {
    pub fn open() -> Option<Self> {
        let name = WideString::new("Local\\KageReplay.Quit");
        unsafe { windows::Win32::System::Threading::CreateEventW(None, true, false, name.pcwstr()).ok().map(QuitSignal) }
    }

    pub fn raise(&self) {
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(self.0);
        }
    }

    pub fn clear(&self) {
        unsafe {
            let _ = windows::Win32::System::Threading::ResetEvent(self.0);
        }
    }

    pub fn raised(&self) -> bool {
        unsafe { windows::Win32::System::Threading::WaitForSingleObject(self.0, 0) == windows::Win32::Foundation::WAIT_OBJECT_0 }
    }
}

impl Drop for QuitSignal {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub fn local_stamp(time: std::time::SystemTime) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let Ok(since) = time.duration_since(std::time::SystemTime::UNIX_EPOCH) else { return String::new() };
    let ticks = since.as_nanos() as u64 / 100 + 116_444_736_000_000_000;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
    }
    format!("{:02}.{:02}.{} {:02}:{:02}", local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute)
}
