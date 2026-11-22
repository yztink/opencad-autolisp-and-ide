//! Pure Rust AutoLISP engine integration for OpenCAD Studio.
//! Bypasses Python completely, executing AutoLISP via `lisp-core` and `lisp-cad`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ocs_plugin_api::host::codec::entities::{
    Arc as CadArc, Circle, Dimension as CadDimension, Insert, Line, LwPolyline, LwVertex, MText, Point, Solid, Text as CadText,
};
use ocs_plugin_api::host::codec::types::{Vector2, Vector3};
use ocs_plugin_api::host::Document as CadDocument;
use ocs_plugin_api::host::{
    EntityType, ExtendedDataRecord, Handle, HostApi, HostSettingValue, LayerConfig,
    XDataValue,
};

use lisp_cad::bridge::{CadBridge, DxfValue};
use lisp_cad::create_cad_env;
use lisp_core::{eval_str, Env, LispError, Value};

use crate::context::{active_interactive, set_active_interactive, with_host, WorkerMsg};
use crate::interactive::{InteractionMode, LispInteractiveBridge, RubberBandSpec};

static REGISTERED_LISP_COMMANDS: OnceLock<Mutex<HashMap<u64, HashMap<String, String>>>> = OnceLock::new();
static GLOBAL_LISP_BRIDGE: OnceLock<Arc<Mutex<OpenCadRustBridge>>> = OnceLock::new();
static LISP_TASK_TX: OnceLock<std::sync::mpsc::Sender<LispTask>> = OnceLock::new();
static LISP_OUTPUT_BUFFER: Mutex<String> = Mutex::new(String::new());

/// Flush any buffered AutoLISP output to the active host session.
///
/// While an interactive command is active the text stays buffered: the host answers
/// requests that arrive during an interactive step with a placeholder host that discards
/// output, so it is delivered when the command ends instead.
pub fn flush_lisp_output() {
    if active_interactive().is_some() {
        return;
    }
    let text = {
        let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
        std::mem::take(&mut *buf)
    };
    if !text.is_empty() {
        for line in text.split('\n') {
            let msg = line.trim_end_matches('\r');
            if !msg.is_empty() {
                let _ = with_host(|h| {
                    h.push_output(msg);
                });
            }
        }
    }
}

/// Text interactive commands left behind when they ended, by ticket. Each is printed by
/// the `LSPFLUSH <ticket>` dispatch its command ends with, which runs with the real host.
static PENDING_COMMAND_OUTPUT: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());
static NEXT_OUTPUT_TICKET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Internal command an ending interactive command dispatches so its remaining text is printed.
pub const FLUSH_OUTPUT_COMMAND: &str = "LSPFLUSH";

/// Keep `text` for the flush dispatch and return the command line that prints it.
fn park_command_output(text: String) -> String {
    let ticket = NEXT_OUTPUT_TICKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut pending = PENDING_COMMAND_OUTPUT.lock().unwrap();
    // A command that ends by committing entities never dispatches; drop what it left.
    if pending.len() >= 16 {
        pending.remove(0);
    }
    pending.push((ticket, text));
    format!("{FLUSH_OUTPUT_COMMAND} {ticket}")
}

/// Print the text a finished interactive command parked under `ticket`.
pub fn flush_pending_command_output(host: &mut dyn HostApi, ticket: &str) {
    let Ok(ticket) = ticket.trim().parse::<u64>() else { return };
    let text = {
        let mut pending = PENDING_COMMAND_OUTPUT.lock().unwrap();
        match pending.iter().position(|(t, _)| *t == ticket) {
            Some(i) => pending.remove(i).1,
            None => return,
        }
    };
    for line in text.lines() {
        if !line.is_empty() {
            host.push_output(line);
        }
    }
}

static PANEL_VALUES: OnceLock<Mutex<HashMap<(String, String), ocs_plugin_api::dialog::DialogValue>>> = OnceLock::new();

fn get_panel_values() -> &'static Mutex<HashMap<(String, String), ocs_plugin_api::dialog::DialogValue>> {
    PANEL_VALUES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn handle_panel_action(
    panel_id: &str,
    control_id: &str,
    action: &str,
    value: Option<&ocs_plugin_api::dialog::DialogValue>,
) {
    if let Some(val) = value {
        let key = (panel_id.to_lowercase(), control_id.to_lowercase());
        get_panel_values().lock().unwrap().insert(key, val.clone());
    }
    if action == "click" {
        let candidates = [
            control_id.to_uppercase(),
            format!("{}_{}", panel_id, control_id).to_uppercase(),
        ];
        for cmd in &candidates {
            if has_lisp_command(cmd) {
                let _ = with_host(|h| h.execute_command(cmd));
                break;
            }
        }
    }
}

fn seed_panel_defaults(panel_id: &str, controls: &[ocs_plugin_api::dialog::DialogControl]) {
    use ocs_plugin_api::dialog::{DialogControl, DialogValue};
    let mut map = get_panel_values().lock().unwrap();
    let pid = panel_id.to_lowercase();
    for ctrl in controls {
        match ctrl {
            DialogControl::Label { id, text, .. } => {
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Text(text.clone()));
            }
            DialogControl::Checkbox { id, default, .. } => {
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Bool(*default));
            }
            DialogControl::Choice { id, options, default_index, .. } => {
                let text = options.get(*default_index).cloned().unwrap_or_default();
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Choice { index: *default_index, text });
            }
            DialogControl::Text { id, default, .. } => {
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Text(default.clone()));
            }
            DialogControl::Number { id, default, .. } => {
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Number(*default));
            }
            DialogControl::Color { id, default, .. } => {
                map.insert((pid.clone(), id.to_lowercase()), DialogValue::Color(*default));
            }
            DialogControl::Row { controls: sub, .. }
            | DialogControl::Column { controls: sub, .. }
            | DialogControl::Fields { controls: sub, .. } => {
                drop(map);
                seed_panel_defaults(panel_id, sub);
                map = get_panel_values().lock().unwrap();
            }
            _ => {}
        }
    }
}

static LAST_LISP_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static LAST_LISP_CMDLINE_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub fn get_last_lisp_error() -> Option<String> {
    LAST_LISP_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

pub fn get_last_lisp_cmdline_error() -> Option<String> {
    LAST_LISP_CMDLINE_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

fn format_and_store_eval_error(err: &LispError) -> String {
    let (cmdline, full) = if let Some(diag) = lisp_core::get_last_error_diagnostic() {
        (diag.format_cmdline(), diag.format_full())
    } else {
        let s = err.to_string();
        (s.clone(), s)
    };
    *LAST_LISP_CMDLINE_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(cmdline);
    *LAST_LISP_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(full.clone());
    full
}

pub fn format_cmdline_with_prefix(prefix: &str, cmdline_err: &str) -> String {
    if prefix.is_empty() {
        cmdline_err.to_string()
    } else if cmdline_err.starts_with("in ") {
        format!("{prefix} {cmdline_err}")
    } else {
        format!("{prefix}: {cmdline_err}")
    }
}

/// Pushes only the concise function error to the OCS command line, and broadcasts
/// the full multi-line diagnostic report (including the DEFUN definition and call stack)
/// to the OCSIDE console if OCSIDE is open.
pub fn push_cad_error_and_notify_ide(host: &mut dyn HostApi, prefix: &str, full_err: &str) {
    if crate::ide::server::has_active_clients() {
        crate::ide::server::ide_log(&format!("\n[{prefix}] {full_err}\n"), true);
    }
    let cmdline = get_last_lisp_cmdline_error()
        .unwrap_or_else(|| full_err.lines().next().unwrap_or(full_err).to_string());
    let msg = format_cmdline_with_prefix(prefix, &cmdline);
    host.push_error(&msg);
}

pub fn push_formatted_error(host: &mut dyn HostApi, prefix: &str, err_text: &str) {
    let mut lines = err_text.lines();
    if let Some(first) = lines.next() {
        if prefix.is_empty() {
            host.push_error(first);
        } else {
            host.push_error(&format!("{prefix}: {first}"));
        }
        for detail in lines {
            host.push_output(&format!("  {detail}"));
        }
    }
}

fn get_registered_commands() -> &'static Mutex<HashMap<u64, HashMap<String, String>>> {
    REGISTERED_LISP_COMMANDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_global_bridge() -> Arc<Mutex<OpenCadRustBridge>> {
    GLOBAL_LISP_BRIDGE.get_or_init(|| Arc::new(Mutex::new(OpenCadRustBridge::new()))).clone()
}

enum LispTask {
    Eval {
        code: String,
        resp: std::sync::mpsc::Sender<Result<Option<String>, String>>,
        worker_tx: std::sync::mpsc::Sender<WorkerMsg>,
    },
    EvalUnspanned {
        code: String,
        resp: std::sync::mpsc::Sender<Result<Option<String>, String>>,
        worker_tx: std::sync::mpsc::Sender<WorkerMsg>,
    },
    Load {
        path: PathBuf,
        resp: std::sync::mpsc::Sender<Result<(), String>>,
        worker_tx: std::sync::mpsc::Sender<WorkerMsg>,
    },
    ExecuteCommand {
        lisp_call: String,
        worker_tx: std::sync::mpsc::Sender<WorkerMsg>,
        done: std::sync::mpsc::Sender<()>,
    },
    CloseTab {
        tab_id: u64,
    },
    WithTab {
        tab_id: u64,
        task: Box<LispTask>,
    },
}

static DEBUG_PAUSED_DURING_TASK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static DEBUG_TASK_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static DEBUG_PAUSE_DEPTH: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub struct DebugPauseGuard;
impl DebugPauseGuard {
    pub fn enter() -> Self {
        DEBUG_PAUSE_DEPTH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}
impl Drop for DebugPauseGuard {
    fn drop(&mut self) {
        DEBUG_PAUSE_DEPTH.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn is_currently_paused_in_debugger() -> bool {
    DEBUG_PAUSE_DEPTH.load(std::sync::atomic::Ordering::SeqCst) > 0
}

pub fn was_paused_during_task() -> bool {
    DEBUG_PAUSED_DURING_TASK.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn is_debug_task_running() -> bool {
    DEBUG_TASK_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn set_debug_task_running(running: bool) {
    DEBUG_TASK_RUNNING.store(running, std::sync::atomic::Ordering::SeqCst);
}

pub fn is_debug_session_active() -> bool {
    is_currently_paused_in_debugger()
        || DEBUG_PAUSED_DURING_TASK.load(std::sync::atomic::Ordering::SeqCst)
        || DEBUG_TASK_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

/// Send a lightweight read-only heartbeat request to the CAD host so any
/// in-flight synchronous V4 RPC (`Dispatch` or `InteractiveEvent`) refreshes
/// its call deadline while the user is inspecting state in the OCSIDE debugger.
pub fn send_host_keepalive() {
    if let Some(sender) = crate::context::get_global_sender() {
        let _ = sender.request(ocs_plugin_api::ipc::protocol::PluginRequest::GetTabId);
    } else if active_interactive().is_none() && crate::context::get_worker_tx().is_some() {
        let _ = with_host(|h| {
            if let Some(sender) = h.plugin_request_sender() {
                let arc_sender: std::sync::Arc<dyn ocs_plugin_api::host::PluginRequestSender> =
                    std::sync::Arc::from(sender);
                crate::context::set_global_sender(Some(arc_sender.clone()));
                let _ = arc_sender.request(ocs_plugin_api::ipc::protocol::PluginRequest::GetTabId);
            }
        });
    }
}

/// Push non-empty output lines to the active host IPC sender when available.
fn try_push_interactive_output(msg: &str) -> bool {
    let Some(sender) = crate::context::get_global_sender() else {
        return false;
    };
    for line in msg.split('\n') {
        let trimmed = line.trim_end_matches('\r');
        if !trimmed.is_empty()
            && sender
                .request(ocs_plugin_api::ipc::protocol::PluginRequest::PushOutput(
                    trimmed.to_string(),
                ))
                .is_err()
        {
            return false;
        }
    }
    true
}

/// Emit text output during LISP execution, preferring synchronous host IPC while interactive
/// and falling back to buffering in `LISP_OUTPUT_BUFFER` for deferred/test delivery.
fn emit_interactive_output(msg: &str) {
    if active_interactive().is_some() && try_push_interactive_output(msg) {
        return;
    }
    for line in msg.split('\n') {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }
        let sent = if active_interactive().is_some() {
            Err("deferred")
        } else {
            with_host(|h| {
                h.push_output(trimmed);
            })
        };
        if sent.is_err() {
            let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(trimmed);
        }
    }
}

fn finish_interactive_or_eval(
    eval_res: Result<Value, LispError>,
    buffered: String,
    resp: Option<std::sync::mpsc::Sender<Result<Option<String>, String>>>,
    worker_tx: Option<&std::sync::mpsc::Sender<WorkerMsg>>,
    global_bridge: &Arc<Mutex<OpenCadRustBridge>>,
) {
    global_bridge.lock().unwrap().reset_stream();

    if let Some(active) = active_interactive() {
        match eval_res {
            Ok(val) => {
                let output = if val == Value::Nil {
                    if buffered.is_empty() {
                        None
                    } else {
                        Some(buffered)
                    }
                } else if resp.is_some() {
                    let val_str = format!("{}", val);
                    if buffered.is_empty() {
                        Some(val_str)
                    } else {
                        Some(format!("{}{}", buffered, val_str))
                    }
                } else if buffered.is_empty() {
                    None
                } else {
                    Some(buffered)
                };
                // Push output synchronously through the plugin request sender before finishing so
                // the host prints it in command history without dispatching `LSPFLUSH`. Fall back
                // to parking `LSPFLUSH` only when no IPC sender exists (e.g. unit tests) and no
                // pending entities would cause the host to skip queued commands.
                if let Some(msg) = output {
                    if !try_push_interactive_output(&msg) {
                        let has_pending_entities = active
                            .coordinator
                            .pending_entities
                            .lock()
                            .map(|e| !e.is_empty())
                            .unwrap_or(false);
                        if !has_pending_entities {
                            active.queue_command(park_command_output(msg));
                        }
                    }
                }
                active.finish();
            }
            Err(ref e) => {
                if !buffered.is_empty() {
                    let _ = try_push_interactive_output(&buffered);
                }
                let err = format_and_store_eval_error(e);
                if crate::ide::server::has_active_clients() {
                    crate::ide::server::ide_log(
                        &format!("\n[AutoLISP Error] {err}\n"),
                        true,
                    );
                }
                let cmdline = get_last_lisp_cmdline_error().unwrap_or_else(|| {
                    err.lines().next().unwrap_or(err.as_str()).to_string()
                });
                let err_msg = format_cmdline_with_prefix("AutoLISP Error", &cmdline);
                if let Some(s) = crate::context::get_global_sender() {
                    let _ = s.request(ocs_plugin_api::ipc::protocol::PluginRequest::PushError(
                        err_msg.clone(),
                    ));
                }
                active.fail(&err_msg);
            }
        }
        set_active_interactive(None);
        flush_deferred_setvars(global_bridge);
        return;
    }

    if let Some(tx) = resp {
        let res = match eval_res {
            Ok(val) => {
                let combined = if val == Value::Nil {
                    if buffered.is_empty() {
                        None
                    } else {
                        Some(buffered)
                    }
                } else {
                    let val_str = format!("{}", val);
                    if buffered.is_empty() {
                        Some(val_str)
                    } else {
                        Some(format!("{}{}", buffered, val_str))
                    }
                };
                Ok(combined)
            }
            Err(e) => {
                if !buffered.is_empty() {
                    let _ = with_host(|h| h.push_output(&buffered));
                }
                Err(format_and_store_eval_error(&e))
            }
        };
        let _ = tx.send(res);
    } else if let Some(wtx) = worker_tx {
        if !buffered.is_empty() {
            let _ = with_host(|h| h.push_output(&buffered));
        }
        let _ = wtx.send(WorkerMsg::Finished(
            eval_res.map(|_| ()).map_err(|e| format_and_store_eval_error(&e)),
        ));
    }
}

fn get_task_tx() -> &'static std::sync::mpsc::Sender<LispTask> {
    LISP_TASK_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<LispTask>();
        std::thread::Builder::new()
            .name("autolisp-worker".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let global_bridge = get_global_bridge();
                let mut tab_envs: HashMap<u64, std::rc::Rc<std::cell::RefCell<Env>>> = HashMap::new();

                lisp_core::set_output_sink(Some(Box::new(|text: &str| {
                    if !text.is_empty() {
                        emit_interactive_output(text);
                    }
                })));

                lisp_core::set_debug_pause_callback(Some(Arc::new(
                    |info: &lisp_core::DebugPauseInfo| -> lisp_core::DebugResumeAction {
                        if !crate::ide::server::has_active_clients() {
                            return lisp_core::DebugResumeAction::Continue;
                        }
                        DEBUG_PAUSED_DURING_TASK.store(true, std::sync::atomic::Ordering::SeqCst);
                        let _pause_guard = DebugPauseGuard::enter();
                        let filename = crate::ide::server::get_active_debug_file();
                        let stack: Vec<(String, usize, String)> = info
                            .stack
                            .iter()
                            .map(|(fn_name, fn_line)| {
                                (
                                    fn_name.clone(),
                                    *fn_line,
                                    filename.clone(),
                                )
                            })
                            .collect();
                        crate::ide::server::broadcast_to_ide(
                            &crate::ide::protocol::PluginToIde::DebugEvt(
                                crate::ide::protocol::DebugEvent::Paused {
                                    filename,
                                    line: info.span.start_line as usize,
                                    col: info.span.start_col as usize,
                                    end_line: info.span.end_line as usize,
                                    end_col: info.span.end_col as usize,
                                    expr_preview: info.expr_preview.clone(),
                                    locals: info.locals.clone(),
                                    stack,
                                },
                            ),
                        );
                        let _ = crate::ide::server::focus_or_check_alive();
                        let cmd = crate::ide::server::wait_for_debug_command();
                        drop(_pause_guard);
                        match cmd {
                            crate::ide::protocol::DebugCommand::Continue => {
                                lisp_core::DebugResumeAction::Continue
                            }
                            crate::ide::protocol::DebugCommand::StepInto => {
                                lisp_core::DebugResumeAction::StepInto
                            }
                            crate::ide::protocol::DebugCommand::StepOver => {
                                lisp_core::DebugResumeAction::StepOver
                            }
                            crate::ide::protocol::DebugCommand::StepOut => {
                                lisp_core::DebugResumeAction::StepOut
                            }
                            crate::ide::protocol::DebugCommand::Stop => {
                                lisp_core::DebugResumeAction::Stop
                            }
                            crate::ide::protocol::DebugCommand::SetBreakpoints(_) => {
                                lisp_core::DebugResumeAction::Continue
                            }
                        }
                    },
                )));

                while let Ok(task) = rx.recv() {
                    let (active_tab, task) = match task {
                        LispTask::CloseTab { tab_id } => {
                            tab_envs.remove(&tab_id);
                            continue;
                        }
                        LispTask::WithTab { tab_id, task } => (tab_id, *task),
                        other => (0, other),
                    };
                    crate::context::set_thread_tab_id(Some(active_tab));
                    let mut env = tab_envs
                        .entry(active_tab)
                        .or_insert_with(|| {
                            let new_env = create_cad_env(global_bridge.clone());
                            register_cad_reflection_builtins(&mut new_env.borrow_mut(), global_bridge.clone());
                            new_env
                        })
                        .clone();
                    match task {
                        LispTask::CloseTab { .. } | LispTask::WithTab { .. } => unreachable!(),
                        LispTask::Eval { code, resp, worker_tx } => {
                            DEBUG_PAUSED_DURING_TASK.store(false, std::sync::atomic::Ordering::SeqCst);
                            global_bridge.lock().unwrap().reset_stream();
                            LISP_OUTPUT_BUFFER.lock().unwrap().clear();
                            crate::context::set_worker_tx(Some(worker_tx));
                            let eval_res = eval_str(&code, &mut env);
                            crate::context::set_worker_tx(None);
                            lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::None);
                            if DEBUG_PAUSED_DURING_TASK.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                crate::ide::server::broadcast_to_ide(
                                    &crate::ide::protocol::PluginToIde::DebugEvt(
                                        crate::ide::protocol::DebugEvent::Finished,
                                    ),
                                );
                            }

                            let buffered = {
                                let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
                                std::mem::take(&mut *buf)
                            };
                            finish_interactive_or_eval(eval_res, buffered, Some(resp), None, &global_bridge);
                        }
                        LispTask::EvalUnspanned { code, resp, worker_tx } => {
                            DEBUG_PAUSED_DURING_TASK.store(false, std::sync::atomic::Ordering::SeqCst);
                            global_bridge.lock().unwrap().reset_stream();
                            LISP_OUTPUT_BUFFER.lock().unwrap().clear();
                            crate::context::set_worker_tx(Some(worker_tx));
                            let eval_res = lisp_core::eval_str_unspanned(&code, &mut env);
                            crate::context::set_worker_tx(None);
                            lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::None);
                            if DEBUG_PAUSED_DURING_TASK.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                crate::ide::server::broadcast_to_ide(
                                    &crate::ide::protocol::PluginToIde::DebugEvt(
                                        crate::ide::protocol::DebugEvent::Finished,
                                    ),
                                );
                            }

                            let buffered = {
                                let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
                                std::mem::take(&mut *buf)
                            };
                            finish_interactive_or_eval(eval_res, buffered, Some(resp), None, &global_bridge);
                        }
                        LispTask::Load { path, resp, worker_tx } => {
                            global_bridge.lock().unwrap().reset_stream();
                            LISP_OUTPUT_BUFFER.lock().unwrap().clear();
                            crate::context::set_worker_tx(Some(worker_tx));
                            let res = match read_lsp_text(&path) {
                                Ok(content) => {
                                    let eval_res = eval_str(&content, &mut env)
                                        .map(|_| ())
                                        .map_err(|e| format_and_store_eval_error(&e));
                                    let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
                                    let buffered = std::mem::take(&mut *buf);
                                    if !buffered.is_empty() {
                                        let _ = with_host(|h| h.push_output(&buffered));
                                    }
                                    eval_res
                                }
                                Err(e) => Err(e.to_string()),
                            };
                            global_bridge.lock().unwrap().reset_stream();
                            crate::context::set_worker_tx(None);
                            let _ = resp.send(res);
                        }
                        LispTask::ExecuteCommand { lisp_call, worker_tx, done } => {
                            DEBUG_PAUSED_DURING_TASK.store(false, std::sync::atomic::Ordering::SeqCst);
                            global_bridge.lock().unwrap().reset_stream();
                            LISP_OUTPUT_BUFFER.lock().unwrap().clear();
                            crate::context::set_worker_tx(Some(worker_tx.clone()));
                            let eval_res = lisp_core::eval_str_unspanned(&lisp_call, &mut env);
                            crate::context::set_worker_tx(None);
                            lisp_core::set_debug_step_mode(lisp_core::DebugStepMode::None);
                            let was_paused = DEBUG_PAUSED_DURING_TASK.swap(false, std::sync::atomic::Ordering::SeqCst);
                            let was_running = DEBUG_TASK_RUNNING.swap(false, std::sync::atomic::Ordering::SeqCst);
                            if was_paused || was_running {
                                crate::ide::server::broadcast_to_ide(
                                    &crate::ide::protocol::PluginToIde::DebugEvt(
                                        crate::ide::protocol::DebugEvent::Finished,
                                    ),
                                );
                            }

                            let buffered = {
                                let mut buf = LISP_OUTPUT_BUFFER.lock().unwrap();
                                std::mem::take(&mut *buf)
                            };
                            finish_interactive_or_eval(eval_res, buffered, None, Some(&worker_tx), &global_bridge);
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("spawn autolisp-worker thread");
        tx
    })
}

fn extract_point(val: &Value) -> Result<[f64; 3], LispError> {
    if let Some(coords) = val.to_vec() {
        if coords.len() >= 2 {
            let x = match coords.first() { Some(Value::Real(r)) => *r, Some(Value::Int(n)) => *n as f64, _ => 0.0 };
            let y = match coords.get(1) { Some(Value::Real(r)) => *r, Some(Value::Int(n)) => *n as f64, _ => 0.0 };
            let z = match coords.get(2) { Some(Value::Real(r)) => *r, Some(Value::Int(n)) => *n as f64, _ => 0.0 };
            return Ok([x, y, z]);
        }
    }
    Err(LispError::TypeError { expected: "point coordinate list '(x y [z])", actual: val.to_string() })
}

fn register_cad_reflection_builtins(env: &mut Env, bridge: Arc<Mutex<OpenCadRustBridge>>) {
    // CAD:ADD-LINE and ADD-LINE
    for name in &["CAD:ADD-LINE", "ADD-LINE"] {
        let b = bridge.clone();
        env.def_subr(name, move |args, _| {
            if args.len() < 2 {
                return Err(LispError::WrongArgCount { expected: "2 or more", actual: args.len() });
            }
            let p1 = extract_point(&args[0])?;
            let p2 = extract_point(&args[1])?;
            let layer = if args.len() > 2 {
                if let Value::Str(s) = &args[2] { s.clone() } else { "0".to_string() }
            } else {
                "0".to_string()
            };
            let mut line = Line::from_coords(p1[0], p1[1], p1[2], p2[0], p2[1], p2[2]);
            line.common.layer = layer;
            if let Some(active) = active_interactive() {
                let handle = active.add_pending_entity(EntityType::Line(line));
                Ok(Value::Ename(handle))
            } else {
                let mut br = b.lock().unwrap();
                let handle = br.ent_make(&[
                    (0, DxfValue::Str("LINE".into())),
                    (8, DxfValue::Str(line.common.layer.clone())),
                    (10, DxfValue::Point(p1)),
                    (11, DxfValue::Point(p2)),
                ]).map_err(LispError::Cad)?;
                Ok(Value::Ename(handle))
            }
        });
    }

    // CAD:ADD-CIRCLE and ADD-CIRCLE
    for name in &["CAD:ADD-CIRCLE", "ADD-CIRCLE"] {
        let b = bridge.clone();
        env.def_subr(name, move |args, _| {
            if args.len() < 2 {
                return Err(LispError::WrongArgCount { expected: "2 or more", actual: args.len() });
            }
            let center = extract_point(&args[0])?;
            let radius = match &args[1] {
                Value::Real(r) => *r,
                Value::Int(n) => *n as f64,
                other => return Err(LispError::TypeError { expected: "real or int", actual: other.to_string() }),
            };
            let layer = if args.len() > 2 {
                if let Value::Str(s) = &args[2] { s.clone() } else { "0".to_string() }
            } else {
                "0".to_string()
            };
            let mut circle = Circle::from_coords(center[0], center[1], center[2], radius);
            circle.common.layer = layer;
            if let Some(active) = active_interactive() {
                let handle = active.add_pending_entity(EntityType::Circle(circle));
                Ok(Value::Ename(handle))
            } else {
                let mut br = b.lock().unwrap();
                let handle = br.ent_make(&[
                    (0, DxfValue::Str("CIRCLE".into())),
                    (8, DxfValue::Str(circle.common.layer.clone())),
                    (10, DxfValue::Point(center)),
                    (40, DxfValue::Real(radius)),
                ]).map_err(LispError::Cad)?;
                Ok(Value::Ename(handle))
            }
        });
    }

    // CAD:LAST-ERROR -> returns the last formatted AutoLISP error string (or nil)
    env.def_subr("CAD:LAST-ERROR", |_, _| {
        Ok(get_last_lisp_error()
            .map(Value::Str)
            .unwrap_or(Value::Nil))
    });

    // Host-aware FINDFILE searching current working directory and OpenCAD scripts directory
    env.def_subr("FINDFILE", |args, _| {
        if args.len() != 1 {
            return Err(LispError::WrongArgCount { expected: "1", actual: args.len() });
        }
        let filename = match &args[0] {
            Value::Str(s) => s.clone(),
            other => return Err(LispError::TypeError { expected: "filename string", actual: other.to_string() }),
        };
        let direct = Path::new(&filename);
        let found = if direct.exists() {
            Some(direct.to_path_buf())
        } else {
            let candidate = scripts_dir().join(&filename);
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        };
        if let Some(p) = found {
            if let Ok(abs) = std::fs::canonicalize(&p) {
                return Ok(Value::Str(abs.to_string_lossy().into_owned()));
            }
            Ok(Value::Str(p.to_string_lossy().into_owned()))
        } else {
            Ok(Value::Nil)
        }
    });

    // Host-aware LOAD resolving against scripts_dir() and decoding UTF-8/BOM or Windows-1252
    env.def_subr("LOAD", |args, env_rc| {
        if args.is_empty() || args.len() > 2 {
            return Err(LispError::WrongArgCount { expected: "1 or 2 (filename [on-failure])", actual: args.len() });
        }
        let filename = match &args[0] {
            Value::Str(s) => s.clone(),
            Value::Sym(s) => s.to_string(),
            other => return Err(LispError::TypeError { expected: "filename string", actual: other.to_string() }),
        };
        if let Some(resolved) = resolve_lisp_script_path(&filename) {
            match read_lsp_text(&resolved) {
                Ok(code) => {
                    if let Ok(mut map) = LOADED_LISP_SCRIPTS.lock() {
                        cache_script_keys(&mut map, &resolved.to_string_lossy(), &code);
                    }
                    return eval_str(&code, env_rc);
                }
                Err(e) => {
                    if args.len() == 2 {
                        return Ok(args[1].clone());
                    }
                    return Err(LispError::Io(format!("Cannot load file '{}': {}", resolved.display(), e)));
                }
            }
        }
        if let Some(in_mem_code) = get_loaded_script(&filename) {
            return eval_str(&in_mem_code, env_rc);
        }
        if args.len() == 2 {
            Ok(args[1].clone())
        } else {
            Err(LispError::Io(format!("Cannot load file '{}': No such file", filename)))
        }
    });
}

#[derive(Debug, Clone)]
pub enum StreamState {
    Line {
        points: Vec<[f64; 3]>,
    },
    Circle {
        center: Option<[f64; 3]>,
    },
    Point,
    Rectang {
        fillet: f64,
        first_corner: Option<[f64; 3]>,
        awaiting_fillet: bool,
    },
}

/// OpenCAD Studio native bridge for AutoLISP
pub struct OpenCadRustBridge {
    pub executed_commands: Vec<String>,
    pub stream_state: Option<StreamState>,
    pub current_color: Option<ocs_plugin_api::host::codec::types::Color>,
    pub sysvars: HashMap<String, DxfValue>,
    pub deferred_setvars: Vec<(String, DxfValue)>,
    pub deferred_entmods: Vec<(Handle, EntityType, Option<Vec<(String, Vec<(i16, DxfValue)>)>>)>,
    pub deferred_entdels: Vec<Handle>,
    pub registered_apps: std::collections::HashSet<String>,
    pub active_host_command: Option<String>,
    pub init_dia_flag: bool,
    /// Pending `initget` bits and keyword string; consumed by the next get* call.
    pub init_get_state: Option<(i32, String)>,
    pub selection_sets: HashMap<u64, Vec<u64>>,
    pub next_ss_handle: u64,
    pub previous_selection: Vec<u64>,
    pub table_cursors: HashMap<String, usize>,
}

fn dxf_to_str(val: &DxfValue) -> String {
    match val {
        DxfValue::Str(s) => s.clone(),
        DxfValue::Int(n) => n.to_string(),
        DxfValue::Real(r) => r.to_string(),
        DxfValue::Point(pt) => format!("{},{}", pt[0], pt[1]),
        DxfValue::Handle(h) => format!("{:X}", h),
        DxfValue::XData(_) => String::new(),
    }
}

pub fn xdata_record_to_dxf(rec: &ExtendedDataRecord) -> Vec<(i16, DxfValue)> {
    let mut pairs = Vec::with_capacity(rec.values.len());
    for val in &rec.values {
        match val {
            XDataValue::String(s) => pairs.push((1000, DxfValue::Str(s.clone()))),
            XDataValue::ControlString(s) => pairs.push((1002, DxfValue::Str(s.clone()))),
            XDataValue::LayerName(s) => pairs.push((1003, DxfValue::Str(s.clone()))),
            XDataValue::BinaryData(b) => {
                pairs.push((
                    1004,
                    DxfValue::Str(b.iter().map(|byte| format!("{:02X}", byte)).collect()),
                ));
            }
            XDataValue::Handle(h) => pairs.push((1005, DxfValue::Str(format!("{:X}", h.value())))),
            XDataValue::Point3D(p) | XDataValue::Position3D(p) => {
                pairs.push((1010, DxfValue::Point([p.x, p.y, p.z])));
            }
            XDataValue::Real(r) => pairs.push((1040, DxfValue::Real(*r))),
            XDataValue::Distance(d) => pairs.push((1041, DxfValue::Real(*d))),
            XDataValue::ScaleFactor(s) => pairs.push((1042, DxfValue::Real(*s))),
            XDataValue::Integer16(n) => pairs.push((1070, DxfValue::Int(*n as i64))),
            XDataValue::Integer32(n) => pairs.push((1071, DxfValue::Int(*n as i64))),
            _ => {}
        }
    }
    pairs
}

pub fn dxf_to_xdata_record(app_name: &str, records: &[(i16, DxfValue)]) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(app_name);
    for (code, val) in records {
        match *code {
            1000 => {
                if let DxfValue::Str(s) = val {
                    rec.add_value(XDataValue::String(s.clone()));
                }
            }
            1002 => {
                if let DxfValue::Str(s) = val {
                    rec.add_value(XDataValue::ControlString(s.clone()));
                }
            }
            1003 => {
                if let DxfValue::Str(s) = val {
                    rec.add_value(XDataValue::LayerName(s.clone()));
                }
            }
            1004 => {
                if let DxfValue::Str(s) = val {
                    let mut bytes = Vec::new();
                    let s_clean = s.trim();
                    for i in (0..s_clean.len()).step_by(2) {
                        if i + 2 <= s_clean.len() {
                            if let Ok(b) = u8::from_str_radix(&s_clean[i..i + 2], 16) {
                                bytes.push(b);
                            }
                        }
                    }
                    rec.add_value(XDataValue::BinaryData(bytes));
                }
            }
            1005 => {
                let h_val = match val {
                    DxfValue::Handle(h) => *h,
                    DxfValue::Str(s) => u64::from_str_radix(s.trim(), 16).unwrap_or(0),
                    DxfValue::Int(n) => *n as u64,
                    _ => 0,
                };
                rec.add_value(XDataValue::Handle(Handle::new(h_val)));
            }
            1010 | 1011 | 1012 | 1013 => {
                if let DxfValue::Point(pt) = val {
                    rec.add_value(XDataValue::Point3D(Vector3::new(pt[0], pt[1], pt[2])));
                }
            }
            1040 => {
                if let Some(r) = dxf_to_f64(val) {
                    rec.add_value(XDataValue::Real(r));
                }
            }
            1041 => {
                if let Some(r) = dxf_to_f64(val) {
                    rec.add_value(XDataValue::Distance(r));
                }
            }
            1042 => {
                if let Some(r) = dxf_to_f64(val) {
                    rec.add_value(XDataValue::ScaleFactor(r));
                }
            }
            1070 => {
                if let Some(n) = dxf_to_i64(val) {
                    rec.add_value(XDataValue::Integer16(n as i16));
                }
            }
            1071 => {
                if let Some(n) = dxf_to_i64(val) {
                    rec.add_value(XDataValue::Integer32(n as i32));
                }
            }
            _ => {}
        }
    }
    rec
}

fn dxf_to_f64(val: &DxfValue) -> Option<f64> {
    match val {
        DxfValue::Real(r) => Some(*r),
        DxfValue::Int(n) => Some(*n as f64),
        DxfValue::Str(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn dxf_to_i64(val: &DxfValue) -> Option<i64> {
    match val {
        DxfValue::Int(n) => Some(*n),
        DxfValue::Real(r) => Some(*r as i64),
        DxfValue::Str(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn dxf_to_bool(val: &DxfValue) -> Option<bool> {
    match val {
        DxfValue::Int(n) => Some(*n != 0),
        DxfValue::Real(r) => Some(*r != 0.0),
        DxfValue::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "t" | "on" | "yes" => Some(true),
            "0" | "false" | "nil" | "off" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn read_header_var(doc: &CadDocument, key: &str) -> Option<DxfValue> {
    let h = &doc.header;
    match key {
        "CLAYER" => Some(DxfValue::Str(h.current_layer_name.clone())),
        "CELTYPE" => Some(DxfValue::Str(h.current_linetype_name.clone())),
        "TEXTSTYLE" => Some(DxfValue::Str(h.current_text_style_name.clone())),
        "DIMSTYLE" => Some(DxfValue::Str(h.current_dimstyle_name.clone())),
        "CMLSTYLE" => Some(DxfValue::Str(h.multiline_style.clone())),
        "LTSCALE" => Some(DxfValue::Real(h.linetype_scale)),
        "CELTSCALE" => Some(DxfValue::Real(h.current_entity_linetype_scale)),
        "PDSIZE" => Some(DxfValue::Real(h.point_display_size)),
        "TEXTSIZE" => Some(DxfValue::Real(h.text_height)),
        "PDMODE" => Some(DxfValue::Int(h.point_display_mode as i64)),
        "ORTHOMODE" => Some(DxfValue::Int(i64::from(h.ortho_mode))),
        "FILLMODE" => Some(DxfValue::Int(i64::from(h.fill_mode))),
        "MIRRTEXT" => Some(DxfValue::Int(i64::from(h.mirror_text))),
        "ATTREQ" => Some(DxfValue::Int(i64::from(h.attribute_request))),
        "ATTDIA" => Some(DxfValue::Int(i64::from(h.attribute_dialog))),
        "DIMASSOC" => Some(DxfValue::Int(h.dimension_associativity as i64)),
        "ANGBASE" => Some(DxfValue::Real(h.angle_base)),
        "ANGDIR" => Some(DxfValue::Int(h.angle_direction as i64)),
        "REGENMODE" => Some(DxfValue::Int(i64::from(h.regen_mode))),
        "BLIPMODE" => Some(DxfValue::Int(i64::from(h.blip_mode))),
        "SPLFRAME" => Some(DxfValue::Int(i64::from(h.spline_frame))),
        "PLINEGEN" => Some(DxfValue::Int(i64::from(h.polyline_linetype_generation))),
        "PSLTSCALE" => Some(DxfValue::Int(i64::from(h.paper_space_linetype_scaling))),
        "DISPSILH" => Some(DxfValue::Int(i64::from(h.display_silhouette))),
        "WORLDVIEW" => Some(DxfValue::Int(i64::from(h.world_view))),
        "LIMCHECK" => Some(DxfValue::Int(i64::from(h.limit_check))),
        "DRAGMODE" => Some(DxfValue::Int(h.drag_mode as i64)),
        "LUNITS" => Some(DxfValue::Int(h.linear_unit_format as i64)),
        "LUPREC" => Some(DxfValue::Int(h.linear_unit_precision as i64)),
        "AUNITS" => Some(DxfValue::Int(h.angular_unit_format as i64)),
        "AUPREC" => Some(DxfValue::Int(h.angular_unit_precision as i64)),
        "THICKNESS" => Some(DxfValue::Real(h.thickness)),
        "ELEVATION" => Some(DxfValue::Real(h.elevation)),
        "INSUNITS" => Some(DxfValue::Int(h.insertion_units as i64)),
        "SPLINETYPE" => Some(DxfValue::Int(h.spline_type as i64)),
        "ISOLINES" => Some(DxfValue::Int(h.isolines as i64)),
        "DIMASO" => Some(DxfValue::Int(i64::from(h.associate_dimensions))),
        "DIMSHO" => Some(DxfValue::Int(i64::from(h.update_dimensions_while_dragging))),
        "QTEXTMODE" => Some(DxfValue::Int(i64::from(h.quick_text_mode))),
        "PLIMCHECK" => Some(DxfValue::Int(i64::from(h.paper_space_limit_check))),
        "VISRETAIN" => Some(DxfValue::Int(i64::from(h.retain_xref_visibility))),
        "USRTIMER" => Some(DxfValue::Int(i64::from(h.user_timer))),
        "ATTMODE" => Some(DxfValue::Int(h.attribute_visibility as i64)),
        "COORDS" => Some(DxfValue::Int(h.coords_mode as i64)),
        "OSMODE" => Some(DxfValue::Int(h.object_snap_mode as i64)),
        "PICKSTYLE" => Some(DxfValue::Int(h.pick_style as i64)),
        "SPLINESEGS" => Some(DxfValue::Int(h.spline_segments as i64)),
        "SURFU" => Some(DxfValue::Int(h.surface_u_density as i64)),
        "SURFV" => Some(DxfValue::Int(h.surface_v_density as i64)),
        "SURFTYPE" => Some(DxfValue::Int(h.surface_type as i64)),
        "SHADEDGE" => Some(DxfValue::Int(h.shade_edge as i64)),
        "MAXACTVP" => Some(DxfValue::Int(h.max_active_viewports as i64)),
        "CMLJUST" => Some(DxfValue::Int(h.multiline_justification as i64)),
        "CMLSCALE" => Some(DxfValue::Real(h.multiline_scale)),
        "TEXTQLTY" => Some(DxfValue::Int(h.text_quality as i64)),
        "SORTENTS" => Some(DxfValue::Int(h.sort_entities as i64)),
        "HALOGAP" => Some(DxfValue::Int(h.halo_gap as i64)),
        "TRACEWID" => Some(DxfValue::Real(h.trace_width)),
        "SKETCHINC" => Some(DxfValue::Real(h.sketch_increment)),
        "SKPOLY" => Some(DxfValue::Int(h.sketch_type as i64)),
        "SKTOLERANCE" => Some(DxfValue::Real(h.sketch_tolerance)),
        _ => None,
    }
}

pub(crate) fn write_header_var(doc: &mut CadDocument, key: &str, val: &DxfValue) -> Option<DxfValue> {
    let h = &mut doc.header;
    match key {
        "CLAYER" => {
            if let DxfValue::Str(s) = val {
                h.current_layer_name = s.clone();
                Some(DxfValue::Str(s.clone()))
            } else {
                None
            }
        }
        "CELTYPE" => {
            if let DxfValue::Str(s) = val {
                h.current_linetype_name = s.clone();
                Some(DxfValue::Str(s.clone()))
            } else {
                None
            }
        }
        "TEXTSTYLE" => {
            if let DxfValue::Str(s) = val {
                h.current_text_style_name = s.clone();
                Some(DxfValue::Str(s.clone()))
            } else {
                None
            }
        }
        "DIMSTYLE" => {
            if let DxfValue::Str(s) = val {
                h.current_dimstyle_name = s.clone();
                Some(DxfValue::Str(s.clone()))
            } else {
                None
            }
        }
        "CMLSTYLE" => {
            if let DxfValue::Str(s) = val {
                h.multiline_style = s.clone();
                Some(DxfValue::Str(s.clone()))
            } else {
                None
            }
        }
        "LTSCALE" => dxf_to_f64(val).filter(|&x| x.is_finite() && x > 0.0).map(|x| {
            h.linetype_scale = x;
            DxfValue::Real(x)
        }),
        "CELTSCALE" => dxf_to_f64(val).filter(|&x| x.is_finite() && x > 0.0).map(|x| {
            h.current_entity_linetype_scale = x;
            DxfValue::Real(x)
        }),
        "PDSIZE" => dxf_to_f64(val).map(|x| {
            h.point_display_size = x;
            DxfValue::Real(x)
        }),
        "TEXTSIZE" => dxf_to_f64(val).map(|x| {
            h.text_height = x;
            DxfValue::Real(x)
        }),
        "PDMODE" => dxf_to_i64(val).map(|x| {
            h.point_display_mode = x as i16;
            DxfValue::Int(x)
        }),
        "ORTHOMODE" => dxf_to_bool(val).map(|b| {
            h.ortho_mode = b;
            DxfValue::Int(i64::from(b))
        }),
        "FILLMODE" => dxf_to_bool(val).map(|b| {
            h.fill_mode = b;
            DxfValue::Int(i64::from(b))
        }),
        "MIRRTEXT" => dxf_to_bool(val).map(|b| {
            h.mirror_text = b;
            DxfValue::Int(i64::from(b))
        }),
        "ATTREQ" => dxf_to_bool(val).map(|b| {
            h.attribute_request = b;
            DxfValue::Int(i64::from(b))
        }),
        "ATTDIA" => dxf_to_bool(val).map(|b| {
            h.attribute_dialog = b;
            DxfValue::Int(i64::from(b))
        }),
        "DIMASSOC" => dxf_to_i64(val).map(|x| {
            h.dimension_associativity = x as i16;
            DxfValue::Int(x)
        }),
        "ANGBASE" => dxf_to_f64(val).map(|x| {
            h.angle_base = x;
            DxfValue::Real(x)
        }),
        "ANGDIR" => dxf_to_i64(val).map(|x| {
            h.angle_direction = x as i16;
            DxfValue::Int(x)
        }),
        "REGENMODE" => dxf_to_bool(val).map(|b| {
            h.regen_mode = b;
            DxfValue::Int(i64::from(b))
        }),
        "BLIPMODE" => dxf_to_bool(val).map(|b| {
            h.blip_mode = b;
            DxfValue::Int(i64::from(b))
        }),
        "SPLFRAME" => dxf_to_bool(val).map(|b| {
            h.spline_frame = b;
            DxfValue::Int(i64::from(b))
        }),
        "PLINEGEN" => dxf_to_bool(val).map(|b| {
            h.polyline_linetype_generation = b;
            DxfValue::Int(i64::from(b))
        }),
        "PSLTSCALE" => dxf_to_bool(val).map(|b| {
            h.paper_space_linetype_scaling = b;
            DxfValue::Int(i64::from(b))
        }),
        "DISPSILH" => dxf_to_bool(val).map(|b| {
            h.display_silhouette = b;
            DxfValue::Int(i64::from(b))
        }),
        "WORLDVIEW" => dxf_to_bool(val).map(|b| {
            h.world_view = b;
            DxfValue::Int(i64::from(b))
        }),
        "LIMCHECK" => dxf_to_bool(val).map(|b| {
            h.limit_check = b;
            DxfValue::Int(i64::from(b))
        }),
        "DRAGMODE" => dxf_to_i64(val).map(|x| {
            h.drag_mode = x as i16;
            DxfValue::Int(x)
        }),
        "LUNITS" => dxf_to_i64(val).map(|x| {
            h.linear_unit_format = x as i16;
            DxfValue::Int(x)
        }),
        "LUPREC" => dxf_to_i64(val).map(|x| {
            let clamped = (x as i16).clamp(0, 8);
            h.linear_unit_precision = clamped;
            DxfValue::Int(clamped as i64)
        }),
        "AUNITS" => dxf_to_i64(val).map(|x| {
            h.angular_unit_format = x as i16;
            DxfValue::Int(x)
        }),
        "AUPREC" => dxf_to_i64(val).map(|x| {
            let clamped = (x as i16).clamp(0, 8);
            h.angular_unit_precision = clamped;
            DxfValue::Int(clamped as i64)
        }),
        "THICKNESS" => dxf_to_f64(val).map(|x| {
            h.thickness = x;
            DxfValue::Real(x)
        }),
        "ELEVATION" => dxf_to_f64(val).map(|x| {
            h.elevation = x;
            DxfValue::Real(x)
        }),
        "INSUNITS" => dxf_to_i64(val).map(|x| {
            h.insertion_units = x as i16;
            DxfValue::Int(x)
        }),
        "SPLINETYPE" => dxf_to_i64(val).map(|x| {
            h.spline_type = x as i16;
            DxfValue::Int(x)
        }),
        "ISOLINES" => dxf_to_i64(val).map(|x| {
            let clamped = (x as i16).clamp(0, 2047);
            h.isolines = clamped;
            DxfValue::Int(clamped as i64)
        }),
        "DIMASO" => dxf_to_bool(val).map(|b| {
            h.associate_dimensions = b;
            DxfValue::Int(i64::from(b))
        }),
        "DIMSHO" => dxf_to_bool(val).map(|b| {
            h.update_dimensions_while_dragging = b;
            DxfValue::Int(i64::from(b))
        }),
        "QTEXTMODE" => dxf_to_bool(val).map(|b| {
            h.quick_text_mode = b;
            DxfValue::Int(i64::from(b))
        }),
        "PLIMCHECK" => dxf_to_bool(val).map(|b| {
            h.paper_space_limit_check = b;
            DxfValue::Int(i64::from(b))
        }),
        "VISRETAIN" => dxf_to_bool(val).map(|b| {
            h.retain_xref_visibility = b;
            DxfValue::Int(i64::from(b))
        }),
        "USRTIMER" => dxf_to_bool(val).map(|b| {
            h.user_timer = b;
            DxfValue::Int(i64::from(b))
        }),
        "ATTMODE" => dxf_to_i64(val).map(|x| {
            h.attribute_visibility = x as i16;
            DxfValue::Int(x)
        }),
        "COORDS" => dxf_to_i64(val).map(|x| {
            h.coords_mode = x as i16;
            DxfValue::Int(x)
        }),
        "OSMODE" => dxf_to_i64(val).map(|x| {
            h.object_snap_mode = x as i32;
            DxfValue::Int(x)
        }),
        "PICKSTYLE" => dxf_to_i64(val).map(|x| {
            h.pick_style = x as i16;
            DxfValue::Int(x)
        }),
        "SPLINESEGS" => dxf_to_i64(val).map(|x| {
            h.spline_segments = x as i16;
            DxfValue::Int(x)
        }),
        "SURFU" => dxf_to_i64(val).map(|x| {
            h.surface_u_density = x as i16;
            DxfValue::Int(x)
        }),
        "SURFV" => dxf_to_i64(val).map(|x| {
            h.surface_v_density = x as i16;
            DxfValue::Int(x)
        }),
        "SURFTYPE" => dxf_to_i64(val).map(|x| {
            h.surface_type = x as i16;
            DxfValue::Int(x)
        }),
        "SHADEDGE" => dxf_to_i64(val).map(|x| {
            h.shade_edge = x as i16;
            DxfValue::Int(x)
        }),
        "MAXACTVP" => dxf_to_i64(val).map(|x| {
            h.max_active_viewports = x as i16;
            DxfValue::Int(x)
        }),
        "CMLJUST" => dxf_to_i64(val).map(|x| {
            h.multiline_justification = x as i16;
            DxfValue::Int(x)
        }),
        "CMLSCALE" => dxf_to_f64(val).map(|x| {
            h.multiline_scale = x;
            DxfValue::Real(x)
        }),
        "TEXTQLTY" => dxf_to_i64(val).map(|x| {
            h.text_quality = x as i16;
            DxfValue::Int(x)
        }),
        "SORTENTS" => dxf_to_i64(val).map(|x| {
            h.sort_entities = x as i16;
            DxfValue::Int(x)
        }),
        "HALOGAP" => dxf_to_i64(val).map(|x| {
            h.halo_gap = x as i16;
            DxfValue::Int(x)
        }),
        "TRACEWID" => dxf_to_f64(val).map(|x| {
            h.trace_width = x;
            DxfValue::Real(x)
        }),
        "SKETCHINC" => dxf_to_f64(val).filter(|&x| x.is_finite() && x > 0.0).map(|x| {
            h.sketch_increment = x;
            DxfValue::Real(x)
        }),
        "SKPOLY" => dxf_to_i64(val).map(|x| {
            h.sketch_type = x as i16;
            DxfValue::Int(x)
        }),
        "SKTOLERANCE" => dxf_to_f64(val).map(|x| {
            h.sketch_tolerance = x;
            DxfValue::Real(x)
        }),
        _ => None,
    }
}

fn parse_cecolor_str(col_str: &str) -> Option<(Option<ocs_plugin_api::host::codec::types::Color>, String)> {
    let upper = col_str.trim().to_uppercase();
    match upper.as_str() {
        "RED" | "1" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(1)), "1".to_string())),
        "YELLOW" | "2" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(2)), "2".to_string())),
        "GREEN" | "3" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(3)), "3".to_string())),
        "CYAN" | "4" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(4)), "4".to_string())),
        "BLUE" | "5" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(5)), "5".to_string())),
        "MAGENTA" | "6" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(6)), "6".to_string())),
        "WHITE" | "7" => Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(7)), "7".to_string())),
        "BYLAYER" | "256" => Some((Some(ocs_plugin_api::host::codec::types::Color::ByLayer), "BYLAYER".to_string())),
        "BYBLOCK" | "0" => Some((Some(ocs_plugin_api::host::codec::types::Color::ByBlock), "BYBLOCK".to_string())),
        other => {
            if let Ok(n) = other.parse::<i16>() {
                if (1..=255).contains(&n) {
                    return Some((Some(ocs_plugin_api::host::codec::types::Color::from_index(n)), n.to_string()));
                }
            }
            None
        }
    }
}

fn default_sysvars() -> HashMap<String, DxfValue> {
    let mut m = HashMap::new();
    let default_doc = CadDocument::new();
    for key in [
        "CLAYER", "CELTYPE", "TEXTSTYLE", "CMLSTYLE", "LTSCALE", "CELTSCALE",
        "PDSIZE", "TEXTSIZE", "PDMODE", "ORTHOMODE", "FILLMODE", "MIRRTEXT",
        "ATTREQ", "ATTDIA", "DIMASSOC", "ANGBASE", "ANGDIR", "REGENMODE",
        "BLIPMODE", "SPLFRAME", "PLINEGEN", "PSLTSCALE", "DISPSILH", "WORLDVIEW",
        "LIMCHECK", "DRAGMODE", "LUNITS", "LUPREC", "AUNITS", "AUPREC",
        "THICKNESS", "ELEVATION", "INSUNITS", "SPLINETYPE", "ISOLINES",
        "DIMASO", "DIMSHO", "QTEXTMODE", "PLIMCHECK", "VISRETAIN", "USRTIMER",
        "ATTMODE", "COORDS", "OSMODE", "PICKSTYLE", "SPLINESEGS", "SURFU",
        "SURFV", "SURFTYPE", "SHADEDGE", "MAXACTVP", "CMLJUST", "CMLSCALE",
        "TEXTQLTY", "SORTENTS", "HALOGAP", "TRACEWID", "SKETCHINC", "SKPOLY",
        "SKTOLERANCE",
    ] {
        if let Some(v) = read_header_var(&default_doc, key) {
            m.insert(key.to_string(), v);
        }
    }
    // Additional standard CAD / OpenCAD Studio system variables
    m.insert("CECOLOR".into(), DxfValue::Str("BYLAYER".into()));
    m.insert("CMDECHO".into(), DxfValue::Int(1));
    m.insert("CTAB".into(), DxfValue::Str("Model".into()));
    m.insert("SNAPANG".into(), DxfValue::Real(0.0));
    m.insert("SNAPMODE".into(), DxfValue::Int(0));
    m.insert("GRIDMODE".into(), DxfValue::Int(1));
    m.insert("DYNMODE".into(), DxfValue::Int(3));
    m.insert("AUTOSNAP".into(), DxfValue::Int(63));
    m.insert("CIRCLERAD".into(), DxfValue::Real(0.0));
    m.insert("FILLETRAD".into(), DxfValue::Real(0.0));
    m.insert("CHAMFERA".into(), DxfValue::Real(0.0));
    m.insert("CHAMFERB".into(), DxfValue::Real(0.0));
    m.insert("OFFSETDIST".into(), DxfValue::Real(-1.0));
    m.insert("PLINEWID".into(), DxfValue::Real(0.0));
    m.insert("DONUTID".into(), DxfValue::Real(0.5));
    m.insert("DONUTOD".into(), DxfValue::Real(1.0));
    m.insert("POLYSIDES".into(), DxfValue::Int(4));
    m.insert("DIMSCALE".into(), DxfValue::Real(1.0));
    m.insert("DIMTXT".into(), DxfValue::Real(2.5));
    m.insert("DIMASZ".into(), DxfValue::Real(2.5));
    m.insert("DIMEXO".into(), DxfValue::Real(0.625));
    m.insert("DIMEXE".into(), DxfValue::Real(1.25));
    m.insert("DIMGAP".into(), DxfValue::Real(0.625));
    m.insert("DIMDEC".into(), DxfValue::Int(4));
    m.insert("DIMSTYLE".into(), DxfValue::Str("Standard".into()));
    m.insert("LASTPOINT".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("VIEWCTR".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("VIEWSIZE".into(), DxfValue::Real(100.0));
    m.insert("LIMMIN".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("LIMMAX".into(), DxfValue::Point([420.0, 297.0, 0.0]));
    m.insert("EXTMIN".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("EXTMAX".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("INSBASE".into(), DxfValue::Point([0.0, 0.0, 0.0]));
    m.insert("HIGHLIGHT".into(), DxfValue::Int(1));
    m.insert("PICKFIRST".into(), DxfValue::Int(1));
    m.insert("PICKADD".into(), DxfValue::Int(2));
    m.insert("PICKBOX".into(), DxfValue::Int(3));
    m.insert("APERTURE".into(), DxfValue::Int(10));
    m.insert("CURSORSIZE".into(), DxfValue::Int(5));
    m.insert("GRIPSIZE".into(), DxfValue::Int(5));
    m.insert("GRIPOBJLIMIT".into(), DxfValue::Int(100));
    m.insert("DELOBJ".into(), DxfValue::Int(1));
    m.insert("EXPERT".into(), DxfValue::Int(0));
    m.insert("FILEDIA".into(), DxfValue::Int(1));
    m.insert("CMDDIA".into(), DxfValue::Int(1));
    m.insert("NOMUTT".into(), DxfValue::Int(0));
    m.insert("ERRNO".into(), DxfValue::Int(0));
    m.insert("CMDACTIVE".into(), DxfValue::Int(0));
    m.insert("ACADVER".into(), DxfValue::Str("24.0".into()));
    m.insert("DWGNAME".into(), DxfValue::Str("Drawing1.dwg".into()));
    m.insert("DWGPREFIX".into(), DxfValue::Str("".into()));
    m.insert("LOGINNAME".into(), DxfValue::Str("User".into()));
    m.insert("PLATFORM".into(), DxfValue::Str("OpenCAD Studio".into()));
    for i in 1..=5 {
        m.insert(format!("USERI{i}"), DxfValue::Int(0));
        m.insert(format!("USERR{i}"), DxfValue::Real(0.0));
        m.insert(format!("USERS{i}"), DxfValue::Str(String::new()));
    }
    m
}

fn parse_point_token(tok: &str, last_pt: Option<[f64; 3]>) -> Option<[f64; 3]> {
    let t = tok.trim();
    let is_relative = t.starts_with('@');
    let clean = if is_relative { &t[1..] } else { t };
    if clean.contains('<') {
        let parts: Vec<&str> = clean.split('<').collect();
        if parts.len() == 2 {
            let dist = parts[0].trim().parse::<f64>().ok()?;
            let angle_deg = parts[1].trim().parse::<f64>().ok()?;
            let angle_rad = angle_deg.to_radians();
            let x = dist * angle_rad.cos();
            let y = dist * angle_rad.sin();
            if is_relative {
                if let Some(base) = last_pt {
                    return Some([base[0] + x, base[1] + y, base[2]]);
                }
            }
            return Some([x, y, 0.0]);
        }
    }
    let parts: Vec<&str> = clean.split(',').collect();
    if parts.len() >= 2 {
        let x = parts[0].trim().parse::<f64>().ok()?;
        let y = parts[1].trim().parse::<f64>().ok()?;
        let z = if parts.len() >= 3 {
            parts[2].trim().parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };
        if is_relative {
            if let Some(base) = last_pt {
                Some([base[0] + x, base[1] + y, base[2] + z])
            } else {
                Some([x, y, z])
            }
        } else {
            Some([x, y, z])
        }
    } else {
        None
    }
}

fn add_stream_entity(entity: EntityType) {
    if let Some(active) = active_interactive() {
        active.add_pending_entity(entity);
    } else {
        let _ = with_host(|h| {
            let layer = entity.common().layer.clone();
            if !layer.is_empty() && layer != "0" && !h.document().layers.contains(&layer) {
                let _ = h.add_layer(LayerConfig {
                    name: layer,
                    ..Default::default()
                });
            }
            h.add_entity(entity);
            h.set_dirty();
            h.bump_geometry();
        });
    }
}

pub fn is_host_setvar_supported(key: &str) -> bool {
    matches!(
        key,
        "PERSPECTIVE"
            | "GRIDMODE"
            | "GRIDUNIT"
            | "CETRANSPARENCY"
            | "LTSCALE"
            | "CELTSCALE"
            | "PDMODE"
            | "PDSIZE"
            | "TEXTSIZE"
            | "ORTHOMODE"
            | "FILLMODE"
            | "MIRRTEXT"
            | "DYNMODE"
            | "ZOOMWHEEL"
            | "ZOOMFACTOR"
            | "SHORTCUTMENU"
            | "SHORTCUTMENUDURATION"
            | "CURSORSIZE"
            | "PICKBOX"
            | "CURSORTYPE"
            | "SNAPANG"
            | "TEXTFILL"
            | "CLIPROMPTLINES"
            | "COMMANDLINEFADETIME"
            | "ATTREQ"
            | "ATTDIA"
            | "DIMASSOC"
            | "DIMCONTINUEMODE"
            | "ANGBASE"
            | "ANGDIR"
            | "REGENMODE"
            | "BLIPMODE"
            | "SPLFRAME"
            | "DELOBJ"
            | "SOLIDHIST"
            | "SHOWHIST"
            | "PLINEGEN"
            | "PSLTSCALE"
            | "DISPSILH"
            | "WORLDVIEW"
            | "LIMCHECK"
            | "DRAGMODE"
            | "LUNITS"
            | "LUPREC"
            | "AUNITS"
            | "AUPREC"
            | "THICKNESS"
            | "ELEVATION"
            | "INSUNITS"
            | "SPLINETYPE"
            | "ISOLINES"
            | "DIMASO"
            | "DIMSHO"
            | "QTEXTMODE"
            | "PLIMCHECK"
            | "VISRETAIN"
            | "USRTIMER"
            | "ATTMODE"
            | "COORDS"
            | "OSMODE"
            | "PICKSTYLE"
            | "SPLINESEGS"
            | "SURFU"
            | "SURFV"
            | "SURFTYPE"
            | "SHADEDGE"
            | "MAXACTVP"
            | "CMLJUST"
            | "CMLSCALE"
            | "TEXTQLTY"
            | "SORTENTS"
            | "HALOGAP"
            | "TRACEWID"
            | "SKETCHINC"
            | "SKPOLY"
            | "SKTOLERANCE"
            | "DONUTID"
            | "DONUTOD"
            | "CENTEREXE"
            | "CENTERLAYER"
            | "CENTERLTYPE"
            | "CENTERLTSCALE"
            | "CENTERLTYPEFILE"
            | "CENTERCROSSSIZE"
            | "CENTERCROSSGAP"
            | "CENTERMARKEXE"
            | "COLORTHEME"
            | "SELECTIONAREA"
            | "SELECTIONAREAOPACITY"
            | "SELECTIONEFFECT"
            | "SELECTIONEFFECTCOLOR"
            | "WINDOWSAREACOLOR"
            | "WINDOWAREACOLOR"
            | "CROSSINGAREACOLOR"
            | "SELECTIONPREVIEW"
            | "GRIPSIZE"
            | "GRIPCOLOR"
            | "GRIPHOT"
            | "GRIPHOVER"
            | "GRIPOBJLIMIT"
            | "CONSTRAINTSOLVEMODE"
            | "CONSTRAINTINFER"
            | "CONSTRAINTBARDISPLAY"
            | "CONSTRAINTBARMODE"
            | "CONSTRAINTNAMEFORMAT"
            | "DYNCONSTRAINTDISPLAY"
            | "CCONSTRAINTFORM"
            | "FRAME"
            | "IMAGEFRAME"
            | "PDFFRAME"
            | "DWFFRAME"
            | "DGNFRAME"
            | "XCLIPFRAME"
            | "WIPEOUTFRAME"
            | "PDFOSNAP"
            | "DWFOSNAP"
            | "DGNOSNAP"
            | "UOSNAP"
            | "FIELDDISPLAY"
            | "SURFACEASSOCIATIVITY"
            | "UCSORTHO"
            | "UCSVIEW"
            | "PDFIMPORTMODE"
            | "PDFIMPORTFILTER"
            | "PDFIMPORTLAYERS"
            | "PDFIMPORTIMAGEPATH"
            | "XDWGFADECTL"
            | "POINTCLOUDCLIPFRAME"
            | "POINTCLOUDDENSITY"
            | "POINTCLOUDPOINTSIZE"
            | "POINTCLOUDLOCK"
            | "POINTCLOUDAUTOUPDATE"
            | "POINTCLOUDBOUNDARY"
            | "POINTCLOUDRTDENSITY"
            | "POINTCLOUDLOD"
            | "POINTCLOUDPOINTMAX"
            | "POINTCLOUDVISRETAIN"
            | "POINTCLOUDSHADING"
            | "POINTCLOUDCACHESIZE"
            | "POINTCLOUD2DVSDISPLAY"
            | "POINTCLOUDLIGHTING"
            | "POINTCLOUDLIGHTSOURCE"
            | "POINTCLOUDPOINTMAXLEGACY"
    )
}

/// Apply a system variable change to the live OpenCAD Studio host across IPC.
pub fn apply_host_sysvar(host: &mut dyn HostApi, key: &str, val: &DxfValue) -> Result<DxfValue, String> {
    match key {
        "CTAB" => {
            let layout_name = dxf_to_str(val).trim().to_string();
            if layout_name.is_empty() {
                return Err("CTAB requires a layout name".to_string());
            }
            host.table_operation(ocs_plugin_api::host::TableOperation::LayoutSetCurrent {
                name: layout_name.clone(),
            })?;
            Ok(DxfValue::Str(layout_name))
        }
        "CLAYER" => {
            let trimmed = dxf_to_str(val).trim().to_string();
            if trimmed.is_empty() {
                return Err("CLAYER requires a layer name".to_string());
            }
            if trimmed != "0" && !host.document().layers.contains(&trimmed) {
                let _ = host.add_layer(LayerConfig {
                    name: trimmed.clone(),
                    ..Default::default()
                });
            }
            let _ = host.set_system_variable("CLAYER", HostSettingValue::Text(trimmed.clone()));
            host.document_mut().header.current_layer_name = trimmed.clone();
            Ok(DxfValue::Str(trimmed))
        }
        "SNAPANG" => {
            if let Some(ang) = dxf_to_f64(val) {
                let _ = host.set_system_variable("SNAPANG", HostSettingValue::Number(ang));
                Ok(DxfValue::Real(ang))
            } else {
                Err("SNAPANG requires a numeric angle".to_string())
            }
        }
        "CECOLOR" => {
            let raw_str = dxf_to_str(val);
            if let Some((col_opt, canonical)) = parse_cecolor_str(&raw_str) {
                let _ = host.execute_command(&format!("COLOR {}\n", canonical));
                if let Some(c) = col_opt {
                    host.document_mut().header.current_entity_color = c;
                }
                Ok(DxfValue::Str(canonical))
            } else {
                Err(format!("Invalid CECOLOR value: {}", raw_str))
            }
        }
        "CELTYPE" => {
            let lt_name = dxf_to_str(val).trim().to_string();
            let _ = host.execute_command(&format!("LINETYPE SET {}\n", lt_name));
            host.document_mut().header.current_linetype_name = lt_name.clone();
            Ok(DxfValue::Str(lt_name))
        }
        "TEXTSTYLE" => {
            let style_name = dxf_to_str(val).trim().to_string();
            let _ = host.table_operation(ocs_plugin_api::host::TableOperation::StyleSetCurrent {
                kind: ocs_plugin_api::host::TableStyleKind::Text,
                name: style_name.clone(),
            });
            host.document_mut().header.current_text_style_name = style_name.clone();
            Ok(DxfValue::Str(style_name))
        }
        "DIMSTYLE" => {
            let style_name = dxf_to_str(val).trim().to_string();
            let _ = host.table_operation(ocs_plugin_api::host::TableOperation::StyleSetCurrent {
                kind: ocs_plugin_api::host::TableStyleKind::Dim,
                name: style_name.clone(),
            });
            host.document_mut().header.current_dimstyle_name = style_name.clone();
            Ok(DxfValue::Str(style_name))
        }
        _ => {
            let mut final_val = val.clone();
            if let Some(updated) = write_header_var(host.document_mut(), key, val) {
                final_val = updated;
            }
            if is_host_setvar_supported(key) {
                let token = dxf_to_str(&final_val);
                let _ = host.execute_command(&format!("SETVAR {} {}\n", key, token));
            }
            Ok(final_val)
        }
    }
}

fn flush_deferred_setvars(bridge: &Arc<Mutex<OpenCadRustBridge>>) {
    let (pending_vars, pending_mods, pending_dels) = {
        let mut br = bridge.lock().unwrap();
        (
            std::mem::take(&mut br.deferred_setvars),
            std::mem::take(&mut br.deferred_entmods),
            std::mem::take(&mut br.deferred_entdels),
        )
    };
    if pending_vars.is_empty() && pending_mods.is_empty() && pending_dels.is_empty() {
        return;
    }
    if let Some(sender) = crate::context::get_global_sender() {
        std::thread::spawn(move || {
            // Wait briefly for the interactive CommandStep to complete on the host
            std::thread::sleep(Duration::from_millis(25));
            let mut host = crate::context::DebugHost::new(Some(sender));
            for (k, v) in pending_vars {
                let _ = apply_host_sysvar(&mut host, &k, &v);
            }
            for (handle, entity, xdata_apps) in pending_mods {
                if let Some(apps) = &xdata_apps {
                    for (app_name, records) in apps {
                        if records.is_empty() {
                            let _ = host.remove_record(handle, app_name);
                        } else {
                            let rec = dxf_to_xdata_record(app_name, records);
                            let _ = host.write_record(handle, rec);
                        }
                    }
                }
                let _ = host.update_entity(entity);
            }
            for handle in pending_dels {
                let _ = host.remove_entity(handle);
            }
        });
    }
}

impl OpenCadRustBridge {
    pub fn new() -> Self {
        Self {
            executed_commands: Vec::new(),
            stream_state: None,
            current_color: None,
            sysvars: default_sysvars(),
            deferred_setvars: Vec::new(),
            deferred_entmods: Vec::new(),
            deferred_entdels: Vec::new(),
            registered_apps: std::collections::HashSet::new(),
            active_host_command: None,
            init_dia_flag: false,
            init_get_state: None,
            selection_sets: HashMap::new(),
            next_ss_handle: 1,
            previous_selection: Vec::new(),
            table_cursors: HashMap::new(),
        }
    }

    pub fn reset_stream(&mut self) {
        self.stream_state = None;
        self.active_host_command = None;
        self.init_dia_flag = false;
    }

    pub fn current_layer(&self) -> String {
        if let Ok(DxfValue::Str(s)) = self.get_var("CLAYER") {
            if !s.is_empty() {
                return s;
            }
        }
        "0".to_string()
    }

    /// Process command tokens through the streaming CAD state machine.
    /// Handles progressive (command ...) pipelines across multiple AutoLISP calls
    /// (e.g. (command "_.LINE" p1) followed by loop of (command pi) and ending with (command "C")).
    pub fn process_stream_tokens(&mut self, tokens: &[&str]) -> bool {
        let mut handled_anything = false;
        let mut idx = 0;

        while idx < tokens.len() {
            let tok = tokens[idx].trim();
            if tok.is_empty() {
                idx += 1;
                continue;
            }

            let upper_verb = tok.trim_start_matches(|c| c == '_' || c == '.').to_uppercase();

            // 1. Check for command verb starting a new stream
            match upper_verb.as_str() {
                "COLOR" => {
                    if idx + 1 < tokens.len() {
                        let col_str = tokens[idx + 1].trim().to_uppercase();
                        if let Some((col_opt, canonical)) = parse_cecolor_str(&col_str) {
                            self.current_color = col_opt;
                            self.sysvars.insert("CECOLOR".into(), DxfValue::Str(canonical));
                        } else {
                            self.current_color = None;
                        }
                        idx += 2;
                    } else {
                        idx += 1;
                    }
                    handled_anything = true;
                    continue;
                }
                "LINE" => {
                    self.stream_state = Some(StreamState::Line { points: Vec::new() });
                    handled_anything = true;
                    idx += 1;
                    continue;
                }
                "CIRCLE" => {
                    self.stream_state = Some(StreamState::Circle { center: None });
                    handled_anything = true;
                    idx += 1;
                    continue;
                }
                "POINT" => {
                    self.stream_state = Some(StreamState::Point);
                    handled_anything = true;
                    idx += 1;
                    continue;
                }
                "RECTANG" | "RECTANGLE" => {
                    self.stream_state = Some(StreamState::Rectang {
                        fillet: 0.0,
                        first_corner: None,
                        awaiting_fillet: false,
                    });
                    handled_anything = true;
                    idx += 1;
                    continue;
                }
                "LAYER" | "-LAYER" => {
                    self.update_layer_sysvar_from_tokens(tokens, idx + 1);
                    self.stream_state = None;
                    idx = tokens.len();
                    continue;
                }
                _ => {}
            }

            let cur_layer = self.current_layer();

            // 2. Feed token to active stream state
            match &mut self.stream_state {
                Some(StreamState::Line { points }) => {
                    if tok.eq_ignore_ascii_case("C") || tok.eq_ignore_ascii_case("CLOSE") {
                        if points.len() >= 3 {
                            let first = points[0];
                            let last = *points.last().unwrap();
                            let mut line = Line::from_coords(
                                last[0], last[1], last[2],
                                first[0], first[1], first[2],
                            );
                            line.common.layer = cur_layer;
                            if let Some(col) = self.current_color {
                                line.common.color = col;
                            }
                            add_stream_entity(EntityType::Line(line));
                        }
                        self.stream_state = None;
                        handled_anything = true;
                    } else if tok.eq_ignore_ascii_case("ENTER") {
                        self.stream_state = None;
                        handled_anything = true;
                    } else if let Some(pt) = parse_point_token(tok, points.last().copied()) {
                        if let Some(prev) = points.last() {
                            let mut line = Line::from_coords(
                                prev[0], prev[1], prev[2],
                                pt[0], pt[1], pt[2],
                            );
                            line.common.layer = cur_layer;
                            if let Some(col) = self.current_color {
                                line.common.color = col;
                            }
                            add_stream_entity(EntityType::Line(line));
                        }
                        points.push(pt);
                        handled_anything = true;
                    }
                }
                Some(StreamState::Circle { center }) => {
                    if tok.eq_ignore_ascii_case("ENTER") {
                        self.stream_state = None;
                        handled_anything = true;
                    } else if center.is_none() {
                        if let Some(pt) = parse_point_token(tok, None) {
                            *center = Some(pt);
                            handled_anything = true;
                        }
                    } else {
                        let c = center.unwrap();
                        if let Ok(r) = tok.parse::<f64>() {
                            let mut circle = Circle::from_coords(c[0], c[1], c[2], r);
                            circle.common.layer = cur_layer;
                            if let Some(col) = self.current_color {
                                circle.common.color = col;
                            }
                            add_stream_entity(EntityType::Circle(circle));
                            self.stream_state = None;
                            handled_anything = true;
                        } else if let Some(pt2) = parse_point_token(tok, None) {
                            let dx = pt2[0] - c[0];
                            let dy = pt2[1] - c[1];
                            let dz = pt2[2] - c[2];
                            let r = (dx * dx + dy * dy + dz * dz).sqrt();
                            let mut circle = Circle::from_coords(c[0], c[1], c[2], r);
                            circle.common.layer = cur_layer;
                            if let Some(col) = self.current_color {
                                circle.common.color = col;
                            }
                            add_stream_entity(EntityType::Circle(circle));
                            self.stream_state = None;
                            handled_anything = true;
                        }
                    }
                }
                Some(StreamState::Point) => {
                    if tok.eq_ignore_ascii_case("ENTER") {
                        self.stream_state = None;
                        handled_anything = true;
                    } else if let Some(pt) = parse_point_token(tok, None) {
                        let mut point = Point::from_coords(pt[0], pt[1], pt[2]);
                        point.common.layer = cur_layer;
                        if let Some(col) = self.current_color {
                            point.common.color = col;
                        }
                        add_stream_entity(EntityType::Point(point));
                        self.stream_state = None;
                        handled_anything = true;
                    }
                }
                Some(StreamState::Rectang { fillet, first_corner, awaiting_fillet }) => {
                    if tok.eq_ignore_ascii_case("_NON") || tok.eq_ignore_ascii_case("NON") {
                        handled_anything = true;
                    } else if tok.eq_ignore_ascii_case("_F") || tok.eq_ignore_ascii_case("F") {
                        *awaiting_fillet = true;
                        handled_anything = true;
                    } else if *awaiting_fillet {
                        if let Ok(f) = tok.parse::<f64>() {
                            *fillet = f;
                        }
                        *awaiting_fillet = false;
                        handled_anything = true;
                    } else if first_corner.is_none() {
                        if let Some(pt) = parse_point_token(tok, None) {
                            *first_corner = Some(pt);
                            handled_anything = true;
                        }
                    } else {
                        if let Some(pt2) = parse_point_token(tok, None) {
                            let pt1 = first_corner.unwrap();
                            let r = *fillet;
                            let min_x = pt1[0].min(pt2[0]);
                            let max_x = pt1[0].max(pt2[0]);
                            let min_y = pt1[1].min(pt2[1]);
                            let max_y = pt1[1].max(pt2[1]);

                            let mut poly = LwPolyline::new();
                            poly.is_closed = true;
                            if r > 0.0 && (max_x - min_x) > 2.0 * r && (max_y - min_y) > 2.0 * r {
                                let b = (std::f64::consts::PI / 8.0).tan();
                                let mut v1 = LwVertex::new(Vector2::new(max_x - r, min_y)); v1.bulge = b;
                                let v2 = LwVertex::new(Vector2::new(max_x, min_y + r));
                                let mut v3 = LwVertex::new(Vector2::new(max_x, max_y - r)); v3.bulge = b;
                                let v4 = LwVertex::new(Vector2::new(max_x - r, max_y));
                                let mut v5 = LwVertex::new(Vector2::new(min_x + r, max_y)); v5.bulge = b;
                                let v6 = LwVertex::new(Vector2::new(min_x, max_y - r));
                                let mut v7 = LwVertex::new(Vector2::new(min_x, min_y + r)); v7.bulge = b;
                                let v8 = LwVertex::new(Vector2::new(min_x + r, min_y));
                                poly.vertices = vec![v1, v2, v3, v4, v5, v6, v7, v8];
                            } else {
                                poly.vertices = vec![
                                    LwVertex::new(Vector2::new(min_x, min_y)),
                                    LwVertex::new(Vector2::new(max_x, min_y)),
                                    LwVertex::new(Vector2::new(max_x, max_y)),
                                    LwVertex::new(Vector2::new(min_x, max_y)),
                                ];
                            }
                            poly.common.layer = cur_layer;
                            if let Some(col) = self.current_color {
                                poly.common.color = col;
                            }
                            add_stream_entity(EntityType::LwPolyline(poly));
                            self.stream_state = None;
                            handled_anything = true;
                        }
                    }
                }
                None => {
                    // No active stream and not a stream verb
                    break;
                }
            }

            idx += 1;
        }

        handled_anything
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSsgetMode {
    pub primary: String,        // "", "X", "W", "C", "WP", "CP", "F", "P", "L", "I"
    pub reject_locked: bool,    // :L
    pub single_select: bool,    // :S
    pub allow_duplicates: bool, // :D
    pub pickbox_select: bool,   // :E
    pub point_only: bool,       // :+ or +. or +
    pub nested: bool,           // :N
    pub subentity: bool,        // :U
    pub viewport: bool,         // :V
}

pub fn parse_ssget_mode(mode: &str) -> ParsedSsgetMode {
    let mut parsed = ParsedSsgetMode::default();
    let upper = mode.to_uppercase();
    let trimmed = upper.trim();

    // Check modifier flags anywhere in string
    if trimmed.contains(":L") {
        parsed.reject_locked = true;
    }
    if trimmed.contains(":S") {
        parsed.single_select = true;
    }
    if trimmed.contains(":D") {
        parsed.allow_duplicates = true;
    }
    if trimmed.contains(":E") {
        parsed.pickbox_select = true;
    }
    if trimmed.contains(":N") {
        parsed.nested = true;
    }
    if trimmed.contains(":U") {
        parsed.subentity = true;
    }
    if trimmed.contains(":V") {
        parsed.viewport = true;
    }
    if trimmed.contains("+.") || trimmed.contains(":+") || trimmed.starts_with('+') {
        parsed.point_only = true;
    }

    // Now extract primary mode:
    // Remove all colon-prefixed modifier tokens and leading underscores/plus signs
    let mut remaining = trimmed.trim_start_matches('_').to_string();
    for token in &[":L", ":S", ":D", ":E", ":N", ":U", ":V", ":+", "+.", "+"] {
        remaining = remaining.replace(token, "");
    }
    remaining = remaining.trim_matches(|c| c == ':' || c == '_' || c == ' ' || c == '+').to_string();

    // Match primary selection method (longest match first: "WP", "CP" before "W", "C", "P")
    if remaining == "WP" {
        parsed.primary = "WP".to_string();
    } else if remaining == "CP" {
        parsed.primary = "CP".to_string();
    } else if remaining == "W" {
        parsed.primary = "W".to_string();
    } else if remaining == "C" {
        parsed.primary = "C".to_string();
    } else if remaining == "F" {
        parsed.primary = "F".to_string();
    } else if remaining == "P" {
        parsed.primary = "P".to_string();
    } else if remaining == "L" {
        parsed.primary = "L".to_string();
    } else if remaining == "I" {
        parsed.primary = "I".to_string();
    } else if remaining == "X" {
        parsed.primary = "X".to_string();
    } else {
        parsed.primary = String::new(); // Interactive selection
    }

    parsed
}

fn entity_to_dxf_with_xdata(
    ent: &EntityType,
    h: Handle,
    app_list: Option<&[String]>,
    host_opt: Option<&dyn HostApi>,
) -> Vec<(i16, DxfValue)> {
    let mut dxf = entity_to_dxf(ent);
    if let Some(apps) = app_list {
        let match_all = apps.iter().any(|a| a == "*");
        let mut xdata_apps = Vec::new();
        if match_all {
            for rec in ent.common().extended_data.records() {
                xdata_apps.push((rec.application_name.clone(), xdata_record_to_dxf(rec)));
            }
        } else {
            for app_name in apps {
                if let Some(rec) = host_opt.and_then(|host| host.read_record(h, app_name)) {
                    xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                } else if let Some(rec) = ent.common().extended_data.get_record(app_name) {
                    xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                }
            }
        }
        if !xdata_apps.is_empty() {
            dxf.push((-3, DxfValue::XData(xdata_apps)));
        }
    }
    dxf
}

fn apply_elist_to_entity(
    ent: &mut EntityType,
    elist: &[(i16, DxfValue)],
    xdata_apps: Option<&[(String, Vec<(i16, DxfValue)>)]>,
) {
    if let Some(apps) = xdata_apps {
        for (app_name, records) in apps {
            if records.is_empty() {
                ent.common_mut().extended_data.remove_record(app_name);
            } else {
                let rec = dxf_to_xdata_record(app_name, records);
                ent.common_mut().extended_data.upsert_record(rec);
            }
        }
    }

    for (code, val) in elist {
        match *code {
            8 => {
                if let DxfValue::Str(layer) = val {
                    ent.common_mut().layer = layer.clone();
                }
            }
            62 => {
                if let Some(color_idx) = dxf_to_i64(val) {
                    ent.common_mut().color = if color_idx == 256 {
                        ocs_plugin_api::host::codec::types::Color::ByLayer
                    } else if color_idx == 0 {
                        ocs_plugin_api::host::codec::types::Color::ByBlock
                    } else {
                        ocs_plugin_api::host::codec::types::Color::from_index(
                            color_idx.abs() as i16,
                        )
                    };
                }
            }
            _ => {}
        }
    }

    match ent {
        EntityType::Line(line) => {
            for (code, val) in elist {
                match *code {
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            line.start = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    11 => {
                        if let DxfValue::Point(pt) = val {
                            line.end = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    _ => {}
                }
            }
        }
        EntityType::Circle(circle) => {
            for (code, val) in elist {
                match *code {
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            circle.center = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    40 => {
                        if let Some(r) = dxf_to_f64(val) {
                            circle.radius = r;
                        }
                    }
                    _ => {}
                }
            }
        }
        EntityType::Arc(arc) => {
            for (code, val) in elist {
                match *code {
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            arc.center = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    40 => {
                        if let Some(r) = dxf_to_f64(val) {
                            arc.radius = r;
                        }
                    }
                    50 => {
                        if let Some(r) = dxf_to_f64(val) {
                            arc.start_angle = r.to_radians();
                        }
                    }
                    51 => {
                        if let Some(r) = dxf_to_f64(val) {
                            arc.end_angle = r.to_radians();
                        }
                    }
                    _ => {}
                }
            }
        }
        EntityType::Point(pt_ent) => {
            for (code, val) in elist {
                if *code == 10 {
                    if let DxfValue::Point(pt) = val {
                        pt_ent.location = Vector3::new(pt[0], pt[1], pt[2]);
                    }
                }
            }
        }
        EntityType::Text(txt) => {
            for (code, val) in elist {
                match *code {
                    1 => {
                        if let DxfValue::Str(s) = val {
                            txt.value = s.clone();
                        }
                    }
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            txt.insertion_point = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    40 => {
                        if let Some(r) = dxf_to_f64(val) {
                            txt.height = r;
                        }
                    }
                    50 => {
                        if let Some(r) = dxf_to_f64(val) {
                            txt.rotation = r.to_radians();
                        }
                    }
                    _ => {}
                }
            }
        }
        EntityType::MText(mt) => {
            for (code, val) in elist {
                match *code {
                    1 => {
                        if let DxfValue::Str(s) = val {
                            mt.value = s.clone();
                        }
                    }
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            mt.insertion_point = Vector3::new(pt[0], pt[1], pt[2]);
                        }
                    }
                    40 => {
                        if let Some(r) = dxf_to_f64(val) {
                            mt.height = r;
                        }
                    }
                    _ => {}
                }
            }
        }
        EntityType::LwPolyline(poly) => {
            let mut new_verts = Vec::new();
            for (code, val) in elist {
                match *code {
                    70 => {
                        if let Some(flags) = dxf_to_i64(val) {
                            poly.is_closed = (flags & 1) != 0;
                        }
                    }
                    38 => {
                        if let Some(elev) = dxf_to_f64(val) {
                            poly.elevation = elev;
                        }
                    }
                    10 => {
                        if let DxfValue::Point(pt) = val {
                            new_verts.push(LwVertex::new(Vector2::new(pt[0], pt[1])));
                        }
                    }
                    _ => {}
                }
            }
            if !new_verts.is_empty() {
                poly.vertices = new_verts;
            }
        }
        _ => {}
    }
}

fn is_command_terminator_token(tok: &str) -> bool {
    let trimmed = tok.trim();
    trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("ENTER")
        || trimmed.eq_ignore_ascii_case("RETURN")
        || trimmed.eq_ignore_ascii_case("ESC")
        || trimmed.eq_ignore_ascii_case("ESCAPE")
        || trimmed.eq_ignore_ascii_case("CANCEL")
}

fn is_cancel_token(tok: &str) -> bool {
    let trimmed = tok.trim();
    trimmed.eq_ignore_ascii_case("ESC")
        || trimmed.eq_ignore_ascii_case("ESCAPE")
        || trimmed.eq_ignore_ascii_case("CANCEL")
}

fn is_pause_token(tok: &str) -> bool {
    let trimmed = tok.trim();
    trimmed == "\\" || trimmed.eq_ignore_ascii_case("PAUSE")
}

/// Maps dual-mode CAD commands (which show a dialog/palette interactively) and
/// their standard aliases to their command-line `-` variants when invoked via
/// AutoLISP `(command ...)` without `(initdia 1)`.
fn map_dual_mode_command_to_cmdline(upper_bare: &str, second_upper: &str) -> Option<&'static str> {
    match upper_bare {
        "LAYER" | "LAYERS" | "LA" => Some("-LAYER"),
        "LAYERSTATE" | "LAS" | "LMAN" => Some("-LAYERSTATE"),
        "LAYTRANS" => Some("-LAYTRANS"),
        "LAYERFILTER" => Some("-LAYERFILTER"),
        "INSERT" | "I" => Some("-INSERT"),
        "BLOCK" | "BMAKE" | "B" => Some("-BLOCK"),
        "WBLOCK" | "WB" | "W" => Some("-WBLOCK"),
        "BEDIT" | "BE" => Some("-BEDIT"),
        "ATTDEF" | "ATT" => Some("-ATTDEF"),
        "ATTEDIT" | "EATTEDIT" | "ATE" | "ATTE" => Some("-ATTEDIT"),
        "REFEDIT" => Some("-REFEDIT"),
        "XREF" | "XR" => Some("-XREF"),
        "XBIND" | "XB" => Some("-XBIND"),
        "ATTACH" | "XATTACH" | "XA" => Some("-ATTACH"),
        "STYLE" | "DDSTYLE" | "ST" => {
            if matches!(second_upper, "SET" | "FONT" | "WIDTH" | "OBLIQUE" | "DIALOG" | "UI") {
                None
            } else {
                Some("-STYLE")
            }
        }
        "DIMSTYLE" | "DDIM" | "D" => Some("-DIMSTYLE"),
        "LINETYPE" | "DDLTYPE" | "LT" => Some("-LINETYPE"),
        "TABLE" | "TB" => {
            if matches!(
                second_upper,
                "CELL"
                    | "INSERTROW"
                    | "DELETEROW"
                    | "INSERTCOLUMN"
                    | "INSERTCOL"
                    | "DELETECOLUMN"
                    | "DELETECOL"
                    | "MERGE"
                    | "UNMERGE"
                    | "ROWHEIGHT"
                    | "COLUMNWIDTH"
                    | "COLWIDTH"
                    | "LOCK"
                    | "BLOCK"
                    | "FORMULA"
                    | "FIELD"
            ) {
                None
            } else {
                Some("-TABLE")
            }
        }
        "TABLEDIT" => Some("-TABLEDIT"),
        "PURGE" | "PU" => Some("-PURGE"),
        "PLOT" | "PRINT" => Some("-PLOT"),
        "PAGESETUP" => Some("-PAGESETUP"),
        "PSETUPIN" => Some("-PSETUPIN"),
        "PUBLISH" => Some("-PUBLISH"),
        "PREVIEW" => Some("-PREVIEW"),
        "LAYOUT" => Some("-LAYOUT"),
        "RENAME" | "REN" => Some("-RENAME"),
        "VIEW" | "V" => Some("-VIEW"),
        "VPORTS" => Some("-VPORTS"),
        "VPSCALE" => Some("-VPSCALE"),
        "VPLOCK" => Some("-VPLOCK"),
        "VPMAX" => Some("-VPMAX"),
        "VPMIN" => Some("-VPMIN"),
        "OVERKILL" => Some("-OVERKILL"),
        "SETBYLAYER" => Some("-SETBYLAYER"),
        "HYPERLINK" => Some("-HYPERLINK"),
        "FIELD" => Some("-FIELD"),
        "TEXTMASK" => Some("-TEXTMASK"),
        "DIMINSPECT" => Some("-DIMINSPECT"),
        "DATAEXTRACTION" | "DX" => Some("-DATAEXTRACTION"),
        "PARAMETERS" | "PAR" => Some("-PARAMETERS"),
        "SCALELISTEDIT" => Some("-SCALELISTEDIT"),
        "OBJECTSCALE" => Some("-OBJECTSCALE"),
        "PDFIMPORT" => Some("-PDFIMPORT"),
        "PDFATTACH" => Some("-PDFATTACH"),
        "DWFATTACH" => Some("-DWFATTACH"),
        "DGNATTACH" => Some("-DGNATTACH"),
        "DGNIMPORT" => Some("-DGNIMPORT"),
        "POINTCLOUDATTACH" => Some("-POINTCLOUDATTACH"),
        "PCEXTRACTSECTION" => Some("-PCEXTRACTSECTION"),
        "IMAGEADJUST" | "IAD" => Some("-IMAGEADJUST"),
        "TOOLBAR" => Some("-TOOLBAR"),
        _ => None,
    }
}

fn is_multi_step_cad_command(upper_cmd: &str) -> bool {
    upper_cmd.starts_with('-')
        || matches!(
            upper_cmd,
            "LINE" | "PLINE" | "CIRCLE" | "ARC" | "RECTANG" | "RECTANGLE" | "POLYGON"
                | "SPLINE" | "ELLIPSE" | "MOVE" | "COPY" | "ROTATE" | "SCALE"
                | "MIRROR" | "ERASE" | "OFFSET" | "TRIM" | "EXTEND" | "FILLET"
                | "CHAMFER" | "ARRAY" | "STRETCH" | "BREAK" | "JOIN" | "EXPLODE"
                | "HATCH" | "TEXT" | "MTEXT" | "DIMLINEAR" | "DIMALIGNED" | "DIMRADIUS"
                | "DIMDIAMETER" | "DIMANGULAR" | "LEADER" | "QLEADER"
        )
}

fn dispatch_command_tokens_to_host(norm_args: &[String], mut send: impl FnMut(&str)) {
    if norm_args.is_empty() {
        return;
    }
    let verb = norm_args[0].trim();
    let second_upper = norm_args
        .get(1)
        .map(|s| s.trim().trim_start_matches('_').to_ascii_uppercase())
        .unwrap_or_default();

    if verb == "-DIMSTYLE" && norm_args.len() > 1 && !norm_args[1].trim_start().starts_with('[') {
        send("-DIMSTYLE");
        send(&norm_args[1..].join(" "));
    } else if verb == "-LINETYPE"
        && norm_args.len() > 1
        && !(norm_args.len() == 4
            && matches!(second_upper.as_str(), "C" | "CREATE")
            && !norm_args[2..].iter().any(|a| a.eq_ignore_ascii_case("ENTER")))
    {
        send("-LINETYPE");
        send(&norm_args[1..].join(" "));
    } else if verb == "-ATTEDIT"
        && norm_args.len() > 1
        && matches!(second_upper.as_str(), "Y" | "YES" | "N" | "NO" | "ENTER")
    {
        send("-ATTEDIT");
        send(&norm_args[1..].join(" "));
    } else if verb == "-WBLOCK" && norm_args.len() > 1 {
        send(&format!("_-WBLOCKFILE {}", norm_args[1]));
        if norm_args.len() > 2 {
            send(&norm_args[2..].join(" "));
        }
    } else {
        send(&norm_args.join(" "));
    }
}

impl OpenCadRustBridge {
    fn update_layer_sysvar_from_tokens<S: AsRef<str>>(&mut self, tokens: &[S], start_idx: usize) {
        let mut i = start_idx;
        while i < tokens.len() {
            let sub = tokens[i].as_ref().trim().trim_start_matches('_').to_ascii_uppercase();
            if matches!(sub.as_str(), "M" | "MAKE" | "S" | "SET") {
                if i + 1 < tokens.len() {
                    let lay_name = tokens[i + 1].as_ref().trim();
                    if !lay_name.is_empty() && !is_command_terminator_token(lay_name) {
                        self.sysvars
                            .insert("CLAYER".into(), DxfValue::Str(lay_name.to_string()));
                    }
                    i += 2;
                    continue;
                }
            } else if matches!(
                sub.as_str(),
                "C" | "COLOR" | "L" | "LTYPE" | "LT" | "LW" | "LWEIGHT" | "T" | "TR" | "TRANSPARENCY"
            ) {
                if i + 2 < tokens.len() {
                    i += 3;
                    continue;
                }
            } else if matches!(
                sub.as_str(),
                "N" | "NEW" | "ON" | "OFF" | "F" | "FREEZE" | "TH" | "THAW" | "LO" | "LOCK" | "U" | "UNLOCK"
            ) {
                if i + 1 < tokens.len() {
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }
    }

    /// Normalize AutoLISP `(command ...)` tokens for host dispatch, resolving
    /// dual-mode commands to their `-` command-line equivalents unless `(initdia 1)`
    /// was called, and tracking multi-call prompt continuation state.
    fn normalize_and_track_command(&mut self, args: &[String]) -> Option<(Vec<String>, Option<String>)> {
        if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
            self.init_dia_flag = false;
            if self.active_host_command.take().is_some() {
                return Some((vec!["ESC".to_string()], None));
            }
            return None;
        }

        if is_cancel_token(&args[0]) {
            self.active_host_command = None;
            self.init_dia_flag = false;
            return Some((args.to_vec(), None));
        }

        let prev_active = self.active_host_command.clone();
        let mut norm_args = args.to_vec();

        if let Some(ref active_cmd) = prev_active {
            let has_terminator = norm_args.iter().any(|t| is_command_terminator_token(t));
            let ends_with_pause = norm_args.last().is_some_and(|t| is_pause_token(t));
            if has_terminator || (active_cmd != "-LAYER" && !ends_with_pause) {
                self.active_host_command = None;
            }
        } else {
            let use_dialog = std::mem::replace(&mut self.init_dia_flag, false);
            let bare = norm_args[0].trim().trim_start_matches(|c| c == '_' || c == '.');
            let upper = bare.to_ascii_uppercase();
            let second_upper = norm_args
                .get(1)
                .map(|s| s.trim().trim_start_matches('_').to_ascii_uppercase())
                .unwrap_or_default();

            if upper.starts_with('-') {
                norm_args[0] = upper;
            } else if !use_dialog {
                if let Some(dash_cmd) = map_dual_mode_command_to_cmdline(&upper, &second_upper) {
                    norm_args[0] = dash_cmd.to_string();
                }
            }

            let verb = norm_args[0]
                .trim()
                .trim_start_matches(|c| c == '_' || c == '.')
                .to_ascii_uppercase();
            let has_terminator = norm_args[1..].iter().any(|t| is_command_terminator_token(t));
            let ends_with_pause = norm_args.last().is_some_and(|t| is_pause_token(t));

            if verb == "-LAYER" {
                if !has_terminator {
                    self.active_host_command = Some("-LAYER".to_string());
                }
            } else if (norm_args.len() == 1 && is_multi_step_cad_command(&verb)) || ends_with_pause {
                self.active_host_command = Some(verb);
            }
        }

        Some((norm_args, prev_active))
    }
}

impl CadBridge for OpenCadRustBridge {
    fn execute_command(&mut self, cmd: &str) -> Result<(), String> {
        let tokens: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
        self.execute_command_args(&tokens)
    }

    fn execute_command_args(&mut self, args: &[String]) -> Result<(), String> {
        let Some((norm_args, prev_active)) = self.normalize_and_track_command(args) else {
            self.executed_commands.push(args.join(" "));
            return Ok(());
        };

        let cmd_line = norm_args.join(" ");
        self.executed_commands.push(cmd_line.clone());

        let is_layer_cmd = norm_args[0]
            .trim()
            .trim_start_matches(|c| c == '_' || c == '.')
            .eq_ignore_ascii_case("-LAYER")
            || norm_args[0]
                .trim()
                .trim_start_matches(|c| c == '_' || c == '.')
                .eq_ignore_ascii_case("LAYER");
        if is_layer_cmd {
            self.update_layer_sysvar_from_tokens(&norm_args, 1);
        } else if prev_active.as_deref() == Some("-LAYER") {
            self.update_layer_sysvar_from_tokens(&norm_args, 0);
        }

        if let Some(bridge) = active_interactive() {
            let token_strs: Vec<&str> = norm_args.iter().map(|s| s.as_str()).collect();
            if self.process_stream_tokens(&token_strs) {
                return Ok(());
            }
            dispatch_command_tokens_to_host(&norm_args, |cmd| bridge.queue_command(cmd.to_string()));
            return Ok(());
        }

        let res = with_host(|host| {
            if norm_args.len() > 1 {
                let first_norm = norm_args[0]
                    .trim()
                    .trim_start_matches(|c| c == '_' || c == '.')
                    .to_uppercase();
                if matches!(
                    first_norm.as_str(),
                    "MOVE" | "3DMOVE" | "COPY" | "ROTATE" | "SCALE" | "STRETCH" | "MIRROR" | "ERASE"
                ) {
                    let _ = host.set_selection(&[]);
                }
            }
            dispatch_command_tokens_to_host(&norm_args, |cmd| {
                let _ = host.execute_command(cmd);
            });
            Ok(())
        });

        match res {
            Ok(r) => r,
            Err(_) => Ok(()),
        }
    }

    fn register_command(&mut self, name: &str, _description: &str) -> Result<(), String> {
        let tab_id = crate::context::get_active_tab_id();
        let mut cmds = get_registered_commands().lock().unwrap();
        cmds.entry(tab_id)
            .or_default()
            .insert(name.to_uppercase(), name.to_string());
        Ok(())
    }

    fn reg_app(&mut self, name: &str) -> Result<bool, String> {
        let is_new = self.registered_apps.insert(name.to_ascii_uppercase());
        let _ = with_host(|h| {
            if !h.document().app_ids.contains(name) {
                // Host APPID table
            }
        });
        Ok(is_new)
    }

    fn ent_get(&self, handle: u64, app_list: Option<&[String]>) -> Result<Vec<(i16, DxfValue)>, String> {
        let h = Handle::new(handle);
        let mut result = None;

        if let Some(bridge) = active_interactive() {
            result = bridge.query_entity(handle, |ent| entity_to_dxf_with_xdata(ent, h, app_list, None));
            if let Some(dxf) = result {
                return Ok(dxf);
            }
            return Err(format!("Entity <{:X}> not found in snapshot", handle));
        }

        if result.is_none() {
            let _ = with_host(|host| {
                if let Some(ent) = host.document().get_entity(h) {
                    result = Some(entity_to_dxf_with_xdata(ent, h, app_list, Some(host)));
                }
            });
        }

        result.ok_or_else(|| format!("Entity handle {} not found", handle))
    }

    fn ent_mod(&mut self, elist: &[(i16, DxfValue)]) -> Result<(), String> {
        let handle_val = elist
            .iter()
            .find(|(code, _)| *code == 5 || *code == -1)
            .and_then(|(_, val)| match val {
                DxfValue::Handle(h) => Some(*h),
                DxfValue::Str(s) => u64::from_str_radix(s.trim(), 16).ok(),
                DxfValue::Int(n) => Some(*n as u64),
                _ => None,
            })
            .ok_or_else(|| "No entity handle specified in entmod list".to_string())?;
        let handle = Handle::new(handle_val);

        let xdata_apps = elist.iter().find(|(code, _)| *code == -3).and_then(|(_, val)| {
            if let DxfValue::XData(apps) = val {
                Some(apps.clone())
            } else {
                None
            }
        });

        if let Some(active) = active_interactive() {
            let mut is_pending = false;
            let mut target_ent = if let Ok(list) = active.coordinator.pending_entities.lock() {
                list.iter()
                    .find(|e| e.common().handle == handle)
                    .cloned()
                    .map(|e| {
                        is_pending = true;
                        e
                    })
            } else {
                None
            };
            if target_ent.is_none() {
                if let Ok(guard) = active.doc_snapshot.lock() {
                    if let Some(ref doc) = *guard {
                        target_ent = doc.get_entity(handle).cloned();
                    }
                }
            }
            if let Some(mut ent) = target_ent {
                apply_elist_to_entity(&mut ent, elist, xdata_apps.as_deref());
                if is_pending {
                    if let Ok(mut list) = active.coordinator.pending_entities.lock() {
                        if let Some(slot) = list.iter_mut().find(|e| e.common().handle == handle) {
                            *slot = ent.clone();
                        }
                    }
                }
                if let Ok(mut guard) = active.doc_snapshot.lock() {
                    if let Some(ref mut doc) = *guard {
                        if let Some(slot) = doc.get_entity_mut(handle) {
                            *slot = ent.clone();
                        }
                    }
                }
                if !is_pending {
                    if let Some(existing) = self.deferred_entmods.iter_mut().find(|(h, _, _)| *h == handle) {
                        *existing = (handle, ent, xdata_apps);
                    } else {
                        self.deferred_entmods.push((handle, ent, xdata_apps));
                    }
                }
            }
            return Ok(());
        }

        with_host(|h| {
            if let Some(apps) = &xdata_apps {
                for (app_name, records) in apps {
                    if records.is_empty() {
                        let _ = h.remove_record(handle, app_name);
                    } else {
                        let rec = dxf_to_xdata_record(app_name, records);
                        let _ = h.write_record(handle, rec);
                    }
                }
            }

            let existing = h.document().get_entity(handle).cloned();
            if let Some(mut ent) = existing {
                apply_elist_to_entity(&mut ent, elist, xdata_apps.as_deref());
                h.update_entity(ent);
                h.set_dirty();
                h.bump_geometry();
            }
        })
        .map_err(|e| e.to_string())
    }

    fn ent_make(&mut self, elist: &[(i16, DxfValue)]) -> Result<u64, String> {
        let default_layer = self.current_layer();
        let entity = dxf_to_entity(elist, &default_layer, self.current_color)?;
        let xdata_apps = elist.iter().find(|(code, _)| *code == -3).and_then(|(_, val)| {
            if let DxfValue::XData(apps) = val {
                Some(apps.clone())
            } else {
                None
            }
        });

        if let Some(bridge) = active_interactive() {
            let handle = bridge.add_pending_entity(entity);
            Ok(handle)
        } else {
            match with_host(|h| {
                let layer = entity.common().layer.clone();
                if !layer.is_empty() && layer != "0" && !h.document().layers.contains(&layer) {
                    let _ = h.add_layer(LayerConfig {
                        name: layer,
                        ..Default::default()
                    });
                }
                let handle = h.add_entity(entity);
                if let Some(apps) = &xdata_apps {
                    for (app_name, records) in apps {
                        if !records.is_empty() {
                            let rec = dxf_to_xdata_record(app_name, records);
                            let _ = h.write_record(handle, rec);
                        }
                    }
                }
                h.set_dirty();
                h.bump_geometry();
                handle.value()
            }) {
                Ok(h) => Ok(h),
                Err(_) => {
                    static OFFLINE_HANDLE_SEQ: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1000);
                    Ok(OFFLINE_HANDLE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                }
            }
        }
    }

    fn ent_del(&mut self, handle: u64) -> Result<(), String> {
        let h = Handle::new(handle);
        if let Some(active) = active_interactive() {
            let mut removed_pending = false;
            if let Ok(mut list) = active.coordinator.pending_entities.lock() {
                let before = list.len();
                list.retain(|e| e.common().handle != h);
                removed_pending = list.len() != before;
            }
            if let Ok(mut guard) = active.doc_snapshot.lock() {
                if let Some(ref mut doc) = *guard {
                    doc.remove_entity(h);
                }
            }
            self.deferred_entmods.retain(|(mh, _, _)| *mh != h);
            if !removed_pending && !self.deferred_entdels.contains(&h) {
                self.deferred_entdels.push(h);
            }
            return Ok(());
        }
        with_host(|host| {
            host.remove_entity(h);
            host.set_dirty();
            host.bump_geometry();
        }).map_err(|e| e.to_string())
    }

    fn ent_next(&self, _handle: Option<u64>) -> Option<u64> {
        None
    }

    fn ent_last(&self) -> Option<u64> {
        let mut last = None;
        if let Some(bridge) = active_interactive() {
            if let Ok(list) = bridge.coordinator.pending_entities.lock() {
                if let Some(ent) = list.last() {
                    last = Some(ent.common().handle.value());
                }
            }
        }
        if last.is_none() {
            let _ = with_host(|h| {
                if let Some(ent) = h.document().entities().last() {
                    last = Some(ent.common().handle.value());
                }
            });
        }
        last
    }

    fn ss_get(
        &mut self,
        mode: &str,
        points: Option<&[[f64; 3]]>,
        filter: Option<&[(i16, DxfValue)]>,
    ) -> Result<Option<u64>, String> {
        let parsed_mode = parse_ssget_mode(mode);
        let single_select = parsed_mode.single_select;
        let allow_duplicates = parsed_mode.allow_duplicates;
        let reject_locked = parsed_mode.reject_locked;
        let point_only = parsed_mode.point_only;
        let _pickbox_select = parsed_mode.pickbox_select;

        let is_prev = parsed_mode.primary == "P";
        let is_last = parsed_mode.primary == "L";
        let is_implied = parsed_mode.primary == "I";
        let is_all = parsed_mode.primary == "X";
        let is_interactive = (points.is_none() || points.unwrap().is_empty())
            && !is_prev
            && !is_last
            && !is_implied
            && !is_all;

        let xdata_apps: Vec<String> = if let Some(filt) = filter {
            filt.iter()
                .filter_map(|(c, v)| {
                    if *c == -3 {
                        if let DxfValue::XData(apps) = v {
                            Some(apps.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .flatten()
                .collect()
        } else {
            Vec::new()
        };
        let app_list = if xdata_apps.is_empty() { None } else { Some(xdata_apps.as_slice()) };

        let mut candidates: Vec<(u64, Vec<(i16, DxfValue)>)> = Vec::new();

        if is_prev {
            for handle in &self.previous_selection {
                if let Ok(elist) = self.ent_get(*handle, app_list) {
                    candidates.push((*handle, elist));
                }
            }
        } else if is_last {
            if let Some(h) = self.ent_last() {
                if let Ok(elist) = self.ent_get(h, app_list) {
                    candidates.push((h, elist));
                }
            }
        } else if is_implied {
            let _ = with_host(|host| {
                for h in host.selection() {
                    if let Some(ent) = host.document().get_entity(h) {
                        candidates.push((h.value(), entity_to_dxf(ent)));
                    }
                }
            });
        } else if is_interactive {
            flush_lisp_output();
            let mut preselected = Vec::new();
            let _ = with_host(|host| {
                for h in host.selection() {
                    if let Some(ent) = host.document().get_entity(h) {
                        preselected.push((h.value(), entity_to_dxf(ent)));
                    }
                }
            });

            if !preselected.is_empty() {
                candidates = preselected;
            } else {
                let bridge = if let Some(b) = active_interactive() {
                    b.set_mode_flags(reject_locked, single_select, point_only);
                    Some(b)
                } else {
                    let base_h = with_host(|h| h.document().next_handle()).unwrap_or(100);
                    let doc = with_host(|h| Some(h.document().clone())).unwrap_or(None);
                    let (b, cmd_box) = LispInteractiveBridge::new("Select objects: ".to_string(), InteractionMode::Selection, base_h, doc);
                    b.set_mode_flags(reject_locked, single_select, point_only);
                    let has_host = if let Some(tx) = crate::context::get_worker_tx() {
                        tx.send(WorkerMsg::InteractiveStarted(cmd_box)).is_ok()
                    } else {
                        false
                    };
                    if has_host {
                        set_active_interactive(Some(b.clone()));
                        Some(b)
                    } else {
                        None
                    }
                };

                if let Some(b) = bridge {
                    b.set_mode_flags(reject_locked, single_select, point_only);
                    let picked_handles = b.get_selection(Some("Select objects: ")).unwrap_or_default();
                    b.set_mode_flags(false, false, false);
                    for h in picked_handles {
                        if let Ok(elist) = self.ent_get(h, app_list) {
                            candidates.push((h, elist));
                        } else {
                            candidates.push((h, vec![
                                (0, DxfValue::Str("LINE".into())),
                                (5, DxfValue::Str(format!("{:X}", h))),
                                (8, DxfValue::Str("0".into())),
                            ]));
                        }
                    }
                } else {
                    // Headless fallback: no interactive host listening, read document entities
                    let _ = with_host(|host| {
                        for ent in host.document().entities() {
                            let mut elist = entity_to_dxf(ent);
                            if let Some(apps) = app_list {
                                let mut xdata_apps = Vec::new();
                                for app_name in apps {
                                    if let Some(rec) = host.read_record(ent.common().handle, app_name) {
                                        xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                                    } else if let Some(rec) = ent.common().extended_data.get_record(app_name) {
                                        xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                                    }
                                }
                                if !xdata_apps.is_empty() {
                                    elist.push((-3, DxfValue::XData(xdata_apps)));
                                }
                            }
                            candidates.push((ent.common().handle.value(), elist));
                        }
                    });
                }
            }
        } else {
            if let Some(bridge) = active_interactive() {
                if let Ok(list) = bridge.coordinator.pending_entities.lock() {
                    for ent in list.iter() {
                        candidates.push((ent.common().handle.value(), entity_to_dxf(ent)));
                    }
                }
            }
            let _ = with_host(|host| {
                for ent in host.document().entities() {
                    let mut elist = entity_to_dxf(ent);
                    if let Some(apps) = app_list {
                        let mut xdata_apps = Vec::new();
                        for app_name in apps {
                            if let Some(rec) = host.read_record(ent.common().handle, app_name) {
                                xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                            } else if let Some(rec) = ent.common().extended_data.get_record(app_name) {
                                xdata_apps.push((app_name.clone(), xdata_record_to_dxf(rec)));
                            }
                        }
                        if !xdata_apps.is_empty() {
                            elist.push((-3, DxfValue::XData(xdata_apps)));
                        }
                    }
                    candidates.push((ent.common().handle.value(), elist));
                }
            });

            if candidates.is_empty() && (points.is_none() || points.unwrap().is_empty()) {
                let _ = with_host(|host| {
                    for h in host.selection() {
                        if let Some(ent) = host.document().get_entity(h) {
                            candidates.push((h.value(), entity_to_dxf(ent)));
                        }
                    }
                });
            }
        }

        let mut matches = Vec::new();
        for (handle, elist) in candidates {
            // 1. Locked layer check
            if reject_locked {
                let layer_name = elist
                    .iter()
                    .find(|(c, _)| *c == 8)
                    .and_then(|(_, v)| match v {
                        DxfValue::Str(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .unwrap_or("0");
                let is_locked = if let Some(b) = active_interactive() {
                    b.query_layer_locked(layer_name)
                } else {
                    with_host(|h| {
                        h.document().layers.get(layer_name).map(|l| l.is_locked()).unwrap_or(false)
                    }).unwrap_or(false)
                };
                if is_locked {
                    continue;
                }
            }

            // 2. Spatial check
            if !is_interactive || (points.is_some() && !points.unwrap().is_empty()) {
                let bbox = lisp_cad::get_entity_bbox(&elist);
                let check_mode = if parsed_mode.primary.is_empty() {
                    mode
                } else {
                    &parsed_mode.primary
                };
                if !lisp_cad::check_spatial(check_mode, points, bbox) {
                    continue;
                }
            }

            // 3. DXF Filter check
            if let Some(filt) = filter {
                if !lisp_cad::eval_dxf_filter(&elist, filt) {
                    continue;
                }
            }

            if allow_duplicates || !matches.contains(&handle) {
                matches.push(handle);
            }

            if single_select {
                break;
            }
        }

        if matches.is_empty() {
            return Ok(None);
        }

        self.previous_selection = matches.clone();
        let ss_handle = self.next_ss_handle;
        self.next_ss_handle += 1;
        self.selection_sets.insert(ss_handle, matches);
        Ok(Some(ss_handle))
    }

    fn ss_length(&self, ss_handle: u64) -> usize {
        self.selection_sets.get(&ss_handle).map(|s| s.len()).unwrap_or(0)
    }

    fn ss_name(&self, ss_handle: u64, index: usize) -> Option<u64> {
        self.selection_sets.get(&ss_handle).and_then(|s| s.get(index).copied())
    }

    fn ss_add(&mut self, ename: u64, ss_handle: Option<u64>) -> Result<u64, String> {
        match ss_handle {
            Some(h) => {
                let set = self.selection_sets.entry(h).or_default();
                if ename != 0 && !set.contains(&ename) {
                    set.push(ename);
                }
                Ok(h)
            }
            None => {
                let new_h = self.next_ss_handle;
                self.next_ss_handle += 1;
                let initial = if ename == 0 { Vec::new() } else { vec![ename] };
                self.selection_sets.insert(new_h, initial);
                Ok(new_h)
            }
        }
    }

    fn ss_del(&mut self, ename: u64, ss_handle: u64) -> Result<(), String> {
        if let Some(set) = self.selection_sets.get_mut(&ss_handle) {
            set.retain(|&h| h != ename);
        }
        Ok(())
    }

    fn ss_memb(&self, ename: u64, ss_handle: u64) -> bool {
        self.selection_sets.get(&ss_handle).map(|s| s.contains(&ename)).unwrap_or(false)
    }

    fn get_point(&mut self, base_pt: Option<[f64; 3]>, prompt: &str) -> Result<Option<[f64; 3]>, String> {
        self.init_get_state = None;
        flush_lisp_output();
        let bridge = if let Some(b) = active_interactive() {
            b
        } else {
            let base_h = with_host(|h| h.document().next_handle()).unwrap_or(100);
            let doc = with_host(|h| Some(h.document().clone())).unwrap_or(None);
            let (b, cmd_box) = LispInteractiveBridge::new(prompt.to_string(), InteractionMode::Point, base_h, doc);
            set_active_interactive(Some(b.clone()));
            if let Some(tx) = crate::context::get_worker_tx() {
                let _ = tx.send(WorkerMsg::InteractiveStarted(cmd_box));
            }
            b
        };

        let rb = base_pt.map(|from| RubberBandSpec::Line { from, color: None });
        let p = if prompt.is_empty() { None } else { Some(prompt) };
        if let Some(prompt_str) = p {
            bridge.coordinator.set_prompt(prompt_str.to_string());
        }
        let res = bridge.get_point_with_rubber_band(p, rb);
        if let Some(pt) = res {
            self.sysvars.insert("LASTPOINT".into(), DxfValue::Point(pt));
        }
        Ok(res)
    }

    fn get_dist(&mut self, base_pt: Option<[f64; 3]>, prompt: &str) -> Result<Option<f64>, String> {
        self.init_get_state = None;
        flush_lisp_output();
        let bridge = if let Some(b) = active_interactive() {
            b
        } else {
            let base_h = with_host(|h| h.document().next_handle()).unwrap_or(100);
            let doc = with_host(|h| Some(h.document().clone())).unwrap_or(None);
            let p_init = if prompt.is_empty() {
                if base_pt.is_some() { "Specify second point: " } else { "Specify first point: " }
            } else {
                prompt
            };
            let (b, cmd_box) = LispInteractiveBridge::new(p_init.to_string(), InteractionMode::Point, base_h, doc);
            set_active_interactive(Some(b.clone()));
            if let Some(tx) = crate::context::get_worker_tx() {
                let _ = tx.send(WorkerMsg::InteractiveStarted(cmd_box));
            }
            b
        };

        let p1 = if let Some(pt) = base_pt {
            pt
        } else {
            let p1_prompt = if prompt.is_empty() {
                "Specify first point: "
            } else {
                prompt
            };
            bridge.coordinator.set_prompt(p1_prompt.to_string());
            match bridge.get_point_with_rubber_band(Some(p1_prompt), None) {
                Some(pt) => pt,
                None => return Ok(None),
            }
        };

        let p2_prompt = if !prompt.is_empty() {
            prompt
        } else {
            "Specify second point: "
        };
        bridge.coordinator.set_prompt(p2_prompt.to_string());
        let rb = RubberBandSpec::Line { from: p1, color: None };
        let res2 = bridge.get_point_with_rubber_band(Some(p2_prompt), Some(rb));

        let dist_opt = match res2 {
            Some(pt2) => {
                self.sysvars.insert("LASTPOINT".into(), DxfValue::Point(pt2));
                let dx = pt2[0] - p1[0];
                let dy = pt2[1] - p1[1];
                let dz = pt2[2] - p1[2];
                Some((dx * dx + dy * dy + dz * dz).sqrt())
            }
            None => None,
        };

        Ok(dist_opt)
    }

    fn get_string(&mut self, has_spaces: bool, prompt: &str) -> Result<Option<String>, String> {
        self.init_get_state = None;
        let Some(text) = prompt_command_line_text(prompt, "Enter string:", true)? else {
            return Ok(None);
        };
        Ok(Some(truncate_getstring(&text, has_spaces)))
    }

    fn get_int(&mut self, prompt: &str) -> Result<Option<i64>, String> {
        let (bits, _) = self.init_get_state.take().unwrap_or((0, String::new()));
        loop {
            let Some(text) = prompt_command_line_text(prompt, "Enter integer:", false)? else {
                return Ok(None);
            };
            match check_initget_int(bits, &text) {
                Ok(v) => return Ok(v),
                Err(msg) => emit_interactive_output(msg),
            }
        }
    }

    fn get_real(&mut self, prompt: &str) -> Result<Option<f64>, String> {
        let (bits, _) = self.init_get_state.take().unwrap_or((0, String::new()));
        loop {
            let Some(text) = prompt_command_line_text(prompt, "Enter number:", false)? else {
                return Ok(None);
            };
            match check_initget_real(bits, &text) {
                Ok(v) => return Ok(v),
                Err(msg) => emit_interactive_output(msg),
            }
        }
    }

    fn get_kword(&mut self, prompt: &str) -> Result<Option<String>, String> {
        let (bits, keywords) = self.init_get_state.take().unwrap_or((0, String::new()));
        let kws = parse_initget_keywords(&keywords);
        let listing = kws.iter().map(|k| k.name.as_str()).collect::<Vec<_>>().join("/");
        let kword_prompt = if listing.is_empty() || prompt.contains('[') {
            prompt.trim().to_string()
        } else {
            let base = prompt.trim().trim_end_matches(':').trim_end();
            if base.is_empty() {
                format!("Enter keyword [{listing}]:")
            } else {
                format!("{base} [{listing}]:")
            }
        };
        loop {
            let Some(text) = prompt_command_line_text(&kword_prompt, "Enter keyword:", false)? else {
                return Ok(None);
            };
            match check_initget_kword(bits, &kws, &listing, &text) {
                Ok(v) => return Ok(v),
                Err(msg) => emit_interactive_output(&msg),
            }
        }
    }

    fn init_get(&mut self, bits: i32, keywords: &str) {
        self.init_get_state = Some((bits, keywords.to_string()));
    }

    fn init_dia(&mut self, use_dialog: bool) {
        self.init_dia_flag = use_dialog;
    }

    fn get_var(&self, name: &str) -> Result<DxfValue, String> {
        let key = name.trim().to_ascii_uppercase();
        if key.is_empty() {
            return Err("Empty system variable name".to_string());
        }

        // 1. If in an interactive session, read from the cached document snapshot (avoids IPC deadlock)
        if let Some(active) = active_interactive() {
            if let Ok(snap) = active.coordinator.doc_snapshot.lock() {
                if let Some(ref doc) = *snap {
                    if let Some(val) = read_header_var(doc, &key) {
                        return Ok(val);
                    }
                }
            }
            return self
                .sysvars
                .get(&key)
                .cloned()
                .ok_or_else(|| format!("Unknown system variable: {}", key));
        }

        // 2. When not interactive, query the live CAD host if connected to a real OpenCAD Studio instance
        if let Ok(Some(host_val)) = with_host(|h| {
            if let Some(sv) = h.system_variable(&key) {
                return Some(match sv {
                    HostSettingValue::Text(s) => DxfValue::Str(s),
                    HostSettingValue::Number(n) => DxfValue::Real(n),
                });
            }
            // Only read from h.document() if the host is a live IPC host (responds to CLAYER system_variable)
            if h.system_variable("CLAYER").is_some() {
                if let Some(v) = read_header_var(h.document(), &key) {
                    return Some(v);
                }
            }
            None
        }) {
            return Ok(host_val);
        }

        // 3. Fall back to local/session system variable table
        self.sysvars
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("Unknown system variable: {}", key))
    }

    fn set_var(&mut self, name: &str, val: DxfValue) -> Result<(), String> {
        let key = name.trim().to_ascii_uppercase();
        if key.is_empty() {
            return Err("Empty system variable name".to_string());
        }

        // Special handling for CECOLOR: sync self.current_color and forward to host
        if key == "CECOLOR" {
            let raw_str = dxf_to_str(&val);
            if let Some((col_opt, canonical)) = parse_cecolor_str(&raw_str) {
                self.current_color = col_opt;
                let canonical_val = DxfValue::Str(canonical);
                if let Some(active) = active_interactive() {
                    if let Ok(mut snap) = active.coordinator.doc_snapshot.lock() {
                        if let Some(ref mut doc) = *snap {
                            if let Some(c) = col_opt {
                                doc.header.current_entity_color = c;
                            }
                        }
                    }
                    self.deferred_setvars.push((key.clone(), canonical_val.clone()));
                } else {
                    let _ = with_host(|h| {
                        let _ = apply_host_sysvar(h, &key, &canonical_val);
                    });
                }
                self.sysvars.insert(key, canonical_val);
                return Ok(());
            } else {
                return Err(format!("Invalid CECOLOR value: {}", raw_str));
            }
        }

        // Coerce value against known default type if present
        let coerced_val = match self.sysvars.get(&key) {
            Some(DxfValue::Int(_)) => {
                if let Some(i) = dxf_to_i64(&val) {
                    DxfValue::Int(i)
                } else if let Some(b) = dxf_to_bool(&val) {
                    DxfValue::Int(i64::from(b))
                } else {
                    val.clone()
                }
            }
            Some(DxfValue::Real(_)) => {
                if let Some(r) = dxf_to_f64(&val) {
                    DxfValue::Real(r)
                } else {
                    val.clone()
                }
            }
            Some(DxfValue::Str(_)) => match &val {
                DxfValue::Str(s) => DxfValue::Str(s.clone()),
                DxfValue::Int(n) => DxfValue::Str(n.to_string()),
                DxfValue::Real(r) => DxfValue::Str(r.to_string()),
                other => other.clone(),
            },
            _ => val.clone(),
        };

        // 1. If in an interactive session, update the snapshot and queue host sync for when the interactive command finishes
        if let Some(active) = active_interactive() {
            let mut updated_val = coerced_val.clone();
            if let Ok(mut snap) = active.coordinator.doc_snapshot.lock() {
                if let Some(ref mut doc) = *snap {
                    if let Some(updated) = write_header_var(doc, &key, &coerced_val) {
                        updated_val = updated;
                    }
                }
            }
            self.deferred_setvars.push((key.clone(), updated_val.clone()));
            self.sysvars.insert(key, updated_val);
            return Ok(());
        }

        // 2. Update the live host document / system variables when available
        let mut final_val = coerced_val.clone();
        if let Ok(res) = with_host(|h| apply_host_sysvar(h, &key, &coerced_val)) {
            match res {
                Ok(v) => final_val = v,
                Err(e) => return Err(e),
            }
        }

        self.sysvars.insert(key, final_val);
        Ok(())
    }

    fn alert(&mut self, message: &str) {
        let msg = message.to_string();
        let _ = with_host(|h| {
            h.push_info(&msg);
        });
    }

    fn msg_box(
        &mut self,
        prompt: &str,
        title: Option<&str>,
        buttons: i32,
    ) -> Result<i32, String> {
        use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogResponse, DialogRow};

        let title_str = title.unwrap_or("OpenCAD Studio");
        let (submit_lbl, cancel_lbl) = match buttons & 0xF {
            1 => ("OK", "Cancel"),
            4 => ("Yes", "No"),
            5 => ("Retry", "Cancel"),
            _ => ("OK", "Cancel"),
        };

        let mut def = DialogDef::new(title_str);
        def.submit_label = submit_lbl.to_string();
        def.cancel_label = cancel_lbl.to_string();
        def.width = Some(360.0);
        def.rows.push(DialogRow::single(DialogControl::InfoText {
            text: prompt.to_string(),
            align_h: None,
        }));

        let bridge = if let Some(b) = crate::context::active_interactive() {
            b
        } else {
            crate::context::ensure_interactive_dialog(title_str, def.clone())?
        };

        let resp = bridge.show_dialog(def);
        match (buttons & 0xF, resp) {
            (4, DialogResponse::Submitted(_)) => Ok(6), // IDYES
            (4, DialogResponse::Cancelled) => Ok(7),    // IDNO
            (1, DialogResponse::Submitted(_)) => Ok(1), // IDOK
            (1, DialogResponse::Cancelled) => Ok(2),    // IDCANCEL
            (5, DialogResponse::Submitted(_)) => Ok(4), // IDRETRY
            (5, DialogResponse::Cancelled) => Ok(2),    // IDCANCEL
            (_, DialogResponse::Submitted(_)) => Ok(1), // IDOK
            (_, DialogResponse::Cancelled) => Ok(2),    // IDCANCEL
        }
    }

    fn show_dialog(&mut self, dialog_json: &str) -> Result<String, String> {
        let def: ocs_plugin_api::dialog::DialogDef = serde_json::from_str(dialog_json)
            .map_err(|e| format!("Invalid dialog definition JSON: {e}"))?;

        let bridge = if let Some(b) = crate::context::active_interactive() {
            b
        } else {
            crate::context::ensure_interactive_dialog(&def.title, def.clone())?
        };

        let response = bridge.show_dialog(def);
        serde_json::to_string(&response)
            .map_err(|e| format!("Failed to serialize dialog response: {e}"))
    }

    fn show_dialog_interactive(
        &mut self,
        dialog_json: &str,
        callback: &mut dyn FnMut(&str) -> Result<String, String>,
    ) -> Result<String, String> {
        let def: ocs_plugin_api::dialog::DialogDef = serde_json::from_str(dialog_json)
            .map_err(|e| format!("Invalid dialog definition JSON: {e}"))?;

        let bridge = if let Some(b) = crate::context::active_interactive() {
            b
        } else {
            crate::context::ensure_interactive_dialog(&def.title, def.clone())?
        };

        let response = bridge.show_dialog_interactive(def, |event| {
            let evt_json = match serde_json::to_string(&event) {
                Ok(s) => s,
                Err(_) => return ocs_plugin_api::dialog::DialogActionResponse::None,
            };
            match callback(&evt_json) {
                Ok(resp_json) => {
                    serde_json::from_str(&resp_json).unwrap_or(ocs_plugin_api::dialog::DialogActionResponse::None)
                }
                Err(_) => ocs_plugin_api::dialog::DialogActionResponse::None,
            }
        });

        serde_json::to_string(&response)
            .map_err(|e| format!("Failed to serialize dialog response: {e}"))
    }

    fn register_panel(&mut self, panel_json: &str) -> Result<bool, String> {
        let def: ocs_plugin_api::dialog::PanelDef = serde_json::from_str(panel_json)
            .map_err(|e| format!("Invalid panel definition JSON: {e}"))?;
        seed_panel_defaults(&def.id, &def.controls);
        let res = crate::context::with_host(|h| h.register_panel(def)).unwrap_or(false);
        Ok(res)
    }

    fn update_panel(&mut self, panel_id: &str, values_json: &str) -> Result<bool, String> {
        let values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue> =
            serde_json::from_str(values_json)
                .map_err(|e| format!("Invalid panel values JSON: {e}"))?;
        {
            let mut map = get_panel_values().lock().unwrap();
            let pid = panel_id.to_lowercase();
            for (k, v) in &values {
                map.insert((pid.clone(), k.to_lowercase()), v.clone());
            }
        }
        if let Some(bridge) = active_interactive() {
            bridge.add_pending_panel_update(panel_id.to_string(), values.clone());
        }
        let res = crate::context::with_host(|h| h.update_panel(panel_id, values)).unwrap_or(false);
        Ok(res)
    }

    fn toggle_panel(&mut self, panel_id: &str, show: Option<bool>) -> Result<bool, String> {
        let res = crate::context::with_host(|h| h.toggle_panel(panel_id, show)).unwrap_or(false);
        Ok(res)
    }

    fn get_panel_value(&self, panel_id: &str, control_id: &str) -> Option<String> {
        let key = (panel_id.to_lowercase(), control_id.to_lowercase());
        let map = get_panel_values().lock().unwrap();
        map.get(&key).and_then(|v| serde_json::to_string(v).ok())
    }

    fn start_undo_mark(&mut self) -> Result<(), String> {
        let _ = crate::context::with_host(|h| {
            h.push_undo("AutoLISP");
        });
        Ok(())
    }

    fn end_undo_mark(&mut self) -> Result<(), String> {
        let _ = crate::context::with_host(|h| {
            h.set_dirty();
        });
        Ok(())
    }

    fn ent_sel(&mut self, prompt: Option<&str>) -> Result<Option<(u64, [f64; 3])>, String> {
        flush_lisp_output();
        let p_str = prompt.filter(|s| !s.is_empty()).unwrap_or("Select object: ");
        let bridge = if let Some(b) = active_interactive() {
            Some(b)
        } else {
            let base_h = with_host(|h| h.document().next_handle()).unwrap_or(100);
            let doc = with_host(|h| Some(h.document().clone())).unwrap_or(None);
            let (b, cmd_box) = LispInteractiveBridge::new(
                p_str.to_string(),
                InteractionMode::EntityPick,
                base_h,
                doc,
            );
            let has_host = if let Some(tx) = crate::context::get_worker_tx() {
                tx.send(WorkerMsg::InteractiveStarted(cmd_box)).is_ok()
            } else {
                false
            };
            if has_host {
                set_active_interactive(Some(b.clone()));
                Some(b)
            } else {
                None
            }
        };

        if let Some(b) = bridge {
            let res = b.get_entity(Some(p_str));
            if let Some((_, pt)) = res {
                self.sysvars.insert("LASTPOINT".into(), DxfValue::Point(pt));
            }
            return Ok(res);
        }

        if let Some(h) = self.ent_last() {
            Ok(Some((h, [0.0, 0.0, 0.0])))
        } else {
            Ok(None)
        }
    }

    fn tbl_search(&self, table: &str, symbol: &str) -> Option<Vec<(i16, DxfValue)>> {
        let tbl_upper = table.to_ascii_uppercase();
        crate::context::with_host(|h| {
            let doc = h.document();
            match tbl_upper.as_str() {
                "LAYER" => doc.layers.get(symbol).map(layer_to_dxf),
                "LTYPE" => doc.line_types.get(symbol).map(ltype_to_dxf),
                "STYLE" => doc.text_styles.get(symbol).map(style_to_dxf),
                "BLOCK" => doc.block_records.get(symbol).map(block_to_dxf),
                "DIMSTYLE" => doc.dim_styles.get(symbol).map(dimstyle_to_dxf),
                "APPID" => doc.app_ids.get(symbol).map(appid_to_dxf),
                "VIEW" => doc.views.get(symbol).map(view_to_dxf),
                "VPORT" => doc.vports.get(symbol).map(vport_to_dxf),
                "UCS" => doc.ucss.get(symbol).map(ucs_to_dxf),
                _ => None,
            }
        }).unwrap_or(None)
    }

    fn tbl_next(&mut self, table: &str, rewind: bool) -> Option<Vec<(i16, DxfValue)>> {
        let tbl_upper = table.to_ascii_uppercase();
        let cursor = self.table_cursors.entry(tbl_upper.clone()).or_insert(0);
        if rewind {
            *cursor = 0;
        }
        let all_entries = crate::context::with_host(|h| {
            let doc = h.document();
            let entries: Vec<Vec<(i16, DxfValue)>> = match tbl_upper.as_str() {
                "LAYER" => doc.layers.iter().map(layer_to_dxf).collect(),
                "LTYPE" => doc.line_types.iter().map(ltype_to_dxf).collect(),
                "STYLE" => doc.text_styles.iter().map(style_to_dxf).collect(),
                "BLOCK" => doc.block_records.iter().map(block_to_dxf).collect(),
                "DIMSTYLE" => doc.dim_styles.iter().map(dimstyle_to_dxf).collect(),
                "APPID" => doc.app_ids.iter().map(appid_to_dxf).collect(),
                "VIEW" => doc.views.iter().map(view_to_dxf).collect(),
                "VPORT" => doc.vports.iter().map(vport_to_dxf).collect(),
                "UCS" => doc.ucss.iter().map(ucs_to_dxf).collect(),
                _ => Vec::new(),
            };
            entries
        }).unwrap_or_default();

        if *cursor < all_entries.len() {
            let item = all_entries[*cursor].clone();
            *cursor += 1;
            Some(item)
        } else {
            None
        }
    }
}

fn layer_to_dxf(layer: &ocs_plugin_api::host::codec::tables::Layer) -> Vec<(i16, DxfValue)> {
    let col_idx = match layer.color {
        ocs_plugin_api::host::codec::types::Color::Index(idx) => idx as i64,
        ocs_plugin_api::host::codec::types::Color::ByLayer => 256,
        ocs_plugin_api::host::codec::types::Color::ByBlock => 0,
        _ => 7,
    };
    let flags = (if layer.flags.frozen { 1 } else { 0 }) | (if layer.flags.locked { 4 } else { 0 });
    let col_final = if layer.flags.off { -col_idx.abs() } else { col_idx.abs() };
    vec![
        (0, DxfValue::Str("LAYER".into())),
        (2, DxfValue::Str(layer.name.clone())),
        (70, DxfValue::Int(flags)),
        (62, DxfValue::Int(col_final)),
        (6, DxfValue::Str(layer.line_type.clone())),
    ]
}

fn ltype_to_dxf(lt: &ocs_plugin_api::host::codec::tables::LineType) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("LTYPE".into())),
        (2, DxfValue::Str(lt.name.clone())),
        (70, DxfValue::Int(0)),
        (3, DxfValue::Str(lt.description.clone())),
        (72, DxfValue::Int(65)),
        (73, DxfValue::Int(0)),
        (40, DxfValue::Real(lt.pattern_length)),
    ]
}

fn style_to_dxf(st: &ocs_plugin_api::host::codec::tables::TextStyle) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("STYLE".into())),
        (2, DxfValue::Str(st.name.clone())),
        (70, DxfValue::Int(0)),
        (40, DxfValue::Real(st.height)),
        (41, DxfValue::Real(st.width_factor)),
        (50, DxfValue::Real(0.0)),
        (71, DxfValue::Int(0)),
        (42, DxfValue::Real(st.last_height)),
        (3, DxfValue::Str(st.font_file.clone())),
        (4, DxfValue::Str(String::new())),
    ]
}

fn block_to_dxf(blk: &ocs_plugin_api::host::codec::tables::BlockRecord) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("BLOCK".into())),
        (2, DxfValue::Str(blk.name.clone())),
        (70, DxfValue::Int(0)),
        (10, DxfValue::Point([0.0, 0.0, 0.0])),
        (1, DxfValue::Str(String::new())),
    ]
}

fn dimstyle_to_dxf(ds: &ocs_plugin_api::host::codec::tables::DimStyle) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("DIMSTYLE".into())),
        (2, DxfValue::Str(ds.name.clone())),
        (70, DxfValue::Int(0)),
        (40, DxfValue::Real(1.0)),
        (41, DxfValue::Real(0.18)),
    ]
}

fn appid_to_dxf(app: &ocs_plugin_api::host::codec::tables::AppId) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("APPID".into())),
        (2, DxfValue::Str(app.name.clone())),
        (70, DxfValue::Int(0)),
    ]
}

fn view_to_dxf(view: &ocs_plugin_api::host::codec::tables::View) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("VIEW".into())),
        (2, DxfValue::Str(view.name.clone())),
        (70, DxfValue::Int(0)),
        (40, DxfValue::Real(view.height)),
        (10, DxfValue::Point([view.center.x, view.center.y, view.center.z])),
        (41, DxfValue::Real(view.width)),
    ]
}

fn vport_to_dxf(vp: &ocs_plugin_api::host::codec::tables::VPort) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("VPORT".into())),
        (2, DxfValue::Str(vp.name.clone())),
        (70, DxfValue::Int(0)),
        (10, DxfValue::Point([vp.lower_left.x, vp.lower_left.y, 0.0])),
        (11, DxfValue::Point([vp.upper_right.x, vp.upper_right.y, 0.0])),
    ]
}

fn ucs_to_dxf(ucs: &ocs_plugin_api::host::codec::tables::Ucs) -> Vec<(i16, DxfValue)> {
    vec![
        (0, DxfValue::Str("UCS".into())),
        (2, DxfValue::Str(ucs.name.clone())),
        (70, DxfValue::Int(0)),
        (10, DxfValue::Point([ucs.origin.x, ucs.origin.y, ucs.origin.z])),
        (11, DxfValue::Point([ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z])),
        (12, DxfValue::Point([ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z])),
    ]
}

/// Convert DXF records `&[(i16, DxfValue)]` to an OpenCAD Studio `EntityType`
fn dxf_to_entity(
    elist: &[(i16, DxfValue)],
    default_layer: &str,
    default_color: Option<ocs_plugin_api::host::codec::types::Color>,
) -> Result<EntityType, String> {
    let mut etype = "";
    let mut layer = default_layer.to_string();
    let mut color: Option<i16> = None;
    let mut p10: Option<[f64; 3]> = None;
    let mut p11: Option<[f64; 3]> = None;
    let mut p12: Option<[f64; 3]> = None;
    let mut p13: Option<[f64; 3]> = None;
    let mut radius = 0.0;
    let mut start_ang = 0.0f64;
    let mut end_ang = 0.0f64;
    let mut text_val = String::new();
    let mut block_name = String::new();
    let mut scale_x = 1.0f64;
    let mut scale_y = 1.0f64;
    let mut scale_z = 1.0f64;
    let mut rotation_deg = 0.0f64;
    let mut poly_flags = 0i64;
    let mut elevation = 0.0f64;
    let mut lw_points: Vec<[f64; 2]> = Vec::new();
    let mut lw_bulges: Vec<f64> = Vec::new();
    let mut cur_bulge = 0.0f64;

    for (code, val) in elist {
        match *code {
            0 => {
                if let DxfValue::Str(s) = val {
                    etype = s.as_str();
                }
            }
            1 => {
                if let DxfValue::Str(s) = val {
                    text_val = s.clone();
                }
            }
            2 => {
                if let DxfValue::Str(s) = val {
                    block_name = s.clone();
                }
            }
            8 => {
                if let DxfValue::Str(s) = val {
                    layer = s.clone();
                }
            }
            62 => {
                if let Some(n) = dxf_to_i64(val) {
                    color = Some(n as i16);
                }
            }
            10 => {
                if let DxfValue::Point(pt) = val {
                    if p10.is_none() {
                        p10 = Some(*pt);
                    }
                    lw_points.push([pt[0], pt[1]]);
                    lw_bulges.push(cur_bulge);
                    cur_bulge = 0.0;
                }
            }
            11 => {
                if let DxfValue::Point(pt) = val {
                    p11 = Some(*pt);
                }
            }
            12 => {
                if let DxfValue::Point(pt) = val {
                    p12 = Some(*pt);
                }
            }
            13 => {
                if let DxfValue::Point(pt) = val {
                    p13 = Some(*pt);
                }
            }
            38 => {
                if let Some(r) = dxf_to_f64(val) {
                    elevation = r;
                }
            }
            40 => {
                if let Some(r) = dxf_to_f64(val) {
                    radius = r;
                }
            }
            41 => {
                if let Some(r) = dxf_to_f64(val) {
                    scale_x = r;
                }
            }
            42 => {
                if let Some(r) = dxf_to_f64(val) {
                    scale_y = r;
                    cur_bulge = r;
                }
            }
            43 => {
                if let Some(r) = dxf_to_f64(val) {
                    scale_z = r;
                }
            }
            50 => {
                if let Some(r) = dxf_to_f64(val) {
                    start_ang = r;
                    rotation_deg = r;
                }
            }
            51 => {
                if let Some(r) = dxf_to_f64(val) {
                    end_ang = r;
                }
            }
            70 => {
                if let Some(n) = dxf_to_i64(val) {
                    poly_flags = n;
                }
            }
            _ => {}
        }
    }

    let ent_color = color
        .map(|c| {
            if c == 256 {
                ocs_plugin_api::host::codec::types::Color::ByLayer
            } else if c == 0 {
                ocs_plugin_api::host::codec::types::Color::ByBlock
            } else {
                ocs_plugin_api::host::codec::types::Color::from_index(c.abs())
            }
        })
        .or(default_color);

    if etype.eq_ignore_ascii_case("LINE") {
        let start = p10.unwrap_or([0.0, 0.0, 0.0]);
        let end = p11.unwrap_or([0.0, 0.0, 0.0]);
        let mut line = Line::from_coords(start[0], start[1], start[2], end[0], end[1], end[2]);
        line.common.layer = layer;
        if let Some(col) = ent_color {
            line.common.color = col;
        }
        Ok(EntityType::Line(line))
    } else if etype.eq_ignore_ascii_case("CIRCLE") {
        let center = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut circle = Circle::from_coords(center[0], center[1], center[2], radius);
        circle.common.layer = layer;
        if let Some(col) = ent_color {
            circle.common.color = col;
        }
        Ok(EntityType::Circle(circle))
    } else if etype.eq_ignore_ascii_case("POINT") {
        let pt = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut point = Point::from_coords(pt[0], pt[1], pt[2]);
        point.common.layer = layer;
        if let Some(col) = ent_color {
            point.common.color = col;
        }
        Ok(EntityType::Point(point))
    } else if etype.eq_ignore_ascii_case("ARC") {
        let center = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut arc = CadArc::from_center_radius_angles(
            Vector3::new(center[0], center[1], center[2]),
            radius,
            start_ang.to_radians(),
            end_ang.to_radians(),
        );
        arc.common.layer = layer;
        if let Some(col) = ent_color {
            arc.common.color = col;
        }
        Ok(EntityType::Arc(arc))
    } else if etype.eq_ignore_ascii_case("LWPOLYLINE") {
        let mut poly = LwPolyline::new();
        poly.is_closed = (poly_flags & 1) != 0;
        poly.elevation = elevation;
        for (i, pt) in lw_points.iter().enumerate() {
            let mut v = LwVertex::new(Vector2::new(pt[0], pt[1]));
            if let Some(&b) = lw_bulges.get(i) {
                v.bulge = b;
            }
            poly.vertices.push(v);
        }
        poly.common.layer = layer;
        if let Some(col) = ent_color {
            poly.common.color = col;
        }
        Ok(EntityType::LwPolyline(poly))
    } else if etype.eq_ignore_ascii_case("TEXT") {
        let pt = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut text = CadText::with_value(text_val, Vector3::new(pt[0], pt[1], pt[2]))
            .with_height(if radius > 0.0 { radius } else { 2.5 })
            .with_rotation(rotation_deg.to_radians());
        text.common.layer = layer;
        if let Some(col) = ent_color {
            text.common.color = col;
        }
        Ok(EntityType::Text(text))
    } else if etype.eq_ignore_ascii_case("MTEXT") {
        let pt = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut mtext = MText::default();
        mtext.value = text_val;
        mtext.insertion_point = Vector3::new(pt[0], pt[1], pt[2]);
        mtext.height = if radius > 0.0 { radius } else { 2.5 };
        mtext.common.layer = layer;
        if let Some(col) = ent_color {
            mtext.common.color = col;
        }
        Ok(EntityType::MText(Box::new(mtext)))
    } else if etype.eq_ignore_ascii_case("SOLID") {
        let c0 = p10.unwrap_or([0.0, 0.0, 0.0]);
        let c1 = p11.unwrap_or(c0);
        let c2 = p12.unwrap_or(c1);
        let c3 = p13.unwrap_or(c2);
        let mut solid = Solid::new(
            Vector3::new(c0[0], c0[1], c0[2]),
            Vector3::new(c1[0], c1[1], c1[2]),
            Vector3::new(c2[0], c2[1], c2[2]),
            Vector3::new(c3[0], c3[1], c3[2]),
        );
        solid.common.layer = layer;
        if let Some(col) = ent_color {
            solid.common.color = col;
        }
        Ok(EntityType::Solid(solid))
    } else if etype.eq_ignore_ascii_case("INSERT") {
        let pt = p10.unwrap_or([0.0, 0.0, 0.0]);
        let mut insert = Insert::new(&block_name, Vector3::new(pt[0], pt[1], pt[2]));
        insert.set_x_scale(scale_x);
        insert.set_y_scale(scale_y);
        insert.set_z_scale(scale_z);
        insert.rotation = rotation_deg.to_radians();
        insert.common.layer = layer;
        if let Some(col) = ent_color {
            insert.common.color = col;
        }
        Ok(EntityType::Insert(insert))
    } else {
        Err(format!("Unsupported entmake entity type: {}", etype))
    }
}

/// DXF group 0 name of a dimension: "DIMENSION" for every kind except the arc-length and
/// large-radial ones, which have their own entity names.
fn dimension_dxf_name(d: &CadDimension) -> &'static str {
    match d {
        CadDimension::Arc(_) => "ARC_DIMENSION",
        CadDimension::LargeRadial(_) => "LARGE_RADIAL_DIMENSION",
        _ => "DIMENSION",
    }
}

/// Convert an `EntityType` back to DXF records
fn entity_to_dxf(ent: &EntityType) -> Vec<(i16, DxfValue)> {
    let mut list = Vec::new();
    list.push((5, DxfValue::Handle(ent.common().handle.value())));
    list.push((8, DxfValue::Str(ent.common().layer.clone())));
    if let ocs_plugin_api::host::codec::types::Color::Index(idx) = ent.common().color {
        list.push((62, DxfValue::Int(idx as i64)));
    }

    match ent {
        EntityType::Line(l) => {
            list.insert(0, (0, DxfValue::Str("LINE".into())));
            list.push((10, DxfValue::Point([l.start.x, l.start.y, l.start.z])));
            list.push((11, DxfValue::Point([l.end.x, l.end.y, l.end.z])));
        }
        EntityType::Circle(c) => {
            list.insert(0, (0, DxfValue::Str("CIRCLE".into())));
            list.push((10, DxfValue::Point([c.center.x, c.center.y, c.center.z])));
            list.push((40, DxfValue::Real(c.radius)));
        }
        EntityType::Point(p) => {
            list.insert(0, (0, DxfValue::Str("POINT".into())));
            list.push((10, DxfValue::Point([p.location.x, p.location.y, p.location.z])));
        }
        EntityType::Arc(a) => {
            list.insert(0, (0, DxfValue::Str("ARC".into())));
            list.push((10, DxfValue::Point([a.center.x, a.center.y, a.center.z])));
            list.push((40, DxfValue::Real(a.radius)));
            list.push((50, DxfValue::Real(a.start_angle.to_degrees())));
            list.push((51, DxfValue::Real(a.end_angle.to_degrees())));
        }
        EntityType::LwPolyline(p) => {
            list.insert(0, (0, DxfValue::Str("LWPOLYLINE".into())));
            list.push((70, DxfValue::Int(if p.is_closed { 1 } else { 0 })));
            list.push((38, DxfValue::Real(p.elevation)));
            list.push((90, DxfValue::Int(p.vertices.len() as i64)));
            for v in &p.vertices {
                list.push((10, DxfValue::Point([v.location.x, v.location.y, 0.0])));
                if v.bulge.abs() > 1e-9 {
                    list.push((42, DxfValue::Real(v.bulge)));
                }
            }
        }
        EntityType::Text(t) => {
            list.insert(0, (0, DxfValue::Str("TEXT".into())));
            list.push((1, DxfValue::Str(t.value.clone())));
            list.push((10, DxfValue::Point([t.insertion_point.x, t.insertion_point.y, t.insertion_point.z])));
            list.push((40, DxfValue::Real(t.height)));
            list.push((50, DxfValue::Real(t.rotation.to_degrees())));
        }
        EntityType::MText(mt) => {
            list.insert(0, (0, DxfValue::Str("MTEXT".into())));
            list.push((1, DxfValue::Str(mt.value.clone())));
            list.push((10, DxfValue::Point([mt.insertion_point.x, mt.insertion_point.y, mt.insertion_point.z])));
            list.push((40, DxfValue::Real(mt.height)));
        }
        EntityType::Solid(s) => {
            list.insert(0, (0, DxfValue::Str("SOLID".into())));
            list.push((10, DxfValue::Point([s.first_corner.x, s.first_corner.y, s.first_corner.z])));
            list.push((11, DxfValue::Point([s.second_corner.x, s.second_corner.y, s.second_corner.z])));
            list.push((12, DxfValue::Point([s.third_corner.x, s.third_corner.y, s.third_corner.z])));
            list.push((13, DxfValue::Point([s.fourth_corner.x, s.fourth_corner.y, s.fourth_corner.z])));
        }
        EntityType::Insert(ins) => {
            list.insert(0, (0, DxfValue::Str("INSERT".into())));
            list.push((2, DxfValue::Str(ins.block_name.clone())));
            list.push((10, DxfValue::Point([ins.insert_point.x, ins.insert_point.y, ins.insert_point.z])));
            list.push((41, DxfValue::Real(ins.x_scale())));
            list.push((42, DxfValue::Real(ins.y_scale())));
            list.push((43, DxfValue::Real(ins.z_scale())));
            list.push((50, DxfValue::Real(ins.rotation.to_degrees())));
        }
        EntityType::Dimension(d) => {
            list.insert(0, (0, DxfValue::Str(dimension_dxf_name(d).into())));
            let base = d.base();
            let dim_text = base
                .user_text
                .as_ref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&base.text)
                .clone();
            list.push((1, DxfValue::Str(dim_text)));
            if !base.block_name.is_empty() {
                list.push((2, DxfValue::Str(base.block_name.clone())));
            }
            list.push((3, DxfValue::Str(base.style_name.clone())));
            list.push((11, DxfValue::Point([base.text_middle_point.x, base.text_middle_point.y, base.text_middle_point.z])));
            let (dim_type_code, def_pt) = match d.as_ref() {
                CadDimension::Linear(dim) => {
                    list.push((13, DxfValue::Point([dim.first_point.x, dim.first_point.y, dim.first_point.z])));
                    list.push((14, DxfValue::Point([dim.second_point.x, dim.second_point.y, dim.second_point.z])));
                    list.push((50, DxfValue::Real(dim.rotation.to_degrees())));
                    list.push((52, DxfValue::Real(dim.ext_line_rotation.to_degrees())));
                    (0, dim.definition_point)
                }
                CadDimension::Aligned(dim) => {
                    list.push((13, DxfValue::Point([dim.first_point.x, dim.first_point.y, dim.first_point.z])));
                    list.push((14, DxfValue::Point([dim.second_point.x, dim.second_point.y, dim.second_point.z])));
                    list.push((52, DxfValue::Real(dim.ext_line_rotation.to_degrees())));
                    (1, dim.definition_point)
                }
                CadDimension::Angular2Ln(dim) => {
                    list.push((13, DxfValue::Point([dim.first_point.x, dim.first_point.y, dim.first_point.z])));
                    list.push((14, DxfValue::Point([dim.second_point.x, dim.second_point.y, dim.second_point.z])));
                    list.push((15, DxfValue::Point([dim.angle_vertex.x, dim.angle_vertex.y, dim.angle_vertex.z])));
                    list.push((16, DxfValue::Point([dim.dimension_arc.x, dim.dimension_arc.y, dim.dimension_arc.z])));
                    (2, dim.definition_point)
                }
                CadDimension::Diameter(dim) => {
                    list.push((15, DxfValue::Point([dim.angle_vertex.x, dim.angle_vertex.y, dim.angle_vertex.z])));
                    list.push((40, DxfValue::Real(dim.leader_length)));
                    (3, dim.definition_point)
                }
                CadDimension::Radius(dim) => {
                    list.push((15, DxfValue::Point([dim.angle_vertex.x, dim.angle_vertex.y, dim.angle_vertex.z])));
                    list.push((40, DxfValue::Real(dim.leader_length)));
                    (4, dim.definition_point)
                }
                CadDimension::Angular3Pt(dim) => {
                    list.push((13, DxfValue::Point([dim.first_point.x, dim.first_point.y, dim.first_point.z])));
                    list.push((14, DxfValue::Point([dim.second_point.x, dim.second_point.y, dim.second_point.z])));
                    list.push((15, DxfValue::Point([dim.angle_vertex.x, dim.angle_vertex.y, dim.angle_vertex.z])));
                    (5, dim.definition_point)
                }
                CadDimension::Ordinate(dim) => {
                    list.push((13, DxfValue::Point([dim.feature_location.x, dim.feature_location.y, dim.feature_location.z])));
                    list.push((14, DxfValue::Point([dim.leader_endpoint.x, dim.leader_endpoint.y, dim.leader_endpoint.z])));
                    (6, dim.definition_point)
                }
                CadDimension::Arc(dim) => {
                    list.push((13, DxfValue::Point([dim.first_extension_point.x, dim.first_extension_point.y, dim.first_extension_point.z])));
                    list.push((14, DxfValue::Point([dim.second_extension_point.x, dim.second_extension_point.y, dim.second_extension_point.z])));
                    list.push((15, DxfValue::Point([dim.center_point.x, dim.center_point.y, dim.center_point.z])));
                    (8, dim.definition_point)
                }
                CadDimension::LargeRadial(dim) => {
                    list.push((13, DxfValue::Point([dim.chord_point.x, dim.chord_point.y, dim.chord_point.z])));
                    list.push((14, DxfValue::Point([dim.override_center.x, dim.override_center.y, dim.override_center.z])));
                    list.push((15, DxfValue::Point([dim.jog_point.x, dim.jog_point.y, dim.jog_point.z])));
                    (9, dim.definition_point)
                }
            };
            let resolved_def_pt = if def_pt == Vector3::ZERO && base.definition_point != Vector3::ZERO {
                base.definition_point
            } else {
                def_pt
            };
            list.push((10, DxfValue::Point([resolved_def_pt.x, resolved_def_pt.y, resolved_def_pt.z])));
            list.push((42, DxfValue::Real(d.measurement())));
            list.push((53, DxfValue::Real(base.text_rotation.to_degrees())));
            list.push((70, DxfValue::Int(dim_type_code)));
        }
        _ => {
            list.insert(0, (0, DxfValue::Str("UNKNOWN".into())));
        }
    }
    list
}

/// Check if a command is registered in the AutoLISP environment for a specific drawing tab
pub fn has_lisp_command_for_tab(tab_id: u64, name: &str) -> bool {
    let upper = name.to_uppercase();
    get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .is_some_and(|cmds| cmds.contains_key(&upper))
}

/// Check if a command is registered in the active drawing tab's AutoLISP environment
pub fn has_lisp_command(name: &str) -> bool {
    has_lisp_command_for_tab(crate::context::get_active_tab_id(), name)
}

static INITIALIZED_TABS: OnceLock<Mutex<std::collections::HashSet<u64>>> = OnceLock::new();

fn get_initialized_tabs() -> &'static Mutex<std::collections::HashSet<u64>> {
    INITIALIZED_TABS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Ensure that a drawing tab has had `scripts/` and Startup Scripts (`autolisp_startup.json`)
/// auto-loaded into its isolated environment at least once.
pub fn ensure_tab_initialized(host: &mut dyn HostApi) {
    let tab_id = host.tab_id();
    crate::context::set_thread_tab_id(Some(tab_id));
    if tab_id == 0 {
        return;
    }
    crate::context::set_active_tab_id(tab_id);
    let already_init = get_initialized_tabs().lock().unwrap().contains(&tab_id);
    if !already_init {
        let _ = auto_load_lisp_scripts(host);
    }
}

/// Clean up and drop the isolated AutoLISP environment and commands for a closed drawing tab
pub fn clean_tab_state(tab_id: u64) {
    get_initialized_tabs().lock().unwrap().remove(&tab_id);
    get_registered_commands().lock().unwrap().remove(&tab_id);
    let _ = get_task_tx().send(LispTask::CloseTab { tab_id });
}

/// Evaluate an AutoLISP expression directly in the pure Rust environment
pub fn eval_lisp_expr(expr: &str, host: &mut dyn HostApi) -> bool {
    let tab_id = host.tab_id();
    crate::context::set_thread_tab_id(Some(tab_id));
    if tab_id != 0 {
        crate::context::set_active_tab_id(tab_id);
    }
    DEBUG_PAUSED_DURING_TASK.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(sender) = host.plugin_request_sender().map(Arc::from) {
        crate::context::set_global_sender(Some(sender));
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let task = LispTask::WithTab {
        tab_id,
        task: Box::new(LispTask::Eval {
            code: expr.to_string(),
            resp: tx,
            worker_tx,
        }),
    };
    let _ = get_task_tx().send(task);

    loop {
        match worker_rx.try_recv() {
            Ok(WorkerMsg::HostCall(mut task)) => {
                task(host);
            }
            Ok(WorkerMsg::InteractiveStarted(cmd)) => {
                host.start_interactive(cmd);
                return true;
            }
            _ => {}
        }
        match rx.try_recv() {
            Ok(Ok(Some(val))) => {
                host.push_output(&val);
                return true;
            }
            Ok(Ok(None)) => return true,
            Ok(Err(err)) => {
                push_cad_error_and_notify_ide(host, "AutoLISP Error", &err);
                return false;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// Evaluate AutoLISP code returning the formatted result or error string.
/// Used by OCSIDE (`RunScript`, `DebugScript`, `ReplEval`) and `py_runtime::execute_lisp_named`.
pub fn eval_lisp_str_headless(code: &str) -> Result<Option<String>, String> {
    eval_lisp_str_headless_impl(code, false)
}

/// Evaluate AutoLISP code without attaching source spans to the top-level parsed expressions.
/// Used for synthetic command invocations like `(c:mycmd)` during debugging so they do not
/// trigger `(1, 1)` breakpoints while still stepping into the function's spanned body.
pub fn eval_lisp_str_unspanned_headless(code: &str) -> Result<Option<String>, String> {
    eval_lisp_str_headless_impl(code, true)
}

fn eval_lisp_str_headless_impl(code: &str, unspanned: bool) -> Result<Option<String>, String> {
    let tab_id = crate::context::get_active_tab_id();
    let (tx, rx) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let inner = if unspanned {
        LispTask::EvalUnspanned {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        }
    } else {
        LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        }
    };
    let task = LispTask::WithTab {
        tab_id,
        task: Box::new(inner),
    };
    let _ = get_task_tx().send(task);

    let mut fallback_host = crate::context::DebugHost::new(crate::context::get_global_sender());

    loop {
        match worker_rx.try_recv() {
            Ok(WorkerMsg::HostCall(mut task)) => {
                if with_host(|h| task(h)).is_err() {
                    task(&mut fallback_host);
                }
            }
            Ok(WorkerMsg::InteractiveStarted(cmd)) => {
                if with_host(|h| h.start_interactive(cmd)).is_err() {
                    // Cannot start interactive if no host available
                }
                return Ok(None);
            }
            _ => {}
        }
        match rx.try_recv() {
            Ok(res) => return res,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("AutoLISP worker disconnected".to_string());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// Returns the directory for user scripts, checking OCS_SCRIPTS_DIR, settings.json, or default.
pub fn scripts_dir() -> PathBuf {
    if let Ok(p) = std::env::var("OCS_SCRIPTS_DIR") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let base = if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata)
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        PathBuf::from(".")
    };
    let settings_file = base.join("OpenCADStudio").join("settings.json");
    if let Ok(content) = std::fs::read_to_string(&settings_file) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(dir) = json
                .get("settings")
                .and_then(|s| s.get("python_ide_scripts_dir"))
                .and_then(|d| d.as_str())
            {
                let trimmed = dir.trim();
                if !trimmed.is_empty() {
                    return PathBuf::from(trimmed);
                }
            }
        }
    }
    base.join("OpenCADStudio").join("scripts")
}

/// Ensure the user scripts directory exists on disk.
pub fn ensure_scripts_dir() -> std::io::Result<PathBuf> {
    let dir = scripts_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// List all available AutoLISP scripts in the scripts directory.
pub fn list_lisp_scripts() -> Vec<String> {
    let Ok(dir) = ensure_scripts_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        scripts.push(stem.to_string());
                    }
                }
            }
        }
    }
    scripts.sort();
    scripts
}

/// Return sorted list of registered AutoLISP `c:NAME` commands for a specific drawing tab.
pub fn get_registered_lisp_commands_for_tab(tab_id: u64) -> Vec<String> {
    let mut list: Vec<String> = get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .map(|cmds| cmds.keys().cloned().collect())
        .unwrap_or_default();
    list.sort();
    list
}

/// Return sorted list of registered AutoLISP `c:NAME` commands in the active drawing tab.
pub fn get_registered_lisp_commands() -> Vec<String> {
    get_registered_lisp_commands_for_tab(crate::context::get_active_tab_id())
}

static LOADED_LISP_SCRIPTS: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

static PENDING_IDE_SCRIPTS: std::sync::LazyLock<Mutex<Vec<(u64, String)>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

fn cache_script_keys(map: &mut HashMap<String, String>, title: &str, code: &str) {
    map.insert(title.to_string(), code.to_string());
    let norm_title = title.replace('\\', "/");
    map.insert(norm_title.clone(), code.to_string());
    map.insert(norm_title.to_lowercase(), code.to_string());

    let path = Path::new(title);
    if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
        map.insert(file_name.to_string(), code.to_string());
        map.insert(file_name.to_lowercase(), code.to_string());
        map.insert(file_name.to_uppercase(), code.to_string());
    }
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        map.insert(stem.to_string(), code.to_string());
        map.insert(stem.to_lowercase(), code.to_string());
        map.insert(stem.to_uppercase(), code.to_string());

        for prefix in &["c-", "c_", "C-", "C_"] {
            if let Some(stripped) = stem.strip_prefix(prefix) {
                map.insert(stripped.to_string(), code.to_string());
                map.insert(stripped.to_lowercase(), code.to_string());
                map.insert(stripped.to_uppercase(), code.to_string());
            }
        }
    }

    let lower = code.to_ascii_lowercase();
    let mut search_from = 0;
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
                map.insert(cmd_name.clone(), code.to_string());
                map.insert(cmd_name.to_uppercase(), code.to_string());
            }
        }
        search_from = abs;
    }
}

/// Store script code loaded or executed from OCSIDE into in-memory cache.
pub fn store_loaded_script(title: &str, code: &str) {
    {
        let mut map = LOADED_LISP_SCRIPTS.lock().unwrap();
        cache_script_keys(&mut map, title, code);
    }
    let current_tab = crate::context::get_active_tab_id();
    PENDING_IDE_SCRIPTS
        .lock()
        .unwrap()
        .push((current_tab, code.to_string()));
}

/// Replay any scripts loaded from OCAIDE if the active drawing tab in OCS differs
/// from the tab ID that was active when OCAIDE ran `eval_lisp_str_headless`.
pub fn sync_pending_ide_scripts_for_tab(host: &mut dyn HostApi) {
    let tab_id = host.tab_id();
    crate::context::set_thread_tab_id(Some(tab_id));
    if tab_id != 0 {
        crate::context::set_active_tab_id(tab_id);
    }
    let pending: Vec<(u64, String)> = {
        let mut queue = PENDING_IDE_SCRIPTS.lock().unwrap();
        std::mem::take(&mut *queue)
    };
    for (loaded_tab, code) in pending {
        if loaded_tab != tab_id {
            let _ = eval_lisp_expr(&code, host);
        }
    }
}

/// Retrieve in-memory script code if previously loaded from OCSIDE.
pub fn get_loaded_script(name: &str) -> Option<String> {
    let map = LOADED_LISP_SCRIPTS.lock().unwrap();
    if let Some(c) = map.get(name) {
        return Some(c.clone());
    }
    let lower = name.to_lowercase();
    if let Some(c) = map.get(&lower) {
        return Some(c.clone());
    }
    let upper = name.to_uppercase();
    if let Some(c) = map.get(&upper) {
        return Some(c.clone());
    }
    let norm = name.replace('\\', "/");
    if let Some(c) = map.get(&norm) {
        return Some(c.clone());
    }
    if let Some(stem) = Path::new(name).file_stem().and_then(|s| s.to_str()) {
        if let Some(c) = map.get(&stem.to_lowercase()) {
            return Some(c.clone());
        }
        for prefix in &["c-", "c_", "C-", "C_"] {
            if let Some(stripped) = stem.strip_prefix(prefix) {
                if let Some(c) = map.get(&stripped.to_lowercase()) {
                    return Some(c.clone());
                }
            }
        }
    }
    None
}

/// One `initget` keyword: its name and how many leading characters must be typed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InitGetKeyword {
    pub name: String,
    pub min_len: usize,
}

/// Parse an `initget` keyword string ("Yes No", "LTYPE,LT", "Oui Non _Yes No").
/// Leading uppercase letters give the shortest accepted abbreviation; with no lowercase
/// letters or no uppercase letters the whole word is required. In the comma form the part
/// after the comma is the abbreviation. Everything from the first `_`-prefixed entry on is
/// the list of global names and is not offered here.
pub(crate) fn parse_initget_keywords(spec: &str) -> Vec<InitGetKeyword> {
    spec.split_whitespace()
        .take_while(|t| !t.starts_with('_'))
        .filter_map(|t| {
            if let Some((name, abbr)) = t.split_once(',') {
                if name.is_empty() {
                    return None;
                }
                let full = name.chars().count();
                let min_len = if abbr.is_empty() { full } else { abbr.chars().count().min(full) };
                return Some(InitGetKeyword { name: name.to_string(), min_len });
            }
            let upper = t.chars().take_while(|c| c.is_uppercase()).count();
            let min_len = if upper == 0 || upper == t.chars().count() { t.chars().count() } else { upper };
            Some(InitGetKeyword { name: t.to_string(), min_len })
        })
        .collect()
}

/// Match typed text to a keyword: case-insensitive, accepts an abbreviation of at least `min_len`.
pub(crate) fn match_initget_keyword(kws: &[InitGetKeyword], input: &str) -> Option<String> {
    let typed = input.trim().to_lowercase();
    if typed.is_empty() {
        return None;
    }
    let n = typed.chars().count();
    kws.iter()
        .find(|k| n >= k.min_len && k.name.to_lowercase().starts_with(&typed))
        .map(|k| k.name.clone())
}

/// AutoLISP integer input: optional sign and digits only.
pub(crate) fn parse_lisp_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

/// AutoLISP real input: decimal number, optional sign/exponent, no inf/nan.
pub(crate) fn parse_lisp_real(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() || !t.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Truncate `getstring` input at the first space when `has_spaces` (`cr`) is false.
pub(crate) fn truncate_getstring(text: &str, has_spaces: bool) -> String {
    if has_spaces {
        text.to_string()
    } else {
        text.split(' ').next().unwrap_or("").to_string()
    }
}

fn check_initget_number<T: Copy>(
    bits: i32,
    text: &str,
    parse: impl FnOnce(&str) -> Option<T>,
    is_zero: impl FnOnce(T) -> bool,
    is_neg: impl FnOnce(T) -> bool,
    requires_msg: &'static str,
) -> Result<Option<T>, &'static str> {
    if text.trim().is_empty() {
        if bits & 1 != 0 {
            return Err(requires_msg);
        }
        return Ok(None);
    }
    let Some(v) = parse(text) else {
        return Err(requires_msg);
    };
    let zero = is_zero(v);
    let neg = is_neg(v);
    if (bits & 2 != 0) && (bits & 4 != 0) && (zero || neg) {
        return Err("Value must be positive and nonzero.");
    }
    if (bits & 2 != 0) && zero {
        return Err("Value must be nonzero.");
    }
    if (bits & 4 != 0) && neg {
        return Err("Value must be nonnegative.");
    }
    Ok(Some(v))
}

/// Validate `getint` text against `initget` bits (1 = NoNull, 2 = NoZero, 4 = NoNegative).
pub(crate) fn check_initget_int(bits: i32, text: &str) -> Result<Option<i64>, &'static str> {
    check_initget_number(bits, text, parse_lisp_int, |v| v == 0, |v| v < 0, "Requires an integer value.")
}

/// Validate `getreal` text against `initget` bits (1 = NoNull, 2 = NoZero, 4 = NoNegative).
pub(crate) fn check_initget_real(bits: i32, text: &str) -> Result<Option<f64>, &'static str> {
    check_initget_number(bits, text, parse_lisp_real, |v| v == 0.0, |v| v < 0.0, "Requires a numeric value.")
}

/// Validate `getkword` text against `initget` bits and parsed keywords.
pub(crate) fn check_initget_kword(
    bits: i32,
    kws: &[InitGetKeyword],
    listing: &str,
    text: &str,
) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        if (bits & 1 != 0) && !kws.is_empty() {
            return Err(format!("Invalid option keyword. Options: {listing}"));
        }
        return Ok(None);
    }
    if let Some(k) = match_initget_keyword(kws, text) {
        return Ok(Some(k));
    }
    if listing.is_empty() {
        Err("Invalid option keyword.".into())
    } else {
        Err(format!("Invalid option keyword. Options: {listing}"))
    }
}

/// Ask for one line of text on the CAD command line. `Ok(None)` = cancelled.
fn prompt_command_line_text(
    prompt: &str,
    fallback: &str,
    free_text: bool,
) -> Result<Option<String>, String> {
    let cleaned = prompt.trim();
    let prompt_text = if cleaned.is_empty() { fallback } else { cleaned };
    let bridge = if let Some(b) = active_interactive() {
        b
    } else {
        crate::context::ensure_interactive_with_mode(
            prompt_text,
            crate::interactive::InteractionMode::Text { free_text },
        )?
    };
    Ok(bridge.get_text(prompt_text, free_text))
}

/// Automatically load all `.lsp` / `.lisp` files in `scripts/` and `scripts/autoload/` on startup.
pub fn auto_load_lisp_scripts(host: &mut dyn HostApi) -> usize {
    let Ok(dir) = ensure_scripts_dir() else {
        return 0;
    };
    let mut files_to_load = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if filename.starts_with('_') || filename.starts_with('.') {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                    if let Ok(content) = read_lsp_text(&path) {
                        let lower = content.to_lowercase();
                        if lower.contains("defun c:") || lower.contains("defun") {
                            files_to_load.push(path);
                        }
                    }
                }
            }
        }
    }

    let autoload_dir = dir.join("autoload");
    if autoload_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&autoload_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !filename.starts_with('_') && !filename.starts_with('.') {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                            files_to_load.push(path);
                        }
                    }
                }
            }
        }
    }

    let tab_id = host.tab_id();
    crate::context::set_active_tab_id(tab_id);
    if tab_id != 0 {
        get_initialized_tabs().lock().unwrap().insert(tab_id);
    }
    let before_count = get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .map_or(0, |m| m.len());
    for path in files_to_load {
        let _ = load_lisp_file(&path.to_string_lossy(), host);
    }
    let _ = crate::appload::auto_load_startup_scripts(host);
    let after_count = get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .map_or(0, |m| m.len());
    after_count.saturating_sub(before_count)
}

/// Automatically load all `.lsp` / `.lisp` files in `scripts/`, `scripts/autoload/`, and Startup Scripts headlessly for the active tab.
pub fn auto_load_lisp_scripts_headless() -> usize {
    let tab_id = crate::context::get_active_tab_id();
    if tab_id != 0 {
        get_initialized_tabs().lock().unwrap().insert(tab_id);
    }
    let Ok(dir) = ensure_scripts_dir() else {
        return crate::appload::auto_load_startup_scripts_headless();
    };
    let mut files_to_load = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if filename.starts_with('_') || filename.starts_with('.') {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                    if let Ok(content) = read_lsp_text(&path) {
                        let lower = content.to_lowercase();
                        if lower.contains("defun c:") || lower.contains("defun") {
                            files_to_load.push(path);
                        }
                    }
                }
            }
        }
    }

    let autoload_dir = dir.join("autoload");
    if autoload_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&autoload_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !filename.starts_with('_') && !filename.starts_with('.') {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                            files_to_load.push(path);
                        }
                    }
                }
            }
        }
    }

    let before_count = get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .map_or(0, |m| m.len());
    for path in files_to_load {
        let _ = load_lisp_file_headless(&path);
    }
    let _ = crate::appload::auto_load_startup_scripts_headless();
    let after_count = get_registered_commands()
        .lock()
        .unwrap()
        .get(&tab_id)
        .map_or(0, |m| m.len());
    after_count.saturating_sub(before_count)
}

/// Load an AutoLISP `.lsp` file into the active tab's environment without requiring a direct `HostApi` ref.
pub fn load_lisp_file_headless(path: &Path) -> Result<(), String> {
    if let Ok(content) = read_lsp_text(path) {
        let mut map = LOADED_LISP_SCRIPTS.lock().unwrap();
        cache_script_keys(&mut map, &path.to_string_lossy(), &content);
    }
    let tab_id = crate::context::get_active_tab_id();
    let (tx, rx) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let task = LispTask::WithTab {
        tab_id,
        task: Box::new(LispTask::Load {
            path: path.to_path_buf(),
            resp: tx,
            worker_tx,
        }),
    };
    let _ = get_task_tx().send(task);

    let mut fallback_host = crate::context::DebugHost::new(crate::context::get_global_sender());

    loop {
        match worker_rx.try_recv() {
            Ok(WorkerMsg::HostCall(mut task)) => {
                if with_host(|h| task(h)).is_err() {
                    task(&mut fallback_host);
                }
            }
            _ => {}
        }
        match rx.try_recv() {
            Ok(res) => return res,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("AutoLISP worker disconnected".to_string());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// Decode `.lsp` bytes: UTF-8 (stripping an optional UTF-8 BOM) when valid, otherwise Windows-1252 (ANSI files from Notepad and similar editors).
pub(crate) fn decode_lsp_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.strip_prefix('\u{FEFF}').unwrap_or(s).to_string();
    }
    // 0x80..=0x9F in Windows-1252; the five undefined slots map to the C1 control of the same value.
    const CP1252_HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
        '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}',
        '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
        '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
            _ => b as char,
        })
        .collect()
}

/// Read a `.lsp` file as text, falling back to Windows-1252 when it is not valid UTF-8.
pub(crate) fn read_lsp_text(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|b| decode_lsp_bytes(&b))
}

fn resolve_lisp_script_path(filename: &str) -> Option<PathBuf> {
    let direct = Path::new(filename);
    if direct.is_file() {
        return Some(direct.to_path_buf());
    }
    let has_lsp_ext = filename.to_ascii_lowercase().ends_with(".lsp")
        || filename.to_ascii_lowercase().ends_with(".lisp");
    if !has_lsp_ext {
        let with_lsp = PathBuf::from(format!("{filename}.lsp"));
        if with_lsp.is_file() {
            return Some(with_lsp);
        }
        let with_lisp = PathBuf::from(format!("{filename}.lisp"));
        if with_lisp.is_file() {
            return Some(with_lisp);
        }
    }
    let dir = scripts_dir();
    let in_dir = dir.join(filename);
    if in_dir.is_file() {
        return Some(in_dir);
    }
    if !has_lsp_ext {
        let with_lsp = dir.join(format!("{filename}.lsp"));
        if with_lsp.is_file() {
            return Some(with_lsp);
        }
        let with_lisp = dir.join(format!("{filename}.lisp"));
        if with_lisp.is_file() {
            return Some(with_lisp);
        }
    }
    None
}

/// Load an AutoLISP `.lsp` file into the pure Rust environment
pub fn load_lisp_file(path_str: &str, host: &mut dyn HostApi) -> bool {
    let tab_id = host.tab_id();
    crate::context::set_thread_tab_id(Some(tab_id));
    if tab_id != 0 {
        crate::context::set_active_tab_id(tab_id);
    }
    let resolved = resolve_lisp_script_path(path_str).unwrap_or_else(|| scripts_dir().join(path_str));

    // If the file is not in `scripts/` on disk, check if it was loaded in memory from OCAIDE or APPLOAD
    if !resolved.is_file() {
        if let Some(in_mem_code) = get_loaded_script(path_str) {
            let ok = eval_lisp_expr(&in_mem_code, host);
            if ok {
                let stem = Path::new(path_str)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(path_str);
                let clean_stem = stem
                    .strip_prefix("c-")
                    .or_else(|| stem.strip_prefix("c_"))
                    .or_else(|| stem.strip_prefix("C-"))
                    .or_else(|| stem.strip_prefix("C_"))
                    .unwrap_or(stem);
                let lower_code = in_mem_code.to_ascii_lowercase();
                let call_pat = format!("(c:{}", clean_stem.to_ascii_lowercase());
                if has_lisp_command_for_tab(host.tab_id(), clean_stem)
                    && !lower_code.contains(&call_pat)
                {
                    return execute_lisp_command(clean_stem, &[], host);
                }
                host.push_info(&format!("Loaded AutoLISP script from IDE: {path_str}"));
            }
            return ok;
        }
    } else if let Ok(content) = read_lsp_text(&resolved) {
        let mut map = LOADED_LISP_SCRIPTS.lock().unwrap();
        cache_script_keys(&mut map, &resolved.to_string_lossy(), &content);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let task = LispTask::WithTab {
        tab_id,
        task: Box::new(LispTask::Load {
            path: resolved.clone(),
            resp: tx,
            worker_tx,
        }),
    };
    let _ = get_task_tx().send(task);

    loop {
        match worker_rx.try_recv() {
            Ok(WorkerMsg::HostCall(mut task)) => {
                task(host);
            }
            Ok(WorkerMsg::InteractiveStarted(cmd)) => {
                host.start_interactive(cmd);
                return true;
            }
            _ => {}
        }
        match rx.try_recv() {
            Ok(Ok(_)) => {
                host.push_info(&format!("Loaded AutoLISP file: {}", resolved.display()));
                return true;
            }
            Ok(Err(err)) => {
                push_cad_error_and_notify_ide(
                    host,
                    &format!("AutoLISP Load Error in {}", resolved.display()),
                    &err,
                );
                return false;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

/// Execute a registered AutoLISP command (like TARGET from c:target)
pub fn execute_lisp_command(cmd_name: &str, _args: &[String], host: &mut dyn HostApi) -> bool {
    let tab_id = host.tab_id();
    crate::context::set_thread_tab_id(Some(tab_id));
    if tab_id != 0 {
        crate::context::set_active_tab_id(tab_id);
    }
    let upper = cmd_name.to_uppercase();
    let lisp_call = format!("(c:{})", upper.to_lowercase());

    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    if let Some(sender) = host.plugin_request_sender().map(Arc::from) {
        crate::context::set_global_sender(Some(sender));
    }

    let task = LispTask::WithTab {
        tab_id,
        task: Box::new(LispTask::ExecuteCommand {
            lisp_call,
            worker_tx,
            done: done_tx,
        }),
    };
    let _ = get_task_tx().send(task);

    while let Ok(msg) = worker_rx.recv() {
        match msg {
            WorkerMsg::HostCall(mut task) => {
                task(host);
            }
            WorkerMsg::InteractiveStarted(cmd) => {
                host.start_interactive(cmd);
                return true;
            }
            WorkerMsg::Finished(res) => {
                if let Err(e) = res {
                    push_cad_error_and_notify_ide(host, "AutoLISP Error", &e);
                }
                return true;
            }
        }
    }

    let _ = done_rx.recv();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_lsp_bytes_utf8_and_cp1252() {
        assert_eq!(decode_lsp_bytes("(princ \"\u{e9}\")".as_bytes()), "(princ \"\u{e9}\")");
        // UTF-8 BOM stripped cleanly so lexer does not treat BOM as a symbol
        assert_eq!(decode_lsp_bytes("\u{FEFF}(setq x 42)".as_bytes()), "(setq x 42)");
        // ANSI file: 0xE9 alone is invalid UTF-8
        assert_eq!(decode_lsp_bytes(b"(princ \"caf\xE9\")"), "(princ \"caf\u{e9}\")");
        assert_eq!(decode_lsp_bytes(b"\x80 \x96 \x99 \xFC \xA7"), "\u{20AC} \u{2013} \u{2122} \u{fc} \u{a7}");

        // Verify AutoLISP (load ...) handles Windows-1252 files via host-aware LOAD override
        let tmp_path = std::env::temp_dir().join("ocad_test_cp1252_load.lsp");
        std::fs::write(&tmp_path, b"(setq _cp1252_loaded \"caf\xE9 90\xB0\")").unwrap();
        let bridge = std::sync::Arc::new(std::sync::Mutex::new(OpenCadRustBridge::new()));
        let env_rc = lisp_cad::create_cad_env(bridge.clone());
        register_cad_reflection_builtins(&mut env_rc.borrow_mut(), bridge);
        let mut env_mut = env_rc;
        let escaped = tmp_path.to_string_lossy().replace('\\', "/");
        let load_expr = format!("(load \"{}\")", escaped);
        lisp_core::eval_str(&load_expr, &mut env_mut).unwrap();
        assert_eq!(
            lisp_core::eval_str("_cp1252_loaded", &mut env_mut).unwrap(),
            Value::Str("caf\u{e9} 90\u{b0}".into())
        );
        let _ = std::fs::remove_file(&tmp_path);
    }

    #[test]
    fn test_entity_to_dxf_dimension_group0() {
        use ocs_plugin_api::host::codec::entities::{DimensionAligned, DimensionArc, DimensionLargeRadial, DimensionLinear};
        let a = Vector3::new(100.0, 100.0, 0.0);
        let b = Vector3::new(110.0, 100.0, 0.0);
        let group0 = |d: CadDimension| {
            let e = EntityType::Dimension(Box::new(d));
            entity_to_dxf(&e)[0].clone()
        };
        assert_eq!(group0(CadDimension::Aligned(DimensionAligned::new(a, b))), (0, DxfValue::Str("DIMENSION".into())));
        assert_eq!(group0(CadDimension::Linear(DimensionLinear::new(a, b))), (0, DxfValue::Str("DIMENSION".into())));
        assert_eq!(group0(CadDimension::Arc(DimensionArc::default())), (0, DxfValue::Str("ARC_DIMENSION".into())));
        assert_eq!(
            group0(CadDimension::LargeRadial(DimensionLargeRadial::default())),
            (0, DxfValue::Str("LARGE_RADIAL_DIMENSION".into()))
        );

        let mut lin = DimensionLinear::new(a, b);
        lin.definition_point = Vector3::new(110.0, 105.0, 0.0);
        lin.base.text_middle_point = Vector3::new(105.0, 105.0, 0.0);
        lin.base.text = "10.0".to_string();
        lin.base.style_name = "Standard".to_string();
        let dxf = entity_to_dxf(&EntityType::Dimension(Box::new(CadDimension::Linear(lin))));
        let get_group = |code: i16| dxf.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
        assert_eq!(get_group(0), Some(DxfValue::Str("DIMENSION".into())));
        assert_eq!(get_group(1), Some(DxfValue::Str("10.0".into())));
        assert_eq!(get_group(3), Some(DxfValue::Str("Standard".into())));
        assert_eq!(get_group(10), Some(DxfValue::Point([110.0, 105.0, 0.0])));
        assert_eq!(get_group(11), Some(DxfValue::Point([105.0, 105.0, 0.0])));
        assert_eq!(get_group(13), Some(DxfValue::Point([100.0, 100.0, 0.0])));
        assert_eq!(get_group(14), Some(DxfValue::Point([110.0, 100.0, 0.0])));
        assert_eq!(get_group(42), Some(DxfValue::Real(10.0)));
        assert_eq!(get_group(50), Some(DxfValue::Real(0.0)));
        assert_eq!(get_group(70), Some(DxfValue::Int(0)));

        // Spatial ssget bbox check: dimension at (100..110, 100..105) matches containing window and rejects distant window
        let bbox = lisp_cad::dxf::get_entity_bbox(&dxf);
        assert_eq!(bbox, Some([100.0, 100.0, 110.0, 105.0]));
        assert!(lisp_cad::dxf::check_spatial("W", Some(&[[90.0, 90.0, 0.0], [120.0, 120.0, 0.0]]), bbox));
        assert!(!lisp_cad::dxf::check_spatial("W", Some(&[[0.0, 0.0, 0.0], [20.0, 20.0, 0.0]]), bbox));
    }

    #[test]
    fn test_initget_keyword_matching_and_number_parsing() {
        let kws = parse_initget_keywords("Yes No");
        assert_eq!(kws, vec![
            InitGetKeyword { name: "Yes".into(), min_len: 1 },
            InitGetKeyword { name: "No".into(), min_len: 1 },
        ]);
        assert_eq!(match_initget_keyword(&kws, "y"), Some("Yes".into()));
        assert_eq!(match_initget_keyword(&kws, " NO "), Some("No".into()));
        assert_eq!(match_initget_keyword(&kws, "maybe"), None);
        let kws = parse_initget_keywords("EXit Width LTYPE,LT _Exit Width Ltype");
        assert_eq!(kws.len(), 3); // the names after "_" are global names, not extra options
        assert_eq!(match_initget_keyword(&kws, "e"), None); // required prefix is "EX"
        assert_eq!(match_initget_keyword(&kws, "ex"), Some("EXit".into()));
        assert_eq!(match_initget_keyword(&kws, "wid"), Some("Width".into()));
        assert_eq!(match_initget_keyword(&kws, "l"), None); // comma form: "LT" is the abbreviation
        assert_eq!(match_initget_keyword(&kws, "lt"), Some("LTYPE".into()));
        assert_eq!(match_initget_keyword(&kws, "ltype"), Some("LTYPE".into()));
        assert_eq!(parse_lisp_int(" -12 "), Some(-12));
        assert_eq!(parse_lisp_int("1.5"), None);
        assert_eq!(parse_lisp_int("abc"), None);
        assert_eq!(parse_lisp_real("3.25"), Some(3.25));
        assert_eq!(parse_lisp_real("-1e2"), Some(-100.0));
        assert_eq!(parse_lisp_real("inf"), None);
        assert_eq!(parse_lisp_real(""), None);

        assert_eq!(truncate_getstring("hello world", false), "hello");
        assert_eq!(truncate_getstring("hello world", true), "hello world");
        assert_eq!(truncate_getstring("", false), "");

        assert_eq!(check_initget_int(0, ""), Ok(None));
        assert_eq!(check_initget_int(1, ""), Err("Requires an integer value."));
        assert_eq!(check_initget_int(2, "0"), Err("Value must be nonzero."));
        assert_eq!(check_initget_int(2, "-5"), Ok(Some(-5)));
        assert_eq!(check_initget_int(4, "-5"), Err("Value must be nonnegative."));
        assert_eq!(check_initget_int(4, "0"), Ok(Some(0)));
        assert_eq!(check_initget_int(6, "0"), Err("Value must be positive and nonzero."));
        assert_eq!(check_initget_int(6, "-5"), Err("Value must be positive and nonzero."));
        assert_eq!(check_initget_int(6, "5"), Ok(Some(5)));

        assert_eq!(check_initget_real(0, ""), Ok(None));
        assert_eq!(check_initget_real(1, ""), Err("Requires a numeric value."));
        assert_eq!(check_initget_real(2, "0.0"), Err("Value must be nonzero."));
        assert_eq!(check_initget_real(2, "-2.5"), Ok(Some(-2.5)));
        assert_eq!(check_initget_real(4, "-2.5"), Err("Value must be nonnegative."));
        assert_eq!(check_initget_real(4, "0.0"), Ok(Some(0.0)));
        assert_eq!(check_initget_real(6, "0.0"), Err("Value must be positive and nonzero."));
        assert_eq!(check_initget_real(6, "-2.5"), Err("Value must be positive and nonzero."));
        assert_eq!(check_initget_real(6, "2.5"), Ok(Some(2.5)));

        let yn = parse_initget_keywords("Yes No");
        assert_eq!(check_initget_kword(0, &yn, "Yes/No", ""), Ok(None));
        assert_eq!(
            check_initget_kword(1, &yn, "Yes/No", ""),
            Err("Invalid option keyword. Options: Yes/No".into())
        );
        assert_eq!(check_initget_kword(1, &yn, "Yes/No", "y"), Ok(Some("Yes".into())));
        assert_eq!(check_initget_kword(1, &[], "", ""), Ok(None));
    }

    #[test]
    fn test_rust_lisp_sample_demo_eval_and_dispatch() {
        let sample_demo = r#"
            (defun draw-target (center radius / p1 p2 p3 p4)
              (entmake (list '(0 . "CIRCLE")
                             (cons 10 center)
                             (cons 40 radius)))
              (setq p1 (polar center 0.0 (* radius 1.25))
                    p2 (polar center pi (* radius 1.25))
                    p3 (polar center (/ pi 2.0) (* radius 1.25))
                    p4 (polar center (* pi 1.5) (* radius 1.25)))
              (entmake (list '(0 . "LINE")
                             (cons 10 p1)
                             (cons 11 p2)))
              (entmake (list '(0 . "LINE")
                             (cons 10 p3)
                             (cons 11 p4)))
            )

            (defun c:target (/ pt rad)
              (setq pt (getpoint "\nSpecify center point: ")
                    rad (getdist pt "\nSpecify target radius <25.0>: "))
              (draw-target pt rad)
            )
        "#;

        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: sample_demo.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok(), "sample_demo should parse and evaluate cleanly in Rust: {:?}", res);

        // Verify c:target auto-registered command in Rust
        assert!(has_lisp_command("TARGET"), "c:target should register TARGET command in Rust");

        // Now simulate interactive execution
        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:target)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                other => panic!("Expected InteractiveStarted, got {:?}", match other {
                    WorkerMsg::HostCall(_) => "HostCall",
                    WorkerMsg::Finished(_) => "Finished",
                    _ => "Other",
                }),
            }
        };

        assert!(cmd.prompt().contains("center point"), "Prompt should contain 'center point', got: {:?}", cmd.prompt());

        // Step 1: Supply Point 1 (Center)
        let step1 = cmd.on_point([10.0, 20.0, 0.0]);
        assert!(matches!(step1, ocs_plugin_api::host::CommandStep::NeedPoint), "step1 should be NeedPoint, got {:?}", step1);
        assert!(cmd.prompt().contains("radius"), "Prompt should contain 'radius', got: {:?}", cmd.prompt());

        // Step 2: Supply Point 2 (Edge point for radius 25)
        let step2 = cmd.on_point([35.0, 20.0, 0.0]);
        match step2 {
            ocs_plugin_api::host::CommandStep::CommitManyAndEnd(entities) => {
                assert_eq!(entities.len(), 3, "Should create 1 circle and 2 lines");
                assert!(matches!(entities[0], EntityType::Circle(_)));
                assert!(matches!(entities[1], EntityType::Line(_)));
                assert!(matches!(entities[2], EntityType::Line(_)));
            }
            other => panic!("Expected CommitManyAndEnd(3), got {:?}", other),
        }
    }

    #[test]
    fn test_rust_lisp_pauseline_enter_mapping() {
        let code = r#"
            (defun c:testpauseline ()
                (command "_.LINE" '(10.0 10.0 0.0) PAUSE "")
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok());

        assert!(has_lisp_command("TESTPAUSELINE"));

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:testpauseline)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut host = crate::context::DebugHost::new(None);
        while let Ok(msg) = worker_rx.recv() {
            match msg {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut host);
                }
                WorkerMsg::Finished(_) => break,
                WorkerMsg::InteractiveStarted(_) => break,
            }
        }

        let executed = get_global_bridge().lock().unwrap().executed_commands.clone();
        let pauseline_cmd = executed.iter().find(|cmd| cmd.contains("testpauseline") || cmd.contains("_.LINE 10.0000,10.0000,0.0000"));
        assert!(pauseline_cmd.is_some(), "Should have recorded executed command: {:?}", executed);
        let cmd_str = pauseline_cmd.unwrap();
        assert!(cmd_str.ends_with("ENTER"), "Command must end with explicit ENTER token for OpenCADStudio to finish pause, got: {}", cmd_str);
    }

    #[test]
    fn test_rust_lisp_smartrect_getpoint_and_streamed_line() {
        let code = r#"
            (defun c:testsmartrect (/ pt1 x y w h p1 p2 p3 p4)
              (setq pt1 (getpoint "\nPick first corner: "))
              (setq x (car pt1)
                    y (cadr pt1)
                    w 40.0
                    h 25.0)
              (setq p1 (list x y 0.0)
                    p2 (list (+ x w) y 0.0)
                    p3 (list (+ x w) (+ y h) 0.0)
                    p4 (list x (+ y h) 0.0))
              (command "_.LINE" p1 p2 p3 p4 "C")
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok());

        assert!(has_lisp_command("TESTSMARTRECT"));

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:testsmartrect)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(res) => panic!("Expected InteractiveStarted, got Finished({:?})", res),
            }
        };

        assert!(cmd.prompt().contains("Pick first corner"));

        // User supplies Corner 1 at (10, 20, 0)
        let step = cmd.on_point([10.0, 20.0, 0.0]);
        match step {
            ocs_plugin_api::host::CommandStep::CommitManyAndEnd(entities) => {
                assert_eq!(entities.len(), 4, "Should create 4 rectangle lines");
                for ent in &entities {
                    assert!(matches!(ent, EntityType::Line(_)));
                }
            }
            other => panic!("Expected CommitManyAndEnd(4), got {:?}", other),
        }
    }

    #[test]
    fn test_rust_lisp_direct_eval_getpoint() {
        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (resp_tx, _resp_rx) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::Eval {
            code: "(getpoint \"\\nClick a point: \")".to_string(),
            resp: resp_tx,
            worker_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(res) => panic!("Expected InteractiveStarted, got Finished({:?})", res),
            }
        };

        assert!(cmd.prompt().contains("Click a point"));

        // Provide point (50, 75, 0)
        let step = cmd.on_point([50.0, 75.0, 0.0]);
        assert!(command_ended(&step), "got {:?}", step);
    }

    /// The command ended: plainly, or by dispatching the flush of the text it left behind.
    fn command_ended(step: &ocs_plugin_api::host::CommandStep) -> bool {
        match step {
            ocs_plugin_api::host::CommandStep::Done => true,
            ocs_plugin_api::host::CommandStep::Dispatch(cmd) => cmd.starts_with(FLUSH_OUTPUT_COMMAND),
            _ => false,
        }
    }

    #[test]
    fn test_user_while_loop_with_princ() {
        let mut mock_host = crate::context::DebugHost::new(None);
        let handled = eval_lisp_expr(
            "(setq aaa 5) (while (> aaa 1) (princ \"test\") (setq aaa (- aaa 1)))",
            &mut mock_host,
        );
        assert!(handled);
        let outputs = mock_host.captured_output.lock().unwrap().clone();
        assert_eq!(outputs, vec!["test", "test", "test", "test", "1"]);
    }

    #[test]
    fn test_rust_lisp_princ_multiline_flushing() {
        let mut mock_host = crate::context::DebugHost::new(None);
        let handled = eval_lisp_expr(
            "(progn (princ \"Hello\\nWorld\\n\") (princ))",
            &mut mock_host,
        );
        assert!(handled);
        let outputs = mock_host.captured_output.lock().unwrap().clone();
        assert_eq!(outputs, vec!["Hello", "World"]);
    }

    #[test]
    fn test_rust_lisp_radical_polygon_multi_command_streaming() {
        let code = r#"
            (defun c:testpoly (/ center cx cy radius sides v0_x v0_y i ang vx vy)
              (setq center (getpoint "\nPick polygon center: "))
              (setq cx (car center)
                    cy (cadr center)
                    radius 50.0
                    sides 8)
              (setq v0_x (+ cx (* radius (cos 0.0)))
                    v0_y (+ cy (* radius (sin 0.0))))
              (command "_.LINE" (list v0_x v0_y 0.0))
              (setq i 1)
              (while (< i sides)
                (setq ang (* 2.0 pi (/ (float i) (float sides))))
                (setq vx (+ cx (* radius (cos ang))))
                (setq vy (+ cy (* radius (sin ang))))
                (command (list vx vy 0.0))
                (setq i (1+ i))
              )
              (command "C")
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok());

        assert!(has_lisp_command("TESTPOLY"));

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:testpoly)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(res) => panic!("Expected InteractiveStarted, got Finished({:?})", res),
            }
        };

        assert!(cmd.prompt().contains("Pick polygon center"));

        // User picks center point (100, 100, 0)
        let step = cmd.on_point([100.0, 100.0, 0.0]);
        match step {
            ocs_plugin_api::host::CommandStep::CommitManyAndEnd(entities) => {
                assert_eq!(entities.len(), 8, "Regular 8-gon must create 8 line entities, got {}", entities.len());
                for ent in &entities {
                    assert!(matches!(ent, EntityType::Line(_)));
                }
            }
            other => panic!("Expected CommitManyAndEnd(8), got {:?}", other),
        }
    }

    #[test]
    fn test_rust_lisp_shape_panel_state_and_get() {
        let code = r#"
            (oca-panel-register "shape_panel" "Shape Generator"
                (oca-choice "shape_color" "Color" '("Red" "Blue" "Green") "Red")
                (oca-checkbox "shape_draw_x" "Draw X Lines" nil)
                (oca-button "btn_draw" "Draw Circle" "(c:draw-shape)")
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok(), "Panel registration failed: {:?}", res);

        // Check defaults
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: r#"(list (oca-panel-get "shape_panel" "shape_color") (oca-panel-get "shape_panel" "shape_draw_x"))"#.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap();
        assert!(res.as_ref().unwrap().contains("Red"));
        assert!(res.as_ref().unwrap().contains("nil"));

        // Simulate host notification when user checks box and selects Green
        handle_panel_action("shape_panel", "shape_draw_x", "toggle", Some(&ocs_plugin_api::dialog::DialogValue::Bool(true)));
        handle_panel_action("shape_panel", "shape_color", "change", Some(&ocs_plugin_api::dialog::DialogValue::Choice { index: 2, text: "Green".to_string() }));

        // Check updated values
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: r#"(list (oca-panel-get "shape_panel" "shape_color") (oca-panel-get "shape_panel" "shape_draw_x"))"#.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap();
        assert!(res.as_ref().unwrap().contains("Green"));
        assert!(res.as_ref().unwrap().contains("T"));
    }

    #[test]
    fn test_panel_command_execution() {
        let code = r#"
            (defun c:draw-shape () (princ "\nDRAW-SHAPE"))
            (defun c:shapepanel () (princ "\nSHAPEPANEL"))
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok());

        assert!(has_lisp_command("DRAW-SHAPE"));
        assert!(has_lisp_command("SHAPEPANEL"));

        // Execute (c:draw-shape)
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: "(c:draw-shape)".to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok(), "draw-shape default failed: {:?}", res);
    }

    #[test]
    fn test_rust_lisp_getvar_and_setvar() {
        let code = r#"
            (progn
                ;; 1. Read default variables (both string and symbol syntax)
                (setq initial_layer (getvar "CLAYER"))
                (setq initial_cmd (getvar 'CMDECHO))

                ;; 2. Set and get CLAYER, OSMODE, LTSCALE, CECOLOR, USERI1, USERR1, USERS1, LASTPOINT
                (setvar "CLAYER" "WALLS")
                (setvar 'OSMODE 35)
                (setvar "LTSCALE" 2.5)
                (setvar "CECOLOR" "RED")
                (setvar "USERI1" 42)
                (setvar "USERR1" 3.14)
                (setvar "USERS1" "hello")
                (setvar "LASTPOINT" '(10.0 20.0 30.0))

                ;; 3. Unknown variable returns nil
                (setq unk (getvar "DOES_NOT_EXIST_VAR"))

                ;; 4. Reset CLAYER back to "0" and CECOLOR back to "BYLAYER" after capturing
                (setq res (list
                    initial_layer
                    initial_cmd
                    (getvar "CLAYER")
                    (getvar 'osmode)
                    (getvar "LTSCALE")
                    (getvar "CECOLOR")
                    (getvar "USERI1")
                    (getvar "USERR1")
                    (getvar "USERS1")
                    (getvar "LASTPOINT")
                    unk
                ))
                (setvar "CLAYER" "0")
                (setvar "CECOLOR" "BYLAYER")
                res
            )
        "#;

        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap().unwrap();
        assert_eq!(
            res,
            r#"("0" 1 "WALLS" 35 2.5 "1" 42 3.14 "hello" (10.0 20.0 30.0) nil)"#
        );
    }

    #[test]
    fn test_rust_lisp_expanded_entmake_and_entmod() {
        // 1. ARC
        let arc_dxf = vec![
            (0, DxfValue::Str("ARC".into())),
            (8, DxfValue::Str("ARCH_LAYER".into())),
            (10, DxfValue::Point([10.0, 20.0, 0.0])),
            (40, DxfValue::Real(15.0)),
            (50, DxfValue::Real(0.0)),
            (51, DxfValue::Real(90.0)),
        ];
        let arc_ent = dxf_to_entity(&arc_dxf, "0", None).expect("dxf_to_entity ARC");
        let arc_roundtrip = entity_to_dxf(&arc_ent);
        assert_eq!(arc_roundtrip[0], (0, DxfValue::Str("ARC".into())));
        assert_eq!(arc_roundtrip[1], (5, DxfValue::Handle(0)));
        assert_eq!(arc_roundtrip[2], (8, DxfValue::Str("ARCH_LAYER".into())));

        // 2. LWPOLYLINE
        let poly_dxf = vec![
            (0, DxfValue::Str("LWPOLYLINE".into())),
            (8, DxfValue::Str("POLY_LAYER".into())),
            (70, DxfValue::Int(1)), // closed
            (38, DxfValue::Real(5.0)),
            (10, DxfValue::Point([0.0, 0.0, 0.0])),
            (10, DxfValue::Point([100.0, 0.0, 0.0])),
            (10, DxfValue::Point([100.0, 50.0, 0.0])),
        ];
        let poly_ent = dxf_to_entity(&poly_dxf, "0", None).expect("dxf_to_entity LWPOLYLINE");
        let poly_roundtrip = entity_to_dxf(&poly_ent);
        assert_eq!(poly_roundtrip[0], (0, DxfValue::Str("LWPOLYLINE".into())));
        assert_eq!(poly_roundtrip[2], (8, DxfValue::Str("POLY_LAYER".into())));

        // 3. TEXT
        let text_dxf = vec![
            (0, DxfValue::Str("TEXT".into())),
            (1, DxfValue::Str("Room 101".into())),
            (10, DxfValue::Point([50.0, 50.0, 0.0])),
            (40, DxfValue::Real(3.5)),
            (50, DxfValue::Real(45.0)),
        ];
        let text_ent = dxf_to_entity(&text_dxf, "0", None).expect("dxf_to_entity TEXT");
        let text_roundtrip = entity_to_dxf(&text_ent);
        assert_eq!(text_roundtrip[0], (0, DxfValue::Str("TEXT".into())));
        assert_eq!(text_roundtrip[3], (1, DxfValue::Str("Room 101".into())));

        // 4. MTEXT
        let mtext_dxf = vec![
            (0, DxfValue::Str("MTEXT".into())),
            (1, DxfValue::Str("Multiline\nNotes".into())),
            (10, DxfValue::Point([0.0, 0.0, 0.0])),
            (40, DxfValue::Real(2.0)),
        ];
        let mtext_ent = dxf_to_entity(&mtext_dxf, "0", None).expect("dxf_to_entity MTEXT");
        let mtext_roundtrip = entity_to_dxf(&mtext_ent);
        assert_eq!(mtext_roundtrip[0], (0, DxfValue::Str("MTEXT".into())));
        assert_eq!(mtext_roundtrip[3], (1, DxfValue::Str("Multiline\nNotes".into())));

        // 5. SOLID
        let solid_dxf = vec![
            (0, DxfValue::Str("SOLID".into())),
            (10, DxfValue::Point([0.0, 0.0, 0.0])),
            (11, DxfValue::Point([10.0, 0.0, 0.0])),
            (12, DxfValue::Point([0.0, 10.0, 0.0])),
            (13, DxfValue::Point([10.0, 10.0, 0.0])),
        ];
        let solid_ent = dxf_to_entity(&solid_dxf, "0", None).expect("dxf_to_entity SOLID");
        let solid_roundtrip = entity_to_dxf(&solid_ent);
        assert_eq!(solid_roundtrip[0], (0, DxfValue::Str("SOLID".into())));

        // 6. INSERT
        let insert_dxf = vec![
            (0, DxfValue::Str("INSERT".into())),
            (2, DxfValue::Str("DOOR_BLOCK".into())),
            (10, DxfValue::Point([15.0, 25.0, 0.0])),
            (41, DxfValue::Real(2.0)),
            (42, DxfValue::Real(2.0)),
            (43, DxfValue::Real(1.0)),
            (50, DxfValue::Real(90.0)),
        ];
        let insert_ent = dxf_to_entity(&insert_dxf, "0", None).expect("dxf_to_entity INSERT");
        let insert_roundtrip = entity_to_dxf(&insert_ent);
        assert_eq!(insert_roundtrip[0], (0, DxfValue::Str("INSERT".into())));
        assert_eq!(insert_roundtrip[3], (2, DxfValue::Str("DOOR_BLOCK".into())));
    }

    #[test]
    fn test_rust_lisp_regapp_and_xdata_roundtrip() {
        let mut bridge = OpenCadRustBridge::new();
        // 1. reg_app
        assert!(bridge.reg_app("SURVEY_DATA").unwrap());
        // Second registration returns false
        assert!(!bridge.reg_app("SURVEY_DATA").unwrap());

        // 2. ExtendedDataRecord conversion
        let records = vec![
            (1000, DxfValue::Str("PointElevation".into())),
            (1040, DxfValue::Real(125.75)),
            (1070, DxfValue::Int(7)),
            (1010, DxfValue::Point([12.5, 34.2, 5.0])),
        ];

        let rec = dxf_to_xdata_record("SURVEY_DATA", &records);
        assert_eq!(rec.application_name, "SURVEY_DATA");
        assert_eq!(rec.values.len(), 4);

        let roundtrip = xdata_record_to_dxf(&rec);
        assert_eq!(roundtrip, records);
    }

    #[test]
    fn test_rust_lisp_command_args_multi_step() {
        let mut bridge = OpenCadRustBridge::new();
        let args = vec![
            "_.LINE".to_string(),
            "0.0,0.0".to_string(),
            "10.0,10.0".to_string(),
            "".to_string(),
        ];
        // In unit test / offline environment, execute_command_args succeeds cleanly
        let res = bridge.execute_command_args(&args);
        assert!(res.is_ok());
        assert_eq!(bridge.executed_commands.len(), 1);
        assert_eq!(bridge.executed_commands[0], "_.LINE 0.0,0.0 10.0,10.0 ");
    }

    #[test]
    fn test_rust_lisp_selection_sets_comprehensive() {
        let mut bridge = OpenCadRustBridge::new();

        // 1. ss_add with no handle creates a new empty selection set
        let ss1 = bridge.ss_add(0, None).unwrap();
        assert_eq!(bridge.ss_length(ss1), 0);
        assert_eq!(bridge.ss_name(ss1, 0), None);
        assert!(!bridge.ss_memb(101, ss1));

        // 2. ss_add adds entities to existing set
        assert_eq!(bridge.ss_add(101, Some(ss1)).unwrap(), ss1);
        assert_eq!(bridge.ss_add(102, Some(ss1)).unwrap(), ss1);
        assert_eq!(bridge.ss_length(ss1), 2);
        assert_eq!(bridge.ss_name(ss1, 0), Some(101));
        assert_eq!(bridge.ss_name(ss1, 1), Some(102));
        assert!(bridge.ss_memb(101, ss1));
        assert!(bridge.ss_memb(102, ss1));
        assert!(!bridge.ss_memb(103, ss1));

        // Adding duplicate entity without :D is a no-op
        assert_eq!(bridge.ss_add(101, Some(ss1)).unwrap(), ss1);
        assert_eq!(bridge.ss_length(ss1), 2);

        // 3. ss_del removes entity
        assert!(bridge.ss_del(101, ss1).is_ok());
        assert_eq!(bridge.ss_length(ss1), 1);
        assert_eq!(bridge.ss_name(ss1, 0), Some(102));
        assert!(!bridge.ss_memb(101, ss1));

        // 4. Test spatial and filter logic via mock-populated selection set
        let line_elist = vec![
            (0, DxfValue::Str("LINE".into())),
            (8, DxfValue::Str("WALLS".into())),
            (10, DxfValue::Point([0.0, 0.0, 0.0])),
            (11, DxfValue::Point([10.0, 10.0, 0.0])),
        ];
        let circle_elist = vec![
            (0, DxfValue::Str("CIRCLE".into())),
            (8, DxfValue::Str("FURNITURE".into())),
            (10, DxfValue::Point([50.0, 50.0, 0.0])),
            (40, DxfValue::Real(5.0)),
        ];

        // Verify spatial bounding box helper
        let bbox_line = lisp_cad::get_entity_bbox(&line_elist).unwrap();
        assert_eq!(bbox_line, [0.0, 0.0, 10.0, 10.0]);

        let bbox_circle = lisp_cad::get_entity_bbox(&circle_elist).unwrap();
        assert_eq!(bbox_circle, [45.0, 45.0, 55.0, 55.0]);

        // Window mode "W"
        assert!(lisp_cad::check_spatial("W", Some(&[[0.0, 0.0, 0.0], [20.0, 20.0, 0.0]]), Some(bbox_line)));
        assert!(!lisp_cad::check_spatial("W", Some(&[[0.0, 0.0, 0.0], [5.0, 5.0, 0.0]]), Some(bbox_line)));

        // Crossing mode "C"
        assert!(lisp_cad::check_spatial("C", Some(&[[5.0, 5.0, 0.0], [15.0, 15.0, 0.0]]), Some(bbox_line)));

        // Point pick
        assert!(lisp_cad::check_spatial("", Some(&[[5.0, 5.0, 0.0]]), Some(bbox_line)));
        assert!(!lisp_cad::check_spatial("", Some(&[[100.0, 100.0, 0.0]]), Some(bbox_line)));

        // DXF Filter logic
        let filter_line = vec![(0, DxfValue::Str("LINE".into()))];
        assert!(lisp_cad::eval_dxf_filter(&line_elist, &filter_line));
        assert!(!lisp_cad::eval_dxf_filter(&circle_elist, &filter_line));

        let filter_or = vec![
            (-4, DxfValue::Str("<OR".into())),
            (0, DxfValue::Str("LINE".into())),
            (0, DxfValue::Str("CIRCLE".into())),
            (-4, DxfValue::Str("OR>".into())),
        ];
        assert!(lisp_cad::eval_dxf_filter(&line_elist, &filter_or));
        assert!(lisp_cad::eval_dxf_filter(&circle_elist, &filter_or));

        let filter_gt = vec![
            (-4, DxfValue::Str(">".into())),
            (40, DxfValue::Real(2.0)),
        ];
        assert!(lisp_cad::eval_dxf_filter(&circle_elist, &filter_gt));
    }

    #[test]
    fn test_eval_autolisp_ssget_pipeline() {
        let code = r#"
            (progn
                ;; 1. Empty set creation and query
                (setq ss_empty (ssadd))
                (setq len0 (sslength ss_empty))

                ;; 2. Point pick and window call syntax without crashing
                (setq ss_pt (ssget '(10.0 20.0 0.0)))
                (setq ss_win (ssget "W" '(0.0 0.0) '(100.0 100.0)))
                (setq ss_cross (ssget "C" '(0.0 0.0) '(100.0 100.0)))
                (setq ss_poly (ssget "WP" '((0 0) (10 0) (10 10) (0 10))))
                (setq ss_fence (ssget "F" '((0 0) (5 5) (10 0))))
                (setq ss_single (ssget ":S"))
                (setq ss_all (ssget "X"))

                ;; 3. Return summary list
                (list len0 (null ss_pt) (null ss_win) (null ss_cross) (null ss_poly) (null ss_fence))
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap().unwrap();
        assert_eq!(res, "(0 T T T T T)");
    }

    #[test]
    fn test_muv_command_move_execution() {
        let code = r#"
            (progn
                (defun c:MUV ( / pt ss)
                  (if (and (princ "\n Select objects to move to 0,0 ")
                           (setq ss (ssget "_:L"))
                           (setq pt (getpoint "\n Enter base point: "))
                           )
                    (command "_MOVE" ss "" "_NONE" pt "_NONE" '(0.0 0.0 0.0))
                    )
                  (princ)
                )

                ;; Create a selection set with an entity handle and execute MOVE
                (setq ss (ssadd))
                (ssadd 42 ss)
                (command "_MOVE" ss "" "_NONE" '(10.0 20.0 0.0) "_NONE" '(0.0 0.0 0.0))
                "MUV_SUCCESS"
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap().unwrap();
        assert_eq!(res, "\"MUV_SUCCESS\"");

        // Interactive simulation of running (c:muv)
        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:muv)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                other => panic!("Expected InteractiveStarted, got {:?}", match other {
                    WorkerMsg::HostCall(_) => "HostCall",
                    WorkerMsg::Finished(_) => "Finished",
                    _ => "Other",
                }),
            }
        };

        // Step 1: Prompt is for object selection
        assert!(cmd.is_selection_gathering(), "MUV should prompt for selection gathering");

        // Step 2: Pick object 42 and Enter
        let step_pick = cmd.on_object_pick(Handle::new(42), [10.0, 20.0, 0.0]);
        assert!(matches!(step_pick, ocs_plugin_api::host::CommandStep::NeedPoint), "step_pick was: {:?}", step_pick);
        let step_enter = cmd.on_enter();
        assert!(matches!(step_enter, ocs_plugin_api::host::CommandStep::NeedPoint), "step_enter was: {:?}", step_enter);

        // Step 3: Base point prompt
        assert!(cmd.prompt().contains("base point"), "Prompt should contain 'base point', got: {:?}", cmd.prompt());
        let step_pt = cmd.on_point([12.5, 3.5, 0.0]);
        match step_pt {
            ocs_plugin_api::host::CommandStep::Dispatch(dispatched) => {
                assert!(dispatched.contains("_MOVE"), "Dispatched command should contain _MOVE: {}", dispatched);
                assert!(dispatched.contains("2A"), "Dispatched command should contain hex handle 2A (42): {}", dispatched);
                assert!(dispatched.contains("12.5000,3.5000,0.0000"), "Dispatched command should contain base point: {}", dispatched);
                assert!(dispatched.contains("0.0000,0.0000,0.0000"), "Dispatched command should contain target point 0,0,0: {}", dispatched);
            }
            other => panic!("Expected CommandStep::Dispatch, got {:?}", other),
        }
    }

    #[test]
    fn test_ssget_interactive_selection_and_locked_layer() {
        use ocs_plugin_api::host::Document as CadDocument;
        use ocs_plugin_api::host::codec::entities::Line;
        use ocs_plugin_api::host::codec::tables::Layer;

        let code = r#"
            (defun c:test_ssget_l ( / ss)
              (setq ss (ssget "_:L"))
              (if ss (sslength ss) 0)
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let _ = rx.recv().unwrap();

        let mut doc = CadDocument::new();
        let mut locked_layer = Layer::new("LOCKED_LAYER");
        locked_layer.lock();
        let _ = doc.layers.add(locked_layer);

        let mut l1 = Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0);
        l1.common.layer = "0".into();
        l1.common.handle = Handle::new(101);
        let _ = doc.add_entity(EntityType::Line(l1));

        let mut l2 = Line::from_coords(0.0, 0.0, 0.0, 0.0, 10.0, 0.0);
        l2.common.layer = "LOCKED_LAYER".into();
        l2.common.handle = Handle::new(102);
        let _ = doc.add_entity(EntityType::Line(l2));

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:test_ssget_l)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        mock_host.doc = std::cell::UnsafeCell::new(Some(doc));
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => {
                    // Seed the doc snapshot for testing
                    break c;
                }
                _ => {}
            }
        };

        // Pick locked object (102) and unlocked object (101)
        let s1 = cmd.on_object_pick(Handle::new(102), [0.0, 5.0, 0.0]);
        assert!(matches!(s1, ocs_plugin_api::host::CommandStep::NeedPoint));
        let s2 = cmd.on_object_pick(Handle::new(101), [5.0, 0.0, 0.0]);
        assert!(matches!(s2, ocs_plugin_api::host::CommandStep::NeedPoint));

        let end_step = cmd.on_enter();
        // Since (c:test_ssget_l) evaluates (sslength ss) and finishes,
        // it should complete with Done!
        assert!(matches!(end_step, ocs_plugin_api::host::CommandStep::Done));
    }

    #[test]
    fn test_ssget_selection_complete_and_enter_clean_exit() {
        use ocs_plugin_api::host::Document as CadDocument;
        use ocs_plugin_api::host::codec::entities::Line;

        let code = r#"
            (defun c:test_ssget_gather ( / ss)
                (setq ss (ssget))
                (if ss (sslength ss) 0)
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let _ = rx.recv().unwrap();

        let mut doc = CadDocument::new();
        let mut line1 = Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0);
        line1.common.handle = Handle::new(201);
        let _ = doc.add_entity(EntityType::Line(line1));

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:test_ssget_gather)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        mock_host.doc = std::cell::UnsafeCell::new(Some(doc));
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                _ => {}
            }
        };

        // Viewport selection engine checks
        assert!(cmd.is_selection_gathering(), "is_selection_gathering must be true for ssget");
        assert!(cmd.accepts_drag_selection(), "accepts_drag_selection must be true for ssget");

        // Simulate host viewport selection completion (e.g. window or crossing box)
        let step_complete = cmd.on_selection_complete(vec![Handle::new(201)]);
        assert!(matches!(step_complete, ocs_plugin_api::host::CommandStep::NeedPoint));

        // Pressing Enter should finish the command cleanly without freezing or timing out
        let start = std::time::Instant::now();
        let end_step = cmd.on_enter();
        let elapsed = start.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(5), "Enter must complete rapidly, elapsed: {:?}", elapsed);
        assert!(matches!(end_step, ocs_plugin_api::host::CommandStep::Done), "Expected Done on enter, got {:?}", end_step);
    }

    #[test]
    fn test_princ_order_and_sequential_setq_no_newline() {
        let code = r#"
            (defun c:test_order_no_nl ( / a b msg ss)
              (if (and (princ "Step 1: Calculating...")
                       (setq a 10)
                       (setq b (+ a 25))
                       (setq msg (strcat "Step 2: Sum is " (itoa b)))
                       (princ msg)
                       (setq ss (ssget))
                       )
                (princ "Step 3: Done")
              )
              (princ)
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let _ = rx.recv().unwrap();

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();

        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:test_order_no_nl)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                _ => {}
            }
        };

        // Check captured output on mock_host BEFORE user does any interaction!
        let output = mock_host.captured_output.lock().unwrap().clone();
        assert!(output.contains(&"Step 1: Calculating...".to_string()), "Output should contain Step 1 before ssget: {:?}", output);
        assert!(output.contains(&"Step 2: Sum is 35".to_string()), "Output should contain Step 2 with calculated b (35) before ssget: {:?}", output);

        let pos1 = output.iter().position(|s| s == "Step 1: Calculating...").unwrap();
        let pos2 = output.iter().position(|s| s == "Step 2: Sum is 35").unwrap();
        assert!(pos1 < pos2, "Step 1 must be emitted before Step 2");

        let _ = cmd.on_enter();
    }

    #[test]
    fn test_ssget_optional_arguments_and_filter_enhancements() {
        let code = r#"
            (progn
                ;; 1. Tolerating nil optional arguments
                (setq ss_nil1 (ssget nil))
                (setq ss_nil2 (ssget "X" nil))

                ;; 2. Two-point box selection without mode string
                (setq ss_box (ssget '(0.0 0.0) '(100.0 100.0)))

                ;; 3. Relational <> and bitwise & filters
                (setq ss_filt (ssget "X" '((-4 . "<>") (0 . "DIMENSION") (-4 . "&") (70 . 1))))

                ;; 4. Combined L:L mode
                (setq ss_last (ssget "L:L"))
                (setq ss_und_last (ssget "_L:L"))

                (list (null ss_nil1) (null ss_box) (null ss_filt) (null ss_last) (null ss_und_last))
            )
        "#;
        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap().unwrap().unwrap();
        assert_eq!(res, "(T T T T T)");
    }

    #[test]
    fn test_parse_ssget_mode_combinations() {
        // 1. Basic / interactive
        let m = parse_ssget_mode("");
        assert_eq!(m.primary, "");
        assert!(!m.reject_locked && !m.single_select && !m.point_only);

        // 2. Modifiers alone
        let m = parse_ssget_mode(":L");
        assert_eq!(m.primary, "");
        assert!(m.reject_locked && !m.single_select);

        let m = parse_ssget_mode("_:L");
        assert_eq!(m.primary, "");
        assert!(m.reject_locked);

        let m = parse_ssget_mode(":S");
        assert_eq!(m.primary, "");
        assert!(m.single_select && !m.reject_locked);

        // 3. Modifier combinations
        let m1 = parse_ssget_mode(":S:L");
        let m2 = parse_ssget_mode(":L:S");
        let m3 = parse_ssget_mode("_:S:L");
        assert_eq!(m1, m2);
        assert_eq!(m1.primary, "");
        assert!(m1.reject_locked && m1.single_select);
        assert!(m3.reject_locked && m3.single_select);

        // 4. Point selection + pickbox (entsel emulation)
        let m = parse_ssget_mode("+.:E:S");
        assert_eq!(m.primary, "");
        assert!(m.point_only && m.pickbox_select && m.single_select && !m.reject_locked);

        let m = parse_ssget_mode("_+.:E:S:L");
        assert_eq!(m.primary, "");
        assert!(m.point_only && m.pickbox_select && m.single_select && m.reject_locked);

        // 5. Database queries with modifiers
        let m = parse_ssget_mode("_X:L");
        assert_eq!(m.primary, "X");
        assert!(m.reject_locked);

        let m = parse_ssget_mode(":L:X");
        assert_eq!(m.primary, "X");
        assert!(m.reject_locked);

        // 6. Geometry / selection methods with modifiers
        let m = parse_ssget_mode("W:L");
        assert_eq!(m.primary, "W");
        assert!(m.reject_locked);

        let m = parse_ssget_mode("_CP:L");
        assert_eq!(m.primary, "CP");
        assert!(m.reject_locked);

        let m = parse_ssget_mode("WP:L");
        assert_eq!(m.primary, "WP");
        assert!(m.reject_locked);

        let m = parse_ssget_mode("F:L");
        assert_eq!(m.primary, "F");
        assert!(m.reject_locked);

        let m = parse_ssget_mode("P:L");
        assert_eq!(m.primary, "P");
        assert!(m.reject_locked);

        // 7. Last entity with reject locked
        let m = parse_ssget_mode("L:L");
        assert_eq!(m.primary, "L");
        assert!(m.reject_locked);

        let m = parse_ssget_mode("_L:L");
        assert_eq!(m.primary, "L");
        assert!(m.reject_locked);

        // 8. Duplicates flag
        let m = parse_ssget_mode(":D:L");
        assert!(m.allow_duplicates && m.reject_locked);
    }

    #[test]
    fn test_bridge_mode_flags_and_drag_selection() {
        let (bridge, cmd) = crate::interactive::LispInteractiveBridge::new(
            "Select: ".to_string(),
            crate::interactive::InteractionMode::Selection,
            1,
            None,
        );

        // Default: selection gathering true, drag selection true, rejects locked false
        assert!(cmd.is_selection_gathering());
        assert!(cmd.accepts_drag_selection());
        assert!(!cmd.selection_rejects_locked_layers());

        // Set :L
        bridge.set_reject_locked(true);
        assert!(cmd.selection_rejects_locked_layers());

        // Set point_only (+.)
        bridge.set_point_only(true);
        assert!(!cmd.accepts_drag_selection(), "point_only must disable drag selection");

        // Reset
        bridge.set_mode_flags(false, false, false);
        assert!(cmd.accepts_drag_selection());
        assert!(!cmd.selection_rejects_locked_layers());
    }

    #[test]
    fn test_send_host_keepalive_dispatches_get_tab_id() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use ocs_plugin_api::host::{PluginRequestSender, PluginRequestError};
        use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};

        struct MockSender {
            calls: Arc<AtomicUsize>,
        }
        impl PluginRequestSender for MockSender {
            fn request(&self, req: PluginRequest) -> Result<PluginResponse, PluginRequestError> {
                if matches!(req, PluginRequest::GetTabId) {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    Ok(PluginResponse::TabId(42))
                } else {
                    Err(PluginRequestError("unexpected request".into()))
                }
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let mock = Arc::new(MockSender { calls: calls.clone() });
        crate::context::set_global_sender(Some(mock));

        send_host_keepalive();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        send_host_keepalive();
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        crate::context::set_global_sender(None);
    }

    #[test]
    fn test_per_drawing_tab_namespace_isolation() {
        let tab_a: u64 = 9001;
        let tab_b: u64 = 9002;

        clean_tab_state(tab_a);
        clean_tab_state(tab_b);

        // 1. In Drawing Tab A: define c:PROJECT_A and a tab-specific c:CALC_PARAM
        crate::context::set_thread_tab_id(Some(tab_a));
        let _ = eval_lisp_str_headless("(defun c:project_a () \"A_ONLY\")").unwrap();
        let _ = eval_lisp_str_headless("(defun c:calc_param () 100)").unwrap();

        assert!(has_lisp_command_for_tab(tab_a, "PROJECT_A"));
        assert!(has_lisp_command_for_tab(tab_a, "CALC_PARAM"));
        assert!(!has_lisp_command_for_tab(tab_b, "PROJECT_A"), "PROJECT_A must not leak into Tab B");
        assert!(!has_lisp_command_for_tab(tab_b, "CALC_PARAM"), "CALC_PARAM must not leak into Tab B");

        // 2. Switch to Drawing Tab B: define c:PROJECT_B and a different c:CALC_PARAM
        crate::context::set_thread_tab_id(Some(tab_b));
        let _ = eval_lisp_str_headless("(defun c:project_b () \"B_ONLY\")").unwrap();
        let _ = eval_lisp_str_headless("(defun c:calc_param () 250)").unwrap();

        assert!(has_lisp_command_for_tab(tab_b, "PROJECT_B"));
        assert!(!has_lisp_command_for_tab(tab_a, "PROJECT_B"), "PROJECT_B must not leak into Tab A");

        // Verify Tab B evaluates its own version of c:calc_param (250) and does not have c:project_a
        let res_b = eval_lisp_str_headless("(c:calc_param)").unwrap();
        assert_eq!(res_b.as_deref(), Some("250"));
        assert!(eval_lisp_str_headless("(c:project_a)").is_err());

        // 3. Switch back to Drawing Tab A: verify Tab A still evaluates its own version (100)
        crate::context::set_thread_tab_id(Some(tab_a));
        let res_a = eval_lisp_str_headless("(c:calc_param)").unwrap();
        assert_eq!(res_a.as_deref(), Some("100"));
        let res_proj_a = eval_lisp_str_headless("(c:project_a)").unwrap();
        assert_eq!(res_proj_a.as_deref(), Some("\"A_ONLY\""));

        // 4. Close Tab A and verify cleanup leaves Tab B untouched
        clean_tab_state(tab_a);
        assert!(!has_lisp_command_for_tab(tab_a, "PROJECT_A"));
        assert!(has_lisp_command_for_tab(tab_b, "PROJECT_B"));

        clean_tab_state(tab_b);
        crate::context::set_thread_tab_id(None);
    }

    #[test]
    fn test_c_testdialog_exact_flow() {
        let code = r#"
            (defun c:testdialog (/ dlg res on-dialog-action)
              (defun on-dialog-action (id action values / picked current-tbl new-tbl count-msg mode-val log-val)
                (cond
                  ((or (= action "pick") (= id "picker"))
                   (setq picked (oca-get "picker" values))
                   (setq current-tbl (oca-get "tbl_files" values))
                   (if (and picked (/= picked ""))
                     (progn
                       (setq new-tbl (append current-tbl (list (list picked "Active"))))
                       (setq count-msg (strcat "Added file. Total in list: " (itoa (length new-tbl))))
                       (list
                         (cons "tbl_files" new-tbl)
                         (cons "picker" "")
                         (cons "lbl_status" count-msg)
                       )
                     )
                     nil
                   ))
                  ((= id "btn_clear")
                   (list
                     (cons "tbl_files" '())
                     (cons "lbl_status" "List cleared. 0 items in table.")
                   ))
                  ((= id "btn_inspect")
                   (setq mode-val (oca-get "cmb_mode" values))
                   (setq log-val (oca-get "chk_log" values))
                   (list
                     (cons "lbl_status"
                           (strcat "Active Mode: " (vl-princ-to-string mode-val)
                                   " | Console Log: " (if log-val "ON" "OFF")))
                   ))
                  (t nil)
                )
              )

              (setq dlg
                (oca-dialog "AutoLISP Reactive Dialog Test"
                  '((width . 560.0) (submit . "Done") (cancel . "Cancel"))
                  (oca-info "lbl_status" "Ready - Pick a file or click buttons to test reactivity")
                  (oca-row
                    (oca-file "picker" "Pick LISP File" "" '((filter . "AutoLISP (*.lsp)") (extensions . ("lsp"))))
                    (oca-button "btn_clear" "Clear List")
                  )
                  (oca-row
                    (oca-table "tbl_files" "Configured Scripts"
                      '("File Path" "Status")
                      '(("C:/example/first.lsp" "Active")
                        ("C:/example/second.lsp" "Active"))
                      '((allow_add . T) (allow_delete . T))
                    )
                  )
                  (oca-row
                    (oca-choice "cmb_mode" "Execution Mode" '("Standard" "Fast" "Debug") "Standard")
                    (oca-checkbox "chk_log" "Log to Console" T)
                    (oca-button "btn_inspect" "Inspect State")
                  )
                )
              )

              (setq res (oca-dialog-show dlg 'on-dialog-action))
              (if res "SUBMITTED" "CANCELLED")
            )
        "#;

        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok(), "eval should succeed: {:?}", res);

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:testdialog)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => {
                    task(&mut mock_host);
                }
                WorkerMsg::InteractiveStarted(c) => break c,
                other => panic!("Expected InteractiveStarted, got {:?}", match other {
                    WorkerMsg::HostCall(_) => "HostCall",
                    WorkerMsg::Finished(r) => panic!("Finished with {:?}", r),
                    _ => "Other",
                }),
            }
        };

        // Verify modal dialog was set
        let dlg = cmd.modal_dialog().expect("Dialog should be set");
        assert_eq!(dlg.title, "AutoLISP Reactive Dialog Test");

        // Simulate button click: btn_inspect
        let mut current_vals = std::collections::HashMap::new();
        current_vals.insert("cmb_mode".to_string(), ocs_plugin_api::dialog::DialogValue::Choice { index: 0, text: "Standard".to_string() });
        current_vals.insert("chk_log".to_string(), ocs_plugin_api::dialog::DialogValue::Bool(true));
        current_vals.insert("tbl_files".to_string(), ocs_plugin_api::dialog::DialogValue::Table(vec![
            vec!["C:/example/first.lsp".to_string(), "Active".to_string()],
        ]));

        let event = ocs_plugin_api::dialog::DialogEvent {
            control_id: "btn_inspect".to_string(),
            action: "click".to_string(),
            value: None,
            current_values: current_vals.clone(),
        };
        let act_resp = cmd.on_dialog_action(event);
        match act_resp {
            ocs_plugin_api::dialog::DialogActionResponse::UpdateValues(vals) => {
                let status = vals.get("lbl_status").expect("lbl_status should be updated");
                match status {
                    ocs_plugin_api::dialog::DialogValue::Text(s) => {
                        assert!(s.contains("Active Mode: Standard"), "Expected 'Active Mode: Standard', got: {s}");
                        assert!(s.contains("Console Log: ON"), "Expected 'Console Log: ON', got: {s}");
                    }
                    other => panic!("Expected Text, got {:?}", other),
                }
            }
            other => panic!("Expected UpdateValues, got {:?}", other),
        }

        // Simulate button click: btn_clear
        let event_clear = ocs_plugin_api::dialog::DialogEvent {
            control_id: "btn_clear".to_string(),
            action: "click".to_string(),
            value: None,
            current_values: current_vals.clone(),
        };
        let act_resp_clear = cmd.on_dialog_action(event_clear);
        match act_resp_clear {
            ocs_plugin_api::dialog::DialogActionResponse::UpdateValues(vals) => {
                let status = vals.get("lbl_status").expect("lbl_status should be updated");
                match status {
                    ocs_plugin_api::dialog::DialogValue::Text(s) => {
                        assert!(s.contains("List cleared"), "Expected 'List cleared', got: {s}");
                    }
                    other => panic!("Expected Text, got {:?}", other),
                }
                let tbl = vals.get("tbl_files").expect("tbl_files should be updated");
                match tbl {
                    ocs_plugin_api::dialog::DialogValue::Table(rows) => {
                        assert_eq!(rows.len(), 0, "Table should be empty");
                    }
                    other => panic!("Expected Table, got {:?}", other),
                }
            }
            other => panic!("Expected UpdateValues, got {:?}", other),
        }

        // Conclude with submit
        let step = cmd.on_dialog_response(ocs_plugin_api::dialog::DialogResponse::Submitted(current_vals));
        assert!(matches!(step, ocs_plugin_api::host::CommandStep::Done));
    }

    #[test]
    fn test_entsel_interactive_object_pick_and_entget_entmod_move() {
        let tab_id: u64 = 9010;
        clean_tab_state(tab_id);

        let code = r#"
            (defun c:entsel_test_flow (/ sel ent elist new_elist)
              (if (setq sel (entsel "\nSelect entity: "))
                (progn
                  (setq ent (car sel))
                  (setq _test_pt (cadr sel))
                  (setq _test_lastp (getvar "LASTPOINT"))
                  (setq elist (entget ent))
                  (setq new_elist (subst '(8 . "MODIFIED_LAYER") (assoc 8 elist) elist))
                  (entmod new_elist)
                  (setq _test_updated (cdr (assoc 8 (entget ent))))
                  (command "_MOVE" sel "" "_NONE" _test_pt "_NONE" '(0.0 0.0 0.0))
                )
              )
              (princ)
            )
            (defun c:entsel_del_test (/ sel)
              (if (setq sel (entsel "\nDelete entity: "))
                (progn
                  (entdel (car sel))
                  (setq _test_deleted (vl-catch-all-error-p (vl-catch-all-apply 'entget (list (car sel)))))
                )
              )
              (princ)
            )
            (defun c:entsel_cancel_test ()
              (setq _test_cancel_sel (entsel "\nPick or press Enter: "))
              (if _test_cancel_sel
                (princ "\nGOT_ENTITY")
                (princ "\nCANCELLED_OK")
              )
              (princ)
            )
        "#;

        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::Eval {
                code: code.to_string(),
                resp: tx,
                worker_tx,
            }),
        });
        assert!(rx.recv().unwrap().is_ok());

        // 1. Test interactive object pick + LASTPOINT + entget + entmod + MOVE dispatch
        let mut mock_host = crate::context::DebugHost::new(None);
        let picked_handle = {
            let line = Line::from_coords(10.0, 20.0, 0.0, 30.0, 40.0, 0.0);
            mock_host.document_mut().add_entity(EntityType::Line(line)).unwrap()
        };

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::ExecuteCommand {
                lisp_call: "(c:entsel_test_flow)".to_string(),
                worker_tx,
                done: done_tx,
            }),
        });

        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => task(&mut mock_host),
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(r) => panic!("Expected InteractiveStarted, got Finished({:?})", r),
            }
        };

        assert!(cmd.needs_object_pick(), "entsel must set needs_object_pick() = true");
        assert!(!cmd.is_selection_gathering(), "entsel should not use box selection gathering");
        assert_eq!(cmd.prompt(), "\nSelect entity: ");

        let step = cmd.on_object_pick(picked_handle, [15.5, 25.25, 0.0]);
        done_rx.recv().unwrap();

        match step {
            ocs_plugin_api::host::CommandStep::Dispatch(dispatched) => {
                assert!(dispatched.contains("_MOVE"), "Expected _MOVE dispatch, got: {dispatched}");
                let hex_h = format!("{:X}", picked_handle.value());
                assert!(
                    dispatched.contains(&hex_h),
                    "Expected hex handle {hex_h} in MOVE dispatch, got: {dispatched}"
                );
                assert!(
                    dispatched.contains("15.5000,25.2500,0.0000"),
                    "Expected pick point in MOVE dispatch, got: {dispatched}"
                );
            }
            other => panic!("Expected CommandStep::Dispatch, got {:?}", other),
        }

        // Verify worker finished cleanly and updated internal state/vars as expected
        let (tx_chk, rx_chk) = std::sync::mpsc::channel();
        let (wtx_chk, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::Eval {
                code: "(list _test_updated _test_lastp (equal _test_pt _test_lastp))".to_string(),
                resp: tx_chk,
                worker_tx: wtx_chk,
            }),
        });
        let chk = rx_chk.recv().unwrap().unwrap().unwrap();
        assert!(
            chk.contains("\"MODIFIED_LAYER\"") && chk.contains("15.5 25.25 0") && chk.contains("T"),
            "Expected entmod layer update and LASTPOINT sync, got: {chk}"
        );

        // 2. Test interactive entdel removes entity from active snapshot
        let (worker_tx_del, worker_rx_del) = std::sync::mpsc::channel();
        let (done_tx_del, done_rx_del) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::ExecuteCommand {
                lisp_call: "(c:entsel_del_test)".to_string(),
                worker_tx: worker_tx_del,
                done: done_tx_del,
            }),
        });
        let mut cmd_del = loop {
            match worker_rx_del.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => task(&mut mock_host),
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(r) => panic!("Expected InteractiveStarted, got Finished({:?})", r),
            }
        };
        assert!(cmd_del.needs_object_pick());
        let step_del = cmd_del.on_object_pick(picked_handle, [10.0, 20.0, 0.0]);
        done_rx_del.recv().unwrap();
        assert!(matches!(step_del, ocs_plugin_api::host::CommandStep::Done));

        let (tx_del_chk, rx_del_chk) = std::sync::mpsc::channel();
        let (wtx_del_chk, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::Eval {
                code: "_test_deleted".to_string(),
                resp: tx_del_chk,
                worker_tx: wtx_del_chk,
            }),
        });
        assert_eq!(rx_del_chk.recv().unwrap().unwrap().as_deref(), Some("T"));

        // 3. Test pressing Enter on entsel returns nil cleanly without falling back to entlast
        let (worker_tx2, worker_rx2) = std::sync::mpsc::channel();
        let (done_tx2, done_rx2) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::ExecuteCommand {
                lisp_call: "(c:entsel_cancel_test)".to_string(),
                worker_tx: worker_tx2,
                done: done_tx2,
            }),
        });

        let mut cmd2 = loop {
            match worker_rx2.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => task(&mut mock_host),
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(r) => panic!("Expected InteractiveStarted, got Finished({:?})", r),
            }
        };
        assert!(cmd2.needs_object_pick());
        assert_eq!(cmd2.prompt(), "\nPick or press Enter: ");

        let step2 = cmd2.on_enter();
        done_rx2.recv().unwrap();
        assert!(
            command_ended(&step2),
            "Expected CommandStep::Done after Enter cancel, got {:?}",
            step2
        );

        let (tx_chk2, rx_chk2) = std::sync::mpsc::channel();
        let (wtx_chk2, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::WithTab {
            tab_id,
            task: Box::new(LispTask::Eval {
                code: "(null _test_cancel_sel)".to_string(),
                resp: tx_chk2,
                worker_tx: wtx_chk2,
            }),
        });
        assert_eq!(rx_chk2.recv().unwrap().unwrap().as_deref(), Some("T"));

        clean_tab_state(tab_id);
    }

    #[test]
    fn test_dual_mode_command_resolution_and_initdia() {
        let bridge = std::sync::Arc::new(std::sync::Mutex::new(OpenCadRustBridge::new()));
        let mut env = lisp_cad::create_cad_env(bridge.clone());

        // 1. (command "LAYER" "M" "TestLay" "") automatically resolves to "-LAYER M TestLay ENTER" and updates CLAYER
        lisp_core::eval_str(r#"(command "LAYER" "M" "TestLay" "")"#, &mut env).unwrap();
        {
            let br = bridge.lock().unwrap();
            assert_eq!(br.executed_commands.last().map(|s| s.as_str()), Some("-LAYER M TestLay ENTER"));
            assert_eq!(br.get_var("CLAYER").unwrap(), DxfValue::Str("TestLay".into()));
            assert_eq!(br.active_host_command, None);
        }

        // 2. Prefix stripping for both "_.layer" and "_.-layer"
        lisp_core::eval_str(r#"(command "_.layer" "_m" "TestLay2" "")"#, &mut env).unwrap();
        assert_eq!(
            bridge.lock().unwrap().executed_commands.last().map(|s| s.as_str()),
            Some("-LAYER _m TestLay2 ENTER")
        );
        lisp_core::eval_str(r#"(command "_.-layer" "_m" "TestLay3" "")"#, &mut env).unwrap();
        assert_eq!(
            bridge.lock().unwrap().executed_commands.last().map(|s| s.as_str()),
            Some("-LAYER _m TestLay3 ENTER")
        );

        // 3. (initdia 1) preserves GUI dialog command once, then resets init_dia_flag
        lisp_core::eval_str(r#"(initdia 1)"#, &mut env).unwrap();
        assert!(bridge.lock().unwrap().init_dia_flag);
        lisp_core::eval_str(r#"(command "LAYER")"#, &mut env).unwrap();
        {
            let br = bridge.lock().unwrap();
            assert_eq!(br.executed_commands.last().map(|s| s.as_str()), Some("LAYER"));
            assert!(!br.init_dia_flag);
        }
        lisp_core::eval_str(r#"(command "LAYER" "M" "AfterDia" "")"#, &mut env).unwrap();
        assert_eq!(
            bridge.lock().unwrap().executed_commands.last().map(|s| s.as_str()),
            Some("-LAYER M AfterDia ENTER")
        );

        // 4. Progressive multi-call (command "LAYER") followed by (command "M" "SplitLay" "")
        lisp_core::eval_str(r#"(command "LAYER")"#, &mut env).unwrap();
        {
            let br = bridge.lock().unwrap();
            assert_eq!(br.executed_commands.last().map(|s| s.as_str()), Some("-LAYER"));
            assert_eq!(br.active_host_command.as_deref(), Some("-LAYER"));
        }
        lisp_core::eval_str(r#"(command "M" "SplitLay" "")"#, &mut env).unwrap();
        {
            let br = bridge.lock().unwrap();
            assert_eq!(br.executed_commands.last().map(|s| s.as_str()), Some("M SplitLay ENTER"));
            assert_eq!(br.get_var("CLAYER").unwrap(), DxfValue::Str("SplitLay".into()));
            assert_eq!(br.active_host_command, None);
        }

        // 5. (vl-cmdf) returns T and resolves INSERT -> -INSERT
        let vl_res = lisp_core::eval_str(r#"(vl-cmdf "INSERT" "MyBlock" '(0 0) 1 1 0)"#, &mut env).unwrap();
        assert_eq!(vl_res, lisp_core::Value::T);
        assert_eq!(
            bridge.lock().unwrap().executed_commands.last().map(|s| s.as_str()),
            Some("-INSERT MyBlock 0.0000,0.0000 1 1 0")
        );

        // 6. (command-s) returns nil and resolves PURGE -> -PURGE
        let cs_res = lisp_core::eval_str(r#"(command-s "PURGE" "A" "*" "N")"#, &mut env).unwrap();
        assert_eq!(cs_res, lisp_core::Value::Nil);
        assert_eq!(
            bridge.lock().unwrap().executed_commands.last().map(|s| s.as_str()),
            Some("-PURGE A * N")
        );

        // 7. Empty (command) cancels an open -LAYER session with ESC
        lisp_core::eval_str(r#"(command "LA" "M" "UnclosedLayer")"#, &mut env).unwrap();
        assert_eq!(bridge.lock().unwrap().active_host_command.as_deref(), Some("-LAYER"));
        lisp_core::eval_str(r#"(command)"#, &mut env).unwrap();
        {
            let br = bridge.lock().unwrap();
            assert_eq!(br.executed_commands.last().map(|s| s.as_str()), Some("ESC"));
            assert_eq!(br.active_host_command, None);
        }
    }

    #[test]
    fn test_command_line_get_functions_without_dialogs() {
        use ocs_plugin_api::host::{CommandStep, InteractiveInputKind};

        let code = r#"
            (defun c:cmdgettest (/ pt s1 s2 s3 n r k)
              (setq pt (getpoint "\nFirst corner: "))
              (setq s1 (getstring "\nWord: "))
              (setq s2 (getstring T "\nSentence: "))
              (setq s3 (getstring "\nDefault: "))
              (initget 1)
              (setq n (getint "\nCount: "))
              (initget 6)
              (setq r (getreal "\nRadius: "))
              (initget 1 "Red Green Blue")
              (setq k (getkword "\nColor: "))
              (princ (strcat "\nRESULT=" s1 "|" s2 "|" s3 "|" (itoa n) "|" (rtos r 2 2) "|" k))
              (princ)
            )
            (defun c:cmdgetstrdirect (/ s)
              (setq s (getstring T "\nDirect: "))
              (princ (strcat "\nDIRECT=" s))
              (princ)
            )
        "#;

        let (tx, rx) = std::sync::mpsc::channel();
        let (worker_tx, _) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::Eval {
            code: code.to_string(),
            resp: tx,
            worker_tx,
        });
        let res = rx.recv().unwrap();
        assert!(res.is_ok(), "eval should succeed: {:?}", res);

        let (worker_tx, worker_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:cmdgettest)".to_string(),
            worker_tx,
            done: done_tx,
        });

        let mut mock_host = crate::context::DebugHost::new(None);
        let mut cmd = loop {
            match worker_rx.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => task(&mut mock_host),
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(r) => panic!("Unexpected early finish: {:?}", r),
            }
        };

        // 1. getpoint -> Point
        assert!(cmd.modal_dialog().is_none(), "getpoint must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::Point);
        assert!(matches!(cmd.on_point([1.0, 2.0, 0.0]), CommandStep::NeedPoint));

        // 2. getstring (has_spaces = false) -> FreeText so casing & literal text survive before space truncation
        assert!(cmd.modal_dialog().is_none(), "getstring must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::FreeText);
        assert!(matches!(cmd.on_text_input("Hello World"), CommandStep::NeedPoint));

        // 3. getstring (has_spaces = true) -> FreeText
        assert!(cmd.modal_dialog().is_none(), "getstring T must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::FreeText);
        assert!(matches!(cmd.on_text_input("Mixed Case Words"), CommandStep::NeedPoint));

        // 4. getstring bare Enter -> ""
        assert!(cmd.modal_dialog().is_none(), "getstring bare Enter must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::FreeText);
        assert!(matches!(cmd.on_enter(), CommandStep::NeedPoint));

        // 5. initget 1 + getint -> SingleToken with invalid retry & empty retry
        assert!(cmd.modal_dialog().is_none(), "getint must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(matches!(cmd.on_text_input("abc"), CommandStep::NeedPoint));
        assert!(cmd.modal_dialog().is_none());
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(matches!(cmd.on_enter(), CommandStep::NeedPoint));
        assert!(cmd.modal_dialog().is_none());
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(matches!(cmd.on_text_input("7"), CommandStep::NeedPoint));

        // 6. initget 6 + getreal -> SingleToken with zero rejected
        assert!(cmd.modal_dialog().is_none(), "getreal must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(matches!(cmd.on_text_input("0"), CommandStep::NeedPoint));
        assert!(cmd.modal_dialog().is_none());
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(matches!(cmd.on_text_input("2.5"), CommandStep::NeedPoint));

        // 7. initget 1 keywords + getkword -> SingleToken with inline options prompt
        assert!(cmd.modal_dialog().is_none(), "getkword must not open a dialog");
        assert_eq!(cmd.input_kind(), InteractiveInputKind::SingleToken);
        assert!(
            cmd.prompt().contains("[Red/Green/Blue]"),
            "Expected keyword options in prompt, got {:?}",
            cmd.prompt()
        );
        let final_step = cmd.on_text_input("g");
        match final_step {
            CommandStep::Dispatch(dispatched) => {
                let ticket = dispatched
                    .strip_prefix(FLUSH_OUTPUT_COMMAND)
                    .expect("expected LSPFLUSH dispatch")
                    .trim();
                flush_pending_command_output(&mut mock_host, ticket);
            }
            other => panic!("Expected Dispatch(LSPFLUSH ...), got {:?}", other),
        }
        done_rx.recv().unwrap();

        let out = mock_host.captured_output.lock().unwrap().clone();
        assert!(
            out.iter().any(|l| l.contains("Requires an integer value.")),
            "Expected retry hint for invalid getint in output: {:?}",
            out
        );
        assert!(
            out.iter().any(|l| l.contains("Value must be positive and nonzero.")),
            "Expected retry hint for zero getreal in output: {:?}",
            out
        );
        assert!(
            out.iter().any(|l| l == "RESULT=Hello|Mixed Case Words||7|2.50|Green"),
            "Expected formatted result line in output: {:?}",
            out
        );

        // 8. Starting a command directly with (getstring ...) must start in FreeText with no dialog
        let (worker_tx2, worker_rx2) = std::sync::mpsc::channel();
        let (done_tx2, done_rx2) = std::sync::mpsc::channel();
        let _ = get_task_tx().send(LispTask::ExecuteCommand {
            lisp_call: "(c:cmdgetstrdirect)".to_string(),
            worker_tx: worker_tx2,
            done: done_tx2,
        });
        let mut mock_host2 = crate::context::DebugHost::new(None);
        let mut cmd2 = loop {
            match worker_rx2.recv().unwrap() {
                WorkerMsg::HostCall(mut task) => task(&mut mock_host2),
                WorkerMsg::InteractiveStarted(c) => break c,
                WorkerMsg::Finished(r) => panic!("Unexpected early finish: {:?}", r),
            }
        };
        assert!(cmd2.modal_dialog().is_none(), "Direct getstring must not open a dialog");
        assert_eq!(cmd2.input_kind(), InteractiveInputKind::FreeText);
        match cmd2.on_text_input("Direct Hello") {
            CommandStep::Dispatch(dispatched) => {
                let ticket = dispatched
                    .strip_prefix(FLUSH_OUTPUT_COMMAND)
                    .expect("expected LSPFLUSH dispatch")
                    .trim();
                flush_pending_command_output(&mut mock_host2, ticket);
            }
            other => panic!("Expected Dispatch(LSPFLUSH ...), got {:?}", other),
        }
        done_rx2.recv().unwrap();
        let out2 = mock_host2.captured_output.lock().unwrap().clone();
        assert!(
            out2.iter().any(|l| l == "DIRECT=Direct Hello"),
            "Expected DIRECT output, got: {:?}",
            out2
        );
    }
}



