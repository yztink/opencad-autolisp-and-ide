// ============================================================================
// DO NOT EDIT — PYTHON SUPPORT IS SHELVED / FROZEN.
// Do NOT modify this file or add features to Python to mirror OpenCAD LISP.
// All active OpenCAD LISP development belongs strictly in:
//   - ../opencad-lisp-engine (crates/lisp-core, crates/lisp-cad)
//   - src/rust_lisp.rs
// ============================================================================

use std::path::PathBuf;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use ocs_plugin_api::host::HostApi;
use crate::context::{HostGuard, WorkerMsg};
use crate::py_module;

/// Returns the directory for user Python scripts, checking OCS_SCRIPTS_DIR, settings.json, or default.
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
            if let Some(dir) = json.get("settings").and_then(|s| s.get("python_ide_scripts_dir")).and_then(|d| d.as_str()) {
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

/// List all available Python scripts in the scripts directory.
pub fn list_scripts() -> Vec<String> {
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
                if ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
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

/// Returns the persistent global dictionary for the drawing session (__main__.__dict__).
/// Ensures opencad environment and builtins are populated so loaded definitions overwrite in place.
pub fn get_or_create_session_globals<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let sys = py.import("sys")?;
    let sys_modules: Bound<'_, PyDict> = sys.getattr("modules")?.downcast_into()?;
    let main_mod = match sys_modules.get_item("__main__")? {
        Some(m) => m,
        None => {
            let m = PyModule::new(py, "__main__")?;
            sys_modules.set_item("__main__", &m)?;
            m.into_any()
        }
    };
    let globals: Bound<'_, PyDict> = main_mod.getattr("__dict__")?.downcast_into()?;

    let opencad_mod = sys_modules.get_item("opencad")?.expect("opencad module");
    globals.set_item("__name__", "__main__")?;
    globals.set_item("__builtins__", py.import("builtins")?)?;
    globals.set_item("opencad", &opencad_mod)?;
    globals.set_item("oc", &opencad_mod)?;
    globals.set_item("OPENCAD", &opencad_mod)?;
    globals.set_item("OC", &opencad_mod)?;

    for attr in opencad_mod.dir()? {
        let name: String = attr.extract()?;
        if !name.starts_with("__") {
            let val = opencad_mod.getattr(&name)?;
            globals.set_item(&name, &val)?;
            globals.set_item(name.to_uppercase(), &val)?;
        }
    }

    if let Ok(Some(opencad_lisp_mod)) = sys_modules.get_item("opencad_lisp") {
        globals.set_item("opencad_lisp", &opencad_lisp_mod)?;
    }

    Ok(globals)
}

static LOADED_SCRIPTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Store script code loaded or executed from OCIDE into in-memory cache so CAD commands prioritize it.
pub fn store_loaded_script(title: &str, code: &str) {
    let mut map = LOADED_SCRIPTS.lock().unwrap();
    map.insert(title.to_string(), code.to_string());

    let norm_title = title.replace('\\', "/");
    map.insert(norm_title.clone(), code.to_string());
    map.insert(norm_title.to_lowercase(), code.to_string());

    let path = std::path::Path::new(title);
    if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
        map.insert(file_name.to_string(), code.to_string());
        map.insert(file_name.to_lowercase(), code.to_string());
        map.insert(file_name.to_uppercase(), code.to_string());
    }
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        map.insert(stem.to_string(), code.to_string());
        map.insert(stem.to_lowercase(), code.to_string());
        map.insert(stem.to_uppercase(), code.to_string());

        let c_dash = format!("c-{stem}");
        let c_under = format!("c_{stem}");
        map.insert(c_dash.clone(), code.to_string());
        map.insert(c_under.clone(), code.to_string());
        map.insert(c_dash.to_lowercase(), code.to_string());
        map.insert(c_under.to_lowercase(), code.to_string());
        map.insert(c_dash.to_uppercase(), code.to_string());
        map.insert(c_under.to_uppercase(), code.to_string());

        for prefix in &["c-", "c_", "C-", "C_"] {
            if let Some(stripped) = stem.strip_prefix(prefix) {
                map.insert(stripped.to_string(), code.to_string());
                map.insert(stripped.to_lowercase(), code.to_string());
                map.insert(stripped.to_uppercase(), code.to_string());
            }
        }

        // Evict any existing module from sys.modules so subsequent imports reload fresh code
        Python::with_gil(|py| {
            if let Ok(sys) = py.import("sys") {
                if let Ok(modules) = sys.getattr("modules") {
                    for key in &[
                        stem,
                        &stem.to_lowercase(),
                        &stem.to_uppercase(),
                        &c_dash,
                        &c_under,
                        title,
                    ] {
                        let _ = modules.del_item(*key);
                    }
                }
            }
        });
    }
}

/// Look up script code loaded from OCIDE by name, title, or stem.
pub fn get_loaded_script(name: &str) -> Option<String> {
    let map = LOADED_SCRIPTS.lock().unwrap();
    if let Some(code) = map.get(name) {
        return Some(code.clone());
    }
    let lower = name.to_lowercase();
    if let Some(code) = map.get(&lower) {
        return Some(code.clone());
    }
    let upper = name.to_uppercase();
    if let Some(code) = map.get(&upper) {
        return Some(code.clone());
    }

    let norm = name.replace('\\', "/");
    if let Some(code) = map.get(&norm) {
        return Some(code.clone());
    }

    let path = std::path::Path::new(name);
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if let Some(code) = map.get(stem) {
            return Some(code.clone());
        }
        let stem_lower = stem.to_lowercase();
        if let Some(code) = map.get(&stem_lower) {
            return Some(code.clone());
        }
        let stem_upper = stem.to_uppercase();
        if let Some(code) = map.get(&stem_upper) {
            return Some(code.clone());
        }
        for prefix in &["c-", "c_", "C-", "C_"] {
            if let Some(stripped) = stem.strip_prefix(prefix) {
                if let Some(code) = map.get(stripped) {
                    return Some(code.clone());
                }
                if let Some(code) = map.get(&stripped.to_lowercase()) {
                    return Some(code.clone());
                }
            }
        }
    }
    if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
        if let Some(code) = map.get(file_name) {
            return Some(code.clone());
        }
        let fn_lower = file_name.to_lowercase();
        if let Some(code) = map.get(&fn_lower) {
            return Some(code.clone());
        }
    }
    None
}

/// Execute a Python code string without host guard (defaulting to script.py).
pub fn execute_code_raw(code: &str) -> Result<(), String> {
    execute_code_named(code, "script.py")
}

/// Execute a Python code string named by filename without shifting line numbers.
pub fn execute_code_named(code: &str, filename: &str) -> Result<(), String> {
    let (clean_filename, actual_code) = if let Some(stripped) = code.strip_prefix("# file: ") {
        if let Some((fname, rest)) = stripped.split_once('\n') {
            (fname.trim(), rest)
        } else {
            (filename, code)
        }
    } else {
        (filename, code)
    };

    crate::py_sandbox::reset_entity_counter();
    Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let globals = get_or_create_session_globals(py)?;
        globals.set_item("__file__", clean_filename)?;

        let builtins = py.import("builtins")?;
        let compile_fn = builtins.getattr("compile")?;
        let exec_fn = builtins.getattr("exec")?;
        let compiled = compile_fn.call1((actual_code, clean_filename, "exec"))?;
        exec_fn.call1((compiled, &globals, &globals))?;

        flush_python_output(py)?;
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }
        Ok(())
    }).map_err(|err| {
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }
        Python::with_gil(|py| {
            let tb = err.traceback(py);
            let msg = if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("Python Error:\n{}\n{}", err, formatted),
                    Err(_) => format!("Python Error: {}", err),
                }
            } else {
                format!("Python Error: {}", err)
            };
            msg
        })
    })
}

/// Execute a Python code string synchronously with direct host guard (used in tests / sync mode).
pub fn execute_code(code: &str, host: &mut dyn HostApi) -> Result<(), String> {
    let _guard = HostGuard::new(host);
    execute_code_raw(code)
}

/// Execute code under debug tracing with breakpoints and step controls without shifting line numbers.
pub fn execute_code_debug_raw(
    code: &str,
    filename: &str,
    session: std::sync::Arc<crate::py_debug::DebugSession>,
) -> Result<(), String> {
    let (clean_filename, actual_code) = if let Some(stripped) = code.strip_prefix("# file: ") {
        if let Some((fname, rest)) = stripped.split_once('\n') {
            (fname.trim(), rest)
        } else {
            (filename, code)
        }
    } else {
        (filename, code)
    };

    crate::py_debug::set_active_session(Some(std::sync::Arc::clone(&session)));

    let res = Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let globals = get_or_create_session_globals(py)?;
        globals.set_item("__file__", clean_filename)?;

        // Install Python tracer hook
        py.run(&std::ffi::CString::new(crate::py_debug::PYTHON_TRACER_BOOTSTRAP).unwrap(), Some(&globals), Some(&globals))?;
        let clean_filename_py = clean_filename.replace('\\', "/");
        let tracer_setup = format!("_tracer = _make_tracer('{}')\nsys.settrace(_tracer)\n", clean_filename_py);
        py.run(&std::ffi::CString::new(tracer_setup).unwrap(), Some(&globals), Some(&globals))?;

        // Compile clean code with the specified filename so line numbers are exact
        let builtins = py.import("builtins")?;
        let compile_fn = builtins.getattr("compile")?;
        let exec_fn = builtins.getattr("exec")?;
        let compiled = match compile_fn.call1((actual_code, clean_filename, "exec")) {
            Ok(c) => c,
            Err(e) => {
                let (lineno, err_text) = {
                    let val = e.value(py);
                    let lineno = val.getattr("lineno").ok().and_then(|l| l.extract::<usize>().ok()).unwrap_or(1);
                    let text = val.getattr("text").ok().and_then(|t| t.extract::<String>().ok()).unwrap_or_default();
                    (lineno, format!("{}: {}\n  {}", e, e, text.trim()))
                };
                let err_msg = format!("\n[SyntaxError at {clean_filename}:{lineno}] {err_text}\n");
                crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                    text: err_msg.clone(),
                    is_error: true,
                });
                let _ = session.event_tx.send(crate::py_debug::DebugEvent::Paused {
                    filename: clean_filename.to_string(),
                    line: lineno,
                    locals: Vec::new(),
                    stack: vec![("<module>".to_string(), lineno, clean_filename.to_string())],
                });
                return Err(pyo3::exceptions::PySyntaxError::new_err(err_msg));
            }
        };

        // Execute target code with tracer active
        let run_result = exec_fn.call1((compiled, &globals, &globals));

        // Always uninstall tracer
        if let Ok(sys) = py.import("sys") {
            let _ = sys.call_method1("settrace", (py.None(),));
        }

        flush_python_output(py)?;
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }

        match run_result {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("_DebugQuit") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }).map_err(|err| {
        let msg = Python::with_gil(|py| {
            if let Some(tb) = err.traceback(py) {
                match tb.format() {
                    Ok(formatted) => format!("Debug Error:\n{}\n{}", err, formatted),
                    Err(_) => format!("Debug Error: {}", err),
                }
            } else {
                format!("Debug Error: {}", err)
            }
        });
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.fail(&msg);
        } else {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                text: format!("\n{msg}\n"),
                is_error: true,
            });
            if let Some(s) = crate::context::get_global_sender() {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n{msg}\n")));
            }
        }
        msg
    });

    crate::py_debug::set_active_session(None);
    if res.is_ok() {
        let _ = session.event_tx.send(crate::py_debug::DebugEvent::Finished);
    }
    res
}

/// Execute code under debug tracing on a worker thread.
pub fn execute_code_debug_dispatched(
    code: &str,
    filename: &str,
    session: std::sync::Arc<crate::py_debug::DebugSession>,
    host: &mut dyn HostApi,
) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let code = code.to_string();
    let filename = filename.to_string();
    let session_clone = std::sync::Arc::clone(&session);

    std::thread::spawn(move || {
        crate::context::set_worker_tx(Some(tx.clone()));
        let res = execute_code_debug_raw(&code, &filename, session_clone);
        crate::context::set_worker_tx(None);
        let _ = tx.send(WorkerMsg::Finished(res));
    });

    while let Ok(msg) = rx.recv() {
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
                    host.push_error(&e);
                }
                return true;
            }
        }
    }
    true
}

/// Execute an OpenCAD LISP code string.
pub fn execute_lisp_raw(code: &str) -> Result<(), String> {
    execute_lisp_named(code, "script.lsp")
}

/// Execute an OpenCAD LISP code string named by filename.
pub fn execute_lisp_named(code: &str, _filename: &str) -> Result<(), String> {
    crate::py_sandbox::reset_entity_counter();
    Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let globals = get_or_create_session_globals(py)?;
        let opencad_lisp_mod = globals.get_item("opencad_lisp")?.expect("opencad_lisp module");
        let eval_lisp = opencad_lisp_mod.getattr("eval_lisp")?;
        eval_lisp.call1((code,))?;
        flush_python_output(py)?;
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }
        Ok(())
    }).map_err(|err| {
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }
        Python::with_gil(|py| {
            let tb = err.traceback(py);
            if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("OpenCAD LISP Error:\n{}\n{}", err, formatted),
                    Err(_) => format!("OpenCAD LISP Error: {}", err),
                }
            } else {
                format!("OpenCAD LISP Error: {}", err)
            }
        })
    })
}

/// Execute OpenCAD LISP code asynchronously on a worker thread during command dispatch.
pub fn execute_lisp_dispatched(code: &str, host: &mut dyn HostApi) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let code_str = code.to_string();

    std::thread::spawn(move || {
        crate::context::set_worker_tx(Some(tx.clone()));
        let res = execute_lisp_raw(&code_str);
        crate::context::set_worker_tx(None);
        let _ = tx.send(WorkerMsg::Finished(res));
    });

    while let Ok(msg) = rx.recv() {
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
                    host.push_error(&e);
                }
                return true;
            }
        }
    }
    true
}

/// Execute Python code asynchronously on a worker thread during command dispatch.
/// Handles interactive commands cleanly by forwarding StartInteractive to the host
/// and allowing dispatch() to return true immediately.
pub fn execute_code_dispatched(code: &str, host: &mut dyn HostApi) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let code_str = code.to_string();

    std::thread::spawn(move || {
        crate::context::set_worker_tx(Some(tx.clone()));
        let res = execute_code_raw(&code_str);
        crate::context::set_worker_tx(None);
        let _ = tx.send(WorkerMsg::Finished(res));
    });

    while let Ok(msg) = rx.recv() {
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
                    host.push_error(&e);
                }
                return true;
            }
        }
    }
    true
}

/// Resolve script path with case-insensitive fallback for Linux.
fn resolve_script_path(dir: &std::path::Path, script_name: &str) -> Option<PathBuf> {
    // 1. Direct path check
    let direct = PathBuf::from(script_name);
    if direct.is_file() {
        return Some(direct);
    }

    // 2. Direct join in scripts directory
    let candidate = dir.join(script_name);
    if candidate.is_file() {
        return Some(candidate);
    }

    let candidate_py = dir.join(format!("{}.py", script_name));
    if candidate_py.is_file() {
        return Some(candidate_py);
    }
    let candidate_lsp = dir.join(format!("{}.lsp", script_name));
    if candidate_lsp.is_file() {
        return Some(candidate_lsp);
    }
    let candidate_lisp = dir.join(format!("{}.lisp", script_name));
    if candidate_lisp.is_file() {
        return Some(candidate_lisp);
    }

    // 3. Case-insensitive search in scripts directory
    let target = script_name
        .strip_suffix(".py")
        .or_else(|| script_name.strip_suffix(".lsp"))
        .or_else(|| script_name.strip_suffix(".lisp"))
        .unwrap_or(script_name);
    let c_dash = format!("c-{}", target);
    let c_under = format!("c_{}", target);
    let stripped_c = target.strip_prefix("c-").or_else(|| target.strip_prefix("c_"));

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.eq_ignore_ascii_case(target)
                        || stem.eq_ignore_ascii_case(&c_dash)
                        || stem.eq_ignore_ascii_case(&c_under)
                        || stripped_c.is_some_and(|sc| stem.eq_ignore_ascii_case(sc))
                    {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Execute a script raw without host guard (for worker thread).
pub fn execute_script_raw(script_name: &str, args: &[String]) -> Result<(), String> {
    let (code, script_display_path) = if let Some(in_mem) = get_loaded_script(script_name) {
        let dir = scripts_dir();
        let p = resolve_script_path(&dir, script_name)
            .unwrap_or_else(|| dir.join(if script_name.ends_with(".py") || script_name.ends_with(".lsp") || script_name.ends_with(".lisp") { script_name.to_string() } else { format!("{script_name}.py") }));
        (in_mem, p.to_string_lossy().to_string())
    } else {
        let dir = ensure_scripts_dir().map_err(|e| e.to_string())?;
        let Some(script_path) = resolve_script_path(&dir, script_name) else {
            return Err(format!(
                "Script '{}' not found in {}",
                script_name,
                dir.display()
            ));
        };
        if !crate::py_sandbox::is_path_trusted(&script_path) {
            return Err(format!(
                "[SECURITY] Blocked execution of untrusted script: '{}'. The folder is not in trusted paths. Use 'PYTRUST <dir>' to trust it.",
                script_path.display()
            ));
        }
        let c = std::fs::read_to_string(&script_path).map_err(|e| {
            format!("Failed to read script {}: {}", script_path.display(), e)
        })?;
        (c, script_path.to_string_lossy().to_string())
    };

    let is_lisp = script_display_path.ends_with(".lsp") || script_display_path.ends_with(".lisp");
    if is_lisp {
        return execute_lisp_named(&code, &script_display_path);
    }

    crate::py_sandbox::reset_entity_counter();
    Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;

        // Set sys.argv and configure sys.path for sibling imports
        let sys = py.import("sys")?;
        let sys_path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;
        let p_buf = PathBuf::from(&script_display_path);
        if let Some(parent) = p_buf.parent() {
            let parent_str = parent.to_string_lossy();
            let mut exists = false;
            for item in sys_path.iter() {
                if item.extract::<String>().is_ok_and(|s| s == parent_str.as_ref()) {
                    exists = true;
                    break;
                }
            }
            if !exists {
                sys_path.insert(0, parent_str.as_ref())?;
            }
        }

        let sys_argv = PyList::empty(py);
        sys_argv.append(script_display_path.as_str())?;
        for arg in args {
            sys_argv.append(arg)?;
        }
        sys.setattr("argv", sys_argv)?;

        // Set up execution globals with __file__ in persistent session
        let globals = get_or_create_session_globals(py)?;
        globals.set_item("__file__", script_display_path.as_str())?;

        // Run the script
        let builtins = py.import("builtins")?;
        let compile_fn = builtins.getattr("compile")?;
        let exec_fn = builtins.getattr("exec")?;
        let compiled = compile_fn.call1((code.as_str(), script_display_path.as_str(), "exec"))?;
        exec_fn.call1((compiled, &globals, &globals))?;

        // If a `run` or `main` function is defined, invoke it with args if appropriate
        let mut invoked = false;
        if let Ok(Some(run_fn)) = globals.get_item("run") {
            if run_fn.is_callable() {
                let py_args = PyList::empty(py);
                for arg in args {
                    py_args.append(arg)?;
                }
                if run_fn.call1((py_args,)).is_err() {
                    let _ = run_fn.call0();
                }
                invoked = true;
            }
        }
        if !invoked {
            if let Ok(Some(main_fn)) = globals.get_item("main") {
                if main_fn.is_callable() {
                    let py_args = PyList::empty(py);
                    for arg in args {
                        py_args.append(arg)?;
                    }
                    if main_fn.call1((py_args,)).is_err() {
                        let _ = main_fn.call0();
                    }
                }
            }
        }


        flush_python_output(py)?;
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }
        Ok(())
    }).map_err(|err| {
        let msg = Python::with_gil(|py| {
            let tb = err.traceback(py);
            if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("Script Error:\n{}\n{}", err, formatted),
                    Err(_) => format!("Script Error: {}", err),
                }
            } else {
                format!("Script Error: {}", err)
            }
        });
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.fail(&msg);
        } else {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                text: format!("\n{msg}\n"),
                is_error: true,
            });
            if let Some(s) = crate::context::get_global_sender() {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n{msg}\n")));
            }
        }
        msg
    })
}

/// Execute a script synchronously with direct host guard (used in tests / sync mode).
pub fn execute_script(
    script_name: &str,
    args: &[String],
    host: &mut dyn HostApi,
) -> Result<(), String> {
    let _guard = HostGuard::new(host);
    execute_script_raw(script_name, args)
}

/// Execute a script raw with active debugger session.
pub fn execute_script_debug_raw(
    script_name: &str,
    args: &[String],
    breakpoints: std::collections::HashMap<String, std::collections::HashSet<usize>>,
) -> Result<(), String> {
    let (code, script_display_path) = if let Some(in_mem) = get_loaded_script(script_name) {
        let dir = scripts_dir();
        let p = resolve_script_path(&dir, script_name)
            .unwrap_or_else(|| dir.join(if script_name.ends_with(".py") { script_name.to_string() } else { format!("{script_name}.py") }));
        (in_mem, p.to_string_lossy().to_string())
    } else {
        let dir = ensure_scripts_dir().map_err(|e| e.to_string())?;
        let Some(script_path) = resolve_script_path(&dir, script_name) else {
            return Err(format!(
                "Script '{}' not found in {}",
                script_name,
                dir.display()
            ));
        };
        if !crate::py_sandbox::is_path_trusted(&script_path) {
            return Err(format!(
                "[SECURITY] Blocked execution of untrusted script: '{}'. The folder is not in trusted paths. Use 'PYTRUST <dir>' to trust it.",
                script_path.display()
            ));
        }
        let c = std::fs::read_to_string(&script_path).map_err(|e| {
            format!("Failed to read script {}: {}", script_path.display(), e)
        })?;
        (c, script_path.to_string_lossy().to_string())
    };

    let (session, _cmd_tx, event_rx) = crate::py_debug::DebugSession::new_multi(breakpoints);
    let session = std::sync::Arc::new(session);
    crate::py_debug::set_active_session(Some(std::sync::Arc::clone(&session)));

    std::thread::spawn(move || {
        while let Ok(evt) = event_rx.recv() {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::DebugEvt(evt));
        }
    });

    crate::py_sandbox::reset_entity_counter();
    let res = Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let globals = get_or_create_session_globals(py)?;

        // Install tracer
        py.run(&std::ffi::CString::new(crate::py_debug::PYTHON_TRACER_BOOTSTRAP).unwrap(), Some(&globals), Some(&globals))?;
        py.run(&std::ffi::CString::new("_tracer = _make_tracer()\nsys.settrace(_tracer)\n").unwrap(), Some(&globals), Some(&globals))?;

        // Set sys.argv and configure sys.path for sibling imports
        let sys = py.import("sys")?;
        let sys_path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;
        let p_buf = PathBuf::from(&script_display_path);
        if let Some(parent) = p_buf.parent() {
            let parent_str = parent.to_string_lossy();
            let mut exists = false;
            for item in sys_path.iter() {
                if item.extract::<String>().is_ok_and(|s| s == parent_str.as_ref()) {
                    exists = true;
                    break;
                }
            }
            if !exists {
                sys_path.insert(0, parent_str.as_ref())?;
            }
        }

        let sys_argv = PyList::empty(py);
        sys_argv.append(script_display_path.as_str())?;
        for arg in args {
            sys_argv.append(arg)?;
        }
        sys.setattr("argv", sys_argv)?;

        globals.set_item("__file__", script_display_path.as_str())?;

        let builtins = py.import("builtins")?;
        let compile_fn = builtins.getattr("compile")?;
        let exec_fn = builtins.getattr("exec")?;
        let compiled = compile_fn.call1((code.as_str(), script_display_path.as_str(), "exec"))?;
        let run_result = exec_fn.call1((compiled, &globals, &globals));

        if let Ok(sys) = py.import("sys") {
            let _ = sys.call_method1("settrace", (py.None(),));
        }
        flush_python_output(py)?;

        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }

        match run_result {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("_DebugQuit") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    });

    crate::py_debug::set_active_session(None);
    if res.is_ok() {
        let _ = session.event_tx.send(crate::py_debug::DebugEvent::Finished);
    }

    res.map_err(|err| {
        let msg = Python::with_gil(|py| {
            let tb = err.traceback(py);
            if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("Script Error:\n{}\n{}", err, formatted),
                    Err(_) => format!("Script Error: {}", err),
                }
            } else {
                format!("Script Error: {}", err)
            }
        });
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.fail(&msg);
        } else {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                text: format!("\n{msg}\n"),
                is_error: true,
            });
            if let Some(s) = crate::context::get_global_sender() {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n{msg}\n")));
            }
        }
        msg
    })
}

/// Execute a script on a worker thread during command dispatch.
/// Handles interactive commands cleanly by forwarding StartInteractive to the host
/// and allowing dispatch() to return true immediately.
pub fn execute_script_dispatched(
    script_name: &str,
    args: &[String],
    host: &mut dyn HostApi,
) -> bool {
    let bps = crate::ide::server::get_active_breakpoints();
    let use_debug = crate::ide::server::has_active_clients();

    if use_debug {
        host.push_info(&format!("Running/debugging '{script_name}' in OCIDE..."));
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let script_name = script_name.to_string();
    let args = args.to_vec();

    std::thread::spawn(move || {
        crate::context::set_worker_tx(Some(tx.clone()));
        let res = if use_debug {
            execute_script_debug_raw(&script_name, &args, bps)
        } else {
            execute_script_raw(&script_name, &args)
        };
        crate::context::set_worker_tx(None);
        let _ = tx.send(WorkerMsg::Finished(res));
    });

    while let Ok(msg) = rx.recv() {
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
                    host.push_error(&e);
                    crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                        text: format!("\n[Error] {}\n", e),
                        is_error: true,
                    });
                }
                return true;
            }
        }
    }
    true
}

/// Execute a decorated Python command raw (on current thread).
pub fn execute_pycmd_raw(cmd_name: &str, args: &[String]) -> Result<(), String> {
    Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;

        let sys = py.import("sys")?;
        let sys_modules: Bound<'_, PyDict> = sys.getattr("modules")?.downcast_into()?;
        let opencad_mod = sys_modules.get_item("opencad")?.expect("opencad module");

        let invoke_fn = opencad_mod.getattr("_invoke_command")?;
        let py_args = PyList::empty(py);
        for arg in args {
            py_args.append(arg)?;
        }

        crate::interactive::dbg_trace(&format!("execute_pycmd_raw: invoking {}", cmd_name));
        let invoke_res = invoke_fn.call1((cmd_name, py_args));
        crate::interactive::dbg_trace(&format!("execute_pycmd_raw: invoke_fn returned ok={}", invoke_res.is_ok()));
        let _ = invoke_res?;

        flush_python_output(py)?;
        crate::interactive::dbg_trace("execute_pycmd_raw: flush_python_output completed");
        if let Some(bridge) = crate::context::active_interactive() {
            crate::interactive::dbg_trace("execute_pycmd_raw: calling bridge.finish()");
            bridge.finish();
            crate::interactive::dbg_trace("execute_pycmd_raw: bridge.finish() returned");
        }
        Ok(())
    }).map_err(|err| {
        let msg = Python::with_gil(|py| {
            let tb = err.traceback(py);
            if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("Command Error ({}):\n{}\n{}", cmd_name, err, formatted),
                    Err(_) => format!("Command Error ({}): {}", cmd_name, err),
                }
            } else {
                format!("Command Error ({}): {}", cmd_name, err)
            }
        });
        crate::interactive::dbg_trace(&format!("execute_pycmd_raw: ERROR: {}", msg));
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.fail(&msg);
        } else {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                text: format!("\n{msg}\n"),
                is_error: true,
            });
            if let Some(s) = crate::context::get_global_sender() {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n{msg}\n")));
            }
        }
        msg
    })
}

/// Execute a decorated Python command raw with active debugger session.
pub fn execute_pycmd_debug_raw(
    cmd_name: &str,
    args: &[String],
    breakpoints: std::collections::HashMap<String, std::collections::HashSet<usize>>,
) -> Result<(), String> {
    let (session, _cmd_tx, event_rx) = crate::py_debug::DebugSession::new_multi(breakpoints);
    let session = std::sync::Arc::new(session);
    crate::py_debug::set_active_session(Some(std::sync::Arc::clone(&session)));

    std::thread::spawn(move || {
        while let Ok(evt) = event_rx.recv() {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::DebugEvt(evt));
        }
    });

    let res = Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let globals = get_or_create_session_globals(py)?;

        // Install tracer
        py.run(&std::ffi::CString::new(crate::py_debug::PYTHON_TRACER_BOOTSTRAP).unwrap(), Some(&globals), Some(&globals))?;
        py.run(&std::ffi::CString::new("_tracer = _make_tracer()\nsys.settrace(_tracer)\n").unwrap(), Some(&globals), Some(&globals))?;

        let sys = py.import("sys")?;
        let sys_modules: Bound<'_, PyDict> = sys.getattr("modules")?.downcast_into()?;
        let opencad_mod = sys_modules.get_item("opencad")?.expect("opencad module");

        let invoke_fn = opencad_mod.getattr("_invoke_command")?;
        let py_args = PyList::empty(py);
        for arg in args {
            py_args.append(arg)?;
        }

        let run_result = invoke_fn.call1((cmd_name, py_args));

        if let Ok(sys) = py.import("sys") {
            let _ = sys.call_method1("settrace", (py.None(),));
        }
        flush_python_output(py)?;

        if let Some(bridge) = crate::context::active_interactive() {
            bridge.finish();
        }

        match run_result {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("_DebugQuit") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    });

    crate::py_debug::set_active_session(None);
    if res.is_ok() {
        let _ = session.event_tx.send(crate::py_debug::DebugEvent::Finished);
    }

    res.map_err(|err| {
        let msg = Python::with_gil(|py| {
            let tb = err.traceback(py);
            if let Some(tb) = tb {
                match tb.format() {
                    Ok(formatted) => format!("Command Error ({}):\n{}\n{}", cmd_name, err, formatted),
                    Err(_) => format!("Command Error ({}): {}", cmd_name, err),
                }
            } else {
                format!("Command Error ({}): {}", cmd_name, err)
            }
        });
        if let Some(bridge) = crate::context::active_interactive() {
            bridge.fail(&msg);
        } else {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                text: format!("\n{msg}\n"),
                is_error: true,
            });
            if let Some(s) = crate::context::get_global_sender() {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n{msg}\n")));
            }
        }
        msg
    })
}

/// Execute a decorated Python command asynchronously on worker thread.
pub fn execute_pycmd_dispatched(
    cmd_name: &str,
    args: &[String],
    host: &mut dyn HostApi,
) -> bool {
    let bps = crate::ide::server::get_active_breakpoints();
    let use_debug = crate::ide::server::has_active_clients();

    if use_debug {
        host.push_info(&format!("Running/debugging @command '{cmd_name}' in OCIDE..."));
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let cmd_name_str = cmd_name.to_string();
    let args_vec = args.to_vec();

    std::thread::spawn(move || {
        crate::context::set_worker_tx(Some(tx.clone()));
        let res = if use_debug {
            execute_pycmd_debug_raw(&cmd_name_str, &args_vec, bps)
        } else {
            execute_pycmd_raw(&cmd_name_str, &args_vec)
        };
        crate::context::set_worker_tx(None);
        let _ = tx.send(WorkerMsg::Finished(res));
    });

    while let Ok(msg) = rx.recv() {
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
                    host.push_error(&e);
                    crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                        text: format!("\n[Error] {}\n", e),
                        is_error: true,
                    });
                }
                return true;
            }
        }
    }
    true
}

/// Load a Python script and register any @command functions it defines.
pub fn load_script_commands(script_name: &str, host: &mut dyn HostApi) -> usize {
    let _guard = HostGuard::new(host);
    let Ok(dir) = ensure_scripts_dir() else {
        return 0;
    };
    let Some(path) = resolve_script_path(&dir, script_name) else {
        host.push_error(&format!("Script '{}' not found in {}", script_name, dir.display()));
        return 0;
    };
    if !crate::py_sandbox::is_path_trusted(&path) {
        host.push_error(&format!(
            "[SECURITY] Blocked loading untrusted script: '{}'. The folder is not in trusted paths. Use 'PYTRUST <dir>' to trust it.",
            path.display()
        ));
        return 0;
    }

    let before_count = py_module::get_registered_commands().len();

    let res = Python::with_gil(|py| -> PyResult<()> {
        setup_python_environment(py)?;
        let code = std::fs::read_to_string(&path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to read {}: {}", path.display(), e))
        })?;

        let is_lisp = path.extension().is_some_and(|ext| ext == "lsp" || ext == "lisp");
        if is_lisp {
            let globals = get_or_create_session_globals(py)?;
            let opencad_lisp_mod = globals.get_item("opencad_lisp")?.expect("opencad_lisp module");
            let eval_lisp = opencad_lisp_mod.getattr("eval_lisp")?;
            eval_lisp.call1((code.as_str(),))?;
            flush_python_output(py)?;
            return Ok(());
        }

        let globals = get_or_create_session_globals(py)?;
        globals.set_item("__file__", path.to_string_lossy().as_ref())?;

        let builtins = py.import("builtins")?;
        let compile_fn = builtins.getattr("compile")?;
        let exec_fn = builtins.getattr("exec")?;
        let compiled = compile_fn.call1((code.as_str(), path.to_string_lossy().as_ref(), "exec"))?;
        exec_fn.call1((compiled, &globals, &globals))?;

        flush_python_output(py)?;
        Ok(())
    });

    if let Err(e) = res {
        host.push_error(&format!("Error loading {}: {}", path.display(), e));
    }

    py_module::sync_registered_aliases(host);
    let after_count = py_module::get_registered_commands().len();
    after_count.saturating_sub(before_count)
}

/// Automatically load all scripts in scripts/ (and scripts/autoload/) that define @command.
pub fn auto_load_decorated_scripts(host: &mut dyn HostApi) -> usize {
    let Ok(dir) = ensure_scripts_dir() else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };

    let _guard = HostGuard::new(host);

    let mut files_to_load = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename.starts_with('_') || filename.starts_with('.') {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("py") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let lower = content.to_lowercase();
                    if lower.contains("@oc.command")
                        || lower.contains("@opencad.command")
                        || lower.contains("@command")
                        || lower.contains("@oc.panel")
                        || lower.contains("@opencad.panel")
                        || lower.contains("@panel")
                    {
                        files_to_load.push(path);
                    }
                }
            } else if ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let lower = content.to_lowercase();
                    if lower.contains("defun c:") || lower.contains("defun") {
                        files_to_load.push(path);
                    }
                }
            }
        }
    }

    // Also check scripts/autoload/
    let autoload_dir = dir.join("autoload");
    if autoload_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&autoload_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !filename.starts_with('_') && !filename.starts_with('.') {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("lsp") || ext.eq_ignore_ascii_case("lisp") {
                            files_to_load.push(path);
                        }
                    }
                }
            }
        }
    }

    let before_count = py_module::get_registered_commands().len();

    Python::with_gil(|py| {
        let _ = setup_python_environment(py);
        let sys = match py.import("sys") {
            Ok(s) => s,
            Err(_) => return,
        };
        let sys_modules: Bound<'_, PyDict> = match sys.getattr("modules").ok().and_then(|m| m.downcast_into().ok()) {
            Some(m) => m,
            None => return,
        };
        let opencad_mod = match sys_modules.get_item("opencad").ok().flatten() {
            Some(m) => m,
            None => return,
        };

        for path in files_to_load {
            if let Ok(code) = std::fs::read_to_string(&path) {
                let is_lisp = path.extension().is_some_and(|ext| ext == "lsp" || ext == "lisp");
                if is_lisp {
                    crate::rust_lisp::load_lisp_file(&path.to_string_lossy(), host);
                    continue;
                }

                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("module");
                let globals = PyDict::new(py);
                let _ = globals.set_item("__file__", path.to_string_lossy().as_ref());
                let _ = globals.set_item("__name__", stem);
                if let Ok(builtins) = py.import("builtins") {
                    let _ = globals.set_item("__builtins__", builtins);
                }
                let _ = globals.set_item("opencad", &opencad_mod);
                let _ = globals.set_item("oc", &opencad_mod);
                let _ = globals.set_item("OPENCAD", &opencad_mod);
                let _ = globals.set_item("OC", &opencad_mod);

                if let Ok(dir_attrs) = opencad_mod.dir() {
                    for attr in dir_attrs {
                        if let Ok(name) = attr.extract::<String>() {
                            if !name.starts_with("__") {
                                if let Ok(val) = opencad_mod.getattr(&name) {
                                    let _ = globals.set_item(&name, &val);
                                    let _ = globals.set_item(name.to_uppercase(), &val);
                                }
                            }
                        }
                    }
                }

                let path_str = path.to_string_lossy().to_string();
                if let Ok(builtins) = py.import("builtins") {
                    if let (Ok(compile_fn), Ok(exec_fn)) = (builtins.getattr("compile"), builtins.getattr("exec")) {
                        match compile_fn.call1((code.as_str(), path_str.as_str(), "exec")) {
                            Ok(compiled) => {
                                if let Err(e) = exec_fn.call1((compiled, &globals, &globals)) {
                                    eprintln!("[plugin] [AUTOLOAD ERROR] {}: {}", path_str, e);
                                    let msg = format!("Script autoload error in '{}': {}", path_str, e);
                                    let _ = crate::context::with_host(|h| h.push_error(&msg));
                                }
                            }
                            Err(e) => {
                                eprintln!("[plugin] [AUTOLOAD COMPILE ERROR] {}: {}", path_str, e);
                                let msg = format!("Script compile error in '{}': {}", path_str, e);
                                let _ = crate::context::with_host(|h| h.push_error(&msg));
                            }
                        }
                    }
                }
            }
        }
        let _ = flush_python_output(py);
    });

    py_module::sync_registered_aliases(host);
    let after_count = py_module::get_registered_commands().len();
    after_count.saturating_sub(before_count)
}

/// Set up Python sys.path, sys.modules["opencad"], and stdout/stderr capture.
pub(crate) fn setup_python_environment(py: Python<'_>) -> PyResult<()> {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::dlopen(b"libpython3.14.so.1.0\0".as_ptr() as *const _, libc::RTLD_NOW | libc::RTLD_GLOBAL);
        libc::dlopen(b"libpython3.14.so\0".as_ptr() as *const _, libc::RTLD_NOW | libc::RTLD_GLOBAL);
        libc::dlopen(b"libpython3.so\0".as_ptr() as *const _, libc::RTLD_NOW | libc::RTLD_GLOBAL);
    }

    let sys = py.import("sys")?;

    // 1. Configure sys.path
    let sys_path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;
    if let Ok(dir) = ensure_scripts_dir() {
        let dir_str = dir.to_string_lossy();
        let mut exists = false;
        for item in sys_path.iter() {
            if item.extract::<String>().is_ok_and(|s| s == dir_str.as_ref()) {
                exists = true;
                break;
            }
        }
        if !exists {
            sys_path.insert(0, dir_str.as_ref())?;
        }
    }

    // 2. Register `opencad`, `oc`, `OPENCAD`, and `OC` modules into sys.modules if not already present
    let sys_modules: Bound<'_, PyDict> = sys.getattr("modules")?.downcast_into()?;
    if sys_modules.get_item("opencad")?.is_none() {
        let opencad_mod = PyModule::new(py, "opencad")?;
        py_module::populate_module(&opencad_mod)?;
        sys_modules.set_item("opencad", &opencad_mod)?;
        sys_modules.set_item("oc", &opencad_mod)?;
        sys_modules.set_item("OPENCAD", &opencad_mod)?;
        sys_modules.set_item("OC", &opencad_mod)?;
    }

    if sys_modules.get_item("opencad_lisp")?.is_none() {
        const OPENCAD_LISP_SRC: &str = include_str!("opencad_lisp.py");
        let c_src = std::ffi::CString::new(OPENCAD_LISP_SRC).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let opencad_lisp_mod = PyModule::from_code(py, &c_src, c"opencad_lisp.py", c"opencad_lisp")?;
        let opencad_mod = sys_modules.get_item("opencad")?.expect("opencad module");
        let init_fn = opencad_lisp_mod.getattr("init_opencad_lisp")?;
        init_fn.call1((&opencad_mod,))?;
        sys_modules.set_item("opencad_lisp", &opencad_lisp_mod)?;
    }

    // 3. Set up in-memory stdout/stderr capture using io.StringIO
    let io = py.import("io")?;
    let stdout_capture = io.getattr("StringIO")?.call0()?;
    let stderr_capture = io.getattr("StringIO")?.call0()?;

    sys.setattr("stdout", stdout_capture)?;
    sys.setattr("stderr", stderr_capture)?;

    // 4. Apply Sandbox Hardening
    let config = crate::py_sandbox::load_config();
    if config.sandbox_enabled {
        let _ = sys.setattr("dont_write_bytecode", true);

        // Network severance
        if !config.allow_network {
            let _ = sys_modules.set_item("_socket", py.None());
            let _ = sys_modules.set_item("socket", py.None());
            let _ = sys_modules.set_item("_ssl", py.None());
        }

        // Subprocess & Shell severance
        if !config.allow_subprocesses {
            let _ = sys_modules.set_item("_winapi", py.None());
            let _ = sys_modules.set_item("_posixsubprocess", py.None());
            let _ = sys_modules.set_item("subprocess", py.None());

            if let Ok(os) = py.import("os") {
                for func in &[
                    "system", "popen", "spawnl", "spawnv", "spawnle", "spawnve",
                    "spawnlp", "spawnvp", "execv", "execve", "execl", "execle",
                    "execlp", "execvp", "startfile",
                ] {
                    if os.hasattr(*func).unwrap_or(false) {
                        let _ = os.delattr(*func);
                    }
                }
            }
        }

        // C / FFI & ctypes blocking
        let _ = sys_modules.set_item("_ctypes", py.None());
        let _ = sys_modules.set_item("ctypes", py.None());

        // Native binary extension restriction
        if !config.allow_native_binaries {
            if let Ok(imp) = py.import("_imp") {
                if imp.hasattr("create_dynamic").unwrap_or(false) {
                    let _ = imp.delattr("create_dynamic");
                }
            }
        }

        // GUI and clipboard snooping protection
        let _ = sys_modules.set_item("_tkinter", py.None());
        let _ = sys_modules.set_item("tkinter", py.None());

        // Reflection & Stack climbing prevention
        if sys.hasattr("_getframe").unwrap_or(false) {
            let _ = sys.delattr("_getframe");
        }

        // Lock / disable sys.setrecursionlimit to prevent C-stack overflow segfaults
        if sys.hasattr("setrecursionlimit").unwrap_or(false) {
            let _ = sys.delattr("setrecursionlimit");
        }

        // Environment variable sanitization
        if let Ok(os) = py.import("os") {
            if let Ok(environ) = os.getattr("environ") {
                let safe_keys = [
                    "PATH", "PATHEXT", "APPDATA", "LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)",
                    "COMMONPROGRAMFILES", "SYSTEMROOT", "WINDIR", "COMSPEC", "TEMP", "TMP",
                    "HOME", "USER", "USERNAME", "USERPROFILE", "LOGNAME", "SHELL", "TERM",
                    "LANG", "LC_ALL", "LC_CTYPE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR",
                    "OCS_SCRIPTS_DIR", "OCS_PLUGIN_CDYLIB_PATH"
                ];
                if let Ok(keys_obj) = environ.call_method0("keys") {
                    if let Ok(keys) = keys_obj.extract::<Vec<String>>() {
                        for k in keys {
                            let k_upper = k.to_uppercase();
                            if !safe_keys.iter().any(|safe| safe.eq_ignore_ascii_case(&k_upper)) {
                                let _ = environ.del_item(k);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Flush captured stdout and stderr to the active host console and OCIDE console.
pub(crate) fn flush_python_output(py: Python<'_>) -> PyResult<()> {
    let sys = py.import("sys")?;

    if let Ok(stdout) = sys.getattr("stdout") {
        if let Ok(val) = stdout.call_method0("getvalue") {
            let text: String = val.extract()?;
            if !text.trim().is_empty() {
                let _ = crate::context::with_host(|h| {
                    for line in text.lines() {
                        h.push_output(line);
                    }
                });
                crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                    text: text.clone(),
                    is_error: false,
                });
                let _ = stdout.call_method1("truncate", (0,));
                let _ = stdout.call_method1("seek", (0,));
            }
        }
    }

    if let Ok(stderr) = sys.getattr("stderr") {
        if let Ok(val) = stderr.call_method0("getvalue") {
            let text: String = val.extract()?;
            if !text.trim().is_empty() {
                let _ = crate::context::with_host(|h| {
                    for line in text.lines() {
                        h.push_error(line);
                    }
                });
                crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
                    text: text.clone(),
                    is_error: true,
                });
                let _ = stderr.call_method1("truncate", (0,));
                let _ = stderr.call_method1("seek", (0,));
            }
        }
    }

    Ok(())
}
