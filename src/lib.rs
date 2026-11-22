pub mod context;
pub mod interactive;
pub mod dispatcher;
pub mod ribbon;
pub mod ide;
pub mod rust_lisp;
pub mod appload;

#[cfg(feature = "python")]
#[path = "../archive/python_shelved/py_module.rs"]
pub mod py_module;
#[cfg(feature = "python")]
#[path = "../archive/python_shelved/py_runtime.rs"]
pub mod py_runtime;
#[cfg(feature = "python")]
#[path = "../archive/python_shelved/py_debug.rs"]
pub mod py_debug;
#[cfg(feature = "python")]
#[path = "../archive/python_shelved/py_sandbox.rs"]
pub mod py_sandbox;

use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{BuiltinPlugin, HostApi, HostNotification};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::CadModule;

pub static MANIFEST: PluginManifest = PluginManifest {
    id: "opencad.autolisp_and_ide",
    name: "OpenCAD AutoLISP & IDE",
    version: env!("CARGO_PKG_VERSION"),
    description: "Run AutoLISP routines, commands, and OCAIDE inside OpenCAD Studio",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 150,
    xdata_apps: &[],
    command_prefixes: &["OCAIDE", "LSP", "LISP", "LSPRUN", "LSPLIST", "LSPRELOAD", "LSPDIR", "LSPERR", "LISPERR", "APPLOAD", "LSPFLUSH"],
};

pub struct AutoLispPlugin;

impl BuiltinPlugin for AutoLispPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(ribbon::AutoLispRibbonModule)
    }

    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        if let Some(sender) = host.plugin_request_sender() {
            context::set_global_sender(Some(std::sync::Arc::from(sender)));
        }
        let handles = interactive::get_cached_selection();
        interactive::update_selection(handles);
        dispatcher::dispatch_command(host, cmd)
    }

    fn on_notification(&mut self, _command_id: Option<u64>, notification: HostNotification) {
        match notification {
            HostNotification::SelectionChanged { handles } => {
                let u64s: Vec<u64> = handles.into_iter().map(|h| h.value()).collect();
                interactive::update_selection(u64s.clone());
                ide::server::ide_selection_changed(u64s);
            }
            HostNotification::SelectionChangedV4 { tab_id, handles } => {
                context::set_active_tab_id(tab_id);
                let u64s: Vec<u64> = handles.into_iter().map(|h| h.value()).collect();
                interactive::update_selection(u64s.clone());
                ide::server::ide_selection_changed(u64s);
            }
            HostNotification::DocumentOpened { tab_id } => {
                rust_lisp::clean_tab_state(tab_id);
                context::set_active_tab_id(tab_id);
                let _ = rust_lisp::auto_load_lisp_scripts_headless();
            }
            HostNotification::DocumentTabClosed { tab_id } => {
                rust_lisp::clean_tab_state(tab_id);
            }
            HostNotification::DocumentChangedV4 { tab_id, .. }
            | HostNotification::CommandStateChanged { tab_id, .. }
            | HostNotification::DrawingChanged { tab_id, .. }
            | HostNotification::DocumentChangedRegions { tab_id, .. }
            | HostNotification::DocumentChangedDelta { tab_id, .. } => {
                context::set_active_tab_id(tab_id);
            }
            HostNotification::PanelAction { panel_id, control_id, action, value } => {
                rust_lisp::handle_panel_action(&panel_id, &control_id, &action, value.as_ref());
            }
            _ => {}
        }
    }

    fn on_load(&mut self, host: &mut dyn HostApi) {
        let _guard = context::HostGuard::new(host);
        if let Some(sender) = host.plugin_request_sender() {
            context::set_global_sender(Some(std::sync::Arc::from(sender)));
        }
        ide::server::start_ide_server();
        scrub_stale_pgp_aliases();
        if let Ok(dir) = rust_lisp::ensure_scripts_dir() {
            let scripts = rust_lisp::list_lisp_scripts();
            let loaded = rust_lisp::auto_load_lisp_scripts(host);
            host.push_info(&format!(
                "OpenCAD AutoLISP loaded ({} .lsp scripts in {}, {} c:commands registered)",
                scripts.len(),
                dir.display(),
                loaded
            ));
        }
    }
}

/// Remove legacy `*PY c-` and `*PYCMD` alias entries from ocad.pgp so that
/// @oc.command decorated functions are the authoritative routing mechanism.
fn scrub_stale_pgp_aliases() {
    let pgp_path = if let Some(appdata) = std::env::var_os("APPDATA") {
        std::path::PathBuf::from(appdata)
            .join("OpenCADStudio")
            .join("ocad.pgp")
    } else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(&pgp_path) else {
        return;
    };
    let cleaned: String = content
        .lines()
        .filter(|line| {
            let upper = line.to_uppercase();
            // Remove lines that route through the old c- or c_ prefix conventions
            // or through the PYCMD dispatcher (these are all replaced by @oc.command).
            !upper.contains("*PY C-")
                && !upper.contains("*PY C_")
                && !upper.contains("*PYCMD ")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if cleaned != content {
        eprintln!("[plugin] Scrubbed stale legacy aliases from ocad.pgp");
        let _ = std::fs::write(&pgp_path, cleaned);
    }
}

export_plugin!(AutoLispPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::host::{CadDocumentReader, CommandStep, Document as CadDocument, DocumentReader, EntityType, Handle, InteractiveCommand};
    use std::any::Any;

    struct MockSender {
        commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        errors: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl ocs_plugin_api::host::PluginRequestSender for MockSender {
        fn request(
            &self,
            req: ocs_plugin_api::ipc::protocol::PluginRequest,
        ) -> Result<ocs_plugin_api::ipc::protocol::PluginResponse, ocs_plugin_api::host::PluginRequestError> {
            match req {
                ocs_plugin_api::ipc::protocol::PluginRequest::ExecuteCommand(cmd) => {
                    self.commands.lock().unwrap().push(cmd);
                    Ok(ocs_plugin_api::ipc::protocol::PluginResponse::Bool(true))
                }
                ocs_plugin_api::ipc::protocol::PluginRequest::PushError(err) => {
                    self.errors.lock().unwrap().push(err);
                    Ok(ocs_plugin_api::ipc::protocol::PluginResponse::Ok)
                }
                _ => Ok(ocs_plugin_api::ipc::protocol::PluginResponse::Ok),
            }
        }
    }

    struct MockHost {
        doc: CadDocument,
        pub infos: Vec<String>,
        pub outputs: Vec<String>,
        pub errors: Vec<String>,
        pub undos: Vec<String>,
        pub commands: Vec<String>,
        pub shared_commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        pub shared_errors: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        #[allow(dead_code)]
        pub aliases: Vec<(String, String)>,
        pub interactive_cmd: Option<Box<dyn InteractiveCommand>>,
        pub dirty: bool,
        pub bumped: bool,
        pub sysvars: std::collections::HashMap<String, ocs_plugin_api::host::HostSettingValue>,
        pub selection: Vec<Handle>,
        pub registered_panels: Vec<ocs_plugin_api::dialog::PanelDef>,
        pub toggled_panels: Vec<(String, Option<bool>)>,
    }

    impl MockHost {
        fn new() -> Self {
            let shared_commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let shared_errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut sysvars = std::collections::HashMap::new();
            sysvars.insert("CLAYER".to_string(), ocs_plugin_api::host::HostSettingValue::Text("0".to_string()));
            Self {
                doc: CadDocument::new(),
                infos: Vec::new(),
                outputs: Vec::new(),
                errors: Vec::new(),
                undos: Vec::new(),
                commands: Vec::new(),
                shared_commands,
                shared_errors,
                aliases: Vec::new(),
                interactive_cmd: None,
                dirty: false,
                bumped: false,
                sysvars,
                selection: Vec::new(),
                registered_panels: Vec::new(),
                toggled_panels: Vec::new(),
            }
        }
    }

    impl HostApi for MockHost {
        fn tab_index(&self) -> usize { 0 }
        fn document(&self) -> &CadDocument { &self.doc }
        fn document_mut(&mut self) -> &mut CadDocument { &mut self.doc }
        fn add_entity(&mut self, mut entity: EntityType) -> Handle {
            if entity.common().handle.is_null() {
                entity.as_entity_mut().set_handle(self.doc.allocate_handle());
            }
            self.doc.add_entity(entity).expect("entity added")
        }
        fn bump_geometry(&mut self) { self.bumped = true; }
        fn read_record(&self, _handle: Handle, _app_name: &str) -> Option<&ocs_plugin_api::host::codec::xdata::ExtendedDataRecord> { None }
        fn write_record(&mut self, _handle: Handle, _record: ocs_plugin_api::host::codec::xdata::ExtendedDataRecord) -> bool { true }
        fn remove_record(&mut self, _handle: Handle, _app_name: &str) -> bool { true }
        fn push_undo(&mut self, label: &str) { self.undos.push(label.to_string()); }
        fn set_dirty(&mut self) { self.dirty = true; }
        fn push_info(&mut self, msg: &str) { self.infos.push(msg.to_string()); }
        fn push_output(&mut self, msg: &str) { self.outputs.push(msg.to_string()); }
        fn push_error(&mut self, msg: &str) { self.errors.push(msg.to_string()); }
        fn start_interactive(&mut self, command: Box<dyn InteractiveCommand>) {
            self.interactive_cmd = Some(command);
        }
        fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> { None }
        fn plugin_state_any_mut(&mut self, _plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> { None }
        fn ensure_plugin_state_any(&mut self, _plugin_id: &'static str, _init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>) -> Option<&mut (dyn Any + Send + Sync)> {
            None
        }
        fn document_reader(&self) -> Box<dyn DocumentReader + '_> {
            Box::new(CadDocumentReader(&self.doc))
        }
        fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<Handle> {
            let trimmed = config.name.trim();
            if trimmed.is_empty() || self.doc.layers.contains(trimmed) {
                return None;
            }
            let mut layer = ocs_plugin_api::host::codec::tables::Layer::new(trimmed);
            let handle = self.doc.allocate_handle();
            layer.handle = handle;
            if let Some(c) = config.color { layer.color = c; }
            if let Some(lt) = config.linetype { layer.line_type = lt; }
            if let Some(lw) = config.lineweight { layer.line_weight = lw; }
            if let Some(off) = config.off { layer.flags.off = off; }
            if let Some(frz) = config.frozen { if frz { layer.freeze(); } else { layer.thaw(); } }
            if let Some(lck) = config.locked { layer.flags.locked = lck; }
            if let Some(plt) = config.plottable { layer.is_plottable = plt; }
            if let Some(tr) = config.transparency { layer.transparency = tr; }
            let _ = self.doc.layers.add(layer);
            Some(handle)
        }
        fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
            let trimmed = config.name.trim();
            if trimmed.is_empty() { return false; }
            let Some(layer) = self.doc.layers.get_mut(trimmed) else { return false; };
            if let Some(c) = config.color { layer.color = c; }
            if let Some(lt) = config.linetype { layer.line_type = lt; }
            if let Some(lw) = config.lineweight { layer.line_weight = lw; }
            if let Some(off) = config.off { layer.flags.off = off; }
            if let Some(frz) = config.frozen { if frz { layer.freeze(); } else { layer.thaw(); } }
            if let Some(lck) = config.locked { layer.flags.locked = lck; }
            if let Some(plt) = config.plottable { layer.is_plottable = plt; }
            if let Some(tr) = config.transparency { layer.transparency = tr; }
            true
        }
        fn plugin_request_sender(&self) -> Option<Box<dyn ocs_plugin_api::host::PluginRequestSender>> {
            Some(Box::new(MockSender {
                commands: std::sync::Arc::clone(&self.shared_commands),
                errors: std::sync::Arc::clone(&self.shared_errors),
            }))
        }
        fn execute_command(&mut self, cmd: &str) -> bool {
            self.commands.push(cmd.to_string());
            self.shared_commands.lock().unwrap().push(cmd.to_string());
            true
        }
        fn system_variable(&self, name: &str) -> Option<ocs_plugin_api::host::HostSettingValue> {
            self.sysvars.get(&name.to_ascii_uppercase()).cloned()
        }
        fn set_system_variable(&mut self, name: &str, value: ocs_plugin_api::host::HostSettingValue) -> Result<ocs_plugin_api::host::HostSettingValue, String> {
            self.sysvars.insert(name.to_ascii_uppercase(), value.clone());
            Ok(value)
        }
        fn selection(&self) -> Vec<Handle> {
            self.selection.clone()
        }
        fn set_selection(&mut self, handles: &[Handle]) -> Result<(), String> {
            self.selection = handles.to_vec();
            Ok(())
        }
        fn solid_operation(&mut self, _operation: ocs_plugin_api::host::SolidOperation) -> Result<Handle, String> {
            Ok(self.doc.allocate_handle())
        }
        fn table_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
            match operation {
                ocs_plugin_api::host::TableOperation::LayerCreate { config } => {
                    self.add_layer(config).ok_or_else(|| "Failed to create layer".to_string())
                }
                ocs_plugin_api::host::TableOperation::LayerModify { config } => {
                    if self.modify_layer(config) {
                        Ok(Handle::new(1))
                    } else {
                        Err("Failed to modify layer".to_string())
                    }
                }
                ocs_plugin_api::host::TableOperation::LayerRename { from, to } => {
                    if let Some(mut layer) = self.doc.layers.remove(&from) {
                        layer.name = to.clone();
                        let _ = self.doc.layers.add(layer);
                        Ok(Handle::new(1))
                    } else {
                        Err("Layer not found".to_string())
                    }
                }
                ocs_plugin_api::host::TableOperation::LayerDelete { name, .. } => {
                    if self.doc.layers.remove(&name).is_some() {
                        Ok(Handle::new(1))
                    } else {
                        Err("Layer not found".to_string())
                    }
                }
                ocs_plugin_api::host::TableOperation::LayoutSetCurrent { name } => {
                    self.sysvars.insert("CTAB".to_string(), ocs_plugin_api::host::HostSettingValue::Text(name));
                    Ok(Handle::new(1))
                }
                _ => Ok(self.doc.allocate_handle()),
            }
        }
        fn register_panel(&mut self, panel: ocs_plugin_api::dialog::PanelDef) -> bool {
            self.registered_panels.push(panel);
            true
        }
        fn toggle_panel(&mut self, panel_id: &str, show: Option<bool>) -> bool {
            self.toggled_panels.push((panel_id.to_string(), show));
            true
        }
    }

    #[cfg(feature = "python")]
    mod python_tests {
        use super::*;

        #[test]
        fn test_python_runtime_and_opencad_bindings() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // Test PYRUN inline code
        let ok = plugin.dispatch(&mut host, "PYRUN import opencad; opencad.info('Hello from OpenCAD test!')");
        assert!(ok);
        assert_eq!(host.infos.len(), 1);
        assert_eq!(host.infos[0], "Hello from OpenCAD test!");

        // Test PYRUN with direct functions (no import needed) and 'oc' alias
        let ok_direct = plugin.dispatch(&mut host, "PYRUN info('Direct function test'); oc.info('oc alias test')");
        assert!(ok_direct);
        assert_eq!(host.infos.len(), 3);
        assert_eq!(host.infos[1], "Direct function test");
        assert_eq!(host.infos[2], "oc alias test");

        // Test geometry creation and automatic layer creation (with 3D and 2D tuples/lists)
        let ok2 = plugin.dispatch(&mut host, "PYRUN import opencad\nh1 = opencad.add_point((10.0, 20.0, 0.0), layer='POINTS')\nh2 = opencad.add_line([0.0, 0.0], [100.0, 100.0], layer='LINES')\nh3 = opencad.add_circle((50.0, 50.0), 25.0, layer='CIRCLES')\nh4 = opencad.add_text('Test Text', (10.0, 20.0), height=3.0, layer='TEXT')\nopencad.add_layer('CUSTOM_LAYER', color=1)\nopencad.set_layer_color('CUSTOM_LAYER', 'green')\nopencad.command('ZOOM EXTENTS')\nopencad.set_current_layer('CUSTOM_LAYER')\nlayers = opencad.get_layers()\nprint(f'Count: {opencad.entity_count()}, Layers: {len(layers)}, Current: {opencad.get_current_layer()}')");
        assert!(ok2);
        assert_eq!(host.doc.entities().count(), 4);
        assert!(host.dirty);
        assert!(host.bumped);
        assert!(host.outputs.iter().any(|o| o.contains("Count: 4")));
        assert!(host.outputs.iter().any(|o| o.contains("Current: CUSTOM_LAYER")));
        assert!(host.doc.layers.contains("POINTS"));
        assert!(host.doc.layers.contains("LINES"));
        assert!(host.doc.layers.contains("CIRCLES"));
        assert!(host.doc.layers.contains("CUSTOM_LAYER"));
        assert_eq!(host.doc.layers.get("CUSTOM_LAYER").unwrap().color, ocs_plugin_api::host::codec::types::Color::from_index(3));

        // Test PYLIST command
        let ok3 = plugin.dispatch(&mut host, "PYLIST");
        assert!(ok3);
    }

    #[test]
    fn test_script_execution_with_args() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let test_script = test_dir.join("test_draw.py");
        std::fs::write(&test_script, r#"
import opencad

def run(args):
    radius = float(args[0]) if len(args) > 0 else 10.0
    opencad.add_circle((0.0, 0.0, 0.0), radius, layer="TEST_LAYER")
    print(f"Drew circle with radius {radius}")
"#).unwrap();

        let ok = plugin.dispatch(&mut host, "PY test_draw 42.5");
        assert!(ok);
        assert_eq!(host.doc.entities().count(), 1);
        assert!(host.outputs.iter().any(|o| o.contains("Drew circle with radius 42.5")));

        // Clean up test script
        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_case_insensitive_script_resolution() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-ci");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let test_script = test_dir.join("hello.py");
        std::fs::write(&test_script, r#"
import opencad
print("Hello executed successfully")
"#).unwrap();

        // Call with uppercase: PY:HELLO
        let ok = plugin.dispatch(&mut host, "PY:HELLO");
        assert!(ok);
        println!("ERRORS: {:?}", host.errors); println!("OUTPUTS: {:?}", host.outputs); assert!(host.outputs.iter().any(|o| o.contains("Hello executed successfully")));

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_spawn_process() {
        use std::path::{Path, PathBuf};
        use std::sync::Arc;
        let candidates = [
            PathBuf::from("../OpenCADStudio-fork/target/release/OpenCADStudio.exe"),
            PathBuf::from("../OpenCADStudio-fork/target/debug/OpenCADStudio.exe"),
            PathBuf::from("target/release/OpenCADStudio.exe"),
        ];
        let found = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .map(PathBuf::from)
            .or_else(|| candidates.into_iter().find(|p| p.exists()));
        let Some(exe_path) = found else {
            eprintln!("Skipping test_spawn_process: no OpenCADStudio.exe candidate found");
            return;
        };
        std::env::set_var("OCS_PLUGIN_RUNNER_EXE", &exe_path);
        let mut host = MockHost::new();
        let dummy_handler = Arc::new(|_: &str, _: Option<u64>, _: ocs_plugin_api::host::PluginNotification| {});
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dll_path = manifest_dir.join("target/release/opencad_autolisp.dll");
        if !dll_path.exists() {
            eprintln!("Skipping test_spawn_process: release dll not built yet");
            return;
        }
        let res = ocs_plugin_api::process::PluginProcess::spawn(
            dll_path,
            &mut host,
            dummy_handler,
        );
        match res {
            Ok(p) => {
                println!("Spawn succeeded! Plugin ID: {}, Name: {}", p.id(), p.manifest().name);
                assert_eq!(p.id(), "opencad.autolisp");
            }
            Err(e) => panic!("Spawn failed with: {:?}", e),
        }
    }

    #[test]
    fn test_on_load() {
        let mut host = MockHost::new();
        let mut plugin = AutoLispPlugin;
        plugin.on_load(&mut host);
    }

    #[test]
    fn test_python_error_handling() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let ok = plugin.dispatch(&mut host, "PYRUN 1 / 0");
        assert!(ok);
        assert!(!host.errors.is_empty());
        assert!(host.errors[0].contains("ZeroDivisionError"));
    }

    #[test]
    fn test_command_registration_solely_via_decorator() {
        let test_dir = std::env::temp_dir().join("ocs_test_scripts_decorators");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        // Files with c- or c_ prefixes should NOT automatically register as commands
        std::fs::write(test_dir.join("c-draw_grid.py"), "print('grid')").unwrap();
        std::fs::write(test_dir.join("c_dg.py"), "print('dg')").unwrap();
        std::fs::write(test_dir.join("normal.py"), "print('normal')").unwrap();

        // Only files with @oc.command should register commands
        let decorated_script = r#"
import opencad as oc

@oc.command("CUSTOM_BOX")
def make_box():
    oc.info("box created")

@oc.command
def my_widget():
    oc.info("widget created")
"#;
        std::fs::write(test_dir.join("decor_demo.py"), decorated_script).unwrap();

        let mut host = MockHost::new();
        let mut plugin = AutoLispPlugin;

        plugin.on_load(&mut host);

        let cmds = crate::py_module::get_registered_commands();
        // c- and c_ scripts should NOT be registered as commands
        assert!(!cmds.contains_key("DRAW_GRID") && !cmds.contains_key("DG") && !cmds.contains_key("NORMAL"));

        // @oc.command decorated functions SHOULD be registered
        assert!(cmds.contains_key("CUSTOM_BOX"));
        assert!(cmds.contains_key("MY_WIDGET"));

        // Test programmatic register_command still works
        let ok = plugin.dispatch(&mut host, "PYRUN register_command('DYNAMIC_CMD', 'normal')");
        assert!(ok);
        let cmds2 = crate::py_module::get_registered_commands();
        assert!(cmds2.contains_key("DYNAMIC_CMD"));

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_interactive_point_and_entity_picking() {
        let (bridge, mut cmd) = interactive::LispInteractiveBridge::new("Pick point: ".to_string(), interactive::InteractionMode::Point, 42, None);

        // Spawn a thread simulating a Python script calling get_point and get_entity
        let bridge_clone = std::sync::Arc::clone(&bridge);
        let worker = std::thread::spawn(move || {
            let pt = bridge_clone.get_point(Some("Specify first point: "));
            assert_eq!(pt, Some([10.0, 20.0, 30.0]));

            let ent = bridge_clone.get_entity(Some("Pick object: "));
            assert_eq!(ent, Some((42, [5.0, 6.0, 7.0])));

            let sel = bridge_clone.get_selection(Some("Select objects: "));
            assert_eq!(sel, Some(vec![101, 102]));

            bridge_clone.finish();
        });

        // Simulating the CAD host driving the InteractiveCommand
        // 1. Point pick
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(cmd.prompt(), "Specify first point: ");
        assert!(!cmd.needs_object_pick());
        cmd.on_point([10.0, 20.0, 30.0]);

        // 2. Entity pick
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(cmd.prompt(), "Pick object: ");
        assert!(cmd.needs_object_pick());
        cmd.on_object_pick(Handle::new(42), [5.0, 6.0, 7.0]);

        // 3. Selection set
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(cmd.prompt(), "Select objects: ");
        assert!(cmd.is_selection_gathering());
        cmd.on_object_pick(Handle::new(101), [1.0, 1.0, 0.0]);
        cmd.on_object_pick(Handle::new(102), [2.0, 2.0, 0.0]);
        cmd.on_enter(); // Finishes selection

        worker.join().expect("worker finished successfully");
        assert!(bridge.coordinator.is_done());
    }

    #[test]
    fn test_interactive_script_end_to_end_via_dispatch() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-interactive-dispatch");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
p1 = oc.get_point("Point 1: ")
if p1:
    p2 = oc.get_point("Point 2: ")
    if p2:
        oc.add_line(p1, p2)
"#;
        std::fs::write(test_dir.join("c-draw_line.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // Dispatch command - should start interactive command and return true
        let ok = plugin.dispatch(&mut host, "PY c-draw_line");
        assert!(ok, "dispatch should return true");
        assert!(host.interactive_cmd.is_some(), "interactive command should be started on host");

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Point 1: ");

        // User feeds point 1
        let step1 = cmd.on_point([1.0, 2.0, 3.0]);
        assert!(matches!(step1, CommandStep::NeedPoint));
        assert_eq!(cmd.prompt(), "Point 2: ");

        // User feeds point 2
        let step2 = cmd.on_point([10.0, 20.0, 30.0]);
        match step2 {
            CommandStep::CommitAndEnd(EntityType::Line(line)) => {
                assert_eq!(line.start.x, 1.0);
                assert_eq!(line.start.y, 2.0);
                assert_eq!(line.start.z, 3.0);
                assert_eq!(line.end.x, 10.0);
                assert_eq!(line.end.y, 20.0);
                assert_eq!(line.end.z, 30.0);
            }
            other => panic!("expected CommitAndEnd(Line), got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_interactive_get_point_then_cmd() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-interactive-cmd");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
@oc.command("testrect")
def testrect():
    pt = oc.get_point("Pick corner: ")
    if pt:
        x, y, _ = pt
        oc.cmd(f"LINE {x},{y} {x+40},{y+25} C")
"#;
        std::fs::write(test_dir.join("testrect.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // Auto-load decorated scripts to register 'testrect'
        let _ = crate::py_runtime::auto_load_decorated_scripts(&mut host);

        let ok = plugin.dispatch(&mut host, "testrect");
        assert!(ok, "dispatch should return true");
        assert!(host.interactive_cmd.is_some(), "interactive command should be started on host");

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Pick corner: ");

        // User feeds point
        let step = cmd.on_point([10.0, 20.0, 0.0]);
        // When Python transitions to oc.cmd(), on_point cleanly returns CommandStep::Done!
        assert!(matches!(step, CommandStep::Done));

        // Allow worker thread to execute oc.cmd
        std::thread::sleep(std::time::Duration::from_millis(100));
        let cmds = host.shared_commands.lock().unwrap();
        assert!(cmds.iter().any(|c| c.contains("LINE 10,20 50,45 C") || c.contains("LINE 10.0,20.0 50.0,45.0 C")), "Expected LINE command in shared_commands: {:?}", *cmds);

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_radical_stream_and_cancellation() {
        let test_dir = std::env::temp_dir().join(format!("ocs-test-radical-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
import math

@oc.command("rad_spiral")
def rad_spiral():
    oc.cmd("LINE 0,0")
    for i in range(1, 10):
        oc.cmd(f"{i*5},{i*5}")
    oc.cmd("")

@oc.command("rad_cancel_recover")
def rad_cancel_recover():
    pt = oc.get_point("Pick cancel point: ")
    if pt is None:
        oc.cmd("CIRCLE 0,0 20\n")
"#;
        std::fs::write(test_dir.join("rad_test.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let _ = crate::py_runtime::auto_load_decorated_scripts(&mut host);

        // 1. Test rapid streaming
        let ok = plugin.dispatch(&mut host, "rad_spiral");
        assert!(ok);
        {
            let cmds = host.shared_commands.lock().unwrap();
            assert!(cmds.iter().any(|c| c.contains("LINE 0,0")));
            assert!(cmds.iter().any(|c| c.contains("45,45")));
        }

        // 2. Test interactive cancellation and immediate recovery
        let ok_cancel = plugin.dispatch(&mut host, "rad_cancel_recover");
        assert!(ok_cancel);
        assert!(host.interactive_cmd.is_some());

        let cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Pick cancel point: ");
        // Host cancels by dropping active_cmd (e.g. user hits ESC)
        drop(cmd);

        // Allow worker thread to catch None and execute recovery oc.cmd("CIRCLE 0,0 20\n")
        std::thread::sleep(std::time::Duration::from_millis(150));
        {
            let cmds = host.shared_commands.lock().unwrap();
            assert!(cmds.iter().any(|c| c.contains("CIRCLE 0,0 20")), "Expected recovery CIRCLE in commands: {:?}", *cmds);
        }

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_decorated_commands_and_helpers() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-decorated");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        // 1. Create a tools module with helper functions and decorated commands
        let tools_code = r#"
import opencad as oc

# Reusable helper functions
def calc_midpoint(p1, p2):
    return ((p1[0] + p2[0]) / 2.0, (p1[1] + p2[1]) / 2.0, (p1[2] + p2[2]) / 2.0)

def offset_pt(pt, dx, dy):
    return (pt[0] + dx, pt[1] + dy, pt[2])

# Decorated commands
@oc.command
def my_box(width: float, height: float):
    """Draw a box from 0,0 with width and height"""
    w, h = float(width), float(height)
    oc.add_line((0.0, 0.0), (w, 0.0), layer="BOXES")
    oc.add_line((w, 0.0), (w, h), layer="BOXES")
    oc.add_line((w, h), (0.0, h), layer="BOXES")
    oc.add_line((0.0, h), (0.0, 0.0), layer="BOXES")
    oc.info(f"Created box {w}x{h}")
    return w * h

@oc.command("TRI")
def draw_triangle(base: float = 30.0, height: float = 40.0):
    """Draw a right triangle"""
    oc.add_line((0.0, 0.0), (base, 0.0), layer="TRIANGLES")
    oc.add_line((base, 0.0), (0.0, height), layer="TRIANGLES")
    oc.add_line((0.0, height), (0.0, 0.0), layer="TRIANGLES")
    oc.info(f"Created triangle with base={base}, height={height}")
"#;
        std::fs::write(test_dir.join("tools.py"), tools_code).unwrap();

        // 2. Create another script that imports both helpers and decorated commands
        let consumer_code = r#"
import opencad as oc
from tools import calc_midpoint, offset_pt, my_box

# Call helper functions
p1 = (0.0, 0.0, 0.0)
p2 = (100.0, 100.0, 0.0)
mid = calc_midpoint(p1, p2)
off = offset_pt(mid, 10.0, 20.0)

# Call decorated command programmatically as a regular function
area = my_box(50.0, 25.0)
oc.info(f"Imported helper midpoint={mid}, offset={off}, area={area}")
"#;
        std::fs::write(test_dir.join("consumer.py"), consumer_code).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // Test PYLOAD tools - should register MY_BOX and TRI
        let ok_load = plugin.dispatch(&mut host, "PYLOAD tools");
        assert!(ok_load);
        let cmds = crate::py_module::get_registered_commands();
        assert!(cmds.contains_key("MY_BOX"), "MY_BOX should be registered");
        assert!(cmds.contains_key("TRI"), "TRI alias should be registered");

        // Test running decorated command from console: MY_BOX 100 50
        let ok_box = plugin.dispatch(&mut host, "MY_BOX 100 50");
        assert!(ok_box);
        assert_eq!(host.doc.entities().count(), 4);
        assert!(host.infos.iter().any(|i| i.contains("Created box 100.0x50.0")));

        // Test running custom alias with defaults: TRI
        let ok_tri = plugin.dispatch(&mut host, "TRI");
        assert!(ok_tri);
        assert_eq!(host.doc.entities().count(), 7); // 4 from box + 3 from triangle
        assert!(host.infos.iter().any(|i| i.contains("base=30.0, height=40.0")));

        // Test running consumer script that imports helper and my_box
        let ok_consumer = plugin.dispatch(&mut host, "PY consumer");
        assert!(ok_consumer);
        assert_eq!(host.doc.entities().count(), 11); // 7 + 4 from my_box(50, 25)
        assert!(host.infos.iter().any(|i| i.contains("Imported helper midpoint=")));

        // Test PYLIST includes registered @commands
        let ok_list = plugin.dispatch(&mut host, "PYLIST");
        assert!(ok_list);
        assert!(host.outputs.iter().any(|o| o.contains("MY_BOX")));
        assert!(host.outputs.iter().any(|o| o.contains("TRI")));

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_pydebug_tcp_stepping_and_breakpoints() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use crate::py_debug::{DebugCommand, DebugEvent};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().unwrap().port();

        let script = "x = 10\ny = 20\nz = x + y\nimport opencad\nopencad.add_circle((0.0, 0.0), float(z))\n";
        let cmd = format!("PYDEBUG {port} 3 test_debug.py\n{script}");

        let handle = std::thread::spawn(move || {
            let mut host = MockHost::new();
            let plugin = AutoLispPlugin;
            let ok = plugin.dispatch(&mut host, &cmd);
            assert!(ok);
        });

        let (stream, _) = listener.accept().expect("accept connection");
        let mut writer = stream.try_clone().expect("clone stream");
        let mut reader = BufReader::new(stream);

        // 1. First event should be paused at line 3 (breakpoint)
        let mut line_buf = String::new();
        reader.read_line(&mut line_buf).expect("read paused event");
        let event: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event");
        match event {
            DebugEvent::Paused { line, locals, .. } => {
                assert_eq!(line, 3);
                assert!(locals.iter().any(|(k, _, v)| k == "x" && v == "10"));
                assert!(locals.iter().any(|(k, _, v)| k == "y" && v == "20"));
            }
            other => panic!("expected Paused, got {:?}", other),
        }

        // 2. Step Over to line 4
        let step_cmd = serde_json::to_string(&DebugCommand::StepOver).unwrap() + "\n";
        writer.write_all(step_cmd.as_bytes()).expect("write step over");
        writer.flush().unwrap();

        // 3. Receive paused event at line 4
        line_buf.clear();
        reader.read_line(&mut line_buf).expect("read stepped event");
        let event2: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event 2");
        match event2 {
            DebugEvent::Paused { line, locals, .. } => {
                assert_eq!(line, 4);
                assert!(locals.iter().any(|(k, _, v)| k == "z" && v == "30"));
            }
            other => panic!("expected Paused at line 4, got {:?}", other),
        }

        // 4. Continue execution to completion
        let cont_cmd = serde_json::to_string(&DebugCommand::Continue).unwrap() + "\n";
        writer.write_all(cont_cmd.as_bytes()).expect("write continue");
        writer.flush().unwrap();

        // 5. Receive finished event
        line_buf.clear();
        reader.read_line(&mut line_buf).expect("read finished event");
        let event3: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event 3");
        assert_eq!(event3, DebugEvent::Finished);

        handle.join().expect("debug worker completed");
    }

    #[test]
    fn test_pydebug_cmd_command_stepping_and_breakpoints() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use crate::py_debug::{DebugCommand, DebugEvent};

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // First load a script that defines @oc.command("TEST_BREAK")
        let script = "import opencad as oc\n\n@oc.command(\"TEST_BREAK\")\ndef test_break():\n    val = 42\n    oc.info(f\"val is {val}\")\n";
        let load_cmd = format!("PYRUN # file: test_cmd.py\n{script}");
        let ok = plugin.dispatch(&mut host, &load_cmd);
        assert!(ok);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().unwrap().port();

        // Line 5 is `val = 42`, Line 6 is `oc.info(...)`
        let debug_cmd = format!("PYDEBUG_CMD {port} test_cmd.py:5,6 PYCMD TEST_BREAK");

        let handle = std::thread::spawn(move || {
            let mut host2 = MockHost::new();
            let plugin2 = AutoLispPlugin;
            let ok = plugin2.dispatch(&mut host2, &debug_cmd);
            assert!(ok);
        });

        let (stream, _) = listener.accept().expect("accept connection");
        let mut writer = stream.try_clone().expect("clone stream");
        let mut reader = BufReader::new(stream);

        // 1. First event should be paused at line 5
        let mut line_buf = String::new();
        reader.read_line(&mut line_buf).expect("read paused event");
        let event: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event");
        match event {
            DebugEvent::Paused { line, .. } => {
                assert_eq!(line, 5);
            }
            other => panic!("expected Paused at line 5, got {:?}", other),
        }

        // 2. Continue to line 6
        let cont_cmd = serde_json::to_string(&DebugCommand::Continue).unwrap() + "\n";
        writer.write_all(cont_cmd.as_bytes()).expect("write continue");
        writer.flush().unwrap();

        line_buf.clear();
        reader.read_line(&mut line_buf).expect("read paused event 2");
        let event2: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event 2");
        match event2 {
            DebugEvent::Paused { line, locals, .. } => {
                assert_eq!(line, 6);
                assert!(locals.iter().any(|(k, _, v)| k == "val" && v == "42"));
            }
            other => panic!("expected Paused at line 6, got {:?}", other),
        }

        // 3. Continue to completion
        writer.write_all(cont_cmd.as_bytes()).expect("write continue");
        writer.flush().unwrap();

        line_buf.clear();
        reader.read_line(&mut line_buf).expect("read finished event");
        let event3: DebugEvent = serde_json::from_str(&line_buf).expect("parse debug event 3");
        assert_eq!(event3, DebugEvent::Finished);

        handle.join().expect("debug worker completed");
    }

    #[test]
    fn test_add_layer_full_properties_and_upsert() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // 1. Create a layer with full properties
        let script1 = r#"
import opencad as oc
h1 = oc.add_layer(
    "STRUCTURAL",
    color="yellow",
    linetype="DASHED",
    lineweight=0.35,
    locked=False,
    frozen=False,
    off=False,
    plottable=True,
    transparency=25
)
print(f"H1:{h1}")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script1}")));
        assert!(host.doc.layers.contains("STRUCTURAL"));
        let l1 = host.doc.layers.get("STRUCTURAL").unwrap();
        assert_eq!(l1.color, ocs_plugin_api::host::codec::types::Color::from_index(2));
        assert_eq!(l1.line_type, "DASHED");
        assert_eq!(l1.line_weight, ocs_plugin_api::host::codec::types::LineWeight::Value(35));
        assert!(!l1.flags.locked);
        assert!(!l1.flags.frozen);
        assert!(!l1.flags.off);
        assert!(l1.is_plottable);
        let h1_val = l1.handle.value();

        // 2. Modify existing layer in-place via modify_layer
        let script2 = r#"
import opencad as oc
ok = oc.modify_layer(
    "STRUCTURAL",
    color="magenta",
    locked=True
)
assert ok == True
print("MODIFIED_STRUCTURAL")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script2}")));
        let l2 = host.doc.layers.get("STRUCTURAL").unwrap();
        // Handle must remain the same
        assert_eq!(l2.handle.value(), h1_val);
        // Color and locked updated
        assert_eq!(l2.color, ocs_plugin_api::host::codec::types::Color::from_index(6));
        assert!(l2.flags.locked);
        // Untouched properties preserved!
        assert_eq!(l2.line_type, "DASHED");
        assert_eq!(l2.line_weight, ocs_plugin_api::host::codec::types::LineWeight::Value(35));
        assert!(l2.is_plottable);

        // 3. Verify modify_layer fails on nonexistent layer and does NOT create it
        let script_mod_nonexistent = r#"
import opencad as oc
try:
    oc.modify_layer("NONEXISTENT_LAYER", color="red")
    print("SHOULD_HAVE_FAILED")
except RuntimeError:
    print("MODIFY_NONEXISTENT_REJECTED")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script_mod_nonexistent}")));
        assert!(host.outputs.iter().any(|o| o.contains("MODIFY_NONEXISTENT_REJECTED")));
        assert!(!host.doc.layers.contains("NONEXISTENT_LAYER"));

        // 4. Verify add_layer fails if layer already exists
        let script_add_duplicate = r#"
import opencad as oc
try:
    oc.add_layer("STRUCTURAL", color="cyan")
    print("SHOULD_HAVE_FAILED")
except RuntimeError:
    print("ADD_DUPLICATE_REJECTED")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script_add_duplicate}")));
        assert!(host.outputs.iter().any(|o| o.contains("ADD_DUPLICATE_REJECTED")));

        // 5. Test get_layer function
        let script3 = r#"
import opencad as oc
data = oc.get_layer("STRUCTURAL")
assert data is not None
assert data["name"] == "STRUCTURAL"
assert data["locked"] == True
assert data["color_index"] == 6
assert data["linetype"] == "DASHED"
assert abs(data["lineweight"] - 0.35) < 0.001

# Test hidden linetype
h_hidden = oc.add_layer("TEST_HIDDEN", linetype="hidden")
data_hidden = oc.get_layer("TEST_HIDDEN")
assert data_hidden is not None
assert data_hidden["linetype"] == "hidden" or data_hidden["linetype"] == "HIDDEN"

# Test integer lineweight (e.g. 30 = 0.30mm)
h_lw30 = oc.add_layer("TEST_LW30", lineweight=30)
data_lw30 = oc.get_layer("TEST_LW30")
assert data_lw30 is not None
assert abs(data_lw30["lineweight"] - 0.30) < 0.001

print("GET_LAYER_VERIFIED")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script3}")));
        assert!(host.outputs.iter().any(|o| o.contains("GET_LAYER_VERIFIED")));
    }

    #[test]
    fn test_layer_props_exact() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

@oc.command("test_props")
def main():
    print("=== Testing Layer Add & Modify API ===")
    
    # 1. Add layer with specific color (Red = 1) and lineweight (30 = 0.30mm)
    print("1. Creating layer 'TEST_SURVEY' (Red, lineweight 30)...")
    oc.add_layer("TEST_SURVEY", color=1, lineweight=30)
    
    # 2. Add layer with defaults
    print("2. Creating layer 'TEST_DEFAULTS' (Default properties)...")
    oc.add_layer("TEST_DEFAULTS")
    
    # 3. Draw a circle on TEST_SURVEY
    print("3. Drawing circle on 'TEST_SURVEY'...")
    oc.add_circle((50, 50), 25, layer="TEST_SURVEY")
    
    # 4. Modify layer TEST_SURVEY to Green (color = 3), lineweight stays 30
    print("4. Modifying 'TEST_SURVEY' to Green (color = 3)...")
    oc.modify_layer("TEST_SURVEY", color=3)
    
    # 5. Verify modify_layer cannot create a new layer
    print("5. Verifying modify_layer fails on non-existent layer...")
    try:
        oc.modify_layer("DOES_NOT_EXIST", color=2)
        print("   ERROR: modify_layer should not have created 'DOES_NOT_EXIST'!")
    except Exception as e:
        print(f"   Success: modify_layer rejected non-existent layer ({e})")
        
    # 6. Verify add_layer cannot overwrite an existing layer
    print("6. Verifying add_layer fails if layer already exists...")
    try:
        oc.add_layer("TEST_SURVEY", color=4)
        print("   ERROR: add_layer should have rejected existing 'TEST_SURVEY'!")
    except Exception as e:
        print(f"   Success: add_layer rejected duplicate layer ({e})")

    print("\nAll tests completed! Check Ribbon dropdown for TEST_SURVEY (Green, 0.30mm) and TEST_DEFAULTS.")
"#;
        assert!(plugin.dispatch(&mut host, &format!("PYRUN {script}")));
        assert!(plugin.dispatch(&mut host, "TEST_PROPS"));
        assert!(host.outputs.iter().any(|o| o.contains("All tests completed!")));

        // Also test executing under active debug session (with tracer installed) on fresh host
        let mut host2 = MockHost::new();
        let bps = std::collections::HashMap::new();
        let _guard = context::HostGuard::new(&mut host2);
        let res = py_runtime::execute_pycmd_debug_raw("TEST_PROPS", &[], bps);
        assert!(res.is_ok(), "execute_pycmd_debug_raw failed: {:?}", res);
    }




    #[test]
    fn test_cmd_and_execute_command() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

res1 = oc.cmd("LINE 0,0 10,10")
assert res1 == True

res2 = oc.execute_command("ZOOM EXTENTS\n")
assert res2 == True

res3 = oc.cmd("LINE 0,0")
assert res3 == True
res4 = oc.cmd("20,20\n")
assert res4 == True

res5 = oc.cmd("\n")
assert res5 == True

res6 = oc.cmd("")
assert res6 == True

res7 = oc.cmd("ENTER")
assert res7 == True

res8 = oc.enter()
assert res8 == True

res9 = oc.cmd("CLAYER TEST\n")
assert res9 == True

res10 = oc.cmd("COLOR CYAN\n")
assert res10 == True

print("CMD_TESTS_PASSED")
"#;
        let dispatched = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        eprintln!("DISPATCHED: {dispatched}");
        eprintln!("ERRORS: {:?}", host.errors);
        eprintln!("COMMANDS: {:?}", host.commands);
        assert!(host.outputs.iter().any(|o| o.contains("CMD_TESTS_PASSED")));
        assert!(host.commands.contains(&"LINE 0,0 10,10".to_string()));
        assert!(host.commands.contains(&"ZOOM EXTENTS\n".to_string()));
        assert!(host.commands.contains(&"LINE 0,0".to_string()));
        assert!(host.commands.contains(&"20,20\n".to_string()));
        assert!(host.commands.contains(&"\n".to_string()));
        assert!(host.commands.contains(&"".to_string()));
        assert!(host.commands.contains(&"ENTER".to_string()));
        assert!(host.commands.contains(&"CLAYER TEST\n".to_string()));
        assert!(host.commands.contains(&"COLOR CYAN\n".to_string()));
    }

    #[test]
    fn test_v7_2d_entities() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc
import math

# 1. add_arc
h_arc = oc.add_arc((10, 20), 5.0, 0.0, math.pi, layer="ARCS", color="red")
assert h_arc > 0

# 2. add_ellipse with ratio
h_el1 = oc.add_ellipse((0, 0), (10, 0), ratio=0.5, layer="ELLIPSES")
assert h_el1 > 0

# 3. add_ellipse with minor_axis scalar
h_el2 = oc.add_ellipse((0, 0), (10, 0), minor_axis=3.0)
assert h_el2 > 0

# 4. add_polyline with bulges
pts = [(0, 0), (10, 0, 0.5), (10, 10), (0, 10)]
h_pline = oc.add_polyline(pts, is_closed=True, layer="PLINES", color="blue")
assert h_pline > 0

# 5. add_mtext with style and rotation
h_mtext = oc.add_mtext((5, 5), "Multi-line\nText", height=3.5, width=20.0, rotation=0.5, style="TITLE")
assert h_mtext > 0

# 6. add_ray
h_ray = oc.add_ray((0, 0), (10, 20), layer="RAYS")
assert h_ray > 0

# 7. add_xline
h_xline = oc.add_xline((0, 0), (0, 10), layer="XLINES")
assert h_xline > 0

# 8. add_solid (quad & triangle)
h_sol1 = oc.add_solid((0, 0), (10, 0), (10, 10), (0, 10), layer="SOLIDS")
assert h_sol1 > 0
h_sol2 = oc.add_solid((0, 0), (5, 5), (0, 5))
assert h_sol2 > 0

# 9. add_text with rotation and style
h_txt = oc.add_text("Styled Text", (1, 2), height=2.0, rotation=1.57, style="ROMANS", layer="ANNOTATION")
assert h_txt > 0

print("2D_ENTITIES_TESTS_PASSED")
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("2D_ENTITIES_TESTS_PASSED")));
        assert!(host.doc.layers.contains("ARCS"));
        assert!(host.doc.layers.contains("ELLIPSES"));
        assert!(host.doc.layers.contains("PLINES"));
        assert!(host.doc.layers.contains("RAYS"));
        assert!(host.doc.layers.contains("XLINES"));
        assert!(host.doc.layers.contains("SOLIDS"));
        assert!(host.doc.layers.contains("ANNOTATION"));
    }

    #[test]
    fn test_v7_3d_solids_and_booleans() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc
import math

# Box (axis-aligned and rotated)
h_b1 = oc.add_box((0, 0, 0), 10.0, 20.0, 30.0, layer="3D_SOLIDS")
assert h_b1 > 0
h_b2 = oc.add_box((5, 5, 0), 10.0, 10.0, 10.0, rotation=math.pi / 4.0)
assert h_b2 > 0

# Wedge
h_w = oc.add_wedge((0, 0, 0), 15.0, 10.0, 5.0, rotation=0.0)
assert h_w > 0

# Cylinder with height
h_cyl1 = oc.add_cylinder((0, 0, 0), 5.0, height=20.0)
assert h_cyl1 > 0

# Cylinder with axis_endpoint (tilted in 3D)
h_cyl2 = oc.add_cylinder((0, 0, 0), 5.0, axis_endpoint=(10.0, 10.0, 20.0))
assert h_cyl2 > 0

# Sphere
h_sph = oc.add_sphere((0, 0, 10), 8.0)
assert h_sph > 0

# Torus with and without axis_endpoint
h_tor1 = oc.add_torus((0, 0, 0), 20.0, 5.0)
assert h_tor1 > 0
h_tor2 = oc.add_torus((0, 0, 0), 20.0, 5.0, axis_endpoint=(0, 10, 10))
assert h_tor2 > 0

# Pyramid with and without axis_endpoint
h_pyr1 = oc.add_pyramid((0, 0, 0), 10.0, height=15.0, sides=6)
assert h_pyr1 > 0
h_pyr2 = oc.add_pyramid((0, 0, 0), 10.0, axis_endpoint=(0, 0, 25.0), sides=4)
assert h_pyr2 > 0

# CSG Booleans
h_union = oc.solid_union(h_b1, h_cyl1)
assert h_union > 0
h_sub = oc.solid_subtract(h_b1, h_sph)
assert h_sub > 0
h_inter = oc.solid_intersect(h_b1, h_tor1)
assert h_inter > 0

print("3D_SOLIDS_TESTS_PASSED")
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        if !ok {
            eprintln!("ERRORS: {:?}", host.errors);
        }
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("3D_SOLIDS_TESTS_PASSED")));
    }

    #[test]
    fn test_v7_tables_and_sysvars() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

# 1. Layer table operations via API v7
h_lay = oc.add_layer("ARCH_WALLS", color="magenta", linetype="CONTINUOUS")
assert h_lay > 0
assert oc.modify_layer("ARCH_WALLS", lineweight=0.35) == True
assert oc.rename_layer("ARCH_WALLS", "WALLS_EXTERIOR") == True
assert oc.delete_layer("WALLS_EXTERIOR") == True

# 2. Text style creation
h_style = oc.add_text_style("ARCH_TITLE", font_family="Arial", height=5.0)
assert h_style > 0

# 3. Block definition and insertion
p1 = oc.add_line((0, 0), (10, 0))
p2 = oc.add_line((10, 0), (10, 10))
h_blk = oc.define_block("DOOR_36", (0, 0), [p1, p2], description="Single door 36in")
assert h_blk > 0

h_ins = oc.insert_block("DOOR_36", (100, 100), scale=(2.0, 2.0, 2.0), rotation=1.57, layer="DOORS")
assert h_ins > 0

# 4. System variables
clayer = oc.get_var("CLAYER")
assert clayer == "0"
assert oc.set_var("CLAYER", "DOORS") == True
assert oc.get_var("CLAYER") == "DOORS"

# 5. Selection
oc.set_selection([h_ins, 1002, 1003])
sel = oc.get_selection()
assert sel == [h_ins, 1002, 1003]

print("TABLES_AND_SYSVARS_TESTS_PASSED")
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        if !ok {
            eprintln!("ERRORS: {:?}", host.errors);
        }
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("TABLES_AND_SYSVARS_TESTS_PASSED")));
    }

    #[test]
    fn test_sandbox_blocks_network_and_subprocesses() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import os
import sys

# 1. Network modules must be blocked
socket_blocked = False
try:
    import socket
except Exception:
    socket_blocked = True
assert socket_blocked, "socket import must be blocked"

# 2. Subprocess modules must be blocked
subproc_blocked = False
try:
    import subprocess
except Exception:
    subproc_blocked = True
assert subproc_blocked, "subprocess import must be blocked"

# 3. Dangerous os functions must be stripped
assert not hasattr(os, "system"), "os.system must be stripped"
assert not hasattr(os, "popen"), "os.popen must be stripped"
assert not hasattr(os, "startfile"), "os.startfile must be stripped"

# 4. ctypes must be blocked
ctypes_blocked = False
try:
    import ctypes
except Exception:
    ctypes_blocked = True
assert ctypes_blocked, "ctypes import must be blocked"

# 5. Tkinter (clipboard snooping / fake UI) must be blocked
tk_blocked = False
try:
    import tkinter
except Exception:
    tk_blocked = True
assert tk_blocked, "tkinter import must be blocked"

# 6. Reflection & stack climbing stripped
assert not hasattr(sys, "_getframe"), "sys._getframe must be stripped"
assert not hasattr(sys, "setrecursionlimit"), "sys.setrecursionlimit must be stripped"

# 7. Bytecode pollution disabled
assert sys.dont_write_bytecode == True, "dont_write_bytecode must be True"

print("SANDBOX_NETWORK_AND_SUBPROCESS_TESTS_PASSED")
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        if !ok {
            eprintln!("ERRORS: {:?}", host.errors);
        }
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("SANDBOX_NETWORK_AND_SUBPROCESS_TESTS_PASSED")));
    }

    #[test]
    fn test_sandbox_security_status_and_trust() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

status = oc.get_security_status()
assert status["sandbox_enabled"] == True
assert status["allow_network"] == False
assert status["allow_subprocesses"] == False
assert oc.is_sandboxed() == True

print("SANDBOX_SECURITY_STATUS_PASSED")
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        if !ok {
            eprintln!("ERRORS: {:?}", host.errors);
        }
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("SANDBOX_SECURITY_STATUS_PASSED")));
    }

    #[test]
    fn test_sandbox_untrusted_script_path_rejection() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // Create a temporary script outside scripts_dir
        let temp_dir = std::env::temp_dir().join(format!("ocs_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let untrusted_script = temp_dir.join("untrusted_test.py");
        std::fs::write(&untrusted_script, "print('SHOULD_NOT_RUN')\n").unwrap();

        let path_str = untrusted_script.to_string_lossy();
        let dir_str = temp_dir.to_string_lossy();

        // 1. Initial execution from untrusted directory must fail
        host.errors.clear();
        let _ok = plugin.dispatch(&mut host, &format!("PY {}", path_str));
        assert!(host.errors.iter().any(|e| e.contains("[SECURITY] Blocked execution of untrusted script")),
            "Expected untrusted script error, got: {:?}", host.errors);

        // 2. Trust the directory via PYTRUST
        let trust_ok = plugin.dispatch(&mut host, &format!("PYTRUST {}", dir_str));
        assert!(trust_ok);

        // 3. Execution should now succeed
        host.errors.clear();
        host.outputs.clear();
        let ok2 = plugin.dispatch(&mut host, &format!("PY {}", path_str));
        assert!(ok2);
        assert!(host.outputs.iter().any(|o| o.contains("SHOULD_NOT_RUN")));

        // 4. Untrust the directory
        let untrust_ok = plugin.dispatch(&mut host, &format!("PYUNTRUST {}", dir_str));
        assert!(untrust_ok);

        // 5. Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sandbox_entity_creation_limit() {
        // Temporarily set a small entity limit
        let mut cfg = crate::py_sandbox::load_config();
        let orig_limit = cfg.max_entities_per_run;
        cfg.max_entities_per_run = 5;
        let _ = crate::py_sandbox::save_config(&cfg);

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc
for i in range(10):
    oc.add_point((i, i, 0))
"#;
        host.errors.clear();
        plugin.dispatch(&mut host, &format!("PYRUN {script}"));

        // Restore original limit
        cfg.max_entities_per_run = orig_limit;
        let _ = crate::py_sandbox::save_config(&cfg);
        crate::py_sandbox::reset_entity_counter();

        assert!(host.errors.iter().any(|e| e.contains("Security limit exceeded") || e.contains("exceeding the ceiling")),
            "Expected entity limit error, got: {:?}", host.errors);
    }

    #[test]
    fn test_get_entity_data() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc
h_line = oc.add_line((10.0, 20.0, 0.0), (30.0, 40.0, 0.0))
data_line = oc.get_entity_data(h_line)
assert data_line["type"] == "LINE", f"Expected LINE, got {data_line}"
assert data_line["start"] == (10.0, 20.0, 0.0)
assert data_line["end"] == (30.0, 40.0, 0.0)

h_arc = oc.add_arc((50.0, 60.0, 0.0), 25.0, 0.0, 1.5707963)
data_arc = oc.entget(h_arc)
assert data_arc["type"] == "ARC"
assert data_arc["center"] == (50.0, 60.0, 0.0)
assert abs(data_arc["radius"] - 25.0) < 1e-6
print("TEST_GET_ENTITY_DATA_PASSED")
"#;
        host.errors.clear();
        host.outputs.clear();
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        assert!(ok);
        assert!(host.errors.is_empty(), "Errors: {:?}", host.errors);
        assert!(host.outputs.iter().any(|o| o.contains("TEST_GET_ENTITY_DATA_PASSED")));
    }

    #[test]
    fn test_interactive_tarc_flow() {
        use ocs_plugin_api::host::codec::entities::Line;
        use ocs_plugin_api::host::Document as CadDocument;
        use ocs_plugin_api::host::CommandStep;

        let mut doc = CadDocument::new();
        let line = Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0);
        let h_line = doc.add_entity(EntityType::Line(line)).unwrap();

        let (bridge, mut cmd) = interactive::LispInteractiveBridge::new(
            "Select object: ".to_string(),
            interactive::InteractionMode::EntityPick,
            100,
            Some(doc),
        );

        // Bridge query_entity succeeds on the snapshot
        let found = bridge.query_entity(h_line.value(), |ent| {
            matches!(ent, EntityType::Line(_))
        });
        assert_eq!(found, Some(true));

        // Canvas point click near (9.0, 0.1, 0.0) hit-tests the line in EntityPick mode
        let bridge_clone = std::sync::Arc::clone(&bridge);
        let worker = std::thread::spawn(move || {
            let picked = bridge_clone.get_entity(Some("Select object: "));
            assert!(picked.is_some());
            let (handle, pt) = picked.unwrap();
            assert_eq!(handle, h_line.value());
            assert_eq!(pt, [9.0, 0.1, 0.0]);

            // Add an arc and then wait for next point
            let mut arc = ocs_plugin_api::host::codec::entities::Arc::new();
            arc.center = ocs_plugin_api::host::codec::types::Vector3::new(10.0, 5.0, 0.0);
            arc.radius = 5.0;
            let _ = bridge_clone.add_pending_entity(EntityType::Arc(arc));

            let _next = bridge_clone.get_point(Some("Next point: "));
            bridge_clone.finish();
        });

        std::thread::sleep(std::time::Duration::from_millis(30));
        let step1 = cmd.on_point([9.0, 0.1, 0.0]);
        // After entity pick, arc was added and called get_point -> host should receive Commit(Arc)
        match step1 {
            CommandStep::Commit(EntityType::Arc(_)) => {}
            other => panic!("Expected CommandStep::Commit(Arc), got {:?}", std::mem::discriminant(&other)),
        }

        // Complete the command
        let _ = cmd.on_enter();
        let _ = worker.join();
    }

    #[test]
    fn test_c_prefix_resolution() {
        let test_dir = std::env::temp_dir().join("opencad_c_prefix_test");
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script_file = test_dir.join("c-mycmd.py");
        std::fs::write(&script_file, "import opencad as oc\nprint('C_MYCMD_EXECUTED')\n").unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // 1. Invoking MYCMD should resolve c-mycmd.py
        let ok = plugin.dispatch(&mut host, "MYCMD");
        assert!(ok);
        assert!(host.outputs.iter().any(|o| o.contains("C_MYCMD_EXECUTED")));

        // 2. Invoking PY mycmd should also resolve c-mycmd.py
        host.outputs.clear();
        let ok2 = plugin.dispatch(&mut host, "PY mycmd");
        assert!(ok2);
        assert!(host.outputs.iter().any(|o| o.contains("C_MYCMD_EXECUTED")));

        let _ = std::fs::remove_dir_all(&test_dir);
        std::env::remove_var("OCS_SCRIPTS_DIR");
    }

    #[test]
    fn test_rubber_band_preview_wires() {
        use crate::interactive::{InteractionMode, LispInteractiveBridge, RubberBandSpec};
        let (bridge, mut cmd) = LispInteractiveBridge::new("Pick: ".to_string(), InteractionMode::Point, 1, None);

        // Initially no rubber band
        assert!(cmd.on_cursor_move([10.0, 10.0, 0.0]).is_empty());

        // 1. Line preview (default color = None)
        bridge.set_rubber_band(RubberBandSpec::Line {
            from: [0.0, 0.0, 0.0],
            color: None,
        });
        let wires = cmd.on_cursor_move([10.0, 0.0, 0.0]);
        assert_eq!(wires.len(), 1);
        assert_eq!(
            wires[0],
            ocs_plugin_api::host::PreviewWire::line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], None)
        );
        assert_eq!(wires[0].color(), None);

        // 2. Line preview with custom color
        bridge.set_rubber_band(RubberBandSpec::Line {
            from: [0.0, 0.0, 0.0],
            color: Some([1.0, 0.0, 0.0, 1.0]),
        });
        let wires = cmd.on_cursor_move([5.0, 5.0, 0.0]);
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].color(), Some([1.0, 0.0, 0.0, 1.0]));

        // 3. Circle preview (analytical)
        bridge.set_rubber_band(RubberBandSpec::Circle {
            center: [0.0, 0.0, 0.0],
            color: None,
        });
        let wires = cmd.on_cursor_move([5.0, 0.0, 0.0]);
        assert_eq!(wires.len(), 1);
        match &wires[0] {
            ocs_plugin_api::host::PreviewWire::Circle { center, radius, color } => {
                assert_eq!(*center, [0.0, 0.0, 0.0]);
                assert!((radius - 5.0).abs() < 1e-6);
                assert_eq!(*color, None);
            }
            other => panic!("Expected PreviewWire::Circle, got {:?}", other),
        }

        // 4. Arc 3-point preview (analytical)
        bridge.set_rubber_band(RubberBandSpec::Arc3Point {
            p1: [0.0, 0.0, 0.0],
            p2: [10.0, 10.0, 0.0],
            color: None,
        });
        let wires = cmd.on_cursor_move([20.0, 0.0, 0.0]);
        assert_eq!(wires.len(), 1);
        match &wires[0] {
            ocs_plugin_api::host::PreviewWire::Arc { center, radius, color, .. } => {
                assert_eq!(*center, [10.0, 0.0, 0.0]);
                assert!((radius - 10.0).abs() < 1e-6);
                assert_eq!(*color, None);
            }
            other => panic!("Expected PreviewWire::Arc, got {:?}", other),
        }

        // 5. Arc center-start preview (analytical)
        bridge.set_rubber_band(RubberBandSpec::ArcCenterStart {
            center: [0.0, 0.0, 0.0],
            start: [10.0, 0.0, 0.0],
            color: None,
        });
        let wires = cmd.on_cursor_move([0.0, 10.0, 0.0]);
        assert_eq!(wires.len(), 1);
        match &wires[0] {
            ocs_plugin_api::host::PreviewWire::Arc { center, radius, .. } => {
                assert_eq!(*center, [0.0, 0.0, 0.0]);
                assert!((radius - 10.0).abs() < 1e-6);
            }
            other => panic!("Expected PreviewWire::Arc, got {:?}", other),
        }

        // 6. Tangent arc preview (analytical)
        bridge.set_rubber_band(RubberBandSpec::TangentArc {
            start: [0.0, 0.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            color: Some([0.0, 1.0, 0.0, 1.0]),
        });
        let wires = cmd.on_cursor_move([10.0, 10.0, 0.0]);
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].color(), Some([0.0, 1.0, 0.0, 1.0]));
        match &wires[0] {
            ocs_plugin_api::host::PreviewWire::Arc { radius, .. } => {
                assert!((radius - 10.0).abs() < 1e-4);
            }
            other => panic!("Expected PreviewWire::Arc, got {:?}", other),
        }
    }

    #[test]
    fn test_interactive_script_rubber_band_preview() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-rubber-band");
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
p1 = oc.get_point("Point 1: ")
if p1:
    p2 = oc.get_point("Point 2: ", base_point=p1, color="red")
    if p2:
        p3 = oc.get_point("Point 3: ", rubber_band=("tangent_arc", p2, (1, 0, 0)), color="cyan")
        if p3:
            oc.add_line(p1, p2)
"#;
        std::fs::write(test_dir.join("c-rb_test.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let ok = plugin.dispatch(&mut host, "PY c-rb_test");
        assert!(ok);
        assert!(host.interactive_cmd.is_some());

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Point 1: ");
        assert!(cmd.on_cursor_move([5.0, 5.0, 0.0]).is_empty());

        let _ = cmd.on_point([0.0, 0.0, 0.0]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(cmd.prompt(), "Point 2: ");

        let preview = cmd.on_cursor_move([10.0, 0.0, 0.0]);
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].color(), Some([1.0, 0.0, 0.0, 1.0]));
        match &preview[0] {
            ocs_plugin_api::host::PreviewWire::Polyline { points, .. } => {
                assert_eq!(*points, vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
            }
            other => panic!("Expected PreviewWire::Polyline, got {:?}", other),
        }

        let _ = cmd.on_point([10.0, 0.0, 0.0]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(cmd.prompt(), "Point 3: ");

        let preview_arc = cmd.on_cursor_move([20.0, 10.0, 0.0]);
        assert_eq!(preview_arc.len(), 1);
        assert_eq!(preview_arc[0].color(), Some([0.0, 1.0, 1.0, 1.0]));
        match &preview_arc[0] {
            ocs_plugin_api::host::PreviewWire::Arc { .. } => {}
            other => panic!("Expected PreviewWire::Arc, got {:?}", other),
        }

        let step3 = cmd.on_point([20.0, 10.0, 0.0]);
        assert!(matches!(step3, CommandStep::CommitAndEnd(_)));

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_interactive_script_commit_many() {
        let test_dir = std::env::temp_dir().join("ocs-test-scripts-commit-many");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
c1 = oc.get_point("Corner 1: ")
if c1:
    c2 = oc.get_point("Corner 2: ", rubber_band=("rectangle", c1), color="orange")
    if c2:
        oc.add_line((c1[0], c1[1]), (c2[0], c1[1]), color="orange")
        oc.add_line((c2[0], c1[1]), (c2[0], c2[1]), color="orange")
        oc.add_line((c2[0], c2[1]), (c1[0], c2[1]), color="orange")
        oc.add_line((c1[0], c2[1]), (c1[0], c1[1]), color="orange")
"#;
        std::fs::write(test_dir.join("c-box_test.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let ok = plugin.dispatch(&mut host, "PY c-box_test");
        assert!(ok);
        assert!(host.interactive_cmd.is_some());

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Corner 1: ");

        let _ = cmd.on_point([0.0, 0.0, 0.0]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(cmd.prompt(), "Corner 2: ");

        let preview = cmd.on_cursor_move([10.0, 5.0, 0.0]);
        assert_eq!(preview.len(), 1);
        match &preview[0] {
            ocs_plugin_api::host::PreviewWire::Polyline { points, .. } => {
                assert_eq!(points.len(), 5);
            }
            other => panic!("Expected PreviewWire::Polyline, got {:?}", other),
        }

        let step2 = cmd.on_point([10.0, 5.0, 0.0]);
        match step2 {
            CommandStep::CommitManyAndEnd(entities) => {
                assert_eq!(entities.len(), 4, "All 4 lines of the box must be committed");
            }
            other => panic!("Expected CommandStep::CommitManyAndEnd with 4 entities, got {:?}", std::mem::discriminant(&other)),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_rubber_band_analytical_circle_and_arc() {
        use ocs_plugin_api::host::PreviewWire;
        let test_dir = std::env::temp_dir().join(format!("ocs-test-rb-analytical-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc
center = (10.0, 20.0, 0.0)
pt = oc.get_point("Pick radius: ", rubber_band=("circle", center), color="red")
"#;
        std::fs::write(test_dir.join("test_rb_circ.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let ok = plugin.dispatch(&mut host, "PY test_rb_circ");
        assert!(ok);
        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Pick radius: ");

        let mut wires = Vec::new();
        for _ in 0..50 {
            wires = cmd.on_cursor_move([20.0, 20.0, 0.0]);
            if !wires.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(wires.len(), 1);
        match &wires[0] {
            PreviewWire::Circle { center, radius, color } => {
                assert_eq!(*center, [10.0, 20.0, 0.0]);
                assert!((radius - 10.0).abs() < 1e-6);
                assert!(color.is_some());
            }
            other => panic!("Expected PreviewWire::Circle, got {:?}", other),
        }

        let _ = cmd.on_point([20.0, 20.0, 0.0]);
        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_interactive_script_error_interception() {
        let test_dir = std::env::temp_dir().join(format!("ocs_test_err_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&test_dir).unwrap();
        std::env::set_var("OCS_SCRIPTS_DIR", &test_dir);

        let script = r#"
import opencad as oc

@oc.command("CRASHCMD")
def crash_command():
    p1 = oc.get_point("Pick first point: ")
    # Deliberate error after point pick
    raise ValueError("Deliberate error in interactive command!")
"#;
        std::fs::write(test_dir.join("crashtest.py"), script).unwrap();

        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let _ = crate::py_runtime::auto_load_decorated_scripts(&mut host);

        let ok = plugin.dispatch(&mut host, "CRASHCMD");
        assert!(ok);
        assert!(host.interactive_cmd.is_some(), "interactive command should be started on host");

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert_eq!(cmd.prompt(), "Pick first point: ");

        // User supplies point, Python resumes and raises ValueError
        let step = cmd.on_point([10.0, 20.0, 0.0]);
        assert!(matches!(step, CommandStep::Cancel));

        // Allow worker to finish fail routing
        std::thread::sleep(std::time::Duration::from_millis(50));
        let shared_errs = host.shared_errors.lock().unwrap();
        assert!(
            host.errors.iter().any(|e| e.contains("Deliberate error in interactive command!"))
                || shared_errs.iter().any(|e| e.contains("Deliberate error in interactive command!")),
            "Expected error in host.errors or shared_errors: host={:?}, shared={:?}",
            host.errors,
            *shared_errs
        );

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_one_click_restart_protocol_and_dispatch() {
        use crate::ide::protocol::IdeToPlugin;
        use crate::dispatcher::{get_last_dispatched_command, set_last_dispatched_command};

        // 1. Verify JSON serialization round-trip of RestartScript
        let restart_msg = IdeToPlugin::RestartScript {
            title: "c-tarc.py".to_string(),
            code: "import opencad as oc\noc.info('fixed!')".to_string(),
            breakpoints: vec![5, 10],
        };
        let json = serde_json::to_string(&restart_msg).expect("Serialize RestartScript");
        let parsed: IdeToPlugin = serde_json::from_str(&json).expect("Deserialize RestartScript");
        match parsed {
            IdeToPlugin::RestartScript { title, code, breakpoints } => {
                assert_eq!(title, "c-tarc.py");
                assert!(code.contains("fixed!"));
                assert_eq!(breakpoints, vec![5, 10]);
            }
            _ => panic!("Expected RestartScript variant"),
        }

        // 2. Verify command tracking in dispatcher
        set_last_dispatched_command(None);
        assert_eq!(get_last_dispatched_command(), None);

        let mut host = MockHost::new();
        // Dispatching a command updates LAST_DISPATCHED_COMMAND
        let dispatched = crate::dispatcher::dispatch_command(&mut host, "PYRUN oc.info('restart test')");
        assert!(dispatched);
        assert_eq!(get_last_dispatched_command(), Some("PYRUN oc.info('restart test')".to_string()));

        // 3. Verify in-memory code update takes precedence
        crate::py_runtime::store_loaded_script("restart_demo.py", "import opencad as oc\noc.info('version 1')");
        assert!(crate::py_runtime::get_loaded_script("restart_demo.py").is_some());

        // Update in-memory script (simulating user edit in OCSIDE)
        crate::py_runtime::store_loaded_script("restart_demo.py", "import opencad as oc\noc.info('version 2 fixed')");
        let loaded = crate::py_runtime::get_loaded_script("restart_demo.py").unwrap();
        assert!(loaded.contains("version 2 fixed"));
    }

    #[test]
    fn test_python_dialog_api() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

cfg = oc.dialog(
    "Circle Parameters",
    oc.row(oc.number(15.0, min=1.0, max=100.0, label="Radius"), fill=True),
    layer="WALLS",
    material=oc.choice(["Steel", "Wood", "Concrete"], default="Wood"),
)

if cfg:
    oc.add_circle((0.0, 0.0), cfg.radius)
"#;
        let ok = plugin.dispatch(&mut host, &format!("PYRUN {script}"));
        assert!(ok, "script dispatch should return true");
        assert!(host.interactive_cmd.is_some(), "interactive command should be started on host");

        let mut cmd = host.interactive_cmd.take().unwrap();
        let dialog_def = cmd.modal_dialog().expect("modal dialog should be defined");
        assert_eq!(dialog_def.title, "Circle Parameters");
        assert_eq!(dialog_def.rows.len(), 3); // row with 2 controls, layer, material

        // Verify the controls
        let first_row = &dialog_def.rows[0];
        assert_eq!(first_row.controls.len(), 2);

        // Submit response
        let mut values = std::collections::HashMap::new();
        values.insert("radius".to_string(), ocs_plugin_api::dialog::DialogValue::Number(25.0));
        values.insert("fill".to_string(), ocs_plugin_api::dialog::DialogValue::Bool(false));
        values.insert("layer".to_string(), ocs_plugin_api::dialog::DialogValue::Text("WALLS_EXT".to_string()));
        values.insert("material".to_string(), ocs_plugin_api::dialog::DialogValue::Choice {
            index: 0,
            text: "Steel".to_string(),
        });

        let step = cmd.on_dialog_response(ocs_plugin_api::dialog::DialogResponse::Submitted(values));
        match step {
            ocs_plugin_api::host::CommandStep::CommitAndEnd(ocs_plugin_api::host::EntityType::Circle(c)) => {
                assert_eq!(c.radius, 25.0);
            }
            other => panic!("Expected CommitAndEnd(Circle), got: {other:?}"),
        }
    }

    #[test]
    fn test_declarative_panel_and_callbacks() {
        let mut host = MockHost::new();
        let mut plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

@oc.panel("Survey Sample", command="SAMPLE_PANEL_CMD", width=350)
def survey_sample_tool(panel):
    def on_click(p):
        p.distance = 123.45
        p.traverse.add_row(["1", "5000.00", "5000.00"])

    return [
        oc.info_text("Point Inverse:"),
        oc.row(
            oc.button("Calculate", on_click=on_click, id="btn_calc"),
            oc.button("Zoom", on_click="ZOOM E", id="btn_zoom"),
        ),
        oc.fields("Results",
            distance=oc.number(0.0, label="Distance"),
        ),
        oc.table(["Pt", "N", "E"], id="traverse"),
    ]
"#;
        let load_cmd = format!("PYRUN {script}");
        let ok = plugin.dispatch(&mut host, &load_cmd);
        assert!(ok);

        // Check that SAMPLE_PANEL_CMD and button action command were registered
        let cmds = crate::py_module::get_registered_commands();
        assert!(cmds.contains_key("SAMPLE_PANEL_CMD"));
        assert!(cmds.contains_key("_PANEL_ACT_SURVEY_SAMPLE_TOOL_BTN_CALC"));

        // Dispatch SAMPLE_PANEL_CMD to test toggle command
        let ok_toggle = plugin.dispatch(&mut host, "SAMPLE_PANEL_CMD");
        assert!(ok_toggle);

        // Simulate host notification: user clicks 'btn_calc'
        plugin.on_notification(
            None,
            HostNotification::PanelAction {
                panel_id: "survey_sample_tool".to_string(),
                control_id: "btn_calc".to_string(),
                action: "click".to_string(),
                value: None,
            },
        );

        // Allow callback runner thread to execute and verify state update
        let mut dist = 0.0;
        let mut row_count = 0;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            pyo3::Python::with_gil(|py| {
                use pyo3::types::{PyAnyMethods, PyDictMethods};
                let m = py.import("opencad").unwrap();
                let registry = m.getattr("_PANEL_REGISTRY").unwrap();
                if let Ok(Some(p)) = registry.downcast::<pyo3::types::PyDict>().unwrap().get_item("survey_sample_tool") {
                    if let Ok(d) = p.getattr("distance") {
                        if let Ok(v) = d.extract::<f64>() {
                            dist = v;
                        }
                    }
                    if let Ok(trav) = p.getattr("traverse") {
                        if let Ok(rows) = trav.getattr("rows") {
                            if let Ok(r) = rows.extract::<Vec<Vec<String>>>() {
                                row_count = r.len();
                            }
                        }
                    }
                }
            });
            if dist == 123.45 && row_count == 1 {
                break;
            }
        }
        assert_eq!(dist, 123.45);
        assert_eq!(row_count, 1);
    }

    #[test]
    fn test_interactive_panel_sample_point_pick() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc
import math

@oc.panel("Traverse Sample", command="MOCKTRAVERSE")
def traverse_sample_tool(panel):
    def on_pick(p):
        p1 = oc.get_point("Point 1: ")
        p2 = oc.get_point("Point 2: ")
        dx = p2[0] - p1[0]
        dy = p2[1] - p1[1]
        dist = math.hypot(dx, dy)
        p.distance = round(dist, 2)
        p.traverse.add_row(["101", "5000", "5000"])
        oc.add_line(p1, p2, layer="SAMPLE_LINES", color="yellow")

    return [
        oc.row(
            oc.button("Pick", on_click=on_pick, id="btn_pick"),
        ),
        oc.fields("Results",
            distance=oc.number(0.0, label="Distance"),
        ),
        oc.table(["Pt", "N", "E"], id="traverse"),
    ]
"#;
        let load_cmd = format!("PYRUN {script}");
        let ok = plugin.dispatch(&mut host, &load_cmd);
        assert!(ok);

        let cmds = crate::py_module::get_registered_commands();
        assert!(cmds.contains_key("_PANEL_ACT_TRAVERSE_SAMPLE_TOOL_BTN_PICK"));

        // Dispatch button command
        let ok_pick = plugin.dispatch(&mut host, "_PANEL_ACT_TRAVERSE_SAMPLE_TOOL_BTN_PICK");
        assert!(ok_pick);

        let mut cmd = host.interactive_cmd.take().expect("interactive command started");
        assert_eq!(cmd.prompt(), "Point 1: ");

        // User picks point 1
        let step1 = cmd.on_point([0.0, 0.0, 0.0]);
        assert!(matches!(step1, CommandStep::NeedPoint));
        assert_eq!(cmd.prompt(), "Point 2: ");

        // User picks point 2
        let step2 = cmd.on_point([30.0, 40.0, 0.0]);
        match step2 {
            CommandStep::CommitAndEnd(EntityType::Line(l)) => {
                assert_eq!(l.start.x, 0.0);
                assert_eq!(l.end.x, 30.0);
                assert_eq!(l.end.y, 40.0);
                assert_eq!(l.common.layer, "SAMPLE_LINES");
            }
            other => panic!("Expected CommitAndEnd with Line, got {:?}", other),
        }

        // Drain pending panel updates from interactive session
        let updates = cmd.take_pending_panel_updates();
        assert!(!updates.is_empty(), "Expected pending panel updates");
        let has_distance = updates.iter().any(|(pid, vals)| {
            pid == "traverse_sample_tool" && vals.contains_key("distance")
        });
        assert!(has_distance, "Expected distance in panel updates");
    }

    #[test]
    fn test_mock_grid_panel_registration_and_toggle_command() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

@oc.panel("Mock Grid", command="MOCKGRID", width=360)
def mock_grid_palette(panel):
    return [
        oc.info_text("Mock Grid"),
        oc.row(
            oc.button("Generate", on_click=lambda p: None, id="btn_gen"),
        ),
    ]
"#;
        let load_cmd = format!("PYRUN {script}");
        let ok = plugin.dispatch(&mut host, &load_cmd);
        assert!(ok);

        // Verify PanelDef has command set
        assert!(!host.registered_panels.is_empty());
        let p_def = host.registered_panels.iter().find(|p| p.id == "mock_grid_palette").expect("mock_grid_palette registered");
        assert_eq!(p_def.command.as_deref(), Some("MOCKGRID"));
        assert_eq!(p_def.title, "Mock Grid");

        // Verify command registry has MOCKGRID
        let cmds = crate::py_module::get_registered_commands();
        assert!(cmds.contains_key("MOCKGRID"));

        // Dispatch MOCKGRID command
        let ok_toggle = plugin.dispatch(&mut host, "MOCKGRID");
        assert!(ok_toggle);

        // Verify toggle_panel was called on host
        let toggled = host.toggled_panels.iter().any(|(id, _)| id == "mock_grid_palette");
        assert!(toggled, "Expected mock_grid_palette in toggled_panels: {:?}", host.toggled_panels);
    }

    #[test]
    fn test_panel_choice_and_table_safety() {
        let mut host = MockHost::new();
        let mut plugin = AutoLispPlugin;

        let script = r#"
import opencad as oc

res_state = {"color": "", "schedule_len": -1, "row_added": False}

@oc.panel("Safety Test", command="SAFETYTEST", width=360)
def safety_test_palette(panel):
    def on_click_action(p):
        res_state["color"] = getattr(p, "color", "")
        res_state["schedule_len"] = len(p.schedule)
        p.schedule.add_row(["new_1", "new_2"])
        res_state["row_added"] = True

    return [
        oc.fields(
            "Props",
            color=oc.choice(["cyan", "green", "yellow"], default="cyan"),
        ),
        oc.table(["Col1", "Col2"], rows=[["a", "b"]], id="schedule"),
        oc.button("Run Action", on_click=on_click_action, id="btn_act"),
    ]
"#;
        let load_cmd = format!("PYRUN {script}");
        let ok = plugin.dispatch(&mut host, &load_cmd);
        assert!(ok);

        // 1. Simulate Choice change with index 1 (green) and empty text (fallback test)
        let notif_choice = HostNotification::PanelAction {
            panel_id: "safety_test_palette".to_string(),
            control_id: "color".to_string(),
            action: "change".to_string(),
            value: Some(ocs_plugin_api::dialog::DialogValue::Choice {
                index: 1,
                text: String::new(),
            }),
        };
        plugin.on_notification(None, notif_choice);

        // 2. Simulate table row removal (passing Number float index)
        let notif_remove = HostNotification::PanelAction {
            panel_id: "safety_test_palette".to_string(),
            control_id: "schedule".to_string(),
            action: "remove_row".to_string(),
            value: Some(ocs_plugin_api::dialog::DialogValue::Number(0.0)),
        };
        plugin.on_notification(None, notif_remove);

        // 3. Trigger button action
        let btn_cmd = "_PANEL_ACT_SAFETY_TEST_PALETTE_BTN_ACT";
        let ok_action = plugin.dispatch(&mut host, btn_cmd);
        assert!(ok_action);

        // 4. Verify in python state that color was green, table was not corrupted into float, and add_row succeeded
        pyo3::Python::with_gil(|py| {
            let py_code = r#"
assert res_state["color"] == "green", f"Expected green, got {res_state['color']}"
assert res_state["row_added"] is True, "add_row failed"
assert res_state["schedule_len"] == 0, f"Expected 0 after remove_row, got {res_state['schedule_len']}"
"#;
            py.run(&std::ffi::CString::new(py_code).unwrap(), None, None).expect("assertions pass");
        });
    }
    } // end mod python_tests

    #[test]
    fn test_autolisp_direct_evaluation_and_reflection() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // 1. Direct S-expression in command line
        let ok_eval = plugin.dispatch(&mut host, "(+ 10 20 30)");
        assert!(ok_eval);

        // 2. Define a CAD command via AutoLISP: (defun c:lisp_box () ...)
        let lisp_def = r#"
        (defun c:lisp_box ()
          (cad:add-line '(0 0 0) '(100 0 0) "LISP_LAYER")
          (cad:add-line '(100 0 0) '(100 50 0) "LISP_LAYER")
          (princ "\nLISP_BOX_CREATED")
        )
        "#;
        let ok_def = plugin.dispatch(&mut host, &format!("LSPRUN {lisp_def}"));
        assert!(ok_def);

        assert!(crate::rust_lisp::has_lisp_command("LISP_BOX"), "c:lisp_box should register LISP_BOX");

        // 3. Execute the registered command directly by name
        let ok_run = plugin.dispatch(&mut host, "LISP_BOX");
        assert!(ok_run);
        assert!(host.doc.entities().count() >= 2);
        assert!(host.doc.layers.contains("LISP_LAYER"));

        // 4. Test calling CAD reflected function directly via S-expression
        let ok_line = plugin.dispatch(&mut host, "(add-line '(10 10 0) '(20 20 0))");
        assert!(ok_line);
        assert!(host.doc.entities().count() >= 3);
    }

    #[test]
    fn test_target_interactive_step() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let sample_code = std::fs::read_to_string("scripts/sample_demo.lsp").unwrap();
        let ok_load = plugin.dispatch(&mut host, &format!("LSPRUN {sample_code}"));
        assert!(ok_load);

        let ok_target = plugin.dispatch(&mut host, "TARGET");
        assert!(ok_target);
        assert!(host.interactive_cmd.is_some());

        let mut cmd = host.interactive_cmd.take().unwrap();
        assert!(cmd.prompt().contains("center point"));

        // User feeds center point (50, 50, 0)
        let step1 = cmd.on_point([50.0, 50.0, 0.0]);
        assert!(matches!(step1, CommandStep::NeedPoint), "Expected NeedPoint, got {:?}", step1);
        assert!(cmd.prompt().contains("radius"));

        // User feeds second point (75, 50, 0) -> radius 25.0
        let step2 = cmd.on_point([75.0, 50.0, 0.0]);
        match &step2 {
            CommandStep::CommitManyAndEnd(ents) => {
                assert_eq!(ents.len(), 3, "Expected 1 circle and 2 lines created via entmake, got {}", ents.len());
            }
            other => {
                panic!("Expected CommitManyAndEnd with 3 entities, got {:?}", other);
            }
        }
    }

    #[test]
    fn test_autolisp_radical_stream_load_and_dispatch() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let code = std::fs::read_to_string("scripts/test_radical_stream.lsp").unwrap();
        let ok_load = plugin.dispatch(&mut host, &format!("LSPRUN {code}"));
        assert!(ok_load, "Loading test_radical_stream.lsp must succeed");

        assert!(crate::rust_lisp::has_lisp_command("AUTOLINE"), "AUTOLINE must be registered");
        assert!(crate::rust_lisp::has_lisp_command("PAUSELINE"), "PAUSELINE must be registered");
        assert!(crate::rust_lisp::has_lisp_command("RADICAL_SPIRAL"), "RADICAL_SPIRAL must be registered");
        assert!(crate::rust_lisp::has_lisp_command("RADICAL_STRESS_GRID"), "RADICAL_STRESS_GRID must be registered");

        // Test running AUTOLINE through dispatch
        let ok_run = plugin.dispatch(&mut host, "AUTOLINE");
        assert!(ok_run, "Dispatching AUTOLINE must succeed");

        // Verify command stream was captured
        let shared = host.shared_commands.lock().unwrap();
        assert!(shared.iter().any(|c| c.contains("LINE")), "Expected streamed LINE command in: {:?}", *shared);
    }

    #[test]
    fn test_autolisp_setvar_dynmode_and_ctab_host_dispatch() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        // 1. Set DYNMODE to 0 via AutoLISP and verify SETVAR DYNMODE 0 is executed on the host
        let ok_dyn = plugin.dispatch(&mut host, "(setvar \"DYNMODE\" 0)");
        assert!(ok_dyn);
        assert!(
            host.commands.iter().any(|c| c.trim() == "SETVAR DYNMODE 0"),
            "Expected 'SETVAR DYNMODE 0' in host.commands, got: {:?}",
            host.commands
        );

        // Verify getvar reads live DYNMODE value
        host.outputs.clear();
        let ok_get_dyn = plugin.dispatch(&mut host, "(getvar \"DYNMODE\")");
        assert!(ok_get_dyn);
        assert_eq!(host.outputs.last().map(|s| s.as_str()), Some("0"));

        // 2. Set OSMODE to 35 via AutoLISP and verify both HeaderVariables and SETVAR OSMODE 35 are updated
        let ok_osmode = plugin.dispatch(&mut host, "(setvar \"OSMODE\" 35)");
        assert!(ok_osmode);
        assert_eq!(host.doc.header.object_snap_mode, 35);
        assert!(
            host.commands.iter().any(|c| c.trim() == "SETVAR OSMODE 35"),
            "Expected 'SETVAR OSMODE 35' in host.commands, got: {:?}",
            host.commands
        );

        // 3. Set CTAB to Layout1 via AutoLISP and verify TableOperation::LayoutSetCurrent updated host CTAB
        host.outputs.clear();
        let ok_ctab = plugin.dispatch(&mut host, "(setvar \"CTAB\" \"Layout1\")");
        assert!(ok_ctab);
        let ok_get_ctab = plugin.dispatch(&mut host, "(getvar \"CTAB\")");
        assert!(ok_get_ctab);
        assert_eq!(host.outputs.last().map(|s| s.as_str()), Some("\"Layout1\""));
    }

    #[test]
    fn test_autolisp_error_prints_function_and_lsperr() {
        let mut host = MockHost::new();
        let plugin = AutoLispPlugin;

        let ok_def = plugin.dispatch(
            &mut host,
            r#"(progn
                (defun calc-offset (base delta / total)
                  (setq total (+ base delta))
                  (* total 2))
                (defun c:badoffset ()
                  (calc-offset 10 "not-a-number")))"#,
        );
        assert!(ok_def);

        host.errors.clear();
        host.outputs.clear();
        let ok_run = plugin.dispatch(&mut host, "BADOFFSET");
        assert!(ok_run);

        // 1. OCS command line should ONLY receive the concise function error, with no multi-line output clutter
        assert_eq!(
            host.errors,
            vec!["AutoLISP Error in CALC-OFFSET: Type error in +: expected number, got \"not-a-number\" [(+ BASE DELTA)]".to_string()],
            "Expected concise function error on OCS command line, got: {:?}",
            host.errors
        );
        assert!(
            host.outputs.is_empty(),
            "OCS command line outputs should not be cluttered with multi-line stack trace, got: {:?}",
            host.outputs
        );

        // 2. Full diagnostic (sent to OCSIDE console when open and stored in LAST_LISP_ERROR) must contain all details
        let full_ide_err = crate::rust_lisp::get_last_lisp_error().expect("full error should be stored");
        assert!(full_ide_err.contains("Type error in +: expected number, got \"not-a-number\""));
        assert!(full_ide_err.contains("; in expression: (+ BASE DELTA)"));
        assert!(full_ide_err.contains("; in function:") && full_ide_err.contains("(CALC-OFFSET 10 \"not-a-number\")"));
        assert!(full_ide_err.contains("; definition:") && full_ide_err.contains("(DEFUN CALC-OFFSET (BASE DELTA / TOTAL) (SETQ TOTAL (+ BASE DELTA)) (* TOTAL 2))"));
        assert!(full_ide_err.contains("; called from:") && full_ide_err.contains("(C:BADOFFSET)"));

        // 3. Verify LSPERR reprints the full error diagnostic when explicitly requested
        host.errors.clear();
        host.outputs.clear();
        let ok_lsperr = plugin.dispatch(&mut host, "LSPERR");
        assert!(ok_lsperr);
        assert!(
            host.errors
                .iter()
                .any(|e| e.contains("Last AutoLISP Error: Type error in +")),
            "Expected LSPERR headline, got: {:?}",
            host.errors
        );
        assert!(
            host.outputs
                .iter()
                .any(|o| o.contains("(DEFUN CALC-OFFSET")),
            "Expected LSPERR to output function definition, got: {:?}",
            host.outputs
        );
    }

    #[test]
    fn test_api_v28_manifest_and_document_changed_delta_tracks_tab() {
        let manifest = include_str!("../plugin.toml");
        let expected_line = format!("api_version = {}", ocs_plugin_api::API_VERSION);
        assert!(
            manifest.lines().any(|line| line.trim() == expected_line),
            "plugin.toml must declare `{expected_line}` to match ocs_plugin_api::API_VERSION"
        );
        assert!(ocs_plugin_api::API_VERSION >= 28);

        let mut plugin = AutoLispPlugin;
        let prev_tab = crate::context::get_active_tab_id();
        plugin.on_notification(
            None,
            HostNotification::DocumentChangedDelta {
                tab_id: 987654,
                version: 1,
                added: Vec::new(),
                modified: Vec::new(),
                deleted: Vec::new(),
                records: Vec::new(),
                snapshot: None,
                full_refresh_required: false,
            },
        );
        assert_eq!(crate::context::get_active_tab_id(), 987654);
        crate::context::set_active_tab_id(prev_tab);
    }
}


