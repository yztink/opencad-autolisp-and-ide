//! ============================================================================
//! DO NOT EDIT — PYTHON SUPPORT IS SHELVED / FROZEN.
//! Do NOT modify this file or add features to Python to mirror OpenCAD LISP.
//! All active OpenCAD LISP development belongs strictly in:
//!   - ../opencad-lisp-engine (crates/lisp-core, crates/lisp-cad)
//!   - src/rust_lisp.rs
//! ============================================================================

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::AtomicUsize;
use pyo3::prelude::*;

pub use crate::ide::protocol::{DebugCommand, DebugEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    None,
    StepInto,
    StepOver(usize), // target call depth
    StepOut(usize),  // target call depth
}

pub struct DebugSession {
    pub breakpoints: Arc<Mutex<HashMap<String, HashSet<usize>>>>,
    pub step_state: Arc<Mutex<StepState>>,
    pub call_depth: Arc<AtomicUsize>,
    pub stop_requested: Arc<std::sync::atomic::AtomicBool>,
    pub event_tx: Sender<DebugEvent>,
    pub cmd_tx: Sender<DebugCommand>,
    pub cmd_rx: Arc<Mutex<Receiver<DebugCommand>>>,
}

impl DebugSession {
    pub fn new_multi(breakpoints: HashMap<String, HashSet<usize>>) -> (Self, Sender<DebugCommand>, Receiver<DebugEvent>) {
        let (cmd_tx, cmd_rx) = channel();
        let (event_tx, event_rx) = channel();
        let session = Self {
            breakpoints: Arc::new(Mutex::new(breakpoints)),
            step_state: Arc::new(Mutex::new(StepState::None)),
            call_depth: Arc::new(AtomicUsize::new(0)),
            stop_requested: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            event_tx,
            cmd_tx: cmd_tx.clone(),
            cmd_rx: Arc::new(Mutex::new(cmd_rx)),
        };
        (session, cmd_tx, event_rx)
    }

    pub fn new(breakpoints: HashSet<usize>) -> (Self, Sender<DebugCommand>, Receiver<DebugEvent>) {
        let mut map = HashMap::new();
        map.insert("*".to_string(), breakpoints);
        Self::new_multi(map)
    }

    pub fn set_breakpoints(&self, bps: HashSet<usize>) {
        if let Ok(mut lock) = self.breakpoints.lock() {
            lock.insert("*".to_string(), bps);
        }
    }
}

// Global active debug session during execution
static ACTIVE_SESSION: Mutex<Option<Arc<DebugSession>>> = Mutex::new(None);

pub fn set_active_session(session: Option<Arc<DebugSession>>) {
    if let Ok(mut lock) = ACTIVE_SESSION.lock() {
        *lock = session;
    }
}

pub fn get_active_session() -> Option<Arc<DebugSession>> {
    ACTIVE_SESSION.lock().ok().and_then(|lock| lock.clone())
}

/// Helper to parse breakpoints specification (e.g. "file1.py:5,10;file2.py:12" or "5,10").
pub fn parse_bps_spec(spec: &str, default_file: &str) -> HashMap<String, HashSet<usize>> {
    let mut map: HashMap<String, HashSet<usize>> = HashMap::new();
    if spec == "none" || spec.trim().is_empty() {
        return map;
    }
    for entry in spec.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((fname, lines_part)) = entry.split_once(':') {
            let mut set = HashSet::new();
            for item in lines_part.split(',') {
                if let Ok(line) = item.trim().parse::<usize>() {
                    set.insert(line);
                }
            }
            if !set.is_empty() {
                map.insert(fname.trim().to_string(), set);
            }
        } else {
            let mut set = HashSet::new();
            for item in entry.split(',') {
                if let Ok(line) = item.trim().parse::<usize>() {
                    set.insert(line);
                }
            }
            if !set.is_empty() {
                map.insert(default_file.to_string(), set);
            }
        }
    }
    map
}

/// Called by Python trace hook to check if execution should pause at this line.
/// Returns: "continue", "step_over", "step_into", "step_out", or "stop"
#[pyfunction]
pub fn _debug_check_line(
    filename: String,
    lineno: usize,
    locals_data: Vec<(String, String, String)>,
    stack_data: Vec<(String, usize, String)>,
    depth: usize,
) -> PyResult<String> {
    let session = match get_active_session() {
        Some(s) => s,
        None => return Ok("continue".to_string()),
    };

    if session.stop_requested.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok("stop".to_string());
    }

    if filename.starts_with('<') || filename == "<string>" {
        return Ok("continue".to_string());
    }

    let is_bp = {
        let bps_map = session.breakpoints.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;
        let mut hit = false;
        let p_fname = std::path::Path::new(&filename);
        let base_fname = p_fname.file_name().and_then(|n| n.to_str()).unwrap_or(&filename);
        let stem_fname = p_fname.file_stem().and_then(|n| n.to_str()).unwrap_or(base_fname);

        for (file_key, lines) in bps_map.iter() {
            let p_key = std::path::Path::new(file_key);
            let base_key = p_key.file_name().and_then(|n| n.to_str()).unwrap_or(file_key);
            let stem_key = p_key.file_stem().and_then(|n| n.to_str()).unwrap_or(base_key);

            if file_key == "*"
                || filename.eq_ignore_ascii_case(file_key)
                || base_fname.eq_ignore_ascii_case(base_key)
                || stem_fname.eq_ignore_ascii_case(stem_key)
                || filename.ends_with(&format!("/{}", file_key))
                || filename.ends_with(&format!("\\{}", file_key))
            {
                if lines.contains(&lineno) {
                    hit = true;
                    break;
                }
            }
        }
        hit
    };

    let current_depth = if depth > 0 { depth } else { stack_data.len() };
    let is_stepping = {
        let state = session.step_state.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;
        match *state {
            StepState::None => false,
            StepState::StepInto => true,
            StepState::StepOver(target_depth) => current_depth <= target_depth,
            StepState::StepOut(target_depth) => current_depth < target_depth,
        }
    };

    if !is_bp && !is_stepping {
        return Ok("continue".to_string());
    }

    // Reset step state
    if let Ok(mut lock) = session.step_state.lock() {
        *lock = StepState::None;
    }

    eprintln!("[plugin] [DEBUG] Pausing at {}:{} (depth={}), sending Paused event", filename, lineno, current_depth);

    // Send pause event to IDE
    let _ = session.event_tx.send(DebugEvent::Paused {
        filename,
        line: lineno,
        locals: locals_data,
        stack: stack_data,
    });

    // Wait for stepping command from IDE
    let rx = Arc::clone(&session.cmd_rx);
    let cmd = loop {
        let lock = rx.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;
        match lock.recv() {
            Ok(DebugCommand::SetBreakpoints(bps)) => {
                session.set_breakpoints(bps.into_iter().collect());
            }
            Ok(c) => break c,
            Err(_) => break DebugCommand::Stop,
        }
    };

    eprintln!("[plugin] [DEBUG] Resuming execution with command: {:?}", cmd);

    match cmd {
        DebugCommand::Continue => Ok("continue".to_string()),
        DebugCommand::StepInto => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepInto;
            }
            Ok("step_into".to_string())
        }
        DebugCommand::StepOver => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepOver(current_depth);
            }
            Ok("step_over".to_string())
        }
        DebugCommand::StepOut => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepOut(current_depth);
            }
            Ok("step_out".to_string())
        }
        DebugCommand::Stop => Ok("stop".to_string()),
        DebugCommand::SetBreakpoints(_) => Ok("continue".to_string()),
    }
}

/// Called by Python trace hook when an exception is raised in user script.
/// Automatically pauses at the line causing the error, routes the error message
/// to the OCIDE console, and allows inspection of variables and stepping.
#[pyfunction]
pub fn _debug_check_exception(
    filename: String,
    lineno: usize,
    exc_type: String,
    exc_msg: String,
    locals_data: Vec<(String, String, String)>,
    stack_data: Vec<(String, usize, String)>,
    depth: usize,
) -> PyResult<String> {
    let session = match get_active_session() {
        Some(s) => s,
        None => return Ok("continue".to_string()),
    };

    if session.stop_requested.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok("stop".to_string());
    }

    if filename.starts_with('<') || filename == "<string>" {
        return Ok("continue".to_string());
    }

    let p_fname = std::path::Path::new(&filename);
    let base_fname = p_fname.file_name().and_then(|n| n.to_str()).unwrap_or(&filename);

    let err_header = format!("\n[Error at {}:{}] {}: {}\n", base_fname, lineno, exc_type, exc_msg);
    eprintln!("[plugin] [DEBUG] Exception at {}:{} - {}: {}", filename, lineno, exc_type, exc_msg);

    // Route error message directly to OCIDE console
    crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
        text: err_header.clone(),
        is_error: true,
    });

    // Also push error to CAD command line
    if let Some(s) = crate::context::get_global_sender() {
        use ocs_plugin_api::ipc::protocol::PluginRequest;
        let _ = s.request(PluginRequest::PushError(err_header));
    }

    let current_depth = if depth > 0 { depth } else { stack_data.len() };

    // Reset step state
    if let Ok(mut lock) = session.step_state.lock() {
        *lock = StepState::None;
    }

    // Send pause event to IDE
    let _ = session.event_tx.send(DebugEvent::Paused {
        filename,
        line: lineno,
        locals: locals_data,
        stack: stack_data,
    });

    // Wait for stepping / continue / stop command from IDE
    let rx = Arc::clone(&session.cmd_rx);
    let cmd = loop {
        let lock = rx.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;
        match lock.recv() {
            Ok(DebugCommand::SetBreakpoints(bps)) => {
                session.set_breakpoints(bps.into_iter().collect());
            }
            Ok(c) => break c,
            Err(_) => break DebugCommand::Stop,
        }
    };

    eprintln!("[plugin] [DEBUG] Resuming after exception with command: {:?}", cmd);

    match cmd {
        DebugCommand::Continue => Ok("continue".to_string()),
        DebugCommand::StepInto => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepInto;
            }
            Ok("step_into".to_string())
        }
        DebugCommand::StepOver => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepOver(current_depth);
            }
            Ok("step_over".to_string())
        }
        DebugCommand::StepOut => {
            if let Ok(mut lock) = session.step_state.lock() {
                *lock = StepState::StepOut(current_depth);
            }
            Ok("step_out".to_string())
        }
        DebugCommand::Stop => Ok("stop".to_string()),
        DebugCommand::SetBreakpoints(_) => Ok("continue".to_string()),
    }
}

/// Run code under debug tracing with bidirectional TCP socket bridge to IDE.
pub fn run_debug_with_tcp_bridge(
    port: u16,
    breakpoints: HashSet<usize>,
    filename: String,
    code: String,
    initial_step: bool,
    sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>,
) -> bool {
    crate::context::set_debug_session_context(sender);
    let (session, cmd_tx, event_rx) = DebugSession::new(breakpoints);
    if initial_step {
        if let Ok(mut lock) = session.step_state.lock() {
            *lock = StepState::StepInto;
        }
    }
    let session = Arc::new(session);

    let mut stream_opt = None;
    for _ in 0..20 {
        if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            stream_opt = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let stream = match stream_opt {
        Some(s) => s,
        None => {
            eprintln!("[plugin] [PYDEBUG] Failed to connect to IDE debug server on port {port}");
            return false;
        }
    };
    let _ = stream.set_nodelay(true);

    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[plugin] [PYDEBUG] Failed to clone debug socket: {e}");
            return false;
        }
    };
    let mut writer_stream = stream;

    // Thread 1: Send DebugEvent -> TCP socket
    std::thread::spawn(move || {
        use std::io::Write;
        while let Ok(event) = event_rx.recv() {
            let is_terminal = matches!(event, DebugEvent::Finished | DebugEvent::Error(_));
            if let Ok(json) = serde_json::to_string(&event) {
                if writer_stream.write_all(json.as_bytes()).is_err()
                    || writer_stream.write_all(b"\n").is_err()
                    || writer_stream.flush().is_err()
                {
                    break;
                }
            }
            if is_terminal {
                break;
            }
        }
    });

    // Thread 2: Read TCP socket -> DebugCommand (send to cmd_tx)
    let cmd_tx_clone = cmd_tx.clone();
    let session_for_reader = Arc::clone(&session);
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(reader_stream);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(cmd) = serde_json::from_str::<DebugCommand>(line) {
                let is_stop = cmd == DebugCommand::Stop;
                if is_stop {
                    session_for_reader.stop_requested.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = cmd_tx_clone.send(cmd);
                if is_stop {
                    break;
                }
            }
        }
    });

    let res = crate::py_runtime::execute_code_debug_raw(&code, &filename, Arc::clone(&session));
    crate::context::clear_debug_session_context();
    match res {
        Ok(_) => {
            let _ = session.event_tx.send(DebugEvent::Finished);
        }
        Err(e) => {
            let _ = session.event_tx.send(DebugEvent::Error(e));
        }
    }
    true
}

/// Run an underlying CAD command (e.g. PYCMD MY_CMD or PY draw_grid) under debug tracing
/// with bidirectional TCP socket bridge to IDE.
pub fn run_debug_cmd_with_tcp_bridge(
    port: u16,
    breakpoints: HashMap<String, HashSet<usize>>,
    cmd_to_run: String,
    sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>,
) -> bool {
    crate::context::set_debug_session_context(sender);
    let (session, cmd_tx, event_rx) = DebugSession::new_multi(breakpoints);
    let session = Arc::new(session);

    let mut stream_opt = None;
    for _ in 0..25 {
        if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            stream_opt = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }

    let stream = match stream_opt {
        Some(s) => s,
        None => {
            eprintln!("[plugin] [PYDEBUG] Failed to connect to IDE debug server on port {port}");
            crate::context::clear_debug_session_context();
            return false;
        }
    };
    let _ = stream.set_nodelay(true);

    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[plugin] [PYDEBUG] Failed to clone debug socket: {e}");
            crate::context::clear_debug_session_context();
            return false;
        }
    };
    let mut writer_stream = stream;

    // Thread 1: Send DebugEvent -> TCP socket
    std::thread::spawn(move || {
        use std::io::Write;
        while let Ok(event) = event_rx.recv() {
            let is_terminal = matches!(event, DebugEvent::Finished | DebugEvent::Error(_));
            if let Ok(json) = serde_json::to_string(&event) {
                if writer_stream.write_all(json.as_bytes()).is_err()
                    || writer_stream.write_all(b"\n").is_err()
                    || writer_stream.flush().is_err()
                {
                    break;
                }
            }
            if is_terminal {
                break;
            }
        }
    });

    // Thread 2: Read TCP socket -> DebugCommand (send to cmd_tx)
    let cmd_tx_clone = cmd_tx.clone();
    let session_for_reader = Arc::clone(&session);
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(reader_stream);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(cmd) = serde_json::from_str::<DebugCommand>(line) {
                let is_stop = cmd == DebugCommand::Stop;
                if is_stop {
                    session_for_reader.stop_requested.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = cmd_tx_clone.send(cmd);
                if is_stop {
                    break;
                }
            }
        }
    });

    // Execute the target command with tracer installed
    set_active_session(Some(Arc::clone(&session)));

    let exec_res = Python::with_gil(|py| -> PyResult<()> {
        crate::py_runtime::setup_python_environment(py)?;
        let globals = crate::py_runtime::get_or_create_session_globals(py)?;

        // Install tracer
        py.run(&std::ffi::CString::new(PYTHON_TRACER_BOOTSTRAP).unwrap(), Some(&globals), Some(&globals))?;
        py.run(&std::ffi::CString::new("_tracer = _make_tracer()\nsys.settrace(_tracer)\n").unwrap(), Some(&globals), Some(&globals))?;

        // Dispatch underlying command
        let run_result = if let Some(rest) = cmd_to_run.strip_prefix("PYCMD ") {
            let (cmd_name, args) = match rest.split_once(char::is_whitespace) {
                Some((c, a)) => (c, a.split_whitespace().map(|s| s.to_string()).collect()),
                None => (rest, Vec::new()),
            };
            crate::py_runtime::execute_pycmd_raw(cmd_name, &args)
        } else if let Some(rest) = cmd_to_run.strip_prefix("PY ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let script_name = parts.first().copied().unwrap_or("");
            let args: Vec<String> = parts.iter().skip(1).map(|s| s.to_string()).collect();
            let cmds = crate::py_module::get_registered_commands();
            if cmds.contains_key(&script_name.to_uppercase()) {
                crate::py_runtime::execute_pycmd_raw(&script_name.to_uppercase(), &args)
            } else {
                crate::py_runtime::execute_script_raw(script_name, &args)
            }
        } else if let Some(code) = cmd_to_run.strip_prefix("PYRUN ") {
            crate::py_runtime::execute_code_raw(code)
        } else {
            crate::py_runtime::execute_code_raw(&cmd_to_run)
        };

        // Always uninstall tracer
        if let Ok(sys) = py.import("sys") {
            let _ = sys.call_method1("settrace", (py.None(),));
        }
        crate::py_runtime::flush_python_output(py)?;

        match run_result {
            Ok(()) => Ok(()),
            Err(e) => {
                if e.contains("_DebugQuit") {
                    Ok(())
                } else {
                    Err(pyo3::exceptions::PyRuntimeError::new_err(e))
                }
            }
        }
    });

    set_active_session(None);
    crate::context::clear_debug_session_context();

    match exec_res {
        Ok(_) => {
            let _ = session.event_tx.send(DebugEvent::Finished);
        }
        Err(e) => {
            let msg = Python::with_gil(|py| {
                let tb = e.traceback(py);
                if let Some(tb) = tb {
                    match tb.format() {
                        Ok(fmt) => format!("{}\n{}", e, fmt),
                        Err(_) => format!("{}", e),
                    }
                } else {
                    format!("{}", e)
                }
            });
            let _ = session.event_tx.send(DebugEvent::Error(msg));
        }
    }
    true
}

/// Python bootstrap code that installs the trace hook using `sys.settrace`
pub const PYTHON_TRACER_BOOTSTRAP: &str = r#"
import sys
import inspect

class _DebugQuit(Exception):
    """Raised to cleanly stop debugging."""
    pass

def _is_internal(fname):
    if not fname:
        return True
    p = fname.replace('\\', '/').lower()
    if p.startswith('<') or p == '<string>' or '<frozen' in p:
        return True
    if ('/lib/python' in p or '/python3' in p or '/lib64/python' in p or '/system/library/frameworks' in p) and ('/site-packages/' not in p and '/dist-packages/' not in p):
        return True
    return False

def _make_tracer(tracked_filename=None):
    call_depth = 0
    last_exc_id = None
    import dis
    parse_exc_table = getattr(dis, '_parse_exception_table', None)
    
    def _trace(frame, event, arg):
        nonlocal call_depth, last_exc_id
        
        if event == 'call':
            call_depth += 1
            return _trace
        elif event == 'return':
            call_depth = max(0, call_depth - 1)
            return _trace
        elif event == 'line':
            fname = frame.f_code.co_filename
            if _is_internal(fname):
                return _trace
            if tracked_filename and tracked_filename != "*":
                tf = tracked_filename.replace('\\', '/').split('/')[-1].lower()
                fn = fname.replace('\\', '/').split('/')[-1].lower()
                if tf != fn and tf.split('.')[0] != fn.split('.')[0]:
                    return _trace
                
            lineno = frame.f_lineno
            
            # Collect local variables (name, type, repr)
            locals_list = []
            for k, v in list(frame.f_locals.items()):
                if k.startswith('__') and k.endswith('__'):
                    continue
                try:
                    t_name = type(v).__name__
                    r_val = repr(v)
                    if len(r_val) > 120:
                        r_val = r_val[:117] + "..."
                    locals_list.append((str(k), str(t_name), str(r_val)))
                except Exception:
                    locals_list.append((str(k), "unknown", "<error formatting>"))
            locals_list.sort(key=lambda x: x[0])
            
            # Collect stack frames
            stack_list = []
            curr = frame
            while curr:
                c_fname = curr.f_code.co_filename
                c_func = curr.f_code.co_name
                c_line = curr.f_lineno
                stack_list.append((str(c_func), int(c_line), str(c_fname)))
                curr = curr.f_back
                
            import opencad
            action = opencad._debug_check_line(fname, lineno, locals_list, stack_list, len(stack_list))
            if action == 'stop':
                raise _DebugQuit("Debugging terminated by user.")

        elif event == 'exception':
            exc_type, exc_value, tb = arg
            if exc_type is _DebugQuit or exc_type is GeneratorExit or exc_type is StopIteration:
                return _trace

            # If this exception is handled by an enclosing try...except block in the stack,
            # let Python's exception handling proceed without pausing or reporting an error.
            if parse_exc_table:
                handled = False
                curr = frame
                while curr:
                    try:
                        for entry in parse_exc_table(curr.f_code):
                            if entry.start <= curr.f_lasti < entry.end:
                                handled = True
                                break
                    except Exception:
                        pass
                    if handled:
                        break
                    curr = curr.f_back
                if handled:
                    return _trace

            curr_exc_id = id(exc_value)
            if curr_exc_id == last_exc_id:
                return _trace
            last_exc_id = curr_exc_id

            fname = frame.f_code.co_filename
            if _is_internal(fname):
                return _trace
            if tracked_filename and tracked_filename != "*":
                tf = tracked_filename.replace('\\', '/').split('/')[-1].lower()
                fn = fname.replace('\\', '/').split('/')[-1].lower()
                if tf != fn and tf.split('.')[0] != fn.split('.')[0]:
                    return _trace


            lineno = frame.f_lineno

            locals_list = []
            for k, v in list(frame.f_locals.items()):
                if k.startswith('__') and k.endswith('__'):
                    continue
                try:
                    t_name = type(v).__name__
                    r_val = repr(v)
                    if len(r_val) > 120:
                        r_val = r_val[:117] + "..."
                    locals_list.append((str(k), str(t_name), str(r_val)))
                except Exception:
                    locals_list.append((str(k), "unknown", "<error formatting>"))
            locals_list.sort(key=lambda x: x[0])

            stack_list = []
            curr = frame
            while curr:
                c_fname = curr.f_code.co_filename
                c_func = curr.f_code.co_name
                c_line = curr.f_lineno
                stack_list.append((str(c_func), int(c_line), str(c_fname)))
                curr = curr.f_back

            exc_name = getattr(exc_type, '__name__', str(exc_type))
            exc_str = str(exc_value)

            import opencad
            action = opencad._debug_check_exception(fname, lineno, exc_name, exc_str, locals_list, stack_list, len(stack_list))
            if action == 'stop':
                raise _DebugQuit("Debugging terminated by user.")

        return _trace

    return _trace
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_session_lifecycle() {
        let mut bps = HashSet::new();
        bps.insert(10);
        bps.insert(20);

        let (session, cmd_tx, event_rx) = DebugSession::new(bps);
        let session = Arc::new(session);
        set_active_session(Some(Arc::clone(&session)));

        // Spawn a thread simulating Python hitting line 10
        let handle = std::thread::spawn(move || {
            let res = _debug_check_line(
                "test.py".to_string(),
                10,
                vec![("x".to_string(), "int".to_string(), "42".to_string())],
                vec![("test_fn".to_string(), 10, "test.py".to_string())],
                1,
            );
            res.expect("debug check")
        });

        // IDE receives paused event
        let ev = event_rx.recv().expect("receive pause event");
        match ev {
            DebugEvent::Paused { line, locals, .. } => {
                assert_eq!(line, 10);
                assert_eq!(locals.len(), 1);
                assert_eq!(locals[0].0, "x");
                assert_eq!(locals[0].2, "42");
            }
            _ => panic!("expected Paused event"),
        }

        // IDE sends StepOver command
        cmd_tx.send(DebugCommand::StepOver).expect("send step over");
        let action = handle.join().expect("thread join");
        assert_eq!(action, "step_over");

        set_active_session(None);
    }
}
