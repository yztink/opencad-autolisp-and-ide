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
                host.push_error(&crate::i18n::tr("cmd-usage-lsprun"));
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
                host.push_info(&crate::i18n::tr("cmd-usage-lsp"));
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
        "OCIDE" => {
            host.push_info(&crate::i18n::tr("cmd-opening-ide"));
            launch_ide();
            true
        }
        "LSPLIST" | "LISPLIST" | "PYLIST" => {
            let scripts = crate::rust_lisp::list_lisp_scripts();
            let dir = crate::rust_lisp::scripts_dir();
            let cmds = crate::rust_lisp::get_registered_lisp_commands_for_tab(host.tab_id());

            if !cmds.is_empty() {
                host.push_info(&crate::i18n::tr_args(
                    "cmd-registered-commands",
                    &[("count", cmds.len().into())],
                ));
                for name in cmds {
                    host.push_output(&format!("  • {}", name));
                }
            }

            if scripts.is_empty() {
                let dir_str = dir.display().to_string();
                host.push_info(&crate::i18n::tr_args(
                    "cmd-no-scripts-found",
                    &[("dir", dir_str.as_str().into())],
                ));
            } else {
                host.push_info(&crate::i18n::tr_args(
                    "cmd-available-scripts",
                    &[("count", scripts.len().into())],
                ));
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
                    let dir_str = dir.display().to_string();
                    host.push_info(&crate::i18n::tr_args(
                        "cmd-reloaded",
                        &[
                            ("scripts", scripts.len().into()),
                            ("dir", dir_str.as_str().into()),
                            ("commands", loaded.into()),
                        ],
                    ));
                }
                Err(e) => {
                    let err_str = e.to_string();
                    host.push_error(&crate::i18n::tr_args(
                        "cmd-reload-failed",
                        &[("error", err_str.as_str().into())],
                    ));
                }
            }
            true
        }
        "LSPDIR" | "PYDIR" => {
            let dir = crate::rust_lisp::scripts_dir();
            let dir_str = dir.display().to_string();
            host.push_info(&crate::i18n::tr_args(
                "cmd-scripts-dir",
                &[("dir", dir_str.as_str().into())],
            ));
            true
        }
        "LSPERR" | "LISPERR" => {
            if let Some(err) = crate::rust_lisp::get_last_lisp_error() {
                let title = crate::i18n::tr("cmd-last-error-title");
                if crate::ide::server::has_active_clients() {
                    crate::ide::server::ide_log(&format!("\n[{title}] {err}\n"), true);
                }
                crate::rust_lisp::push_formatted_error(host, &title, &err);
            } else {
                host.push_info(&crate::i18n::tr("cmd-no-errors"));
            }
            true
        }
        "PY" | "PYRUN" | "PYLOAD" | "PYCMD" | "PYDEBUG" | "PYDEBUG_CMD" | "PYTRUST" | "PYUNTRUST" | "PYTRUST_LIST" | "PYSECURITY" => {
            host.push_info(&crate::i18n::tr("cmd-python-shelved"));
            true
        }
        _ => {
            // Check if matches a registered OpenCAD LISP command in the active drawing tab (from defun c:NAME)
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
            exe_candidates.push(dir.join("opencad_lisp_ide.exe"));
            exe_candidates.push(dir.join("opencad_lisp_ide"));
        }
    }
    if let Ok(curr) = std::env::current_exe() {
        if let Some(dir) = curr.parent() {
            exe_candidates.push(dir.join("opencad_lisp_ide.exe"));
            exe_candidates.push(dir.join("opencad_lisp_ide"));
        }
    }
    exe_candidates.push(std::path::PathBuf::from("opencad_lisp_ide.exe"));
    exe_candidates.push(std::path::PathBuf::from("opencad_lisp_ide"));

    let host_lang = ocs_plugin_api::host_language();
    let spawn_with_lang = |mut cmd: std::process::Command| {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(ref lang) = host_lang {
            cmd.env(ocs_plugin_api::HOST_LANGUAGE_ENV, lang);
        }
        let _ = cmd.spawn();
    };

    for candidate in exe_candidates {
        if candidate.exists() {
            spawn_with_lang(std::process::Command::new(candidate));
            return;
        }
    }
    spawn_with_lang(std::process::Command::new("opencad_lisp_ide"));
}
