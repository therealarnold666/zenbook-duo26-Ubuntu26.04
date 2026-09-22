use std::env;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use evdev::{AbsoluteAxisType, Device, EventType};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command as TokioCommand;
use tokio::time::{interval, timeout, MissedTickBehavior};

use crate::ipc::protocol::{
    DaemonRequest, DaemonResponse, Envelope, SessionBackend, SessionCommand, SessionResponse,
};
use crate::models::Orientation;
use crate::runtime::{paths, state::RuntimeState};

const DOCK_COMMAND_TIMEOUT: Duration = Duration::from_secs(6);
const DOCK_VERIFY_TIMEOUT: Duration = Duration::from_secs(2);
const KEYBOARD_BACKLIGHT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn run() -> Result<(), String> {
    ensure_user_runtime_dir()?;
    let listener = bind_session_listener(paths::current_user_session_socket_path().as_path())?;

    let backend = wait_for_ready_backend().await?;
    register_with_daemon(backend).await?;
    tokio::spawn(async {
        if let Err(err) = watch_rotation().await {
            log::warn!("session-agent rotation watcher failed: {err}");
            let _ = send_runtime_notification(
                "Zenbook Duo Runtime Error",
                &format!("Rotation watcher failed: {err}"),
                true,
            );
        }
    });
    tokio::spawn(async {
        if let Err(err) = watch_brightness_sync().await {
            log::warn!("session-agent brightness watcher failed: {err}");
            let _ = send_runtime_notification(
                "Zenbook Duo Runtime Error",
                &format!("Brightness sync watcher failed: {err}"),
                true,
            );
        }
    });
    tokio::task::spawn_blocking(|| {
        if let Err(err) = watch_keyboard_hotkeys() {
            log::warn!("session-agent keyboard hotkey watcher failed: {err}");
            let _ = send_runtime_notification(
                "Zenbook Duo Runtime Error",
                &format!("Keyboard hotkey watcher failed: {err}"),
                true,
            );
        }
    });

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| format!("Failed to accept session-agent client: {e}"))?;
        tokio::spawn(async move {
            if let Err(err) = handle_session_command(stream).await {
                log::warn!("session-agent client error: {err}");
            }
        });
    }
}

async fn register_with_daemon(backend: SessionBackend) -> Result<(), String> {
    let stream = UnixStream::connect(paths::daemon_socket_path())
        .await
        .map_err(|e| format!("Failed to connect to daemon socket: {e}"))?;
    let (reader, mut writer) = stream.into_split();

    let request = Envelope::new(DaemonRequest::RegisterSessionAgent {
        session_id: detect_session_id(),
        backend,
        socket_path: paths::current_user_session_socket_path()
            .to_string_lossy()
            .into_owned(),
    });
    let line = serde_json::to_string(&request)
        .map_err(|e| format!("Failed to encode registration: {e}"))?;
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("Failed to send registration: {e}"))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("Failed to terminate registration: {e}"))?;

    let mut lines = BufReader::new(reader).lines();
    let reply = lines
        .next_line()
        .await
        .map_err(|e| format!("Failed reading daemon registration reply: {e}"))?
        .ok_or_else(|| "Daemon closed before replying to session registration".to_string())?;

    let envelope: Envelope<DaemonResponse> = serde_json::from_str(&reply)
        .map_err(|e| format!("Invalid daemon registration response: {e}"))?;
    match envelope.payload {
        DaemonResponse::Ack => Ok(()),
        DaemonResponse::Error { message } => Err(message),
        other => Err(format!(
            "Unexpected daemon registration response: {other:?}"
        )),
    }
}

async fn handle_session_command(stream: UnixStream) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("Failed to read session command: {e}"))?
    {
        let envelope: Envelope<SessionCommand> = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid session command JSON: {e}"))?;
        let response = match envelope.payload {
            SessionCommand::GetDisplayLayout => {
                match crate::hardware::display_config::get_display_layout() {
                    Ok(layout) => SessionResponse::DisplayLayout { layout },
                    Err(message) => SessionResponse::Error { message },
                }
            }
            SessionCommand::SetDockMode { attached, scale } => {
                match apply_dock_mode(attached, scale).await {
                    Ok(()) => SessionResponse::Ack,
                    Err(message) => SessionResponse::Error { message },
                }
            }
            SessionCommand::ApplyDisplayLayout { layout } => {
                match crate::hardware::display_config::apply_display_layout(&layout) {
                    Ok(()) => SessionResponse::Ack,
                    Err(message) => SessionResponse::Error { message },
                }
            }
            SessionCommand::SetOrientation { orientation, scale } => {
                match crate::hardware::display_config::set_orientation_with_scale(
                    &orientation,
                    scale,
                ) {
                    Ok(()) => SessionResponse::Ack,
                    Err(message) => SessionResponse::Error { message },
                }
            }
            SessionCommand::ShowNotification {
                title,
                message,
                urgent,
            } => match send_runtime_notification(&title, &message, urgent) {
                Ok(()) => SessionResponse::Ack,
                Err(message) => SessionResponse::Error { message },
            },
            SessionCommand::OpenEmojiPicker => SessionResponse::Ack,
        };
        let line = serde_json::to_string(&Envelope::new(response))
            .map_err(|e| format!("Failed to encode session response: {e}"))?;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Failed to write session response: {e}"))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to terminate session response: {e}"))?;
    }

    Ok(())
}

fn bind_session_listener(path: &Path) -> Result<UnixListener, String> {
    remove_stale_socket(path);
    UnixListener::bind(path).map_err(|e| format!("Failed to bind session agent socket: {e}"))
}

fn niri_runtime_dir() -> Option<std::path::PathBuf> {
    env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)
}

fn resolve_niri_socket() -> Option<std::path::PathBuf> {
    let env_socket = env::var_os("NIRI_SOCKET").map(std::path::PathBuf::from);
    resolve_niri_socket_from(env_socket.as_deref(), niri_runtime_dir().as_deref())
}

fn resolve_niri_socket_from(
    env_socket: Option<&Path>,
    runtime_dir: Option<&Path>,
) -> Option<std::path::PathBuf> {
    if let Some(env_socket) = env_socket {
        if env_socket.exists() {
            return Some(env_socket.to_path_buf());
        }
    }

    let runtime_dir = runtime_dir?;
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;

    for entry in fs::read_dir(runtime_dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        let name = path.file_name()?.to_str()?;
        if !name.starts_with("niri.") || !name.ends_with(".sock") {
            continue;
        }

        let metadata = entry.metadata().ok()?;
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &newest {
            Some((current_modified, _)) if *current_modified >= modified => {}
            _ => newest = Some((modified, path)),
        }
    }

    newest.map(|(_, path)| path)
}

fn ensure_user_runtime_dir() -> Result<(), String> {
    crate::runtime::runtime_dir::ensure_current_user_runtime_dir()
}

fn remove_stale_socket(path: &Path) {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_socket() {
            let _ = fs::remove_file(path);
        }
    }
}

fn detect_session_id() -> String {
    env::var("XDG_SESSION_ID").unwrap_or_else(|_| "unknown-session".to_string())
}

fn detect_backend() -> SessionBackend {
    let current = env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| env::var("XDG_SESSION_DESKTOP"))
        .or_else(|_| env::var("DESKTOP_SESSION"))
        .unwrap_or_default()
        .to_lowercase();
    detect_backend_from(current, resolve_niri_socket().is_some())
}

fn detect_backend_from(current: String, has_niri_socket: bool) -> SessionBackend {
    if current.contains("gnome") {
        SessionBackend::Gnome
    } else if current.contains("plasma") || current.contains("kde") {
        SessionBackend::Kde
    } else if current.contains("niri") || has_niri_socket {
        SessionBackend::Niri
    } else {
        SessionBackend::Unknown
    }
}

fn detect_ready_backend() -> SessionBackend {
    detect_ready_backend_from(detect_backend(), backend_is_ready)
}

fn detect_ready_backend_from<F>(hinted: SessionBackend, mut is_ready: F) -> SessionBackend
where
    F: FnMut(SessionBackend) -> bool,
{
    for backend in backend_probe_order(hinted) {
        if is_ready(backend.clone()) {
            return backend;
        }
    }
    SessionBackend::Unknown
}

fn backend_probe_order(hinted: SessionBackend) -> Vec<SessionBackend> {
    let mut order = Vec::new();
    if hinted != SessionBackend::Unknown {
        order.push(hinted);
    }
    for backend in [
        SessionBackend::Niri,
        SessionBackend::Gnome,
        SessionBackend::Kde,
    ] {
        if !order.contains(&backend) {
            order.push(backend);
        }
    }
    order
}

fn backend_is_ready(backend: SessionBackend) -> bool {
    match backend {
        SessionBackend::Gnome => {
            if !gui_session_env_ready() {
                return false;
            }
            Command::new("gdctl")
                .arg("show")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
        SessionBackend::Kde => {
            if !gui_session_env_ready() {
                return false;
            }
            Command::new("kscreen-doctor")
                .arg("-j")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
        SessionBackend::Niri => build_niri_command(&["msg", "--json", "outputs"])
            .and_then(|mut command| {
                command
                    .output()
                    .map_err(|e| format!("Failed to run niri readiness probe: {e}"))
            })
            .map(|output| output.status.success())
            .unwrap_or(false),
        SessionBackend::Unknown => false,
    }
}

fn gui_session_env_ready() -> bool {
    let has_runtime_dir = env::var_os("XDG_RUNTIME_DIR").is_some();
    let has_wayland = env::var_os("WAYLAND_DISPLAY").is_some();
    let has_x11 = env::var_os("DISPLAY").is_some();
    has_runtime_dir && (has_wayland || has_x11)
}

async fn wait_for_ready_backend() -> Result<SessionBackend, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut hinted_backend = detect_backend();

    loop {
        let backend = detect_ready_backend_from(hinted_backend.clone(), backend_is_ready);
        if backend != SessionBackend::Unknown {
            return Ok(backend);
        }

        if tokio::time::Instant::now() >= deadline {
            return Err("No supported session backend became ready before timeout".into());
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
        hinted_backend = detect_backend();
    }
}

async fn apply_dock_mode(attached: bool, scale: f64) -> Result<(), String> {
    let backend_hint = detect_backend();
    let backend = if backend_hint == SessionBackend::Unknown {
        detect_ready_backend()
    } else {
        backend_hint
    };
    let started = Instant::now();
    let _ = crate::runtime::logger::append_line(format!(
        "session-agent: applying dock mode (backend={:?}, attached={}, scale={:.3})",
        backend, attached, scale
    ));

    let result = match backend {
        SessionBackend::Gnome => apply_gnome_dock_mode(attached, scale).await,
        SessionBackend::Kde => apply_kde_dock_mode(attached),
        SessionBackend::Niri => apply_niri_dock_mode(attached),
        SessionBackend::Unknown => Err("Unsupported session backend for dock mode".into()),
    };

    match &result {
        Ok(()) => {
            let _ = crate::runtime::logger::append_line(format!(
                "session-agent: dock mode applied (backend={:?}, attached={}, elapsed_ms={})",
                backend,
                attached,
                started.elapsed().as_millis()
            ));
        }
        Err(err) => {
            let _ = crate::runtime::logger::append_line(format!(
                "session-agent: dock mode failed (backend={:?}, attached={}, elapsed_ms={}, err={})",
                backend,
                attached,
                started.elapsed().as_millis(),
                err
            ));
        }
    }

    result?;

    if let Err(err) = send_dock_mode_notification(attached) {
        log::warn!("failed to send dock-mode notification: {err}");
    }

    Ok(())
}

async fn apply_gnome_dock_mode(attached: bool, scale: f64) -> Result<(), String> {
    let args = build_gnome_dock_mode_args(attached, scale);
    if !attached {
        run_command_async("gdctl", &args, DOCK_COMMAND_TIMEOUT).await?;
        return verify_gnome_dock_mode(attached).await;
    }

    match run_command_async("gdctl", &args, DOCK_COMMAND_TIMEOUT).await {
        Ok(()) => verify_gnome_dock_mode(attached).await,
        Err(primary_err) => {
            // Older mutter/gdctl combinations may reject explicit secondary off.
            // Keep a compatibility fallback to the historical attached layout call.
            let fallback_args = build_gnome_attached_fallback_args(scale);
            log::warn!(
                "gnome attached dock mode with explicit eDP-2 off failed ({}), trying fallback",
                primary_err
            );
            run_command_async("gdctl", &fallback_args, DOCK_COMMAND_TIMEOUT).await?;
            verify_gnome_dock_mode(attached).await
        }
    }
}

fn build_gnome_dock_mode_args(attached: bool, scale: f64) -> Vec<String> {
    let scale_str = format!("{scale:.6}");
    if attached {
        vec![
            "set".to_string(),
            "--logical-monitor".to_string(),
            "--primary".to_string(),
            "--scale".to_string(),
            scale_str,
            "--monitor".to_string(),
            "eDP-1".to_string(),
            "--transform".to_string(),
            "180".to_string(),
            "--monitor".to_string(),
            "eDP-2".to_string(),
            "--mode".to_string(),
            "off".to_string(),
        ]
    } else {
        vec![
            "set".to_string(),
            "--logical-monitor".to_string(),
            "--primary".to_string(),
            "--scale".to_string(),
            scale_str.clone(),
            "--monitor".to_string(),
            "eDP-1".to_string(),
            "--transform".to_string(),
            "180".to_string(),
            "--logical-monitor".to_string(),
            "--scale".to_string(),
            scale_str,
            "--monitor".to_string(),
            "eDP-2".to_string(),
            "--below".to_string(),
            "eDP-1".to_string(),
            "--transform".to_string(),
            "normal".to_string(),
        ]
    }
}

fn build_gnome_attached_fallback_args(scale: f64) -> Vec<String> {
    let scale_str = format!("{scale:.6}");
    vec![
        "set".to_string(),
        "--logical-monitor".to_string(),
        "--primary".to_string(),
        "--scale".to_string(),
        scale_str,
        "--monitor".to_string(),
        "eDP-1".to_string(),
        "--transform".to_string(),
        "180".to_string(),
    ]
}

async fn verify_gnome_dock_mode(attached: bool) -> Result<(), String> {
    let output = run_command_capture_async("gdctl", &["show"], DOCK_VERIFY_TIMEOUT)
        .await
        .map_err(|e| format!("Failed to run gdctl verification: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "gdctl verification failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let snapshot = gnome_logical_snapshot_from_show(&stdout);
    let matched = gnome_snapshot_matches_expected(attached, snapshot.0, snapshot.1);

    let _ = crate::runtime::logger::append_line(format!(
        "session-agent: gnome dock apply_result={} attached={} verify_snapshot=logical_edp1:{} logical_edp2:{}",
        if matched { "command_ok" } else { "verify_failed" },
        attached,
        snapshot.0,
        snapshot.1
    ));

    if matched {
        Ok(())
    } else {
        Err(format!(
            "GNOME dock-mode verify failed (attached={}, logical_edp1={}, logical_edp2={})",
            attached, snapshot.0, snapshot.1
        ))
    }
}

fn gnome_logical_snapshot_from_show(show_output: &str) -> (bool, bool) {
    let logical = show_output
        .split("Logical monitors:")
        .nth(1)
        .unwrap_or_default();
    let has_edp1 = logical.contains("eDP-1 (Built-in display)");
    let has_edp2 = logical.contains("eDP-2 (Built-in display)");
    (has_edp1, has_edp2)
}

fn gnome_snapshot_matches_expected(attached: bool, logical_edp1: bool, logical_edp2: bool) -> bool {
    if attached {
        logical_edp1 && !logical_edp2
    } else {
        logical_edp1 && logical_edp2
    }
}

fn apply_kde_dock_mode(attached: bool) -> Result<(), String> {
    ensure_gui_session_env("KDE display control")?;
    if attached {
        run_command(
            "kscreen-doctor",
            &["output.eDP-1.enable", "output.eDP-2.disable"],
        )
    } else {
        let (_, h) = kde_output_logical_size("eDP-1").unwrap_or((0, 0));
        run_command(
            "kscreen-doctor",
            &[
                "output.eDP-1.enable",
                "output.eDP-2.enable",
                "output.eDP-1.position.0,0",
                &format!("output.eDP-2.position.0,{h}"),
            ],
        )
    }
}

fn apply_niri_dock_mode(attached: bool) -> Result<(), String> {
    if attached {
        run_niri_command(&["msg", "output", "eDP-1", "on"])?;
        run_niri_command(&["msg", "output", "eDP-2", "off"])
    } else {
        run_niri_command(&["msg", "output", "eDP-1", "on"])?;
        run_niri_command(&["msg", "output", "eDP-2", "on"])?;
        let (_, h) = niri_output_logical_size("eDP-1").unwrap_or((0, 0));
        run_niri_command(&["msg", "output", "eDP-1", "position", "set", "0", "0"])?;
        run_niri_command(&[
            "msg",
            "output",
            "eDP-2",
            "position",
            "set",
            "0",
            &h.to_string(),
        ])
    }
}

fn kde_output_logical_size(name: &str) -> Result<(i64, i64), String> {
    ensure_gui_session_env("KDE display query")?;
    let output = Command::new("kscreen-doctor")
        .arg("-j")
        .output()
        .map_err(|e| format!("Failed to run kscreen-doctor: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid kscreen JSON: {e}"))?;
    let outputs = value
        .get("outputs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing KDE outputs array".to_string())?;
    for output in outputs {
        if output.get("name").and_then(|v| v.as_str()) == Some(name) {
            let size = output
                .get("size")
                .and_then(|v| v.as_object())
                .ok_or_else(|| "Missing KDE output size".to_string())?;
            let scale = output.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let width = size.get("width").and_then(|v| v.as_i64()).unwrap_or(0);
            let height = size.get("height").and_then(|v| v.as_i64()).unwrap_or(0);
            return Ok((
                (width as f64 / scale).round() as i64,
                (height as f64 / scale).round() as i64,
            ));
        }
    }
    Err(format!("KDE output {name} not found"))
}

fn niri_output_logical_size(name: &str) -> Result<(i64, i64), String> {
    let output = build_niri_command(&["msg", "--json", "outputs"])?
        .output()
        .map_err(|e| format!("Failed to run niri msg: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("Invalid niri JSON: {e}"))?;
    let outputs = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(obj) = value.as_object() {
        obj.values().cloned().collect()
    } else {
        return Err("Unexpected niri outputs shape".into());
    };
    for output in outputs {
        if output.get("name").and_then(|v| v.as_str()) == Some(name) {
            let logical = output
                .get("logical")
                .and_then(|v| v.as_object())
                .ok_or_else(|| "Missing niri logical size".to_string())?;
            let width = logical.get("width").and_then(|v| v.as_i64()).unwrap_or(0);
            let height = logical.get("height").and_then(|v| v.as_i64()).unwrap_or(0);
            return Ok((width, height));
        }
    }
    Err(format!("Niri output {name} not found"))
}

fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args.iter().map(|arg| arg.as_ref()))
        .output()
        .map_err(|e| format!("Failed to run {program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

async fn run_command_async<S: AsRef<str>>(
    program: &str,
    args: &[S],
    timeout_duration: Duration,
) -> Result<(), String> {
    let output = run_command_capture_async(program, args, timeout_duration).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

async fn run_command_capture_async<S: AsRef<str>>(
    program: &str,
    args: &[S],
    timeout_duration: Duration,
) -> Result<std::process::Output, String> {
    let mut command = TokioCommand::new(program);
    command.args(args.iter().map(|arg| arg.as_ref()));
    command.kill_on_drop(true);
    let output_future = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match timeout(timeout_duration, output_future).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(format!("Failed to run {program}: {e}")),
        Err(_) => {
            let args_rendered = args
                .iter()
                .map(|arg| arg.as_ref().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            Err(format!(
                "{program} timed out after {}s (args: {})",
                timeout_duration.as_secs(),
                args_rendered
            ))
        }
    }
}

fn ensure_gui_session_env(action: &str) -> Result<(), String> {
    if gui_session_env_ready() {
        Ok(())
    } else {
        Err(format!(
            "{action} requires XDG_RUNTIME_DIR and either WAYLAND_DISPLAY or DISPLAY"
        ))
    }
}

fn build_niri_command(args: &[&str]) -> Result<Command, String> {
    let mut command = Command::new("niri");
    command.args(args);
    if let Some(socket) = resolve_niri_socket() {
        command.env("NIRI_SOCKET", socket);
    }
    Ok(command)
}

fn run_niri_command(args: &[&str]) -> Result<(), String> {
    let output = build_niri_command(args)?
        .output()
        .map_err(|e| format!("Failed to run niri msg: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn dock_mode_notification_message(attached: bool) -> &'static str {
    if attached {
        "Keyboard attached: bottom screen disabled"
    } else {
        "Keyboard detached: bottom screen enabled"
    }
}

fn send_dock_mode_notification(attached: bool) -> Result<(), String> {
    send_runtime_notification(
        "Zenbook Duo Control",
        dock_mode_notification_message(attached),
        false,
    )
}

fn send_runtime_notification(title: &str, message: &str, urgent: bool) -> Result<(), String> {
    let runtime_dir = env::var("XDG_RUNTIME_DIR")
        .map_err(|_| "XDG_RUNTIME_DIR is not set for runtime notifications".to_string())?;
    let bus_address = env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={runtime_dir}/bus"));
    let urgency = if urgent { "critical" } else { "normal" };

    Command::new("notify-send")
        .args([
            "-a",
            "Zenbook Duo Control",
            "-u",
            urgency,
            "-i",
            "input-keyboard",
            title,
            message,
        ])
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("DBUS_SESSION_BUS_ADDRESS", bus_address)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch runtime notification: {e}"))
}

async fn watch_brightness_sync() -> Result<(), String> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut last_seen: Option<u32> = None;
    let mut last_keyboard_attached: Option<bool> = None;
    let mut topology_settles_at: Option<Instant> = None;

    loop {
        interval.tick().await;

        if !brightness_sync_enabled() {
            last_seen = None;
            last_keyboard_attached = None;
            topology_settles_at = None;
            continue;
        }

        let keyboard_attached = keyboard_attached_from_runtime();
        if last_keyboard_attached != Some(keyboard_attached) {
            // Changing dock mode can make firmware temporarily report maximum
            // brightness while eDP-2 is disabled or re-enabled. Do not adopt
            // that transient value as a user brightness selection.
            last_keyboard_attached = Some(keyboard_attached);
            topology_settles_at = Some(Instant::now() + Duration::from_secs(3));
            last_seen = None;
            continue;
        }

        if let Some(settles_at) = topology_settles_at {
            if Instant::now() < settles_at {
                continue;
            }

            topology_settles_at = None;
            let settings = crate::commands::settings::load_settings_local();
            if let Some(percent) = settings.last_display_brightness_percent {
                // Reapply only the user's persisted level after the display
                // topology is stable, so both panels retain it across a dock
                // or undock operation.
                set_display_brightness(percent)?;
            }
            last_seen = Some(crate::hardware::sysfs::read_display_brightness());
            continue;
        }

        let level = crate::hardware::sysfs::read_display_brightness();
        let max = crate::hardware::sysfs::read_max_brightness();
        if level == 0 || max == 0 {
            continue;
        }

        if last_seen.is_none() {
            last_seen = Some(level);
            continue;
        }

        if last_seen == Some(level) {
            continue;
        }

        set_display_brightness(brightness_percent(level, max))?;
        last_seen = Some(level);
    }
}

fn brightness_sync_enabled() -> bool {
    crate::commands::settings::load_settings_local().sync_brightness
}

fn keyboard_attached_from_runtime() -> bool {
    let Ok(raw) = fs::read_to_string(paths::state_file_path()) else {
        return false;
    };
    let Ok(state) = serde_json::from_str::<RuntimeState>(&raw) else {
        return false;
    };
    state.status.keyboard_attached
}

fn brightness_percent(level: u32, max: u32) -> u8 {
    ((level.saturating_mul(100) + max / 2) / max).min(100) as u8
}

fn set_display_brightness(percent: u8) -> Result<(), String> {
    match crate::runtime::client::request(DaemonRequest::SetDisplayBrightness { percent }) {
        Ok(DaemonResponse::Ack) => Ok(()),
        Ok(DaemonResponse::Error { message }) => Err(message),
        Ok(other) => Err(format!(
            "Unexpected daemon response while setting display brightness: {other:?}"
        )),
        Err(message) => Err(message),
    }
}

fn watch_keyboard_hotkeys() -> Result<(), String> {
    loop {
        let device_paths = find_keyboard_abs_devices()?;
        if device_paths.is_empty() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }

        let mut opened = Vec::new();
        for path in &device_paths {
            match Device::open(path) {
                Ok(device) => opened.push((path.clone(), device)),
                Err(err) => {
                    log::warn!("failed to open {}: {err}", path.display());
                }
            }
        }

        if opened.is_empty() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let mut last_input = Instant::now();
        let mut last_power_save_check = Instant::now();
        let mut backlight_disabled_for_idle = false;

        loop {
            let mut lost_device = false;
            for (path, device) in &mut opened {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            let is_keyboard_input =
                                event.event_type() == EventType::KEY && event.value() != 0;
                            let is_known_hotkey = event.event_type() == EventType::ABSOLUTE
                                && is_hotkey_abs_code(event.code());
                            if is_keyboard_input || is_known_hotkey {
                                last_input = Instant::now();
                                if backlight_disabled_for_idle {
                                    let restore_level =
                                        crate::commands::settings::load_settings_local()
                                            .default_backlight;
                                    if restore_level > 0 {
                                        if let Err(err) =
                                            crate::commands::backlight::set_backlight_daemon_first(
                                                restore_level,
                                            )
                                        {
                                            log::warn!("failed to restore keyboard backlight after input: {err}");
                                        }
                                    }
                                    backlight_disabled_for_idle = false;
                                }
                            }

                            if is_known_hotkey {
                                let value = event.value();
                                if let Err(err) = handle_abs_misc_value(value) {
                                    log::warn!(
                                        "failed to handle hotkey ABS value {} (code {}) from {}: {}",
                                        value,
                                        event.code(),
                                        path.display(),
                                        err
                                    );
                                }
                            }

                            if event.event_type() == EventType::KEY
                                && event.code() == evdev::Key::KEY_F12.code()
                                && event.value() == 1
                            {
                                open_control_window();
                            }
                        }
                    }
                    Err(err) => {
                        log::warn!("keyboard hotkey device lost ({}): {err}", path.display());
                        lost_device = true;
                        break;
                    }
                }
            }

            if lost_device {
                break;
            }

            if last_power_save_check.elapsed() >= Duration::from_secs(1) {
                last_power_save_check = Instant::now();
                let settings = crate::commands::settings::load_settings_local();
                if !settings.keyboard_backlight_power_save {
                    last_input = Instant::now();
                    backlight_disabled_for_idle = false;
                } else if main_screen_is_off() {
                    if !backlight_disabled_for_idle
                        && crate::hardware::sysfs::read_backlight_level() > 0
                    {
                        match crate::commands::backlight::set_backlight_daemon_first(0) {
                            Ok(()) => {
                                backlight_disabled_for_idle = true;
                                log::info!("disabled keyboard backlight while main display is off");
                            }
                            Err(err) => log::warn!(
                                "failed to disable keyboard backlight for display-off: {err}"
                            ),
                        }
                    }
                } else if keyboard_attached_from_runtime() {
                    // Keep the idle timer paused while docked. If display-off
                    // power save turned the light off, the next key press
                    // restores it through the normal input path above.
                    last_input = Instant::now();
                } else if !backlight_disabled_for_idle
                    && last_input.elapsed() >= KEYBOARD_BACKLIGHT_IDLE_TIMEOUT
                    && crate::hardware::sysfs::read_backlight_level() > 0
                {
                    match crate::commands::backlight::set_backlight_daemon_first(0) {
                        Ok(()) => {
                            backlight_disabled_for_idle = true;
                            log::info!("disabled detached keyboard backlight after inactivity");
                        }
                        Err(err) => log::warn!("failed to disable idle keyboard backlight: {err}"),
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

fn main_screen_is_off() -> bool {
    let backlight_off = fs::read_to_string("/sys/class/backlight/intel_backlight/bl_power")
        .map(|value| value.trim() != "0")
        .unwrap_or(false)
        || fs::read_to_string("/sys/class/backlight/intel_backlight/brightness")
            .map(|value| value.trim() == "0")
            .unwrap_or(false);
    if backlight_off {
        return true;
    }

    let dpms_off = fs::read_to_string("/sys/class/drm/card0-eDP-1/dpms")
        .map(|value| value.trim() != "On")
        .unwrap_or(false);
    if dpms_off {
        return true;
    }

    let screen_saver_active = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.ScreenSaver",
            "--object-path",
            "/org/gnome/ScreenSaver",
            "--method",
            "org.gnome.ScreenSaver.GetActive",
        ])
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("true")
        })
        .unwrap_or(false);
    if screen_saver_active {
        return true;
    }

    gnome_idle_timeout_ms()
        .zip(gnome_idle_time_ms())
        .is_some_and(|(timeout, idle)| timeout > 0 && idle >= timeout)
}

fn gnome_idle_timeout_ms() -> Option<u64> {
    static IDLE_TIMEOUT_MS: OnceLock<Option<u64>> = OnceLock::new();
    *IDLE_TIMEOUT_MS.get_or_init(|| {
        let output = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.session", "idle-delay"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .split(|c: char| !c.is_ascii_digit())
            .find(|value| !value.is_empty())?
            .parse::<u64>()
            .ok()
            .map(|seconds| seconds * 1_000)
    })
}

fn gnome_idle_time_ms() -> Option<u64> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.IdleMonitor",
            "--object-path",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "--method",
            "org.gnome.Mutter.IdleMonitor.GetIdletime",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split(|c: char| !c.is_ascii_digit())
        .find(|value| !value.is_empty())?
        .parse()
        .ok()
}

fn find_keyboard_abs_devices() -> Result<Vec<std::path::PathBuf>, String> {
    let mut devices = Vec::new();
    let entries =
        fs::read_dir("/dev/input").map_err(|e| format!("Failed to read /dev/input: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("event") {
            continue;
        }

        let Ok(device) = Device::open(&path) else {
            continue;
        };
        let device_name = device
            .name()
            .map(str::to_string)
            .unwrap_or_default()
            .to_lowercase();
        if !device_name.contains("zenbook duo keyboard") && !device_name.contains("asus_duo") {
            continue;
        }

        let supports_hotkeys = device.supported_absolute_axes().is_some_and(|axes| {
            supported_hotkey_abs_codes()
                .into_iter()
                .any(|axis| axes.contains(axis))
        });
        let supports_keys = device
            .supported_keys()
            .is_some_and(|keys| keys.iter().next().is_some());
        if supports_hotkeys || supports_keys {
            devices.push(path);
        }
    }

    Ok(devices)
}

fn supported_hotkey_abs_codes() -> [AbsoluteAxisType; 2] {
    [AbsoluteAxisType(0x28), AbsoluteAxisType::ABS_VOLUME]
}

fn is_hotkey_abs_code(code: u16) -> bool {
    supported_hotkey_abs_codes()
        .into_iter()
        .any(|axis| axis.0 == code)
}

fn handle_abs_misc_value(value: i32) -> Result<(), String> {
    match value {
        199 => cycle_backlight(),
        16 => step_brightness("down"),
        32 => step_brightness("up"),
        _ => Ok(()),
    }
}

fn cycle_backlight() -> Result<(), String> {
    let current = crate::hardware::sysfs::read_backlight_level();
    let next = match current {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => 0,
    };
    crate::commands::backlight::set_backlight_daemon_first(next)
}

/// The UI is a single-instance application: invoking its installed launcher
/// focuses the existing tray-minimized window, or starts it if needed.
fn open_control_window() {
    let mut command = Command::new("/usr/bin/zenbook-duo-control");
    if env::var_os("XDG_RUNTIME_DIR")
        .map(|runtime_dir| Path::new(&runtime_dir).join("wayland-0").exists())
        .unwrap_or(false)
    {
        command.env("WAYLAND_DISPLAY", "wayland-0");
    }
    let _ = command.spawn();
}

fn step_brightness(direction: &str) -> Result<(), String> {
    let bl = Path::new("/sys/class/backlight/intel_backlight");
    if !bl.exists() {
        return Err("no intel_backlight device found".into());
    }

    let max = fs::read_to_string(bl.join("max_brightness"))
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(0);
    let current = fs::read_to_string(bl.join("brightness"))
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(0);
    let current_percent = brightness_percent(current.max(0) as u32, max.max(0) as u32);
    let next_percent = if direction == "up" {
        current_percent.saturating_add(5).min(100)
    } else {
        current_percent.saturating_sub(5)
    };

    // Route hotkeys through the daemon. This updates the persisted percentage
    // and writes every active internal panel, instead of leaving a stale
    // value that would be restored after the next keyboard dock transition.
    set_display_brightness(next_percent)
}

async fn watch_rotation() -> Result<(), String> {
    // Some Intel ISH firmwares expose acceleration samples but never emit the
    // orientation change notifications consumed by monitor-sensor. Read the
    // IIO axes directly so those machines still support automatic rotation.
    let mut timer = interval(Duration::from_millis(250));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_orientation: Option<Orientation> = None;

    loop {
        timer.tick().await;
        let Some(orientation) = read_accelerometer_orientation() else {
            continue;
        };

        if last_orientation.as_ref() == Some(&orientation) {
            continue;
        }
        last_orientation = Some(orientation.clone());

        let settings = crate::commands::settings::load_settings_local();
        let dual_screen = dual_screen_is_active();
        if !settings.auto_rotate || !dual_screen {
            continue;
        }

        if let Err(err) = crate::hardware::display_config::set_orientation_with_scale(
            &orientation,
            settings.default_scale,
        ) {
            log::warn!("failed to apply accelerometer orientation: {err}");
        } else {
            log::info!("applied accelerometer orientation: {orientation:?}");
        }
    }
}

fn read_accelerometer_orientation() -> Option<Orientation> {
    let device = fs::read_dir("/sys/bus/iio/devices")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| fs::read_to_string(path.join("name")).ok().as_deref() == Some("accel_3d\n"))?;

    let read_axis = |axis: &str| -> Option<i64> {
        fs::read_to_string(device.join(format!("in_accel_{axis}_raw")))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    let (x, y, z) = (read_axis("x")?, read_axis("y")?, read_axis("z")?);
    let (abs_x, abs_y, abs_z) = (x.abs(), y.abs(), z.abs());

    // Ignore face-up/face-down positions. In those positions the z axis is
    // dominant and there is no unambiguous screen orientation.
    if abs_z >= abs_x.max(abs_y) {
        return None;
    }

    let orientation = if abs_x > abs_y {
        if x >= 0 {
            Orientation::Right
        } else {
            Orientation::Left
        }
    } else if y >= 0 {
        Orientation::Inverted
    } else {
        Orientation::Normal
    };

    Some(orientation)
}

fn dual_screen_is_active() -> bool {
    // `gdctl` is the authoritative source for the active GNOME logical
    // monitors.  Do not route this guard through the generic layout parser:
    // a transient compositor refresh can omit a monitor there and suppress an
    // otherwise valid rotation event.
    Command::new("gdctl")
        .arg("show")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| line.contains("Logical monitor #"))
                .count()
                >= 2
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn watches_both_known_hotkey_abs_codes() {
        assert!(is_hotkey_abs_code(0x28));
        assert!(is_hotkey_abs_code(AbsoluteAxisType::ABS_VOLUME.0));
        assert!(!is_hotkey_abs_code(0x27));
    }

    #[test]
    fn dock_mode_notification_mentions_bottom_screen_enabled_on_detach() {
        assert_eq!(
            dock_mode_notification_message(false),
            "Keyboard detached: bottom screen enabled"
        );
    }

    #[test]
    fn dock_mode_notification_mentions_bottom_screen_disabled_on_attach() {
        assert_eq!(
            dock_mode_notification_message(true),
            "Keyboard attached: bottom screen disabled"
        );
    }

    #[test]
    fn gnome_dock_mode_detached_sets_flipped_secondary_transform() {
        let args = build_gnome_dock_mode_args(false, 1.66);
        let joined = args.join(" ");
        assert!(joined.contains("--monitor eDP-1 --transform 180"));
        assert!(joined.contains("--monitor eDP-2 --below eDP-1 --transform normal"));
        assert!(joined.contains("--below eDP-1"));
    }

    #[test]
    fn gnome_dock_mode_attached_explicitly_turns_off_secondary_panel() {
        let args = build_gnome_dock_mode_args(true, 1.66);
        let joined = args.join(" ");
        assert!(joined.contains("--monitor eDP-1 --transform 180"));
        assert!(joined.contains("--monitor eDP-2 --mode off"));
    }

    #[test]
    fn gnome_attached_fallback_targets_primary_panel_only() {
        let args = build_gnome_attached_fallback_args(1.66);
        let joined = args.join(" ");
        assert!(joined.contains("--monitor eDP-1 --transform 180"));
        assert!(!joined.contains("eDP-2"));
    }

    #[test]
    fn gnome_verify_snapshot_parser_reads_logical_section_only() {
        let sample = r#"
Monitors:
└──Monitor eDP-2 (Built-in display)
Logical monitors:
├──Logical monitor #1
│  └──eDP-1 (Built-in display)
└──Logical monitor #2
   └──eDP-2 (Built-in display)
"#;
        let (has_edp1, has_edp2) = gnome_logical_snapshot_from_show(sample);
        assert!(has_edp1);
        assert!(has_edp2);
    }

    #[test]
    fn gnome_verify_expectations_differ_for_attached_and_detached() {
        assert!(gnome_snapshot_matches_expected(true, true, false));
        assert!(!gnome_snapshot_matches_expected(true, true, true));
        assert!(gnome_snapshot_matches_expected(false, true, true));
        assert!(!gnome_snapshot_matches_expected(false, true, false));
    }

    #[tokio::test]
    async fn bind_session_listener_creates_socket_before_registration() {
        let socket_path = unique_test_socket_path("session-listener");
        let listener = bind_session_listener(&socket_path).expect("bind test session listener");

        assert!(
            socket_path.exists(),
            "listener should create the socket path"
        );

        drop(listener);
        let _ = fs::remove_file(&socket_path);
    }

    #[test]
    fn resolve_niri_socket_prefers_existing_env_socket() {
        let runtime_dir = temp_runtime_dir("niri-env");
        let env_socket = runtime_dir.join("niri.wayland-1.env.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&env_socket).expect("bind env socket");

        let resolved =
            resolve_niri_socket_from(Some(env_socket.as_path()), Some(runtime_dir.as_path()))
                .expect("resolve niri socket");

        assert_eq!(resolved, env_socket);

        drop(listener);
        let _ = fs::remove_file(&env_socket);
        let _ = fs::remove_dir_all(&runtime_dir);
    }

    #[test]
    fn resolve_niri_socket_falls_back_to_latest_runtime_socket() {
        let runtime_dir = temp_runtime_dir("niri-fallback");
        let older_socket = runtime_dir.join("niri.wayland-1.older.sock");
        let newer_socket = runtime_dir.join("niri.wayland-1.newer.sock");
        let older_listener =
            std::os::unix::net::UnixListener::bind(&older_socket).expect("bind older socket");
        std::thread::sleep(Duration::from_millis(10));
        let newer_listener =
            std::os::unix::net::UnixListener::bind(&newer_socket).expect("bind newer socket");

        let resolved = resolve_niri_socket_from(None, Some(runtime_dir.as_path()))
            .expect("resolve niri socket");

        assert_eq!(resolved, newer_socket);

        drop(older_listener);
        drop(newer_listener);
        let _ = fs::remove_file(&older_socket);
        let _ = fs::remove_file(&newer_socket);
        let _ = fs::remove_dir_all(&runtime_dir);
    }

    #[test]
    fn detect_backend_falls_back_to_niri_when_socket_is_available() {
        assert_eq!(
            detect_backend_from("".to_string(), true),
            SessionBackend::Niri
        );
    }

    #[test]
    fn detect_backend_remains_unknown_without_desktop_hint_or_socket() {
        assert_eq!(
            detect_backend_from("".to_string(), false),
            SessionBackend::Unknown
        );
    }

    #[test]
    fn detect_ready_backend_prefers_hint_when_ready() {
        let ready = detect_ready_backend_from(SessionBackend::Kde, |backend| {
            backend == SessionBackend::Kde
        });
        assert_eq!(ready, SessionBackend::Kde);
    }

    #[test]
    fn detect_ready_backend_falls_through_to_other_ready_backend() {
        let ready = detect_ready_backend_from(SessionBackend::Unknown, |backend| {
            backend == SessionBackend::Niri
        });
        assert_eq!(ready, SessionBackend::Niri);
    }

    #[test]
    fn detect_ready_backend_returns_unknown_when_nothing_is_ready() {
        let ready = detect_ready_backend_from(SessionBackend::Gnome, |_| false);
        assert_eq!(ready, SessionBackend::Unknown);
    }

    fn unique_test_socket_path(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("zenbook-duo-{label}-{nanos}-{id}.sock"))
    }

    fn temp_runtime_dir(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("zenbook-duo-{label}-{nanos}-{id}"));
        fs::create_dir_all(&dir).expect("create temp runtime dir");
        dir
    }
}
