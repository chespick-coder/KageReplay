use std::ffi::c_void;

use anyhow::{anyhow, bail, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

use crate::util::{self, WideString};

pub struct Reply {
    pub status: u32,
    pub body: Vec<u8>,
    pub etag: Option<String>,
}

struct Handle(*mut c_void);

impl Handle {
    fn checked(raw: *mut c_void, what: &str) -> Result<Self> {
        if raw.is_null() {
            return Err(offline(what, windows::core::Error::from_thread()));
        }
        Ok(Self(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn offline(stage: &str, error: windows::core::Error) -> anyhow::Error {
    crate::log_warn!("http {stage} failed: {}", error.message());
    anyhow!("нет соединения с интернетом")
}

fn query_number(request: &Handle, info: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    unsafe { WinHttpQueryHeaders(request.0, info | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut value as *mut u32 as *mut c_void), &mut size, std::ptr::null_mut()) }
        .ok()
        .map(|_| value)
}

fn query_text(request: &Handle, info: u32) -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    unsafe { WinHttpQueryHeaders(request.0, info, PCWSTR::null(), Some(buffer.as_mut_ptr() as *mut c_void), &mut size, std::ptr::null_mut()) }
        .ok()
        .map(|_| util::from_wide(&buffer[..size as usize / 2]))
}

pub fn get(url: &str, headers: &[(&str, &str)], mut progress: impl FnMut(u64, Option<u64>) -> bool) -> Result<Reply> {
    let rest = url.strip_prefix("https://").ok_or_else(|| anyhow!("поддерживается только https"))?;
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    let agent = WideString::new(&format!("KageReplay/{}", env!("CARGO_PKG_VERSION")));
    let host = WideString::new(host);
    let path = WideString::new(&path);
    let header_lines: String = headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")).collect();
    let header_wide: Vec<u16> = header_lines.encode_utf16().collect();
    unsafe {
        let session = Handle::checked(WinHttpOpen(agent.pcwstr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "open")?;
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 30_000)?;
        let connection = Handle::checked(WinHttpConnect(session.0, host.pcwstr(), INTERNET_DEFAULT_HTTPS_PORT, 0), "connect")?;
        let request = Handle::checked(
            WinHttpOpenRequest(connection.0, w!("GET"), path.pcwstr(), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "request",
        )?;
        WinHttpSendRequest(request.0, (!header_wide.is_empty()).then_some(header_wide.as_slice()), None, 0, 0, 0).map_err(|e| offline("send", e))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| offline("receive", e))?;
        let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        let total = query_number(&request, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
        let etag = query_text(&request, WINHTTP_QUERY_ETAG);
        let mut body = Vec::with_capacity(total.unwrap_or(0).min(256 << 20) as usize);
        let mut chunk = vec![0u8; 256 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read).map_err(|e| offline("read", e))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if !progress(body.len() as u64, total) {
                bail!("загрузка отменена");
            }
        }
        Ok(Reply { status, body, etag })
    }
}
