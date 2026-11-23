use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use crate::ide::protocol::{
    port_file_path, DebugCommand, DebugEvent, IdeToPlugin, ParenBreakpoint, PluginToIde,
};

static ACTIVE_CLIENTS: Mutex<Vec<Arc<Mutex<TcpStream>>>> = Mutex::new(Vec::new());
static ACTIVE_BREAKPOINTS: Mutex<Option<HashMap<String, HashSet<ParenBreakpoint>>>> =
    Mutex::new(None);
static ACTIVE_DEBUG_FILE: Mutex<Option<String>> = Mutex::new(None);
static DEBUG_COMMAND_QUEUE: OnceLock<(Mutex<VecDeque<DebugCommand>>, Condvar)> = OnceLock::new();

fn debug_cmd_sync() -> &'static (Mutex<VecDeque<DebugCommand>>, Condvar) {
    DEBUG_COMMAND_QUEUE.get_or_init(|| (Mutex::new(VecDeque::new()), Condvar::new()))
}

pub fn set_active_debug_file(file: String) {
    *ACTIVE_DEBUG_FILE.lock().unwrap() = Some(file);
}

pub fn get_active_debug_file() -> String {
    ACTIVE_DEBUG_FILE
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "script.lsp".to_string())
}

pub fn clear_debug_command_queue() {
    let (lock, _) = debug_cmd_sync();
    lock.lock().unwrap().clear();
}

pub fn push_debug_command(cmd: DebugCommand) {
    if let DebugCommand::SetBreakpoints(bps) = cmd {
        let active_file = get_active_debug_file();
        sync_breakpoints_for_file(active_file, bps);
        return;
    }
    let (lock, cvar) = debug_cmd_sync();
    lock.lock().unwrap().push_back(cmd);
    cvar.notify_one();
}

pub fn wait_for_debug_command() -> DebugCommand {
    let (lock, cvar) = debug_cmd_sync();
    let mut queue = lock.lock().unwrap();
    let mut ticks = 0u32;
    loop {
        if let Some(cmd) = queue.pop_front() {
            return cmd;
        }
        if !has_active_clients() {
            return DebugCommand::Stop;
        }
        // Send a keepalive immediately on first wait (ticks == 0),
        // and subsequently every 15 seconds (60 ticks @ 250ms).
        if ticks % 60 == 0 && queue.is_empty() {
            drop(queue);
            crate::rust_lisp::send_host_keepalive();
            queue = lock.lock().unwrap();
        }
        let (q, _) = cvar
            .wait_timeout(queue, std::time::Duration::from_millis(250))
            .unwrap();
        queue = q;
        ticks = ticks.wrapping_add(1);
    }
}

fn push_breakpoints_to_lisp_core(map: &HashMap<String, HashSet<ParenBreakpoint>>) {
    let mut all_bps: HashSet<(u32, u32)> = HashSet::new();
    for set in map.values() {
        for bp in set {
            all_bps.insert((bp.line as u32, bp.col as u32));
        }
    }
    let vec_bps: Vec<(u32, u32)> = all_bps.into_iter().collect();
    lisp_core::set_debug_breakpoints(vec_bps);
}

pub fn sync_breakpoints_for_file(file: String, breakpoints: Vec<ParenBreakpoint>) {
    set_active_debug_file(file.clone());
    let mut guard = ACTIVE_BREAKPOINTS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let key = std::path::Path::new(&file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(&file)
        .to_lowercase();
    if breakpoints.is_empty() {
        map.remove(&key);
        map.remove(&file);
    } else {
        let set: HashSet<ParenBreakpoint> = breakpoints.into_iter().collect();
        map.insert(key, set.clone());
        map.insert(file, set);
    }
    push_breakpoints_to_lisp_core(map);
}

pub fn get_active_breakpoints() -> HashMap<String, HashSet<ParenBreakpoint>> {
    ACTIVE_BREAKPOINTS.lock().unwrap().clone().unwrap_or_default()
}

pub fn has_active_breakpoints() -> bool {
    ACTIVE_BREAKPOINTS
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|m| m.values().any(|s| !s.is_empty()))
}

fn extract_defined_c_command(code: &str) -> Option<String> {
    let lower = code.to_ascii_lowercase();
    let mut search_from = 0;
    let mut last_cmd = None;
    while let Some(pos) = lower[search_from..].find("defun") {
        let abs = search_from + pos + 5;
        let rest = &lower[abs..];
        let trimmed = rest.trim_start();
        if let Some(after_c) = trimmed.strip_prefix("c:") {
            let cmd_name: String = after_c
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')' && *c != ';')
                .collect();
            if !cmd_name.is_empty() {
                last_cmd = Some(cmd_name);
            }
        }
        search_from = abs;
    }
    if let Some(ref cmd) = last_cmd {
        // Check if `(c:cmd` is already invoked outside the defun
        let call_pat = format!("(c:{cmd}");
        if lower.contains(&call_pat) {
            return None;
        }
    }
    last_cmd
}

pub fn has_active_clients() -> bool {
    let mut clients = ACTIVE_CLIENTS.lock().unwrap();
    clients.retain(|client_mutex| {
        if let Ok(stream) = client_mutex.lock() {
            stream.peer_addr().is_ok()
        } else {
            false
        }
    });
    !clients.is_empty()
}

/// Attempts to focus an active IDE window. Returns true if at least one client
/// was reachable and successfully received the focus message. If no clients exist
/// or all fail, prunes disconnected clients and returns false.
pub fn focus_or_check_alive() -> bool {
    let Ok(json) = serde_json::to_string(&PluginToIde::FocusWindow) else { return false };
    let line = format!("{json}\n");
    let mut clients = ACTIVE_CLIENTS.lock().unwrap();
    let mut any_alive = false;
    clients.retain(|client_mutex| {
        if let Ok(mut stream) = client_mutex.lock() {
            let ok = stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok();
            if ok {
                any_alive = true;
            }
            ok
        } else {
            false
        }
    });
    any_alive
}

/// Broadcast a message from the plugin to all connected IDE instances.
pub fn broadcast_to_ide(msg: &PluginToIde) {
    let Ok(json) = serde_json::to_string(msg) else { return };
    let line = format!("{json}\n");
    let mut clients = ACTIVE_CLIENTS.lock().unwrap();
    clients.retain(|client_mutex| {
        if let Ok(mut stream) = client_mutex.lock() {
            let ok_write = stream.write_all(line.as_bytes()).is_ok();
            let _ = stream.flush();
            ok_write
        } else {
            false
        }
    });
}

/// Broadcast a console log string to the connected IDE console.
pub fn ide_log(text: &str, is_error: bool) {
    broadcast_to_ide(&PluginToIde::ConsoleOutput {
        text: text.to_string(),
        is_error,
    });
}

/// Broadcast a selection change event to the connected IDE.
pub fn ide_selection_changed(handles: Vec<u64>) {
    broadcast_to_ide(&PluginToIde::SelectionChanged { handles });
}

static SERVER_ONCE: std::sync::Once = std::sync::Once::new();

/// Start the background IPC server listening on localhost ephemeral port.
pub fn start_ide_server() {
    SERVER_ONCE.call_once(|| {
        thread::spawn(move || {
            let listener = match TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[plugin] [OCIDE Server] Failed to bind TCP listener: {e}");
                    return;
                }
            };

            let port = match listener.local_addr() {
                Ok(addr) => addr.port(),
                Err(e) => {
                    eprintln!("[plugin] [OCIDE Server] Failed to get local port: {e}");
                    return;
                }
            };

            // Write port file for discovery
            let port_path = port_file_path();
            if let Some(parent) = port_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&port_path, port.to_string()) {
                eprintln!("[plugin] [OCIDE Server] Failed to write port file {}: {e}", port_path.display());
            } else {
                eprintln!("[plugin] [OCIDE Server] Listening on 127.0.0.1:{port}, port written to {}", port_path.display());
            }

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let _ = stream.set_nodelay(true);
                        let stream_arc = Arc::new(Mutex::new(stream));
                        {
                            ACTIVE_CLIENTS.lock().unwrap().push(Arc::clone(&stream_arc));
                        }
                        thread::spawn(move || {
                            handle_client(stream_arc);
                        });
                    }
                    Err(e) => {
                        eprintln!("[plugin] [OCIDE Server] Incoming connection error: {e}");
                    }
                }
            }
        });
    });
}

struct ClientCleanup(Arc<Mutex<TcpStream>>);
impl Drop for ClientCleanup {
    fn drop(&mut self) {
        let mut clients = ACTIVE_CLIENTS.lock().unwrap();
        clients.retain(|c| !Arc::ptr_eq(c, &self.0));
    }
}

fn handle_client(stream_arc: Arc<Mutex<TcpStream>>) {
    let _cleanup = ClientCleanup(Arc::clone(&stream_arc));
    let reader_stream = {
        let stream = stream_arc.lock().unwrap();
        match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        }
    };

    let reader = BufReader::new(reader_stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<IdeToPlugin>(&line) else { continue };

        match req {
            IdeToPlugin::Ping => {
                let resp = PluginToIde::Pong;
                send_to_stream(&stream_arc, &resp);
            }
            IdeToPlugin::RunScript { title, code } => {
                eprintln!("[plugin] [OCIDE Server] Running OpenCAD LISP script: {title}");
                crate::dispatcher::set_last_dispatched_command(None);
                crate::rust_lisp::store_loaded_script(&title, &code);

                if title.ends_with(".py") {
                    send_to_stream(
                        &stream_arc,
                        &PluginToIde::ConsoleOutput {
                            text: "\n[IDE] Python support is currently shelved; only OpenCAD LISP (.lsp) execution is active.\n".to_string(),
                            is_error: true,
                        },
                    );
                    continue;
                }

                let stream_clone = Arc::clone(&stream_arc);
                let code_clone = code.clone();
                thread::spawn(move || {
                    match crate::rust_lisp::eval_lisp_str_headless(&code_clone) {
                        Ok(out_opt) => {
                            if let Some(out) = out_opt {
                                if !out.is_empty() {
                                    send_to_stream(
                                        &stream_clone,
                                        &PluginToIde::ConsoleOutput {
                                            text: format!("{out}\n"),
                                            is_error: false,
                                        },
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            send_to_stream(
                                &stream_clone,
                                &PluginToIde::ConsoleOutput {
                                    text: format!("\n[IDE OpenCAD LISP Error] {e}\n"),
                                    is_error: true,
                                },
                            );
                        }
                    }
                });
            }
            IdeToPlugin::DebugScript {
                title,
                code,
                breakpoints,
                step_into,
            } => {
                eprintln!("[plugin] [OCIDE Server] Running/Debugging OpenCAD LISP script: {title} (step_into={step_into}, bps={})", breakpoints.len());
                crate::dispatcher::set_last_dispatched_command(None);
                crate::rust_lisp::store_loaded_script(&title, &code);
                clear_debug_command_queue();
                sync_breakpoints_for_file(title.clone(), breakpoints);

                if title.ends_with(".py") {
                    send_to_stream(
                        &stream_arc,
                        &PluginToIde::ConsoleOutput {
                            text: "\n[IDE] Python support is currently shelved; only OpenCAD LISP (.lsp) execution is active.\n".to_string(),
                            is_error: true,
                        },
                    );
                    continue;
                }

                crate::rust_lisp::set_debug_task_running(true);
                let stream_clone = Arc::clone(&stream_arc);
                let code_clone = code.clone();
                thread::spawn(move || {
                    let c_cmd_opt = if step_into {
                        extract_defined_c_command(&code_clone)
                    } else {
                        None
                    };

                    if step_into && c_cmd_opt.is_none() {
                        lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::StepInto);
                    } else {
                        lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::None);
                    }

                    match crate::rust_lisp::eval_lisp_str_headless(&code_clone) {
                        Ok(out_opt) => {
                            if let Some(out) = out_opt {
                                if !out.is_empty() {
                                    send_to_stream(
                                        &stream_clone,
                                        &PluginToIde::ConsoleOutput {
                                            text: format!("{out}\n"),
                                            is_error: false,
                                        },
                                    );
                                }
                            }
                            if let Some(cmd_name) = c_cmd_opt {
                                lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::StepInto);
                                if !dispatch_command_to_cad_automation(&cmd_name) {
                                    let call_expr = format!("(c:{cmd_name})");
                                    match crate::rust_lisp::eval_lisp_str_unspanned_headless(&call_expr) {
                                        Ok(Some(out)) if !out.is_empty() => {
                                            send_to_stream(
                                                &stream_clone,
                                                &PluginToIde::ConsoleOutput {
                                                    text: format!("{out}\n"),
                                                    is_error: false,
                                                },
                                            );
                                        }
                                        Ok(_) => {}
                                        Err(e) => {
                                            crate::rust_lisp::set_debug_task_running(false);
                                            send_to_stream(
                                                &stream_clone,
                                                &PluginToIde::ConsoleOutput {
                                                    text: format!("\n[IDE OpenCAD LISP Error] {e}\n"),
                                                    is_error: true,
                                                },
                                            );
                                            broadcast_to_ide(&PluginToIde::DebugEvt(DebugEvent::Error(e)));
                                            return;
                                        }
                                    }
                                    crate::rust_lisp::set_debug_task_running(false);
                                    broadcast_to_ide(&PluginToIde::DebugEvt(DebugEvent::Finished));
                                }
                            } else {
                                crate::rust_lisp::set_debug_task_running(false);
                                if step_into {
                                    broadcast_to_ide(&PluginToIde::DebugEvt(DebugEvent::Finished));
                                }
                            }
                        }
                        Err(e) => {
                            crate::rust_lisp::set_debug_task_running(false);
                            lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::None);
                            send_to_stream(
                                &stream_clone,
                                &PluginToIde::ConsoleOutput {
                                    text: format!("\n[IDE OpenCAD LISP Error] {e}\n"),
                                    is_error: true,
                                },
                            );
                            broadcast_to_ide(&PluginToIde::DebugEvt(DebugEvent::Error(e)));
                        }
                    }
                });
            }
            IdeToPlugin::DebugCmd(cmd) => {
                eprintln!("[plugin] [DEBUG] Received IdeToPlugin::DebugCmd({:?})", cmd);
                if matches!(cmd, DebugCommand::Stop) {
                    crate::rust_lisp::set_debug_task_running(false);
                }
                push_debug_command(cmd);
            }
            IdeToPlugin::ReplEval { code, mode } => {
                let stream_clone = Arc::clone(&stream_arc);
                thread::spawn(move || {
                    let res = match mode {
                        crate::ide::protocol::ReplMode::Lisp => {
                            crate::rust_lisp::eval_lisp_str_headless(&code)
                                .map(|opt| opt.unwrap_or_default())
                        }
                        crate::ide::protocol::ReplMode::Python => {
                            Err("Python support is currently shelved; switch REPL mode to OpenCAD LISP.".to_string())
                        }
                    };

                    match res {
                        Ok(out) => {
                            if !out.is_empty() {
                                send_to_stream(
                                    &stream_clone,
                                    &PluginToIde::ConsoleOutput {
                                        text: format!("{out}\n"),
                                        is_error: false,
                                    },
                                );
                            }
                        }
                        Err(e) => {
                            send_to_stream(
                                &stream_clone,
                                &PluginToIde::ConsoleOutput {
                                    text: format!("Error: {e}\n"),
                                    is_error: true,
                                },
                            );
                        }
                    }
                });
            }
            IdeToPlugin::SyncBreakpoints { file, breakpoints } => {
                sync_breakpoints_for_file(file, breakpoints);
            }
            IdeToPlugin::RestartScript {
                title,
                code,
                breakpoints,
            } => {
                eprintln!("[plugin] [OCIDE Server] Restarting script: {title}");
                if let Some(bridge) = crate::context::active_interactive() {
                    bridge.coordinator.notify_finished();
                }

                crate::rust_lisp::store_loaded_script(&title, &code);
                sync_breakpoints_for_file(title.clone(), breakpoints);

                let last_cmd = crate::dispatcher::get_last_dispatched_command();
                let title_stem = std::path::Path::new(&title)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&title)
                    .to_lowercase();
                let clean_stem = title_stem
                    .strip_prefix("c-")
                    .or_else(|| title_stem.strip_prefix("c_"))
                    .unwrap_or(&title_stem);

                let cmd_to_dispatch = if let Some(ref cmd) = last_cmd {
                    let lower_cmd = cmd.to_lowercase();
                    let parts: Vec<&str> = lower_cmd.split_whitespace().collect();
                    if parts.contains(&clean_stem)
                        || parts.contains(&title_stem.as_str())
                        || lower_cmd.starts_with(clean_stem)
                        || lower_cmd.starts_with(&title_stem)
                    {
                        cmd.clone()
                    } else {
                        clean_stem.to_string()
                    }
                } else {
                    clean_stem.to_string()
                };

                let stream_clone = Arc::clone(&stream_arc);
                let code_clone = code.clone();

                thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(50));

                    eprintln!("[plugin] [OCIDE Server] Attempting dispatch to CAD automation: {cmd_to_dispatch}");
                    if dispatch_command_to_cad_automation(&cmd_to_dispatch) {
                        send_to_stream(
                            &stream_clone,
                            &PluginToIde::ConsoleOutput {
                                text: format!("\n[IDE] Restarted command '{cmd_to_dispatch}' in OpenCADStudio.\n"),
                                is_error: false,
                            },
                        );
                    } else {
                        match crate::rust_lisp::eval_lisp_str_headless(&code_clone) {
                            Ok(out_opt) => {
                                if let Some(out) = out_opt {
                                    if !out.is_empty() {
                                        send_to_stream(
                                            &stream_clone,
                                            &PluginToIde::ConsoleOutput {
                                                text: format!("{out}\n"),
                                                is_error: false,
                                            },
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                send_to_stream(
                                    &stream_clone,
                                    &PluginToIde::ConsoleOutput {
                                        text: format!("\n[IDE OpenCAD LISP Error] {e}\n"),
                                        is_error: true,
                                    },
                                );
                            }
                        }
                    }
                });
            }
        }
    }
}

pub fn automation_dir() -> std::path::PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        std::path::PathBuf::from(appdata).join("OpenCADStudio").join("automation")
    } else if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        #[cfg(target_os = "macos")]
        {
            p.push("Library");
            p.push("Application Support");
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
                p = std::path::PathBuf::from(xdg);
            } else {
                p.push(".config");
            }
        }
        p.join("OpenCADStudio").join("automation")
    } else {
        std::path::PathBuf::from("automation")
    }
}

pub fn dispatch_command_to_cad_automation(cmd: &str) -> bool {
    let dir = automation_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return false;
    };

    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            files.push((mtime, path));
        }
    }

    // Sort by last modified time descending (newest first)
    files.sort_by(|a, b| b.0.cmp(&a.0));

    for (_mtime, path) in files {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };

        let Some(port) = val["port"].as_u64().map(|p| p as u16) else {
            continue;
        };
        let Some(token) = val["token"].as_str() else {
            continue;
        };

        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(600)) else {
            // Stale file from an old terminated session, prune it
            let _ = std::fs::remove_file(&path);
            continue;
        };

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));

        let req = serde_json::json!({
            "token": token,
            "op": "run",
            "cmd": cmd,
        });

        let line = format!("{}\n", req.to_string());
        if stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok() {
            let mut reader = std::io::BufReader::new(stream);
            let mut reply_line = String::new();
            if reader.read_line(&mut reply_line).is_ok() {
                eprintln!("[plugin] [Automation] Sent '{cmd}' to CAD (port {port}), reply: {}", reply_line.trim());
                return true;
            }
        }
    }

    false
}

fn send_to_stream(stream_arc: &Arc<Mutex<TcpStream>>, msg: &PluginToIde) {
    if let Ok(json) = serde_json::to_string(msg) {
        let line = format!("{json}\n");
        if let Ok(mut stream) = stream_arc.lock() {
            let _ = stream.write_all(line.as_bytes());
            let _ = stream.flush();
        }
    }
}

#[cfg(windows)]
pub fn force_foreground_ide() {
    std::thread::spawn(|| unsafe {
        extern "system" {
            fn GetForegroundWindow() -> *mut std::ffi::c_void;
            fn BringWindowToTop(hWnd: *mut std::ffi::c_void) -> i32;
            fn SetForegroundWindow(hWnd: *mut std::ffi::c_void) -> i32;
            fn ShowWindow(hWnd: *mut std::ffi::c_void, nCmdShow: i32) -> i32;
            fn IsIconic(hWnd: *mut std::ffi::c_void) -> i32;
            fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, lParam: isize) -> i32;
            fn GetWindowTextW(hWnd: *mut std::ffi::c_void, lpString: *mut u16, nMaxCount: i32) -> i32;
            fn SetWindowPos(
                hWnd: *mut std::ffi::c_void,
                hWndInsertAfter: *mut std::ffi::c_void,
                X: i32,
                Y: i32,
                cx: i32,
                cy: i32,
                uFlags: u32,
            ) -> i32;
        }

        const HWND_TOPMOST: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;
        const HWND_NOTOPMOST: *mut std::ffi::c_void = -2isize as *mut std::ffi::c_void;
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_SHOWWINDOW: u32 = 0x0040;
        const SW_RESTORE: i32 = 9;

        unsafe extern "system" fn enum_proc(hwnd: *mut std::ffi::c_void, _: isize) -> i32 {
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                if title.contains("(OCIDE)") {
                    let fg = GetForegroundWindow();
                    if fg != hwnd {
                        if IsIconic(hwnd) != 0 {
                            ShowWindow(hwnd, SW_RESTORE);
                        }
                        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                        BringWindowToTop(hwnd);
                        SetForegroundWindow(hwnd);
                    }
                    return 0;
                }
            }
            1
        }
        EnumWindows(enum_proc, 0);
    });
}

#[cfg(windows)]
pub fn force_foreground_ocide() {
    force_foreground_ide();
}

#[cfg(target_os = "macos")]
pub fn force_foreground_ide() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application \"opencad_lisp_ide\" to activate"])
        .spawn();
}

#[cfg(target_os = "macos")]
pub fn force_foreground_ocide() {
    force_foreground_ide();
}

#[cfg(target_os = "linux")]
pub fn force_foreground_ide() {
    let _ = std::process::Command::new("wmctrl")
        .args(["-a", "OCIDE"])
        .spawn()
        .or_else(|_| {
            std::process::Command::new("xdotool")
                .args(["search", "--name", "OCIDE", "windowactivate"])
                .spawn()
        });
}

#[cfg(target_os = "linux")]
pub fn force_foreground_ocide() {
    force_foreground_ide();
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn force_foreground_ide() {}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn force_foreground_ocide() {}

#[cfg(windows)]
pub fn force_foreground_cad() {
    std::thread::spawn(|| unsafe {
        extern "system" {
            fn GetForegroundWindow() -> *mut std::ffi::c_void;
            fn BringWindowToTop(hWnd: *mut std::ffi::c_void) -> i32;
            fn SetForegroundWindow(hWnd: *mut std::ffi::c_void) -> i32;
            fn ShowWindow(hWnd: *mut std::ffi::c_void, nCmdShow: i32) -> i32;
            fn IsIconic(hWnd: *mut std::ffi::c_void) -> i32;
            fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, lParam: isize) -> i32;
            fn GetWindowTextW(hWnd: *mut std::ffi::c_void, lpString: *mut u16, nMaxCount: i32) -> i32;
            fn SetWindowPos(
                hWnd: *mut std::ffi::c_void,
                hWndInsertAfter: *mut std::ffi::c_void,
                X: i32,
                Y: i32,
                cx: i32,
                cy: i32,
                uFlags: u32,
            ) -> i32;
        }

        const HWND_TOPMOST: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;
        const HWND_NOTOPMOST: *mut std::ffi::c_void = -2isize as *mut std::ffi::c_void;
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_SHOWWINDOW: u32 = 0x0040;
        const SW_RESTORE: i32 = 9;

        unsafe extern "system" fn enum_proc(hwnd: *mut std::ffi::c_void, _: isize) -> i32 {
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                let is_cad = (title.contains("Open CAD Studio") || title.contains("OpenCAD"))
                    && !title.contains("(OCIDE)");
                if is_cad {
                    let fg = GetForegroundWindow();
                    if fg != hwnd {
                        if IsIconic(hwnd) != 0 {
                            ShowWindow(hwnd, SW_RESTORE);
                        }
                        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                        BringWindowToTop(hwnd);
                        SetForegroundWindow(hwnd);
                    }
                    return 0;
                }
            }
            1
        }
        EnumWindows(enum_proc, 0);
    });
}

#[cfg(target_os = "macos")]
pub fn force_foreground_cad() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", "tell application \"OpenCADStudio\" to activate"])
        .spawn();
}

#[cfg(target_os = "linux")]
pub fn force_foreground_cad() {
    let _ = std::process::Command::new("wmctrl")
        .args(["-a", "Open CAD Studio"])
        .spawn()
        .or_else(|_| {
            std::process::Command::new("xdotool")
                .args(["search", "--name", "Open CAD Studio", "windowactivate"])
                .spawn()
        });
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn force_foreground_cad() {}
