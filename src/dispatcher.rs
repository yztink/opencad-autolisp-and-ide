use ocs_plugin_api::host::HostApi;

pub static LAST_DISPATCHED_COMMAND: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn get_last_dispatched_command() -> Option<String> {
    LAST_DISPATCHED_COMMAND.lock().unwrap().clone()
}

pub fn set_last_dispatched_command(cmd: Option<String>) {
    *LAST_DISPATCHED_COMMAND.lock().unwrap() = cmd;
}

pub fn parse_script_path_arg(rest: &str) -> &str {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return "";
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        return &trimmed[1..trimmed.len() - 1];
    }
    if let Some(stripped) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = stripped.find('"') {
            return &stripped[..end_quote];
        }
    }
    if std::path::Path::new(trimmed).is_file() {
        return trimmed;
    }
    trimmed.split_whitespace().next().unwrap_or(trimmed)
}

pub fn dispatch_command(host: &mut dyn HostApi, cmd: &str) -> bool {
    crate::rust_lisp::ensure_tab_initialized(host);
    crate::rust_lisp::sync_pending_ide_scripts_for_tab(host);
    let mut trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Some((name, ticket)) = trimmed.split_once(' ') {
        if name.eq_ignore_ascii_case(crate::rust_lisp::FLUSH_OUTPUT_COMMAND) {
            crate::rust_lisp::flush_pending_command_output(host, ticket);
            return true;
        }
    }

    // Strip leading '>' if passed through
    if let Some(stripped) = trimmed.strip_prefix('>') {
        trimmed = stripped.trim();
    }

    // Direct S-expression in CAD command line: e.g. (+ 1 2) or (command "_.CIRCLE" ...)
    if trimmed.starts_with('(') {
        set_last_dispatched_command(Some(trimmed.to_string()));
        let _ = crate::rust_lisp::eval_lisp_expr(trimmed, host);
        return true;
    }

    // Extract command verb and remainder
    let (verb, rest) = if let Some(r) = trimmed.strip_prefix("LSP:") {
        ("LSP", r.trim())
    } else if let Some(r) = trimmed.strip_prefix("lsp:") {
        ("LSP", r.trim())
    } else if let Some(r) = trimmed.strip_prefix("PY:") {
        ("PY", r.trim())
    } else if let Some(r) = trimmed.strip_prefix("py:") {
        ("PY", r.trim())
    } else if let Some(r) = trimmed.strip_prefix("PYRUN:") {
        ("PYRUN", r.trim())
    } else if let Some(r) = trimmed.strip_prefix("pyrun:") {
        ("PYRUN", r.trim())
    } else {
        match trimmed.split_once(char::is_whitespace) {
            Some((v, r)) => (v, r.trim()),
            None => (trimmed, ""),
        }
    };

    let primary = verb.to_uppercase();

    match primary.as_str() {
        "LSPRUN" | "LISPRUN" => {
            if rest.is_empty() {
                host.push_error("Usage: LSPRUN <lisp_code> or (command ...)");
                return true;
            }
            set_last_dispatched_command(Some(trimmed.to_string()));
            crate::rust_lisp::eval_lisp_expr(rest, host);
            true
        }
        "LSP" | "LISP" | "APPLOAD" => {
            let script_name = parse_script_path_arg(rest);
            if script_name.is_empty() {
                if primary == "APPLOAD" {
                    host.start_interactive(Box::new(crate::appload::ApploadCommand::new()));
                    return true;
                }
                host.push_info("Usage: LSP <script_name_or_file> or APPLOAD [<path>]");
                return true;
            }
            set_last_dispatched_command(Some(trimmed.to_string()));
            crate::rust_lisp::load_lisp_file(script_name, host);
            true
        }
        "STARTUP" | "STARTUPSCRIPTS" => {
            host.start_interactive(Box::new(crate::appload::ApploadCommand::startup_only()));
            true
        }
        "OCAIDE" => {
            host.push_info("Opening OpenCAD AutoLISP IDE (OCAIDE)...");
            launch_ide();
            true
        }
        "LSPLIST" | "LISPLIST" | "PYLIST" => {
            let scripts = crate::rust_lisp::list_lisp_scripts();
            let dir = crate::rust_lisp::scripts_dir();
            let cmds = crate::rust_lisp::get_registered_lisp_commands_for_tab(host.tab_id());

            if !cmds.is_empty() {
                host.push_info(&format!("Registered AutoLISP commands ({}):", cmds.len()));
                for name in cmds {
                    host.push_output(&format!("  • {}", name));
                }
            }

            if scripts.is_empty() {
                host.push_info(&format!("No AutoLISP (.lsp) files found in {}", dir.display()));
            } else {
                host.push_info(&format!("Available AutoLISP (.lsp) files ({}):", scripts.len()));
                for s in scripts {
                    host.push_output(&format!("  • LSP {s}.lsp"));
                }
            }
            true
        }
        "LSPRELOAD" | "PYRELOAD" => {
            match crate::rust_lisp::ensure_scripts_dir() {
                Ok(dir) => {
                    let scripts = crate::rust_lisp::list_lisp_scripts();
                    let loaded = crate::rust_lisp::auto_load_lisp_scripts(host);
                    host.push_info(&format!(
                        "AutoLISP runner reloaded: {} .lsp scripts in {}, {} new c:commands registered",
                        scripts.len(),
                        dir.display(),
                        loaded
                    ));
                }
                Err(e) => {
                    host.push_error(&format!("Failed to reload scripts directory: {}", e));
                }
            }
            true
        }
        "LSPDIR" | "PYDIR" => {
            let dir = crate::rust_lisp::scripts_dir();
            host.push_info(&format!("Scripts directory: {}", dir.display()));
            true
        }
        "LSPERR" | "LISPERR" => {
            if let Some(err) = crate::rust_lisp::get_last_lisp_error() {
                if crate::ide::server::has_active_clients() {
                    crate::ide::server::ide_log(&format!("\n[Last AutoLISP Error] {err}\n"), true);
                }
                crate::rust_lisp::push_formatted_error(host, "Last AutoLISP Error", &err);
            } else {
                host.push_info("No AutoLISP errors recorded in this session.");
            }
            true
        }
        "PY" | "PYRUN" | "PYLOAD" | "PYCMD" | "PYDEBUG" | "PYDEBUG_CMD" | "PYTRUST" | "PYUNTRUST" | "PYTRUST_LIST" | "PYSECURITY" => {
            host.push_info("Python support is currently shelved. Use AutoLISP (LSP, APPLOAD, LSPRUN, or direct S-expressions).");
            true
        }
        _ => {
            // Check if matches a registered AutoLISP command in the active drawing tab (from defun c:NAME)
            if crate::rust_lisp::has_lisp_command_for_tab(host.tab_id(), &primary) {
                let args: Vec<String> = if rest.is_empty() {
                    Vec::new()
                } else {
                    rest.split_whitespace().map(|s| s.to_string()).collect()
                };
                set_last_dispatched_command(Some(trimmed.to_string()));
                return crate::rust_lisp::execute_lisp_command(&primary, &args, host);
            }
            false
        }
    }
}

pub fn launch_ide() {
    if crate::ide::server::focus_or_check_alive() {
        return;
    }

    let mut exe_candidates = Vec::new();
    if let Ok(path) = std::env::var("OCS_PLUGIN_CDYLIB_PATH") {
        let p = std::path::PathBuf::from(path);
        if let Some(dir) = p.parent() {
            exe_candidates.push(dir.join("opencad_autolisp_ide.exe"));
            exe_candidates.push(dir.join("opencad_autolisp_ide"));
        }
    }
    if let Ok(curr) = std::env::current_exe() {
        if let Some(dir) = curr.parent() {
            exe_candidates.push(dir.join("opencad_autolisp_ide.exe"));
            exe_candidates.push(dir.join("opencad_autolisp_ide"));
        }
    }
    exe_candidates.push(std::path::PathBuf::from("opencad_autolisp_ide.exe"));
    exe_candidates.push(std::path::PathBuf::from("opencad_autolisp_ide"));

    for candidate in exe_candidates {
        if candidate.exists() {
            let _ = std::process::Command::new(candidate)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            return;
        }
    }
    let _ = std::process::Command::new("opencad_autolisp_ide")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
