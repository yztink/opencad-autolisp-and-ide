use std::cell::RefCell;
use std::sync::Arc;
use ocs_plugin_api::host::{Document as CadDocument, HostApi, InteractiveCommand};
use crate::interactive::LispInteractiveBridge;

pub type HostTask = Box<dyn FnMut(&mut dyn HostApi) + Send + 'static>;

pub enum WorkerMsg {
    HostCall(HostTask),
    InteractiveStarted(Box<dyn InteractiveCommand>),
    Finished(Result<(), String>),
}

thread_local! {
    static ACTIVE_HOST: RefCell<Option<*mut (dyn HostApi + 'static)>> = const { RefCell::new(None) };
    static ACTIVE_INTERACTIVE: RefCell<Option<Arc<LispInteractiveBridge>>> = const { RefCell::new(None) };
    static WORKER_TX: RefCell<Option<std::sync::mpsc::Sender<WorkerMsg>>> = const { RefCell::new(None) };
    static SENDER: RefCell<Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>> = const { RefCell::new(None) };
    static DEBUG_ACTIVE: RefCell<bool> = const { RefCell::new(false) };
    static THREAD_TAB_ID: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

static GLOBAL_SENDER: std::sync::Mutex<Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>> =
    std::sync::Mutex::new(None);

static ACTIVE_TAB_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_thread_tab_id(tab_id: Option<u64>) {
    THREAD_TAB_ID.with(|cell| cell.set(tab_id));
}

pub fn set_active_tab_id(tab_id: u64) {
    THREAD_TAB_ID.with(|cell| cell.set(Some(tab_id)));
    ACTIVE_TAB_ID.store(tab_id, std::sync::atomic::Ordering::SeqCst);
}

pub fn get_active_tab_id() -> u64 {
    if let Some(tid) = THREAD_TAB_ID.with(|cell| cell.get()) {
        return tid;
    }
    ACTIVE_TAB_ID.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn set_global_sender(sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>) {
    *GLOBAL_SENDER.lock().unwrap() = sender;
}

pub fn get_global_sender() -> Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>> {
    GLOBAL_SENDER.lock().unwrap().clone()
}

pub fn set_worker_tx(tx: Option<std::sync::mpsc::Sender<WorkerMsg>>) {
    WORKER_TX.with(|cell| {
        *cell.borrow_mut() = tx;
    });
}

pub fn get_worker_tx() -> Option<std::sync::mpsc::Sender<WorkerMsg>> {
    WORKER_TX.with(|cell| cell.borrow().clone())
}

pub fn set_debug_session_context(sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>) {
    SENDER.with(|cell| {
        *cell.borrow_mut() = sender;
    });
    DEBUG_ACTIVE.with(|cell| {
        *cell.borrow_mut() = true;
    });
}

pub fn clear_debug_session_context() {
    SENDER.with(|cell| {
        *cell.borrow_mut() = None;
    });
    DEBUG_ACTIVE.with(|cell| {
        *cell.borrow_mut() = false;
    });
}

pub fn set_active_interactive(bridge: Option<Arc<LispInteractiveBridge>>) {
    ACTIVE_INTERACTIVE.with(|cell| {
        *cell.borrow_mut() = bridge;
    });
}

/// RAII guard that sets the active `HostApi` for the duration of a command dispatch.
pub struct HostGuard;

impl HostGuard {
    pub fn new(host: &mut dyn HostApi) -> Self {
        let tid = host.tab_id();
        set_thread_tab_id(Some(tid));
        if tid != 0 {
            ACTIVE_TAB_ID.store(tid, std::sync::atomic::Ordering::SeqCst);
        }
        ACTIVE_HOST.with(|cell| {
            let erased: *mut (dyn HostApi + 'static) =
                unsafe { std::mem::transmute(host as *mut dyn HostApi) };
            *cell.borrow_mut() = Some(erased);
        });
        HostGuard
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        set_thread_tab_id(None);
        ACTIVE_HOST.with(|cell| {
            *cell.borrow_mut() = None;
        });
        ACTIVE_INTERACTIVE.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

/// Thread-safe host adapter used on background debugger threads.
pub struct DebugHost {
    pub sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>,
    pub doc: std::cell::UnsafeCell<Option<CadDocument>>,
    pub captured_output: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl DebugHost {
    pub fn new(sender: Option<Arc<dyn ocs_plugin_api::host::PluginRequestSender>>) -> Self {
        Self {
            sender,
            doc: std::cell::UnsafeCell::new(None),
            captured_output: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn ensure_doc(&mut self) -> &mut CadDocument {
        let doc_ptr = self.doc.get();
        unsafe {
            if (*doc_ptr).is_none() {
                let mut d_out = CadDocument::new();
                if let Some(bridge) = active_interactive() {
                    if let Ok(guard) = bridge.doc_snapshot.lock() {
                        if let Some(ref d) = *guard {
                            d_out = d.clone();
                        }
                    }
                    if let Ok(list) = bridge.coordinator.pending_entities.lock() {
                        for ent in list.iter() {
                            let _ = d_out.add_entity(ent.clone());
                        }
                    }
                } else if let Some(s) = &self.sender {
                    use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
                    if let Ok(PluginResponse::Document(d)) = s.request(PluginRequest::DocumentSnapshot) {
                        d_out = *d;
                    }
                }
                *doc_ptr = Some(d_out);
            }
            (*doc_ptr).as_mut().unwrap()
        }
    }
}

impl HostApi for DebugHost {
    fn tab_index(&self) -> usize { 0 }
    fn document(&self) -> &CadDocument {
        let doc_ptr = self.doc.get();
        unsafe {
            if (*doc_ptr).is_none() {
                let mut d_out = CadDocument::new();
                if let Some(bridge) = active_interactive() {
                    if let Ok(guard) = bridge.doc_snapshot.lock() {
                        if let Some(ref d) = *guard {
                            d_out = d.clone();
                        }
                    }
                    if let Ok(list) = bridge.coordinator.pending_entities.lock() {
                        for ent in list.iter() {
                            let _ = d_out.add_entity(ent.clone());
                        }
                    }
                } else if let Some(s) = &self.sender {
                    use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
                    if let Ok(PluginResponse::Document(d)) = s.request(PluginRequest::DocumentSnapshot) {
                        d_out = *d;
                    }
                }
                *doc_ptr = Some(d_out);
            }
            (*doc_ptr).as_ref().unwrap()
        }
    }
    fn document_mut(&mut self) -> &mut CadDocument {
        self.ensure_doc()
    }
    fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
        Box::new(ocs_plugin_api::host::CadDocumentReader(self.document()))
    }
    fn add_entity(&mut self, entity: ocs_plugin_api::host::EntityType) -> ocs_plugin_api::host::Handle {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            match s.request(PluginRequest::AddEntity(entity.clone())) {
                Ok(PluginResponse::Handle(h)) => {
                    let _ = s.request(PluginRequest::SetDirty);
                    let _ = s.request(PluginRequest::BumpGeometry);
                    unsafe { *self.doc.get() = None; }
                    return h;
                }
                _ => {}
            }
        }
        let doc = self.ensure_doc();
        doc.add_entity(entity).unwrap_or_default()
    }
    fn add_entities(&mut self, entities: Vec<ocs_plugin_api::host::EntityType>) -> Vec<ocs_plugin_api::host::Handle> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            match s.request(PluginRequest::AddEntities(entities.clone())) {
                Ok(PluginResponse::Handles(hs)) => {
                    let _ = s.request(PluginRequest::SetDirty);
                    let _ = s.request(PluginRequest::BumpGeometry);
                    return hs;
                }
                _ => {}
            }
        }
        let doc = self.ensure_doc();
        entities.into_iter().map(|e| doc.add_entity(e).unwrap_or_default()).collect()
    }
    fn update_entity(&mut self, entity: ocs_plugin_api::host::EntityType) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            match s.request(PluginRequest::UpdateEntity(entity.clone())) {
                Ok(PluginResponse::Bool(b)) => {
                    let _ = s.request(PluginRequest::SetDirty);
                    let _ = s.request(PluginRequest::BumpGeometry);
                    return b;
                }
                _ => {}
            }
        }
        false
    }
    fn remove_entity(&mut self, handle: ocs_plugin_api::host::Handle) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            match s.request(PluginRequest::RemoveEntity { handle }) {
                Ok(PluginResponse::Bool(b)) => {
                    let _ = s.request(PluginRequest::SetDirty);
                    let _ = s.request(PluginRequest::BumpGeometry);
                    return b;
                }
                _ => {}
            }
        }
        false
    }
    fn bump_geometry(&mut self) {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::PluginRequest;
            let _ = s.request(PluginRequest::BumpGeometry);
        }
    }
    fn read_record(&self, _handle: ocs_plugin_api::host::Handle, _app_name: &str) -> Option<&ocs_plugin_api::host::codec::xdata::ExtendedDataRecord> { None }
    fn write_record(&mut self, _handle: ocs_plugin_api::host::Handle, _record: ocs_plugin_api::host::codec::xdata::ExtendedDataRecord) -> bool { true }
    fn remove_record(&mut self, _handle: ocs_plugin_api::host::Handle, _app_name: &str) -> bool { true }
    fn push_undo(&mut self, label: &str) {
        if let Some(s) = &self.sender {
            let label = label.to_string();
            let s = s.clone();
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushUndo { label });
            });
        }
    }
    fn set_dirty(&mut self) {
        if let Some(s) = &self.sender {
            let s = s.clone();
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::SetDirty);
            });
        }
    }
    fn push_info(&mut self, msg: &str) {
        if let Some(s) = &self.sender {
            let msg = msg.to_string();
            let s = s.clone();
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushInfo(msg));
            });
        } else {
            println!("[INFO] {}", msg);
        }
    }
    fn push_output(&mut self, msg: &str) {
        self.captured_output.lock().unwrap().push(msg.to_string());
        if let Some(s) = &self.sender {
            let msg = msg.to_string();
            let s = s.clone();
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushOutput(msg));
            });
        } else {
            println!("[OUTPUT] {}", msg);
        }
    }
    fn push_error(&mut self, msg: &str) {
        if let Some(s) = &self.sender {
            let msg = msg.to_string();
            let s = s.clone();
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(msg));
            });
        } else {
            println!("[ERROR] {}", msg);
        }
    }
    fn start_interactive(&mut self, _command: Box<dyn InteractiveCommand>) {}
    fn execute_command(&mut self, cmd: &str) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            match s.request(PluginRequest::ExecuteCommand(cmd.to_string())) {
                Ok(PluginResponse::Bool(b)) => {
                    unsafe { *self.doc.get() = None; }
                    return b;
                }
                _ => {}
            }
        }
        true
    }
    fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<ocs_plugin_api::host::Handle> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::OptHandle(h)) = s.request(PluginRequest::AddLayer(config)) {
                if h.is_some() {
                    unsafe { *self.doc.get() = None; }
                }
                return h;
            }
        }
        None
    }
    fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::Bool(b)) = s.request(PluginRequest::ModifyLayer(config)) {
                if b {
                    unsafe { *self.doc.get() = None; }
                }
                return b;
            }
        }
        false
    }
    fn solid_operation(&mut self, operation: ocs_plugin_api::host::SolidOperation) -> Result<ocs_plugin_api::host::Handle, String> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::SolidResult(res)) = s.request(PluginRequest::SolidOperation { operation }) {
                return res;
            }
        }
        Ok(ocs_plugin_api::host::Handle::new(1001))
    }
    fn table_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<ocs_plugin_api::host::Handle, String> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::TableResult(res)) = s.request(PluginRequest::TableOperation { operation: operation.clone() }) {
                unsafe { *self.doc.get() = None; }
                return res;
            }
        }
        match operation {
            ocs_plugin_api::host::TableOperation::LayerCreate { config } => {
                if let Some(h) = self.add_layer(config) {
                    Ok(h)
                } else {
                    Err("Failed to create layer".to_string())
                }
            }
            ocs_plugin_api::host::TableOperation::LayerModify { config } => {
                if self.modify_layer(config) {
                    Ok(ocs_plugin_api::host::Handle::new(1))
                } else {
                    Err("Failed to modify layer".to_string())
                }
            }
            _ => Ok(ocs_plugin_api::host::Handle::new(1)),
        }
    }
    fn system_variable(&self, name: &str) -> Option<ocs_plugin_api::host::HostSettingValue> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::SystemVariable(res)) = s.request(PluginRequest::GetSystemVariable { name: name.to_string() }) {
                return res;
            }
        }
        None
    }
    fn set_system_variable(&mut self, name: &str, value: ocs_plugin_api::host::HostSettingValue) -> Result<ocs_plugin_api::host::HostSettingValue, String> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::SystemVariableResult(res)) = s.request(PluginRequest::SetSystemVariable { name: name.to_string(), value: value.clone() }) {
                unsafe { *self.doc.get() = None; }
                return res;
            }
        }
        Ok(value)
    }
    fn selection(&self) -> Vec<ocs_plugin_api::host::Handle> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::Selection(res)) = s.request(PluginRequest::GetSelection) {
                return res;
            }
        }
        Vec::new()
    }
    fn set_selection(&mut self, handles: &[ocs_plugin_api::host::Handle]) -> Result<(), String> {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::SelectionResult(res)) = s.request(PluginRequest::SetSelection { handles: handles.to_vec() }) {
                return res;
            }
        }
        Ok(())
    }
    fn show_dialog(&mut self, dialog: ocs_plugin_api::dialog::DialogDef) -> ocs_plugin_api::dialog::DialogResponse {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::DialogResult(res)) = s.request(PluginRequest::ShowDialog(dialog)) {
                return res;
            }
        }
        ocs_plugin_api::dialog::DialogResponse::Cancelled
    }
    fn register_panel(&mut self, panel: ocs_plugin_api::dialog::PanelDef) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::PanelRegistered(res)) = s.request(PluginRequest::RegisterPanel(panel)) {
                return res;
            }
        }
        false
    }
    fn update_panel(
        &mut self,
        panel_id: &str,
        values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>,
    ) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::PanelUpdated(res)) = s.request(PluginRequest::UpdatePanel {
                panel_id: panel_id.to_string(),
                values,
            }) {
                return res;
            }
        }
        false
    }
    fn toggle_panel(&mut self, panel_id: &str, show: Option<bool>) -> bool {
        if let Some(s) = &self.sender {
            use ocs_plugin_api::ipc::protocol::{PluginRequest, PluginResponse};
            if let Ok(PluginResponse::PanelToggled(res)) = s.request(PluginRequest::TogglePanel {
                panel_id: panel_id.to_string(),
                show,
            }) {
                return res;
            }
        }
        false
    }
    fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn std::any::Any + Send + Sync)> { None }
    fn plugin_state_any_mut(&mut self, _plugin_id: &str) -> Option<&mut (dyn std::any::Any + Send + Sync)> { None }
    fn ensure_plugin_state_any(&mut self, _plugin_id: &'static str, _init: &mut dyn FnMut() -> Box<dyn std::any::Any + Send + Sync>) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
        None
    }
}

/// Execute a closure with mutable access to the active `HostApi`.
pub fn with_host<F, R: Send>(f: F) -> Result<R, &'static str>
where
    F: FnOnce(&mut dyn HostApi) -> R + Send,
{
    // 1. Direct host access (if running directly on main thread)
    let has_direct = ACTIVE_HOST.with(|cell| cell.borrow().is_some());
    if has_direct {
        return ACTIVE_HOST.with(|cell| {
            if let Some(ptr) = *cell.borrow() {
                let host = unsafe { &mut *ptr };
                Ok(f(host))
            } else {
                Err("No active OpenCAD Studio host session")
            }
        });
    }

    // 2. Debug worker thread fallback (using sender or standalone host)
    let is_debug = DEBUG_ACTIVE.with(|cell| *cell.borrow());
    if is_debug {
        let sender = SENDER.with(|cell| cell.borrow().clone());
        let mut host = DebugHost::new(sender);
        return Ok(f(&mut host));
    }

    // 3. If an interactive session is active, the dispatch loop on the main thread
    // has already yielded control to OpenCADStudio's interactive coordinator.
    // WorkerMsg::HostCall cannot be handled by the main thread while it's in wait_for_step.
    if active_interactive().is_some() {
        if let Some(sender) = get_global_sender() {
            let mut host = DebugHost::new(Some(sender));
            return Ok(f(&mut host));
        }
        return Err("Host API is deferred during interactive command execution");
    }

    // 4. Worker thread forwarding to main thread
    let tx_opt = WORKER_TX.with(|cell| cell.borrow().clone());
    if let Some(tx) = tx_opt {
        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let f_cell = std::sync::Arc::new(std::sync::Mutex::new(Some(f)));
        let f_clone = std::sync::Arc::clone(&f_cell);
        let task: Box<dyn FnMut(&mut dyn HostApi) + Send> = Box::new(move |host: &mut dyn HostApi| {
            if let Some(func) = f_clone.lock().unwrap().take() {
                let res = func(host);
                let _ = resp_tx.send(res);
            }
        });

        // Erase lifetime for channel transport.
        // Safety: We synchronously block on resp_rx.recv() below, ensuring task has finished
        // before with_host returns and before any captured references expire.
        let erased_task: HostTask = unsafe { std::mem::transmute(task) };

        if tx.send(WorkerMsg::HostCall(erased_task)).is_ok() {
            if let Ok(res) = resp_rx.recv() {
                return Ok(res);
            }
        }
        // Channel closed (e.g. main thread finished dispatch loop after start_interactive).
        // Clear WORKER_TX so subsequent calls fall through to step 4.
        WORKER_TX.with(|cell| *cell.borrow_mut() = None);
        if let Some(func) = f_cell.lock().unwrap().take() {
            if let Some(sender) = get_global_sender() {
                let mut host = DebugHost::new(Some(sender));
                return Ok(func(&mut host));
            }
        }
        return Err("No active OpenCAD Studio host session");
    }

    // 4. Background thread fallback via global sender (e.g. IDE RunScript, DebugScript, or REPL)
    if let Some(sender) = get_global_sender() {
        let mut host = DebugHost::new(Some(sender));
        return Ok(f(&mut host));
    }

    Err("No active OpenCAD Studio host session")
}

/// Obtain a clone of the active interactive bridge, if any.
pub fn active_interactive() -> Option<Arc<LispInteractiveBridge>> {
    ACTIVE_INTERACTIVE.with(|cell| cell.borrow().clone())
}

fn start_interactive_bridge(
    initial_prompt: &str,
    mode: crate::interactive::InteractionMode,
    dialog: Option<ocs_plugin_api::dialog::DialogDef>,
) -> Result<Arc<LispInteractiveBridge>, String> {
    crate::rust_lisp::flush_lisp_output();
    if let Some(bridge) = active_interactive() {
        return Ok(bridge);
    }

    let (base_handle, doc_snapshot) = with_host(|h| {
        let next = h.document().next_handle();
        let snapshot = h.document().clone();
        (next, Some(snapshot))
    }).unwrap_or_else(|_| (1, None));
    let (bridge, cmd) = if let Some(def) = dialog {
        LispInteractiveBridge::new_with_dialog(initial_prompt.to_string(), base_handle, doc_snapshot, def)
    } else {
        LispInteractiveBridge::new(initial_prompt.to_string(), mode, base_handle, doc_snapshot)
    };
    let bridge_clone = Arc::clone(&bridge);

    ACTIVE_INTERACTIVE.with(|cell| {
        *cell.borrow_mut() = Some(bridge_clone);
    });

    let tx_opt = WORKER_TX.with(|cell| cell.borrow().clone());
    if let Some(tx) = tx_opt {
        tx.send(WorkerMsg::InteractiveStarted(cmd))
            .map_err(|e| e.to_string())?;
    } else {
        with_host(|h| {
            h.start_interactive(cmd);
        }).map_err(|e| e.to_string())?;
    }

    Ok(bridge)
}

/// Ensure an interactive bridge is active with an explicit interaction mode.
pub fn ensure_interactive_with_mode(
    initial_prompt: &str,
    mode: crate::interactive::InteractionMode,
) -> Result<Arc<LispInteractiveBridge>, String> {
    start_interactive_bridge(initial_prompt, mode, None)
}

/// Ensure an interactive bridge is active, creating and starting one on the host if not already running.
pub fn ensure_interactive(initial_prompt: &str, is_entity_pick: bool) -> Result<Arc<LispInteractiveBridge>, String> {
    let mode = if is_entity_pick {
        crate::interactive::InteractionMode::EntityPick
    } else {
        crate::interactive::InteractionMode::Point
    };
    start_interactive_bridge(initial_prompt, mode, None)
}

/// Ensure an interactive bridge with an initial modal dialog is active.
pub fn ensure_interactive_dialog(initial_prompt: &str, dialog: ocs_plugin_api::dialog::DialogDef) -> Result<Arc<LispInteractiveBridge>, String> {
    start_interactive_bridge(initial_prompt, crate::interactive::InteractionMode::Point, Some(dialog))
}
