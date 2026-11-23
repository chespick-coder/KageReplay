use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::config::AppConfig;
use crate::ipc::{Client, Command, DaemonStatus, DeviceList, Request, Response};

pub enum Outgoing {
    Command(Command),
    SetConfig(Box<AppConfig>),
    RefreshDevices,
}

#[derive(Default)]
pub struct RemoteState {
    pub connected: bool,
    pub status: Option<DaemonStatus>,
    pub config: Option<(AppConfig, u64)>,
    pub devices: Option<DeviceList>,
    pub devices_loading: bool,
    pub last_error: Option<String>,
    pub fps_history: Vec<VecDeque<(f64, f64)>>,
    pub command_error: Option<String>,
    pub saved_ok: Option<Instant>,
    pub levels: Vec<(String, f32)>,
}

pub struct Remote {
    pub state: Arc<Mutex<RemoteState>>,
    sender: Sender<Outgoing>,
    levels_until: Arc<Mutex<Instant>>,
}

impl Remote {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let state = Arc::new(Mutex::new(RemoteState { devices_loading: true, ..Default::default() }));
        let (sender, receiver) = unbounded();
        let thread_state = state.clone();
        let levels_until = Arc::new(Mutex::new(Instant::now()));
        let thread_levels = levels_until.clone();
        std::thread::Builder::new()
            .name("kage-ui-remote".into())
            .spawn(move || worker(thread_state, receiver, ctx, thread_levels))
            .expect("remote thread");
        let _ = sender.send(Outgoing::RefreshDevices);
        Self { state, sender, levels_until }
    }

    pub fn want_levels(&self) {
        *self.levels_until.lock() = Instant::now() + Duration::from_millis(400);
    }

    pub fn send(&self, message: Outgoing) {
        let _ = self.sender.send(message);
    }

    pub fn command(&self, command: Command) {
        self.send(Outgoing::Command(command));
    }
}

fn ensure_daemon() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--background").spawn();
    }
}

fn perform(client: &mut Client, message: Outgoing, state: &Arc<Mutex<RemoteState>>) -> bool {
    let is_config = matches!(message, Outgoing::SetConfig(_));
    let request = match message {
        Outgoing::Command(cmd) => Request::Command(cmd),
        Outgoing::SetConfig(cfg) => Request::SetConfig(cfg),
        Outgoing::RefreshDevices => {
            state.lock().devices_loading = true;
            Request::Devices
        }
    };
    match client.request(&request) {
        Ok(Response::Devices(devices)) => {
            let mut s = state.lock();
            s.devices = Some(*devices);
            s.devices_loading = false;
            true
        }
        Ok(Response::Error(err)) => {
            state.lock().command_error = Some(err);
            true
        }
        Ok(Response::Ok) => {
            if is_config {
                state.lock().saved_ok = Some(Instant::now());
            }
            true
        }
        Ok(_) => true,
        Err(_) => false,
    }
}

fn poll(client: &mut Client, state: &Arc<Mutex<RemoteState>>, known_revision: &mut u64) -> bool {
    let Ok(Response::Status(status)) = client.request(&Request::Status) else { return false };
    let revision = status.config_revision;
    {
        let mut s = state.lock();
        s.connected = true;
        s.last_error = None;
        while s.fps_history.len() < status.engine.video.len() {
            s.fps_history.push(VecDeque::new());
        }
        s.fps_history.truncate(status.engine.video.len());
        for (i, v) in status.engine.video.iter().enumerate() {
            let history = &mut s.fps_history[i];
            history.push_back((v.unique_fps, v.output_fps));
            while history.len() > 120 {
                history.pop_front();
            }
        }
        s.status = Some(*status);
    }
    if revision != *known_revision {
        match client.request(&Request::Config) {
            Ok(Response::Config(config, rev)) => {
                *known_revision = rev;
                state.lock().config = Some((*config, rev));
            }
            _ => return false,
        }
    }
    true
}

fn worker(state: Arc<Mutex<RemoteState>>, receiver: Receiver<Outgoing>, ctx: eframe::egui::Context, levels_until: Arc<Mutex<Instant>>) {
    let mut last_levels = Instant::now();
    let mut client: Option<Client> = None;
    let mut started_daemon = false;
    let mut known_revision = 0u64;
    let mut last_poll = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
    let mut queued: VecDeque<Outgoing> = VecDeque::new();
    let quit_signal = crate::util::QuitSignal::open();
    loop {
        let Some(c) = client.as_mut() else {
            match Client::connect(Duration::from_millis(400)) {
                Ok(c) => {
                    client = Some(c);
                    known_revision = 0;
                }
                Err(err) => {
                    if quit_signal.as_ref().map(|q| q.raised()).unwrap_or(false) {
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                        ctx.request_repaint();
                        return;
                    }
                    if !started_daemon {
                        ensure_daemon();
                        started_daemon = true;
                    }
                    {
                        let mut s = state.lock();
                        s.connected = false;
                        s.last_error = Some(err.to_string());
                    }
                    ctx.request_repaint();
                    while let Ok(m) = receiver.try_recv() {
                        queued.push_back(m);
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            continue;
        };
        let mut ok = true;
        while let Some(message) = queued.pop_front() {
            if !perform(c, message, &state) {
                ok = false;
                break;
            }
            last_poll = Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);
        }
        if ok && last_poll.elapsed() >= Duration::from_millis(500) {
            last_poll = Instant::now();
            ok = poll(c, &state, &mut known_revision);
            ctx.request_repaint();
        }
        let wants_levels = Instant::now() < *levels_until.lock();
        if ok && wants_levels && last_levels.elapsed() >= Duration::from_millis(33) {
            last_levels = Instant::now();
            match c.request(&Request::Levels) {
                Ok(Response::Levels(levels)) => {
                    state.lock().levels = levels;
                    ctx.request_repaint();
                }
                Ok(_) => {}
                Err(_) => ok = false,
            }
        } else if !wants_levels && !state.lock().levels.is_empty() {
            state.lock().levels.clear();
        }
        if !ok {
            client = None;
            state.lock().connected = false;
            ctx.request_repaint();
            continue;
        }
        let wait = if wants_levels { Duration::from_millis(15) } else { Duration::from_millis(100) };
        match receiver.recv_timeout(wait) {
            Ok(message) => {
                queued.push_back(message);
                while let Ok(m) = receiver.try_recv() {
                    queued.push_back(m);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        }
    }
}