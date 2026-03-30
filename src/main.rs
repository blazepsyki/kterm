// SPDX-License-Identifier: MIT OR Apache-2.0

use env_logger::Env;
use iced::{window, Task};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

mod app;
mod connection;
mod platform;
mod remote_display;
mod terminal;
mod ui;

use app::{Message, State};

pub(crate) const RDP_RESOLUTION_PRESETS: &[(u16, u16)] = &[
    (1024, 768),
    (1280, 720),
    (1280, 1024),
    (1366, 768),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
];

static RDP_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
static SESSION_LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
static SESSION_LOG_FILE: OnceLock<Arc<Mutex<File>>> = OnceLock::new();

struct TeeLoggerWriter {
    file: Arc<Mutex<File>>,
}

impl Write for TeeLoggerWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut f) = self.file.lock() {
            f.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Ok(mut f) = self.file.lock() {
            f.flush()?;
        }
        Ok(())
    }
}

fn session_log_path() -> &'static std::path::Path {
    SESSION_LOG_PATH
        .get_or_init(|| {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            Path::new("logs").join(format!("kterm_{}.log", timestamp))
        })
        .as_path()
}

pub fn runtime_log_path() -> std::path::PathBuf {
    session_log_path().to_path_buf()
}

pub(crate) fn rdp_trace_enabled() -> bool {
    *RDP_TRACE_ENABLED.get_or_init(|| {
        std::env::var("KTERM_RDP_TRACE")
            .map(|v| {
                let v = v.to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes" || v == "on"
            })
            .unwrap_or(false)
    })
}

fn init_session_log_file() -> Result<(), String> {
    let path = session_log_path().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create log dir: {}", e))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("cannot open runtime log: {}", e))?;

    writeln!(
        file,
        "\n=== kterm run start {} ===",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    )
    .map_err(|e| format!("cannot write runtime log header: {}", e))?;

    let _ = SESSION_LOG_FILE.set(Arc::new(Mutex::new(file)));
    Ok(())
}

pub fn main() -> iced::Result {
    let _ = init_session_log_file();

    let mut logger = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
    logger.format_timestamp_millis();
    if let Some(file) = SESSION_LOG_FILE.get().cloned() {
        logger.target(env_logger::Target::Pipe(Box::new(TeeLoggerWriter { file })));
    }
    let _ = logger.try_init();

    log::info!("[LOG] unified session log file: {}", session_log_path().display());

    iced::application(
        || {
            let font_task =
                iced::font::load(include_bytes!("../assets/fonts/D2Coding.ttf")).map(Message::FontLoaded);
            let win_id_task = window::oldest()
                .map(|opt_id| Message::WindowIdCaptured(opt_id.expect("No window found")));
            (State::default(), Task::batch(vec![font_task, win_id_task]))
        },
        app::update::update,
        ui::view::view,
    )
    .window(window::Settings {
        decorations: false,
        ..Default::default()
    })
    .subscription(app::subscription::subscription)
    .title("k_term")
    .run()
}
