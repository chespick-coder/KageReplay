use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::config::AppConfig;
use crate::engine::audio_capture::AudioDeviceInfo;
use crate::engine::devices::{MonitorInfo, WindowInfo};
use crate::engine::video_encoder::EncoderDescriptor;
use crate::engine::EngineStatus;
use crate::util::WideString;

fn pipe_name() -> String {
    let mut session = 0u32;
    unsafe {
        let _ = windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(std::process::id(), &mut session);
    }
    format!(r"\\.\pipe\KageReplay.v1.{session}")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    SaveReplay(Option<u32>),
    ToggleRecording,
    ToggleBuffer,
    Screenshot,
    SetProfile(usize),
    OpenOutputFolder,
    ClearCache,
    ToggleMute { mic: bool },
    ShowUi,
    ShowPage(String),
    CheckUpdates,
    SnoozeUpdate,
    Quit,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceList {
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
    pub outputs: Vec<AudioDeviceInfo>,
    pub inputs: Vec<AudioDeviceInfo>,
    pub encoders: Vec<EncoderDescriptor>,
    pub processes: Vec<String>,
    pub graphics_capture: bool,
    pub os_build: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityItem {
    pub id: u64,
    pub time: String,
    pub text: String,
    pub path: Option<String>,
    pub error: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub engine: EngineStatus,
    pub activity: Vec<ActivityItem>,
    pub hotkey_errors: Vec<String>,
    pub config_revision: u64,
    pub version: String,
    #[serde(default)]
    pub ui_request: Option<(u64, String)>,
    #[serde(default)]
    pub update: crate::update::UpdateStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Status,
    Levels,
    Config,
    SetConfig(Box<AppConfig>),
    Devices,
    Command(Command),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Status(Box<DaemonStatus>),
    Levels(Vec<(String, f32)>),
    Config(Box<AppConfig>, u64),
    Devices(Box<DeviceList>),
    Ok,
    Error(String),
}

fn write_frame(stream: &mut File, payload: &[u8]) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    stream.write_all(&frame)?;
    stream.flush()
}

fn read_frame(stream: &mut File) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;
    Ok(data)
}

pub fn serve(handler: impl Fn(Request) -> Response + Send + Sync + Clone + 'static) {
    let name = WideString::new(&pipe_name());
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                name.pcwstr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                1 << 16,
                1 << 16,
                0,
                None,
            )
        };
        if pipe.is_invalid() {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if connected.is_err() && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(pipe);
            }
            continue;
        }
        let handler = handler.clone();
        let raw = pipe.0 as isize;
        std::thread::spawn(move || {
            let mut stream = unsafe { File::from_raw_handle(raw as *mut _) };
            while let Ok(frame) = read_frame(&mut stream) {
                let response = match serde_json::from_slice::<Request>(&frame) {
                    Ok(request) => handler(request),
                    Err(err) => Response::Error(format!("bad request: {err}")),
                };
                let Ok(bytes) = serde_json::to_vec(&response) else { break };
                if write_frame(&mut stream, &bytes).is_err() {
                    break;
                }
            }
        });
    }
}

pub struct Client {
    stream: File,
}

impl Client {
    pub fn connect(timeout: Duration) -> Result<Self> {
        let name = WideString::new(&pipe_name());
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let handle = unsafe {
                CreateFileW(
                    name.pcwstr(),
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    Some(HANDLE::default()),
                )
            };
            match handle {
                Ok(h) => return Ok(Self { stream: unsafe { File::from_raw_handle(h.0) } }),
                Err(err) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(anyhow!("служба Kage Replay не отвечает: {err}"));
                    }
                    if err.code() == ERROR_PIPE_BUSY.to_hresult() {
                        unsafe {
                            let _ = WaitNamedPipeW(name.pcwstr(), 200);
                        }
                    } else {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
    }

    pub fn request(&mut self, request: &Request) -> Result<Response> {
        let bytes = serde_json::to_vec(request)?;
        write_frame(&mut self.stream, &bytes)?;
        let frame = read_frame(&mut self.stream)?;
        Ok(serde_json::from_slice(&frame)?)
    }
}

pub fn send_command(command: Command) -> Result<()> {
    let mut client = Client::connect(Duration::from_secs(2))?;
    match client.request(&Request::Command(command))? {
        Response::Error(e) => Err(anyhow!(e)),
        _ => Ok(()),
    }
}
