use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use ocs_plugin_api::dialog::{
    DialogActionResponse, DialogControl, DialogDef, DialogEvent, DialogResponse, DialogValue,
    HorizontalAlign,
};
use ocs_plugin_api::host::{CommandStep, HostApi, InteractiveCommand};

/// Persistent list of OpenCAD LISP (.lsp) files that are automatically loaded into every new drawing.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StartupScripts {
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

/// Returns the path to `opencad_lisp_startup.json` in the OpenCAD Studio configuration directory.
/// Can be overridden via `OCS_STARTUP_SCRIPTS_FILE` for isolated testing.
pub fn startup_scripts_file() -> PathBuf {
    if let Ok(custom) = std::env::var("OCS_STARTUP_SCRIPTS_FILE") {
        let trimmed = custom.trim();
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
    base.join("OpenCADStudio").join("opencad_lisp_startup.json")
}

/// Deduplicates a slice of file paths while preserving order (case-insensitive on Windows).
pub fn deduplicate_startup_paths(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut deduped: Vec<PathBuf> = Vec::new();
    for path in files {
        let s = path.to_string_lossy();
        if s.trim().is_empty() {
            continue;
        }
        let cleaned = PathBuf::from(s.trim());
        let exists = deduped.iter().any(|existing| {
            if cfg!(windows) {
                existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&cleaned.to_string_lossy())
            } else {
                existing == &cleaned
            }
        });
        if !exists {
            deduped.push(cleaned);
        }
    }
    deduped
}

/// Load the Startup Scripts list from a specific JSON file path.
pub fn load_startup_scripts_from(path: &Path) -> Vec<PathBuf> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<StartupScripts>(&content) else {
        return Vec::new();
    };
    deduplicate_startup_paths(&parsed.files)
}

/// Load the remembered Startup Scripts list from the default config location.
pub fn load_startup_scripts_list() -> Vec<PathBuf> {
    load_startup_scripts_from(&startup_scripts_file())
}

/// Save the Startup Scripts list to a specific JSON file path.
pub fn save_startup_scripts_to(path: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let data = StartupScripts {
        files: deduplicate_startup_paths(files),
    };
    let json = serde_json::to_string_pretty(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Save the remembered Startup Scripts list to the default config location.
pub fn save_startup_scripts_list(files: &[PathBuf]) -> std::io::Result<()> {
    save_startup_scripts_to(&startup_scripts_file(), files)
}

/// Automatically load all remembered Startup Scripts into the active drawing using `host`.
pub fn auto_load_startup_scripts(host: &mut dyn HostApi) -> usize {
    let files = load_startup_scripts_list();
    let mut loaded = 0;
    for path in files {
        if path.is_file() {
            if crate::rust_lisp::load_lisp_file(&path.to_string_lossy(), host) {
                loaded += 1;
            }
        } else {
            let path_str = path.display().to_string();
            host.push_error(&crate::i18n::tr_args(
                "appload-err-not-found",
                &[("path", path_str.as_str().into())],
            ));
        }
    }
    loaded
}

/// Automatically load all remembered Startup Scripts into the active drawing without a direct `HostApi` ref.
pub fn auto_load_startup_scripts_headless() -> usize {
    let files = load_startup_scripts_list();
    let mut loaded = 0;
    for path in files {
        if path.is_file() {
            match crate::rust_lisp::load_lisp_file_headless(&path) {
                Ok(()) => {
                    loaded += 1;
                }
                Err(err) => {
                    if let Some(sender) = crate::context::get_global_sender() {
                        let path_str = path.display().to_string();
                        let _ = sender.request(ocs_plugin_api::ipc::protocol::PluginRequest::PushError(
                            crate::i18n::tr_args(
                                "appload-err-load-failed",
                                &[
                                    ("path", path_str.as_str().into()),
                                    ("error", err.as_str().into()),
                                ],
                            ),
                        ));
                    }
                }
            }
        } else if let Some(sender) = crate::context::get_global_sender() {
            let path_str = path.display().to_string();
            let _ = sender.request(ocs_plugin_api::ipc::protocol::PluginRequest::PushError(
                crate::i18n::tr_args(
                    "appload-err-not-found",
                    &[("path", path_str.as_str().into())],
                ),
            ));
        }
    }
    loaded
}

/// Recursively flatten any nested `DialogValue::Fields` maps so control IDs inside
/// `DialogControl::Fields` sections can be looked up directly by ID.
pub fn flatten_dialog_values(values: &HashMap<String, DialogValue>) -> HashMap<String, DialogValue> {
    fn visit(src: &HashMap<String, DialogValue>, out: &mut HashMap<String, DialogValue>) {
        for (k, v) in src {
            match v {
                DialogValue::Fields(inner) => {
                    visit(inner, out);
                }
                other => {
                    out.insert(k.clone(), other.clone());
                }
            }
        }
    }
    let mut flat = HashMap::new();
    visit(values, &mut flat);
    flat
}


/// Interactive command that displays the modal `APPLOAD` dialog.
pub struct ApploadCommand {
    pub startup_files: Vec<PathBuf>,
    pub dialog_open: bool,
    pub startup_only: bool,
}

impl ApploadCommand {
    pub fn new() -> Self {
        Self {
            startup_files: load_startup_scripts_list(),
            dialog_open: true,
            startup_only: false,
        }
    }

    pub fn startup_only() -> Self {
        Self {
            startup_files: load_startup_scripts_list(),
            dialog_open: true,
            startup_only: true,
        }
    }

    pub fn with_startup_files(startup_files: Vec<PathBuf>) -> Self {
        Self {
            startup_files: deduplicate_startup_paths(&startup_files),
            dialog_open: true,
            startup_only: false,
        }
    }

    /// Process a dialog submission using a custom config path (used by unit tests and `on_dialog_response`).
    pub fn process_response_with_file(
        &mut self,
        response: DialogResponse,
        config_path: &Path,
    ) -> CommandStep {
        self.dialog_open = false;
        match response {
            DialogResponse::Submitted(raw_values) => {
                let values = flatten_dialog_values(&raw_values);

                let load_file = values.get("load_file").and_then(|v| match v {
                    DialogValue::FilePath(s) | DialogValue::Text(s) => {
                        let trimmed = s.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    }
                    _ => None,
                });

                let mut updated_startup: Vec<PathBuf> = Vec::new();
                if let Some(DialogValue::Table(rows)) = values.get("startup_files") {
                    for row in rows {
                        if let Some(first) = row.first() {
                            let trimmed = first.trim();
                            if !trimmed.is_empty() {
                                updated_startup.push(PathBuf::from(trimmed));
                            }
                        }
                    }
                } else {
                    updated_startup.extend(self.startup_files.iter().cloned());
                }

                let startup_add_file = values.get("startup_add_file").and_then(|v| match v {
                    DialogValue::FilePath(s) | DialogValue::Text(s) => {
                        let trimmed = s.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(trimmed))
                        }
                    }
                    _ => None,
                });

                if let Some(ref add_path) = startup_add_file {
                    updated_startup.push(add_path.clone());
                }

                let also_add = values
                    .get("also_add_to_startup")
                    .and_then(|v| match v {
                        DialogValue::Bool(b) => Some(*b),
                        _ => None,
                    })
                    .unwrap_or(false);

                if also_add {
                    if let Some(ref lf) = load_file {
                        updated_startup.push(PathBuf::from(lf));
                    }
                }

                let deduped = deduplicate_startup_paths(&updated_startup);
                let prev_set = self.startup_files.clone();
                self.startup_files = deduped.clone();

                if let Err(e) = save_startup_scripts_to(config_path, &deduped) {
                    if let Some(sender) = crate::context::get_global_sender() {
                        let err_str = e.to_string();
                        let _ = sender.request(
                            ocs_plugin_api::ipc::protocol::PluginRequest::PushError(
                                crate::i18n::tr_args(
                                    "appload-err-save-failed",
                                    &[("error", err_str.as_str().into())],
                                ),
                            ),
                        );
                    }
                } else if deduped != prev_set {
                    if let Some(sender) = crate::context::get_global_sender() {
                        let _ = sender.request(
                            ocs_plugin_api::ipc::protocol::PluginRequest::PushInfo(
                                crate::i18n::tr_args(
                                    "appload-info-updated",
                                    &[("count", deduped.len().into())],
                                ),
                            ),
                        );
                    }
                }

                // Load any newly added startup scripts into the current drawing immediately as well.
                for path in &deduped {
                    let is_already_in_prev = prev_set.iter().any(|p| {
                        if cfg!(windows) {
                            p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy())
                        } else {
                            p == path
                        }
                    });
                    let matches_load_file = load_file.as_ref().is_some_and(|lf| {
                        if cfg!(windows) {
                            Path::new(lf)
                                .to_string_lossy()
                                .eq_ignore_ascii_case(&path.to_string_lossy())
                        } else {
                            Path::new(lf) == path.as_path()
                        }
                    });
                    if !is_already_in_prev && !matches_load_file && path.is_file() {
                        let _ = crate::rust_lisp::load_lisp_file_headless(path);
                    }
                }

                if let Some(lf) = load_file {
                    let lf_path = Path::new(&lf);
                    if lf_path.is_file() {
                        let _ = crate::rust_lisp::load_lisp_file_headless(lf_path);
                    }
                    CommandStep::Dispatch(format!("APPLOAD \"{lf}\""))
                } else {
                    CommandStep::Done
                }
            }
            DialogResponse::Cancelled => CommandStep::Done,
        }
    }
}

impl Default for ApploadCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveCommand for ApploadCommand {
    fn prompt(&self) -> String {
        if self.startup_only {
            "STARTUP".to_string()
        } else {
            "APPLOAD".to_string()
        }
    }

    fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
        CommandStep::Done
    }

    fn modal_dialog(&self) -> Option<DialogDef> {
        if !self.dialog_open {
            return None;
        }

        let rows: Vec<Vec<String>> = self
            .startup_files
            .iter()
            .map(|p| vec![p.to_string_lossy().to_string()])
            .collect();

        if self.startup_only {
            let dialog = DialogDef::new(crate::i18n::tr("appload-startup-title"))
                .width(580.0)
                .submit_label(crate::i18n::tr("appload-btn-ok"))
                .cancel_label(crate::i18n::tr("appload-btn-cancel"))
                .with_control(DialogControl::FilePicker {
                    id: "startup_add_file".to_string(),
                    label: crate::i18n::tr("appload-label-pick-startup"),
                    default_path: String::new(),
                    filter_name: crate::i18n::tr("appload-filter-lisp"),
                    extensions: vec!["lsp".to_string(), "lisp".to_string()],
                    save: false,
                })
                .with_control(DialogControl::InfoText {
                    text: crate::i18n::tr("appload-info-auto-add"),
                    align_h: Some(HorizontalAlign::Left),
                })
                .with_control(DialogControl::Table {
                    id: "startup_files".to_string(),
                    label: Some(crate::i18n::tr("appload-table-startup-only")),
                    headers: vec![crate::i18n::tr("appload-col-path")],
                    rows,
                    allow_add: true,
                    allow_delete: true,
                });
            return Some(dialog);
        }

        let dialog = DialogDef::new(crate::i18n::tr("appload-title"))
            .width(580.0)
            .submit_label(crate::i18n::tr("appload-btn-ok"))
            .cancel_label(crate::i18n::tr("appload-btn-cancel"))
            .with_control(DialogControl::Fields {
                id: "load_section".to_string(),
                title: Some(crate::i18n::tr("appload-section-load")),
                collapsed: false,
                controls: vec![
                    DialogControl::FilePicker {
                        id: "load_file".to_string(),
                        label: crate::i18n::tr("appload-label-load-file"),
                        default_path: String::new(),
                        filter_name: crate::i18n::tr("appload-filter-lisp"),
                        extensions: vec!["lsp".to_string(), "lisp".to_string()],
                        save: false,
                    },
                    DialogControl::Checkbox {
                        id: "also_add_to_startup".to_string(),
                        label: crate::i18n::tr("appload-checkbox-add-startup"),
                        default: false,
                    },
                ],
            })
            .with_control(DialogControl::Fields {
                id: "startup_section".to_string(),
                title: Some(crate::i18n::tr("appload-section-startup")),
                collapsed: false,
                controls: vec![
                    DialogControl::FilePicker {
                        id: "startup_add_file".to_string(),
                        label: crate::i18n::tr("appload-label-pick-startup"),
                        default_path: String::new(),
                        filter_name: crate::i18n::tr("appload-filter-lisp"),
                        extensions: vec!["lsp".to_string(), "lisp".to_string()],
                        save: false,
                    },
                    DialogControl::InfoText {
                        text: crate::i18n::tr("appload-info-auto-add"),
                        align_h: Some(HorizontalAlign::Left),
                    },
                    DialogControl::Table {
                        id: "startup_files".to_string(),
                        label: Some(crate::i18n::tr("appload-table-remembered")),
                        headers: vec![crate::i18n::tr("appload-col-path")],
                        rows,
                        allow_add: true,
                        allow_delete: true,
                    },
                ],
            });

        Some(dialog)
    }

    fn on_dialog_action(&mut self, event: DialogEvent) -> DialogActionResponse {
        if event.control_id == "startup_add_file" && event.action == "pick" {
            if let Some(DialogValue::FilePath(p)) = event.value {
                let trimmed = p.trim();
                if !trimmed.is_empty() {
                    let path = PathBuf::from(trimmed);
                    self.startup_files.push(path.clone());
                    self.startup_files = deduplicate_startup_paths(&self.startup_files);
                    let rows: Vec<Vec<String>> = self
                        .startup_files
                        .iter()
                        .map(|f| vec![f.to_string_lossy().to_string()])
                        .collect();
                    let mut updates = HashMap::new();
                    updates.insert("startup_files".to_string(), DialogValue::Table(rows));
                    updates.insert("startup_add_file".to_string(), DialogValue::FilePath(String::new()));
                    let filename = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let status_msg = crate::i18n::tr_args(
                        "appload-status-added",
                        &[("file", filename.as_str().into())],
                    );
                    updates.insert(
                        "startup_info".to_string(),
                        DialogValue::Text(status_msg.clone()),
                    );
                    updates.insert(
                        crate::i18n::tr("appload-info-auto-add"),
                        DialogValue::Text(status_msg),
                    );
                    return DialogActionResponse::UpdateValues(updates);
                }
            }
        } else if event.control_id == "startup_files" && event.action == "remove_row" {
            if let Some(DialogValue::Number(idx)) = event.value {
                let row_idx = idx as usize;
                if row_idx < self.startup_files.len() {
                    let removed = self.startup_files.remove(row_idx);
                    let filename = removed.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let mut updates = HashMap::new();
                    let status_msg = crate::i18n::tr_args(
                        "appload-status-removed",
                        &[("file", filename.as_str().into())],
                    );
                    updates.insert(
                        "startup_info".to_string(),
                        DialogValue::Text(status_msg.clone()),
                    );
                    updates.insert(
                        crate::i18n::tr("appload-info-auto-add"),
                        DialogValue::Text(status_msg),
                    );
                    return DialogActionResponse::UpdateValues(updates);
                }
            }
        }
        DialogActionResponse::None
    }

    fn on_dialog_response(&mut self, response: DialogResponse) -> CommandStep {
        let path = startup_scripts_file();
        self.process_response_with_file(response, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_scripts_persistence_and_deduplication() {
        let temp_dir = std::env::temp_dir().join(format!("ocs_appload_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let config_path = temp_dir.join("opencad_lisp_startup.json");

        let input = vec![
            PathBuf::from("C:/cad/scripts/beam.lsp"),
            PathBuf::from("C:/cad/scripts/grid.lsp"),
            PathBuf::from("C:/cad/scripts/beam.lsp"), // duplicate
            PathBuf::from(""),                        // empty
        ];

        save_startup_scripts_to(&config_path, &input).expect("save startup scripts");
        let loaded = load_startup_scripts_from(&config_path);
        assert_eq!(
            loaded,
            vec![
                PathBuf::from("C:/cad/scripts/beam.lsp"),
                PathBuf::from("C:/cad/scripts/grid.lsp"),
            ]
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_appload_modal_dialog_nested_fields_submission_and_close() {
        let temp_dir = std::env::temp_dir().join(format!("ocs_appload_dlg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let config_path = temp_dir.join("opencad_lisp_startup.json");

        let mut cmd = ApploadCommand::with_startup_files(vec![PathBuf::from("C:/existing/init.lsp")]);
        let dlg = cmd.modal_dialog().expect("modal dialog should be open initially");
        assert_eq!(dlg.title, "Load/Unload Applications (APPLOAD)");
        assert_eq!(dlg.rows.len(), 2);

        // Simulate OpenCADStudio's PluginDialogState::collect_response(), which nests
        // controls inside DialogValue::Fields under "load_section" and "startup_section".
        let mut load_section = HashMap::new();
        load_section.insert(
            "load_file".to_string(),
            DialogValue::FilePath("C:/my tools/new_tool.lsp".to_string()),
        );
        load_section.insert("also_add_to_startup".to_string(), DialogValue::Bool(true));

        let mut startup_section = HashMap::new();
        startup_section.insert(
            "startup_files".to_string(),
            DialogValue::Table(vec![
                vec!["C:/existing/init.lsp".to_string()],
                vec!["D:/extra/helper.lsp".to_string()],
            ]),
        );

        let mut values = HashMap::new();
        values.insert("load_section".to_string(), DialogValue::Fields(load_section));
        values.insert("startup_section".to_string(), DialogValue::Fields(startup_section));

        let step = cmd.process_response_with_file(DialogResponse::Submitted(values), &config_path);
        assert!(
            cmd.modal_dialog().is_none(),
            "modal_dialog() must return None after response so OpenCAD Studio closes the modal"
        );
        match step {
            CommandStep::Dispatch(line) => {
                assert_eq!(line, "APPLOAD \"C:/my tools/new_tool.lsp\"");
            }
            other => panic!("Expected CommandStep::Dispatch, got {:?}", other),
        }

        let saved = load_startup_scripts_from(&config_path);
        assert_eq!(
            saved,
            vec![
                PathBuf::from("C:/existing/init.lsp"),
                PathBuf::from("D:/extra/helper.lsp"),
                PathBuf::from("C:/my tools/new_tool.lsp"),
            ]
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    #[test]
    fn test_startup_only_command_modal_dialog() {
        let temp_dir = std::env::temp_dir().join(format!("ocs_startup_dlg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let config_path = temp_dir.join("opencad_lisp_startup.json");

        let mut cmd = ApploadCommand::startup_only();
        assert_eq!(cmd.prompt(), "STARTUP");
        let dlg = cmd.modal_dialog().expect("modal dialog should open");
        assert_eq!(dlg.title, "Startup Scripts");
        assert_eq!(dlg.submit_label, "OK");
        assert_eq!(dlg.cancel_label, "Cancel");

        let mut values = HashMap::new();
        values.insert(
            "startup_add_file".to_string(),
            DialogValue::FilePath("C:/scripts/auto_init.lsp".to_string()),
        );
        let step = cmd.process_response_with_file(DialogResponse::Submitted(values), &config_path);
        assert!(matches!(step, CommandStep::Done));
        assert!(cmd.modal_dialog().is_none());

        let saved = load_startup_scripts_from(&config_path);
        assert!(saved.iter().any(|p| p.to_string_lossy().contains("auto_init.lsp")));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_appload_command_on_dialog_action_pick_and_remove() {
        let mut cmd = ApploadCommand::startup_only();
        cmd.startup_files.clear();
        assert_eq!(cmd.startup_files.len(), 0);

        // 1. Pick a file -> should immediately update table and reset picker path
        let pick_event = DialogEvent {
            control_id: "startup_add_file".to_string(),
            action: "pick".to_string(),
            value: Some(DialogValue::FilePath("C:/scripts/instant_script.lsp".to_string())),
            current_values: HashMap::new(),
        };

        let action_resp = cmd.on_dialog_action(pick_event);
        match action_resp {
            DialogActionResponse::UpdateValues(updates) => {
                assert_eq!(
                    updates.get("startup_add_file"),
                    Some(&DialogValue::FilePath(String::new()))
                );
                assert_eq!(
                    updates.get("startup_files"),
                    Some(&DialogValue::Table(vec![vec![
                        "C:/scripts/instant_script.lsp".to_string()
                    ]]))
                );
                assert!(updates.contains_key("startup_info"));
            }
            other => panic!("Expected DialogActionResponse::UpdateValues, got {:?}", other),
        }
        assert_eq!(cmd.startup_files.len(), 1);

        // 2. Remove row -> should update table
        let remove_event = DialogEvent {
            control_id: "startup_files".to_string(),
            action: "remove_row".to_string(),
            value: Some(DialogValue::Number(0.0)),
            current_values: HashMap::new(),
        };

        let action_resp = cmd.on_dialog_action(remove_event);
        match action_resp {
            DialogActionResponse::UpdateValues(updates) => {
                assert_eq!(
                    updates.get("startup_info"),
                    Some(&DialogValue::Text("Removed \"instant_script.lsp\" from startup scripts.".to_string()))
                );
            }
            other => panic!("Expected DialogActionResponse::UpdateValues, got {:?}", other),
        }
        assert_eq!(cmd.startup_files.len(), 0);
    }
}