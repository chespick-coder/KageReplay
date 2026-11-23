use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::{RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{http, util};

pub const RELEASES_REPO: &str = "chespick-coder/KageReplay";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const SNOOZE_FOR: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub published_at: String,
    pub page_url: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateStatus {
    pub current: String,
    pub available: Option<Release>,
    pub snoozed: bool,
    pub checking: bool,
    pub last_check: u64,
    pub updated_at: u64,
    pub updated_from: Option<String>,
    pub error: Option<String>,
    pub check_id: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    installed_version: String,
    updated_at: u64,
    updated_from: Option<String>,
    last_check: u64,
    etag: String,
    latest: Option<Release>,
    snooze_until: u64,
    last_error: Option<String>,
}

impl Stored {
    fn path() -> PathBuf {
        util::app_data_dir().join("update.json")
    }

    fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, data);
        }
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn unix_to_time(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn version_parts(version: &str) -> Vec<u64> {
    version.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|part| part.parse().ok()).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (version_parts(candidate), version_parts(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    !a.is_empty() && a > b
}

struct Inner {
    stored: Stored,
    checking: bool,
    check_id: u64,
}

#[derive(Clone)]
pub struct Updater {
    inner: Arc<Mutex<Inner>>,
    wake: Sender<()>,
}

impl Updater {
    pub fn start() -> Self {
        let mut stored = Stored::load();
        if stored.installed_version != CURRENT_VERSION {
            let previous = std::mem::replace(&mut stored.installed_version, CURRENT_VERSION.to_string());
            stored.updated_from = (!previous.is_empty()).then_some(previous);
            stored.updated_at = unix_now();
            stored.snooze_until = 0;
            if stored.latest.as_ref().is_some_and(|r| !is_newer(&r.version, CURRENT_VERSION)) {
                stored.latest = None;
            }
            stored.save();
            crate::log_info!("version {CURRENT_VERSION} recorded as installed");
        }
        let inner = Arc::new(Mutex::new(Inner { stored, checking: false, check_id: 0 }));
        let (wake, woken) = crossbeam_channel::unbounded::<()>();
        {
            let inner = inner.clone();
            std::thread::Builder::new()
                .name("kage-updates".into())
                .spawn(move || {
                    let mut first = true;
                    loop {
                        let since = unix_now().saturating_sub(inner.lock().stored.last_check);
                        let mut wait = CHECK_EVERY.saturating_sub(Duration::from_secs(since));
                        if first {
                            wait = wait.max(FIRST_CHECK_DELAY);
                            first = false;
                        }
                        match woken.recv_timeout(wait) {
                            Ok(()) | Err(RecvTimeoutError::Timeout) => run_check(&inner),
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                })
                .expect("update thread");
        }
        Self { inner, wake }
    }

    pub fn status(&self) -> UpdateStatus {
        let inner = self.inner.lock();
        let s = &inner.stored;
        UpdateStatus {
            current: CURRENT_VERSION.to_string(),
            available: s.latest.clone().filter(|r| is_newer(&r.version, CURRENT_VERSION)),
            snoozed: unix_now() < s.snooze_until,
            checking: inner.checking,
            last_check: s.last_check,
            updated_at: s.updated_at,
            updated_from: s.updated_from.clone(),
            error: s.last_error.clone(),
            check_id: inner.check_id,
        }
    }

    pub fn check_now(&self) {
        self.inner.lock().checking = true;
        let _ = self.wake.send(());
    }

    pub fn snooze(&self) {
        let mut inner = self.inner.lock();
        inner.stored.snooze_until = unix_now() + SNOOZE_FOR.as_secs();
        inner.stored.save();
    }
}

fn run_check(inner: &Arc<Mutex<Inner>>) {
    let (etag, known) = {
        let mut i = inner.lock();
        i.checking = true;
        (i.stored.etag.clone(), i.stored.latest.clone())
    };
    let outcome = fetch_latest(RELEASES_REPO, if known.is_some() { &etag } else { "" });
    let mut i = inner.lock();
    let s = &mut i.stored;
    s.last_check = unix_now();
    match outcome {
        Ok(Fetched::Unchanged) => s.last_error = None,
        Ok(Fetched::Missing) => {
            s.latest = None;
            s.etag.clear();
            s.last_error = None;
        }
        Ok(Fetched::Release(release, etag)) => {
            if s.latest.as_ref().map(|r| &r.version) != Some(&release.version) {
                s.snooze_until = 0;
                if is_newer(&release.version, CURRENT_VERSION) {
                    crate::log_info!("update available: {}", release.version);
                }
            }
            s.latest = Some(release);
            s.etag = etag.unwrap_or_default();
            s.last_error = None;
        }
        Err(err) => {
            crate::log_warn!("update check failed: {err:#}");
            s.last_error = Some(format!("Не удалось проверить: {err}"));
        }
    }
    s.save();
    i.checking = false;
    i.check_id += 1;
}

enum Fetched {
    Unchanged,
    Missing,
    Release(Release, Option<String>),
}

fn fetch_latest(repo: &str, etag: &str) -> Result<Fetched> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let mut headers = vec![("Accept", "application/vnd.github+json"), ("X-GitHub-Api-Version", "2022-11-28")];
    if !etag.is_empty() {
        headers.push(("If-None-Match", etag));
    }
    let reply = http::get(&url, &headers, |_, _| true)?;
    match reply.status {
        304 => return Ok(Fetched::Unchanged),
        404 => return Ok(Fetched::Missing),
        403 | 429 => bail!("сервер обновлений занят, попробуйте позже"),
        200 => {}
        code => bail!("сервер обновлений ответил с кодом {code}"),
    }
    let json: Value = serde_json::from_slice(&reply.body)?;
    Ok(Fetched::Release(parse_release(&json)?, reply.etag))
}

fn parse_release(json: &Value) -> Result<Release> {
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let tag = text(json, "tag_name");
    if version_parts(&tag).is_empty() {
        bail!("непонятный номер версии «{tag}»");
    }
    let assets = json.get("assets").and_then(Value::as_array).cloned().unwrap_or_default();
    let installer = assets
        .iter()
        .find(|a| {
            let name = text(a, "name").to_lowercase();
            name.ends_with(".exe") && name.contains("setup")
        })
        .or_else(|| assets.iter().find(|a| text(a, "name").to_lowercase().ends_with(".exe")))
        .ok_or_else(|| anyhow!("в версии {tag} нет установщика"))?;
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: text(json, "body").replace("\r\n", "\n").trim().to_string(),
        published_at: text(json, "published_at"),
        page_url: text(json, "html_url"),
        asset_name: text(installer, "name"),
        asset_url: text(installer, "browser_download_url"),
        asset_size: installer.get("size").and_then(Value::as_u64).unwrap_or(0),
        sha256: installer.get("digest").and_then(Value::as_str).and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_ascii_lowercase()),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStage {
    Downloading { done: u64, total: u64 },
    Verifying,
    Launching,
    Started,
    Failed(String),
}

pub struct Download {
    pub stage: Arc<Mutex<DownloadStage>>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    pub fn start(release: Release, on_change: impl Fn() + Send + 'static) -> Self {
        let stage = Arc::new(Mutex::new(DownloadStage::Downloading { done: 0, total: release.asset_size }));
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let stage = stage.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("kage-update-download".into())
                .spawn(move || {
                    let result = download_and_launch(&release, &stage, &cancel, &on_change);
                    *stage.lock() = match result {
                        Ok(()) => DownloadStage::Started,
                        Err(err) => {
                            crate::log_warn!("update install failed: {err:#}");
                            DownloadStage::Failed(err.to_string())
                        }
                    };
                    on_change();
                })
                .expect("download thread");
        }
        Self { stage, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn download_and_launch(release: &Release, stage: &Mutex<DownloadStage>, cancel: &AtomicBool, on_change: &dyn Fn()) -> Result<()> {
    let mut last_report = 0u64;
    let reply = http::get(&release.asset_url, &[], |done, total| {
        if done - last_report >= 128 * 1024 || Some(done) == total {
            last_report = done;
            *stage.lock() = DownloadStage::Downloading { done, total: total.unwrap_or(release.asset_size) };
            on_change();
        }
        !cancel.load(Ordering::SeqCst)
    })?;
    if reply.status != 200 {
        bail!("сервер обновлений ответил с кодом {}", reply.status);
    }
    *stage.lock() = DownloadStage::Verifying;
    on_change();
    if release.asset_size > 0 && reply.body.len() as u64 != release.asset_size {
        bail!("файл скачался не полностью");
    }
    if let Some(expected) = &release.sha256 {
        if &sha256_hex(&reply.body)? != expected {
            bail!("контрольная сумма не совпала, файл повреждён");
        }
    }
    if !reply.body.starts_with(b"MZ") {
        bail!("скачанный файл не похож на установщик");
    }
    let path = std::env::temp_dir().join(util::sanitize_file_name(&release.asset_name));
    std::fs::write(&path, &reply.body)?;
    *stage.lock() = DownloadStage::Launching;
    on_change();
    launch_installer(&path)
}

fn sha256_hex(data: &[u8]) -> Result<String> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut digest = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut digest) };
    if status.is_err() {
        bail!("не удалось проверить файл");
    }
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn launch_installer(path: &Path) -> Result<()> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let file = util::WideString::new(&path.display().to_string());
    let parameters = util::WideString::new("/SILENT /SUPPRESSMSGBOXES /NORESTART /UPDATE=1");
    let result = unsafe { ShellExecuteW(None, w!("open"), file.pcwstr(), parameters.pcwstr(), PCWSTR::null(), SW_SHOWNORMAL) };
    if (result.0 as isize) <= 32 {
        bail!("установка отменена");
    }
    Ok(())
}

pub fn readable_notes(markdown: &str) -> String {
    let mut out = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let mut text = trimmed.trim_start_matches('#').trim_start().to_string();
        if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
            text = format!("•  {rest}");
        }
        for marker in ["**", "__", "`"] {
            text = text.replace(marker, "");
        }
        while let (Some(open), Some(mid)) = (text.find('['), text.find("](")) {
            let Some(close) = text[mid..].find(')').map(|c| mid + c) else { break };
            if open > mid {
                break;
            }
            text = format!("{}{}{}", &text[..open], &text[open + 1..mid], &text[close + 1..]);
        }
        out.push(text);
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("1.1", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("latest", "1.0.0"));
    }

    #[test]
    fn cleans_markdown() {
        let notes = readable_notes("## Что нового\n- **Быстрее** запись\n* см. [страницу](https://example.com) `код`");
        assert_eq!(notes, "Что нового\n•  Быстрее запись\n•  см. страницу код");
    }
}
