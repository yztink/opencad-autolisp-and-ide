use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use ocs_plugin_api::host::{CommandStep, Document as CadDocument, EntityType, Handle, InteractiveCommand, PreviewWire};

#[derive(Debug, Clone)]
pub enum InteractiveResponse {
    Point([f64; 3]),
    Text(String),
    Enter,
    EntityPick { handle: u64, pt: [f64; 3] },
    Selection(Vec<u64>),
    Dialog(ocs_plugin_api::dialog::DialogResponse),
    DialogAction {
        event: ocs_plugin_api::dialog::DialogEvent,
        resp_tx: Sender<ocs_plugin_api::dialog::DialogActionResponse>,
    },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Point,
    EntityPick,
    Selection,
    Text { free_text: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerInteractiveState {
    WaitingForInput,
    Finished,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum RubberBandSpec {
    #[default]
    None,
    Line { from: [f64; 3], color: Option<[f32; 4]> },
    Rectangle { corner: [f64; 3], color: Option<[f32; 4]> },
    Circle { center: [f64; 3], color: Option<[f32; 4]> },
    Arc3Point { p1: [f64; 3], p2: [f64; 3], color: Option<[f32; 4]> },
    ArcCenterStart { center: [f64; 3], start: [f64; 3], color: Option<[f32; 4]> },
    TangentArc { start: [f64; 3], tangent: [f64; 3], color: Option<[f32; 4]> },
    Custom { wires: Vec<Vec<[f64; 3]>>, color: Option<[f32; 4]> },
}

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub fn dbg_trace(msg: &str) {
    if std::env::var_os("OCS_TRACE").is_some() {
        use std::io::Write;
        let log_dir = std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("OpenCADStudio");
        let _ = std::fs::create_dir_all(&log_dir);
        let path = log_dir.join("interactive_trace.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now(), msg);
        }
    }
}

fn circumcircle_3p(p1: [f64; 3], p2: [f64; 3], p3: [f64; 3]) -> Option<([f64; 3], f64, bool)> {
    let ax = p1[0];
    let ay = p1[1];
    let bx = p2[0];
    let by = p2[1];
    let cx = p3[0];
    let cy = p3[1];

    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-10 {
        return None;
    }

    let a_sq = ax * ax + ay * ay;
    let b_sq = bx * bx + by * by;
    let c_sq = cx * cx + cy * cy;

    let ux = (a_sq * (by - cy) + b_sq * (cy - ay) + c_sq * (ay - by)) / d;
    let uy = (a_sq * (cx - bx) + b_sq * (ax - cx) + c_sq * (bx - ax)) / d;
    let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();

    let cross = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
    let ccw = cross > 0.0;

    Some(([ux, uy, p1[2]], r, ccw))
}

fn dist_to_segment(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

fn distance_to_entity(entity: &EntityType, pt: [f64; 3]) -> f64 {
    let px = pt[0];
    let py = pt[1];
    match entity {
        EntityType::Line(l) => dist_to_segment(px, py, l.start.x, l.start.y, l.end.x, l.end.y),
        EntityType::Arc(a) => {
            let dx = px - a.center.x;
            let dy = py - a.center.y;
            let d_center = (dx * dx + dy * dy).sqrt();
            let d_radius = (d_center - a.radius).abs();
            let angle = dy.atan2(dx).rem_euclid(2.0 * std::f64::consts::PI);
            let s = a.start_angle.rem_euclid(2.0 * std::f64::consts::PI);
            let e = a.end_angle.rem_euclid(2.0 * std::f64::consts::PI);
            let on_arc = if s <= e {
                angle >= s && angle <= e
            } else {
                angle >= s || angle <= e
            };
            if on_arc {
                d_radius
            } else {
                let p1x = a.center.x + a.radius * a.start_angle.cos();
                let p1y = a.center.y + a.radius * a.start_angle.sin();
                let p2x = a.center.x + a.radius * a.end_angle.cos();
                let p2y = a.center.y + a.radius * a.end_angle.sin();
                let d1 = ((px - p1x).powi(2) + (py - p1y).powi(2)).sqrt();
                let d2 = ((px - p2x).powi(2) + (py - p2y).powi(2)).sqrt();
                d1.min(d2)
            }
        }
        EntityType::Circle(c) => {
            let dx = px - c.center.x;
            let dy = py - c.center.y;
            ((dx * dx + dy * dy).sqrt() - c.radius).abs()
        }
        EntityType::Point(p) => {
            let dx = px - p.location.x;
            let dy = py - p.location.y;
            (dx * dx + dy * dy).sqrt()
        }
        _ => f64::MAX,
    }
}

pub struct InteractiveCoordinator {
    prompt: Mutex<String>,
    mode: Mutex<InteractionMode>,
    selection_buffer: Mutex<Vec<u64>>,
    tx: Mutex<Sender<InteractiveResponse>>,
    done: Mutex<bool>,
    pub error: Mutex<Option<String>>,
    status_tx: Mutex<Sender<WorkerInteractiveState>>,
    status_rx: Mutex<Receiver<WorkerInteractiveState>>,
    waiting_in_step: AtomicBool,
    ack_tx: Mutex<Sender<()>>,
    ack_rx: Mutex<Receiver<()>>,
    pub pending_entities: Mutex<Vec<EntityType>>,
    pub doc_snapshot: Arc<Mutex<Option<CadDocument>>>,
    pub rubber_band: Mutex<RubberBandSpec>,
    pub modal_dialog: Mutex<Option<ocs_plugin_api::dialog::DialogDef>>,
    pub pending_panel_updates: Mutex<Vec<(String, std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>)>>,
    pub queued_commands: Mutex<Vec<String>>,
    pub is_single_selection: AtomicBool,
    pub is_reject_locked: AtomicBool,
    pub is_point_only: AtomicBool,
}

impl InteractiveCoordinator {
    pub fn new(
        initial_prompt: String,
        mode: InteractionMode,
        tx: Sender<InteractiveResponse>,
        status_tx: Sender<WorkerInteractiveState>,
        status_rx: Receiver<WorkerInteractiveState>,
        doc_snapshot: Arc<Mutex<Option<CadDocument>>>,
    ) -> Self {
        let (ack_tx, ack_rx) = channel();
        Self {
            prompt: Mutex::new(initial_prompt),
            mode: Mutex::new(mode),
            selection_buffer: Mutex::new(Vec::new()),
            tx: Mutex::new(tx),
            done: Mutex::new(false),
            error: Mutex::new(None),
            status_tx: Mutex::new(status_tx),
            status_rx: Mutex::new(status_rx),
            waiting_in_step: AtomicBool::new(false),
            ack_tx: Mutex::new(ack_tx),
            ack_rx: Mutex::new(ack_rx),
            pending_entities: Mutex::new(Vec::new()),
            doc_snapshot,
            rubber_band: Mutex::new(RubberBandSpec::None),
            modal_dialog: Mutex::new(None),
            pending_panel_updates: Mutex::new(Vec::new()),
            queued_commands: Mutex::new(Vec::new()),
            is_single_selection: AtomicBool::new(false),
            is_reject_locked: AtomicBool::new(false),
            is_point_only: AtomicBool::new(false),
        }
    }

    pub fn set_modal_dialog(&self, dialog: Option<ocs_plugin_api::dialog::DialogDef>) {
        if let Ok(mut d) = self.modal_dialog.lock() {
            *d = dialog;
        }
    }

    pub fn get_modal_dialog(&self) -> Option<ocs_plugin_api::dialog::DialogDef> {
        self.modal_dialog.lock().ok().and_then(|d| d.clone())
    }

    pub fn handle_dialog_action(&self, event: ocs_plugin_api::dialog::DialogEvent) -> ocs_plugin_api::dialog::DialogActionResponse {
        if self.is_done() || self.get_error().is_some() {
            return ocs_plugin_api::dialog::DialogActionResponse::None;
        }
        let (resp_tx, resp_rx) = channel();
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if tx.send(InteractiveResponse::DialogAction { event, resp_tx }).is_err() {
            return ocs_plugin_api::dialog::DialogActionResponse::None;
        }
        resp_rx.recv().unwrap_or(ocs_plugin_api::dialog::DialogActionResponse::None)
    }

    pub fn handle_dialog_response(&self, response: ocs_plugin_api::dialog::DialogResponse) -> CommandStep {
        if self.is_done() || self.get_error().is_some() {
            return CommandStep::Cancel;
        }
        if let Ok(mut d) = self.modal_dialog.lock() {
            *d = None;
        }
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let _ = tx.send(InteractiveResponse::Dialog(response));
        self.wait_for_step()
    }

    pub fn set_rubber_band(&self, spec: RubberBandSpec) {
        if let Ok(mut rb) = self.rubber_band.lock() {
            *rb = spec;
        }
    }

    pub fn generate_preview_wires(&self, pt: [f64; 3]) -> Vec<PreviewWire> {
        let spec = if let Ok(rb) = self.rubber_band.lock() {
            rb.clone()
        } else {
            return Vec::new();
        };

        match spec {
            RubberBandSpec::None => Vec::new(),
            RubberBandSpec::Line { from, color } => {
                vec![PreviewWire::line(from, pt, color)]
            }
            RubberBandSpec::Rectangle { corner, color } => {
                let x1 = corner[0];
                let y1 = corner[1];
                let z = corner[2];
                let x2 = pt[0];
                let y2 = pt[1];
                vec![PreviewWire::Polyline {
                    points: vec![
                        [x1, y1, z],
                        [x2, y1, z],
                        [x2, y2, z],
                        [x1, y2, z],
                        [x1, y1, z],
                    ],
                    color,
                }]
            }
            RubberBandSpec::Circle { center, color } => {
                let dx = pt[0] - center[0];
                let dy = pt[1] - center[1];
                let r = (dx * dx + dy * dy).sqrt();
                if r < 1e-6 {
                    return Vec::new();
                }
                vec![PreviewWire::circle(center, r, color)]
            }
            RubberBandSpec::Arc3Point { p1, p2, color } => {
                if let Some((center, radius, ccw)) = circumcircle_3p(p1, p2, pt) {
                    let sa = (p1[1] - center[1]).atan2(p1[0] - center[0]);
                    let ea = (pt[1] - center[1]).atan2(pt[0] - center[0]);
                    let (start_angle_rad, end_angle_rad) = if ccw {
                        (sa, ea)
                    } else {
                        (ea, sa)
                    };
                    vec![PreviewWire::arc(center, radius, start_angle_rad, end_angle_rad, color)]
                } else {
                    vec![PreviewWire::Polyline {
                        points: vec![p1, p2, pt],
                        color,
                    }]
                }
            }
            RubberBandSpec::ArcCenterStart { center, start, color } => {
                let dx = start[0] - center[0];
                let dy = start[1] - center[1];
                let radius = (dx * dx + dy * dy).sqrt();
                if radius < 1e-6 {
                    return Vec::new();
                }
                let sa = dy.atan2(dx);
                let ea = (pt[1] - center[1]).atan2(pt[0] - center[0]);
                vec![PreviewWire::arc(center, radius, sa, ea, color)]
            }
            RubberBandSpec::TangentArc { start, tangent, color } => {
                let qx = pt[0] - start[0];
                let qy = pt[1] - start[1];
                let qlen = (qx * qx + qy * qy).sqrt();
                if qlen < 1e-6 {
                    return Vec::new();
                }
                let tlen = (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt();
                if tlen < 1e-9 {
                    return vec![PreviewWire::line(start, pt, color)];
                }
                let tx = tangent[0] / tlen;
                let ty = tangent[1] / tlen;
                let nx = -ty;
                let ny = tx;
                let dot = qx * nx + qy * ny;
                if dot.abs() < 1e-9 {
                    return vec![PreviewWire::line(start, pt, color)];
                }
                let h = (qlen * qlen) / (2.0 * dot);
                let cx = start[0] + h * nx;
                let cy = start[1] + h * ny;
                let radius = h.abs();
                let sa = (start[1] - cy).atan2(start[0] - cx);
                let ea = (pt[1] - cy).atan2(pt[0] - cx);
                let ccw = h > 0.0;
                let (start_angle_rad, end_angle_rad) = if ccw {
                    (sa, ea)
                } else {
                    (ea, sa)
                };
                vec![PreviewWire::arc([cx, cy, start[2]], radius, start_angle_rad, end_angle_rad, color)]
            }
            RubberBandSpec::Custom { wires, color } => {
                wires.into_iter().map(|points| PreviewWire::Polyline { points, color }).collect()
            }
        }
    }

    pub fn set_prompt(&self, prompt: String) {
        if let Ok(mut p) = self.prompt.lock() {
            *p = prompt;
        }
    }

    pub fn set_mode(&self, mode: InteractionMode) {
        if let Ok(mut m) = self.mode.lock() {
            *m = mode;
        }
        if mode == InteractionMode::Selection {
            if let Ok(mut s) = self.selection_buffer.lock() {
                s.clear();
            }
        }
    }

    pub fn queue_command(&self, cmd: String) {
        if let Ok(mut cmds) = self.queued_commands.lock() {
            cmds.push(cmd);
        }
    }

    pub fn set_single_selection(&self, single: bool) {
        self.is_single_selection.store(single, Ordering::SeqCst);
    }

    pub fn set_reject_locked(&self, reject: bool) {
        self.is_reject_locked.store(reject, Ordering::SeqCst);
    }

    pub fn set_point_only(&self, point_only: bool) {
        self.is_point_only.store(point_only, Ordering::SeqCst);
    }

    pub fn set_mode_flags(&self, reject_locked: bool, single_select: bool, point_only: bool) {
        self.set_reject_locked(reject_locked);
        self.set_single_selection(single_select);
        self.set_point_only(point_only);
    }

    pub fn mark_done(&self) {
        if let Ok(mut d) = self.done.lock() {
            *d = true;
        }
    }

    pub fn is_done(&self) -> bool {
        *self.done.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn prompt_str(&self) -> String {
        self.prompt.lock().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn wants_object_pick(&self) -> bool {
        if let Ok(m) = self.mode.lock() {
            matches!(*m, InteractionMode::EntityPick)
        } else {
            false
        }
    }

    pub fn is_selection_gathering(&self) -> bool {
        if let Ok(m) = self.mode.lock() {
            *m == InteractionMode::Selection
        } else {
            false
        }
    }

    pub fn get_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn notify_error(&self, err_msg: String) {
        if let Ok(mut e) = self.error.lock() {
            *e = Some(err_msg.clone());
        }
        // Broadcast error directly to OCSIDE console
        crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::ConsoleOutput {
            text: format!("\n[Error] {}\n", err_msg),
            is_error: true,
        });
        // Unblock wait_for_step first so host UI thread does not deadlock!
        self.notify_finished();
        // Push error directly to CAD command line asynchronously
        if let Some(s) = crate::context::get_global_sender() {
            std::thread::spawn(move || {
                use ocs_plugin_api::ipc::protocol::PluginRequest;
                let _ = s.request(PluginRequest::PushError(format!("\n[Error] {}\n", err_msg)));
            });
        }
    }

    pub fn notify_waiting_for_input(&self) {
        dbg_trace("notify_waiting_for_input: sending WaitingForInput");
        if let Ok(tx) = self.status_tx.lock() {
            let _ = tx.send(WorkerInteractiveState::WaitingForInput);
        }
    }

    pub fn notify_finished(&self) {
        self.mark_done();
        let was_waiting = self.waiting_in_step.load(Ordering::SeqCst);
        dbg_trace(&format!("notify_finished: start, was_waiting={}", was_waiting));
        if let Ok(tx) = self.status_tx.lock() {
            let _ = tx.send(WorkerInteractiveState::Finished);
        }
        dbg_trace("notify_finished: sent Finished");
        if was_waiting {
            if let Ok(rx) = self.ack_rx.lock() {
                dbg_trace("notify_finished: waiting ack_rx...");
                let ack_res = rx.recv_timeout(Duration::from_millis(500));
                dbg_trace(&format!("notify_finished: ack_rx returned {:?}", ack_res));
            }
        }
        dbg_trace("notify_finished: done");
    }

    pub fn wait_for_step(&self) -> CommandStep {
        self.waiting_in_step.store(true, Ordering::SeqCst);
        let rx = self.status_rx.lock().unwrap_or_else(|e| e.into_inner());
        dbg_trace("wait_for_step: start debug-aware recv loop");
        let poll_interval = Duration::from_millis(250);
        let mut non_debug_elapsed = Duration::ZERO;
        let mut debug_ticks = 0u32;
        let res = loop {
            match rx.recv_timeout(poll_interval) {
                Ok(state) => break Ok(state),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(std::sync::mpsc::RecvTimeoutError::Disconnected);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if crate::rust_lisp::is_currently_paused_in_debugger()
                        || crate::rust_lisp::is_debug_session_active()
                    {
                        non_debug_elapsed = Duration::ZERO;
                        debug_ticks = debug_ticks.wrapping_add(1);
                        if debug_ticks % 8 == 0 {
                            crate::rust_lisp::send_host_keepalive();
                        }
                    } else {
                        non_debug_elapsed += poll_interval;
                        if non_debug_elapsed >= Duration::from_secs(3) {
                            break Err(std::sync::mpsc::RecvTimeoutError::Timeout);
                        }
                    }
                }
            }
        };
        dbg_trace(&format!("wait_for_step: recv loop returned {:?}", res));
        self.waiting_in_step.store(false, Ordering::SeqCst);
        if let Ok(tx) = self.ack_tx.lock() {
            let _ = tx.send(());
        }

        if self.get_error().is_some() {
            dbg_trace(&format!("wait_for_step: returning Cancel due to error: {:?}", self.get_error()));
            return CommandStep::Cancel;
        }

        match res {
            Ok(WorkerInteractiveState::WaitingForInput) => {
                let mut entities = self.pending_entities.lock().unwrap_or_else(|e| e.into_inner());
                let count = entities.len();
                if count > 1 {
                    let list = std::mem::take(&mut *entities);
                    dbg_trace(&format!("wait_for_step: returning CommitMany({})", list.len()));
                    CommandStep::CommitMany(list)
                } else if count == 1 {
                    let first = entities.remove(0);
                    dbg_trace("wait_for_step: returning Commit(1)");
                    CommandStep::Commit(first)
                } else {
                    dbg_trace("wait_for_step: returning NeedPoint");
                    CommandStep::NeedPoint
                }
            }
            Ok(WorkerInteractiveState::Finished) => {
                if self.get_error().is_some() {
                    return CommandStep::Cancel;
                }
                let mut entities = self.pending_entities.lock().unwrap_or_else(|e| e.into_inner());
                let list = std::mem::take(&mut *entities);
                if list.len() > 1 {
                    dbg_trace(&format!("wait_for_step: returning CommitManyAndEnd({})", list.len()));
                    CommandStep::CommitManyAndEnd(list)
                } else if let Some(first) = list.into_iter().next() {
                    dbg_trace("wait_for_step: returning CommitAndEnd(1)");
                    CommandStep::CommitAndEnd(first)
                } else {
                    let mut cmds = self.queued_commands.lock().unwrap_or_else(|e| e.into_inner());
                    if !cmds.is_empty() {
                        let joined = cmds.drain(..).collect::<Vec<_>>().join("\n");
                        dbg_trace(&format!("wait_for_step: returning Dispatch: {}", joined));
                        CommandStep::Dispatch(joined)
                    } else {
                        dbg_trace("wait_for_step: returning Done");
                        CommandStep::Done
                    }
                }
            }
            Err(e) => {
                dbg_trace(&format!("wait_for_step: worker status channel {:?}, concluding step cleanly", e));
                if self.get_error().is_some() {
                    return CommandStep::Cancel;
                }
                let mut entities = self.pending_entities.lock().unwrap_or_else(|e| e.into_inner());
                let list = std::mem::take(&mut *entities);
                if list.len() > 1 {
                    CommandStep::CommitManyAndEnd(list)
                } else if let Some(first) = list.into_iter().next() {
                    CommandStep::CommitAndEnd(first)
                } else {
                    CommandStep::Done
                }
            }
        }
    }

    pub fn handle_point(&self, pt: [f64; 3]) -> CommandStep {
        dbg_trace(&format!("handle_point: called with pt={:?}", pt));
        if self.is_done() || self.get_error().is_some() {
            dbg_trace(&format!("handle_point: early cancel, is_done={}, error={:?}", self.is_done(), self.get_error()));
            return CommandStep::Cancel;
        }
        let mode = {
            if let Ok(m) = self.mode.lock() {
                *m
            } else {
                InteractionMode::Point
            }
        };

        if mode == InteractionMode::Point {
            let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
            let _ = tx.send(InteractiveResponse::Point(pt));
            dbg_trace("handle_point: sent InteractiveResponse::Point to worker");
            return self.wait_for_step();
        } else if mode == InteractionMode::EntityPick {
            let mut closest: Option<(u64, f64)> = None;
            if let Ok(guard) = self.doc_snapshot.lock() {
                if let Some(doc) = guard.as_ref() {
                    for ent in doc.entities() {
                        let d = distance_to_entity(ent, pt);
                        if d < closest.map_or(f64::MAX, |(_, min_d)| min_d) {
                            closest = Some((ent.common().handle.value(), d));
                        }
                    }
                }
            }
            if let Ok(list) = self.pending_entities.lock() {
                for ent in list.iter() {
                    let d = distance_to_entity(ent, pt);
                    if d < closest.map_or(f64::MAX, |(_, min_d)| min_d) {
                        closest = Some((ent.common().handle.value(), d));
                    }
                }
            }

            if let Some((handle, _)) = closest {
                let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
                let _ = tx.send(InteractiveResponse::EntityPick { handle, pt });
                return self.wait_for_step();
            }
        } else if mode == InteractionMode::Selection {
            let mut closest: Option<(u64, f64)> = None;
            if let Ok(guard) = self.doc_snapshot.lock() {
                if let Some(doc) = guard.as_ref() {
                    for ent in doc.entities() {
                        let d = distance_to_entity(ent, pt);
                        if d < closest.map_or(f64::MAX, |(_, min_d)| min_d) {
                            closest = Some((ent.common().handle.value(), d));
                        }
                    }
                }
            }
            if let Ok(list) = self.pending_entities.lock() {
                for ent in list.iter() {
                    let d = distance_to_entity(ent, pt);
                    if d < closest.map_or(f64::MAX, |(_, min_d)| min_d) {
                        closest = Some((ent.common().handle.value(), d));
                    }
                }
            }

            if let Some((handle, _)) = closest {
                let mut should_finish = false;
                if let Ok(mut sel) = self.selection_buffer.lock() {
                    if !sel.contains(&handle) {
                        sel.push(handle);
                    }
                    if self.is_single_selection.load(Ordering::SeqCst) {
                        should_finish = true;
                    }
                }
                if should_finish {
                    let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
                    let list = if let Ok(mut sel) = self.selection_buffer.lock() {
                        std::mem::take(&mut *sel)
                    } else {
                        Vec::new()
                    };
                    let _ = tx.send(InteractiveResponse::Selection(list));
                    return self.wait_for_step();
                }
            }
        }
        CommandStep::NeedPoint
    }

    pub fn handle_object_pick(&self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        dbg_trace(&format!("handle_object_pick: called with handle={:X}, pt={:?}", handle.value(), pt));
        if self.is_done() || self.get_error().is_some() {
            dbg_trace("handle_object_pick: is_done or error, returning Cancel");
            return CommandStep::Cancel;
        }

        let mode = {
            if let Ok(m) = self.mode.lock() {
                *m
            } else {
                InteractionMode::Point
            }
        };
        dbg_trace(&format!("handle_object_pick: mode={:?}", mode));

        match mode {
            InteractionMode::EntityPick => {
                let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
                let _ = tx.send(InteractiveResponse::EntityPick {
                    handle: handle.value(),
                    pt,
                });
                self.wait_for_step()
            }
            InteractionMode::Selection => {
                let val = handle.value();
                let mut should_finish = false;
                if let Ok(mut sel) = self.selection_buffer.lock() {
                    if !sel.contains(&val) {
                        sel.push(val);
                    }
                    if self.is_single_selection.load(Ordering::SeqCst) {
                        should_finish = true;
                    }
                }
                if should_finish {
                    let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
                    let list = if let Ok(mut sel) = self.selection_buffer.lock() {
                        std::mem::take(&mut *sel)
                    } else {
                        Vec::new()
                    };
                    let _ = tx.send(InteractiveResponse::Selection(list));
                    return self.wait_for_step();
                }
                CommandStep::NeedPoint
            }
            InteractionMode::Point => {
                let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
                let _ = tx.send(InteractiveResponse::Point(pt));
                self.wait_for_step()
            }
            InteractionMode::Text { .. } => CommandStep::NeedPoint,
        }
    }

    pub fn handle_selection_complete(&self, handles: Vec<Handle>) -> CommandStep {
        dbg_trace(&format!("handle_selection_complete: received {} handles from host", handles.len()));
        if self.is_done() || self.get_error().is_some() {
            dbg_trace("handle_selection_complete: is_done or error, returning Cancel");
            return CommandStep::Cancel;
        }

        let mut should_finish = false;
        if let Ok(mut sel) = self.selection_buffer.lock() {
            for h in handles {
                let v = h.value();
                if !sel.contains(&v) {
                    sel.push(v);
                }
            }
            dbg_trace(&format!("handle_selection_complete: buffer now has {} handles", sel.len()));
            if self.is_single_selection.load(Ordering::SeqCst) && !sel.is_empty() {
                should_finish = true;
            }
        }

        if should_finish {
            dbg_trace("handle_selection_complete: single selection finished, sending to worker");
            let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
            let list = if let Ok(mut sel) = self.selection_buffer.lock() {
                std::mem::take(&mut *sel)
            } else {
                Vec::new()
            };
            let _ = tx.send(InteractiveResponse::Selection(list));
            return self.wait_for_step();
        }

        CommandStep::NeedPoint
    }

    pub fn handle_enter(&self) -> CommandStep {
        dbg_trace("handle_enter: called");
        if self.is_done() || self.get_error().is_some() {
            dbg_trace("handle_enter: is_done or error, returning Cancel");
            return CommandStep::Cancel;
        }

        let mode = {
            if let Ok(m) = self.mode.lock() {
                *m
            } else {
                InteractionMode::Point
            }
        };
        dbg_trace(&format!("handle_enter: mode={:?}", mode));

        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        match mode {
            InteractionMode::Selection => {
                let list = if let Ok(mut sel) = self.selection_buffer.lock() {
                    std::mem::take(&mut *sel)
                } else {
                    Vec::new()
                };
                dbg_trace(&format!("handle_enter: sending Selection with {} items to worker", list.len()));
                let _ = tx.send(InteractiveResponse::Selection(list));
            }
            InteractionMode::Text { .. } => {
                dbg_trace("handle_enter: sending Enter to worker");
                let _ = tx.send(InteractiveResponse::Enter);
            }
            _ => {
                dbg_trace("handle_enter: sending Cancelled to worker");
                let _ = tx.send(InteractiveResponse::Cancelled);
            }
        }
        self.wait_for_step()
    }

    pub fn input_kind(&self) -> ocs_plugin_api::host::InteractiveInputKind {
        match self.mode.lock().ok().map(|m| *m) {
            Some(InteractionMode::Text { free_text: true }) => {
                ocs_plugin_api::host::InteractiveInputKind::FreeText
            }
            Some(InteractionMode::Text { free_text: false }) => {
                ocs_plugin_api::host::InteractiveInputKind::SingleToken
            }
            _ => ocs_plugin_api::host::InteractiveInputKind::Point,
        }
    }

    pub fn handle_text_input(&self, text: &str) -> CommandStep {
        dbg_trace(&format!("handle_text_input: called with text={:?}", text));
        if self.is_done() || self.get_error().is_some() {
            return CommandStep::Cancel;
        }
        let is_text = matches!(
            self.mode.lock().ok().map(|m| *m),
            Some(InteractionMode::Text { .. })
        );
        if !is_text {
            return CommandStep::NeedPoint;
        }
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let _ = tx.send(InteractiveResponse::Text(text.to_string()));
        self.wait_for_step()
    }

    pub fn add_panel_update(&self, panel_id: String, values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>) {
        if let Ok(mut list) = self.pending_panel_updates.lock() {
            list.push((panel_id, values));
        }
    }

    pub fn take_pending_panel_updates(&self) -> Vec<(String, std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>)> {
        if let Ok(mut list) = self.pending_panel_updates.lock() {
            std::mem::take(&mut *list)
        } else {
            Vec::new()
        }
    }
}

pub struct CoordinatorProxy(pub Arc<InteractiveCoordinator>);

impl InteractiveCommand for CoordinatorProxy {
    fn prompt(&self) -> String {
        self.0.prompt_str()
    }
    fn needs_object_pick(&self) -> bool {
        self.0.wants_object_pick()
    }
    fn is_selection_gathering(&self) -> bool {
        self.0.is_selection_gathering()
    }
    fn accepts_drag_selection(&self) -> bool {
        self.0.is_selection_gathering() && !self.0.is_point_only.load(Ordering::SeqCst)
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CommandStep {
        self.0.handle_selection_complete(handles)
    }
    fn selection_rejects_locked_layers(&self) -> bool {
        self.0.is_reject_locked.load(Ordering::SeqCst)
    }
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep {
        self.0.handle_point(pt)
    }
    fn on_object_pick(&mut self, handle: Handle, pt: [f64; 3]) -> CommandStep {
        self.0.handle_object_pick(handle, pt)
    }
    fn on_enter(&mut self) -> CommandStep {
        self.0.handle_enter()
    }
    fn on_cursor_move(&mut self, pt: [f64; 3]) -> Vec<PreviewWire> {
        self.0.generate_preview_wires(pt)
    }
    fn modal_dialog(&self) -> Option<ocs_plugin_api::dialog::DialogDef> {
        self.0.get_modal_dialog()
    }
    fn on_dialog_action(&mut self, event: ocs_plugin_api::dialog::DialogEvent) -> ocs_plugin_api::dialog::DialogActionResponse {
        self.0.handle_dialog_action(event)
    }
    fn on_dialog_response(&mut self, response: ocs_plugin_api::dialog::DialogResponse) -> CommandStep {
        self.0.handle_dialog_response(response)
    }
    fn take_pending_panel_updates(&mut self) -> Vec<(String, std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>)> {
        self.0.take_pending_panel_updates()
    }
    fn input_kind(&self) -> ocs_plugin_api::host::InteractiveInputKind {
        self.0.input_kind()
    }
    fn on_text_input(&mut self, text: &str) -> CommandStep {
        self.0.handle_text_input(text)
    }
}

impl Drop for CoordinatorProxy {
    fn drop(&mut self) {
        if !self.0.is_done() {
            let tx = self.0.tx.lock().unwrap_or_else(|e| e.into_inner());
            let _ = tx.send(InteractiveResponse::Cancelled);
            self.0.notify_finished();
        }
    }
}

pub struct LispInteractiveBridge {
    pub coordinator: Arc<InteractiveCoordinator>,
    pub rx: Mutex<Receiver<InteractiveResponse>>,
    has_started: Mutex<bool>,
    base_handle: u64,
    next_offset: AtomicU64,
    pub doc_snapshot: Arc<Mutex<Option<CadDocument>>>,
}

pub type PythonInteractiveBridge = LispInteractiveBridge;

impl LispInteractiveBridge {
    pub fn new(
        initial_prompt: String,
        mode: InteractionMode,
        base_handle: u64,
        doc_snapshot: Option<CadDocument>,
    ) -> (Arc<Self>, Box<dyn InteractiveCommand>) {
        let (tx, rx) = channel();
        let (status_tx, status_rx) = channel();
        let snapshot_arc = Arc::new(Mutex::new(doc_snapshot));
        let coordinator = Arc::new(InteractiveCoordinator::new(
            initial_prompt,
            mode,
            tx,
            status_tx,
            status_rx,
            Arc::clone(&snapshot_arc),
        ));
        let bridge = Arc::new(LispInteractiveBridge {
            coordinator: Arc::clone(&coordinator),
            rx: Mutex::new(rx),
            has_started: Mutex::new(false),
            base_handle,
            next_offset: AtomicU64::new(0),
            doc_snapshot: snapshot_arc,
        });
        (bridge, Box::new(CoordinatorProxy(coordinator)))
    }

    pub fn new_with_dialog(
        initial_prompt: String,
        base_handle: u64,
        doc_snapshot: Option<CadDocument>,
        dialog: ocs_plugin_api::dialog::DialogDef,
    ) -> (Arc<Self>, Box<dyn InteractiveCommand>) {
        let (tx, rx) = channel();
        let (status_tx, status_rx) = channel();
        let snapshot_arc = Arc::new(Mutex::new(doc_snapshot));
        let coordinator = Arc::new(InteractiveCoordinator::new(
            initial_prompt,
            InteractionMode::Point,
            tx,
            status_tx,
            status_rx,
            Arc::clone(&snapshot_arc),
        ));
        coordinator.set_modal_dialog(Some(dialog));
        let bridge = Arc::new(LispInteractiveBridge {
            coordinator: Arc::clone(&coordinator),
            rx: Mutex::new(rx),
            has_started: Mutex::new(false),
            base_handle,
            next_offset: AtomicU64::new(0),
            doc_snapshot: snapshot_arc,
        });
        (bridge, Box::new(CoordinatorProxy(coordinator)))
    }

    pub fn query_layer_locked(&self, layer_name: &str) -> bool {
        if let Ok(guard) = self.doc_snapshot.lock() {
            if let Some(doc) = guard.as_ref() {
                if let Some(layer) = doc.layers.get(layer_name) {
                    return layer.is_locked();
                }
            }
        }
        false
    }

    pub fn query_entity<R>(&self, handle: u64, f: impl FnOnce(&EntityType) -> R) -> Option<R> {
        if let Ok(list) = self.coordinator.pending_entities.lock() {
            if let Some(ent) = list.iter().find(|e| e.common().handle.value() == handle) {
                return Some(f(ent));
            }
        }
        if let Ok(guard) = self.doc_snapshot.lock() {
            if let Some(doc) = guard.as_ref() {
                if let Some(ent) = doc.get_entity(Handle::new(handle)) {
                    return Some(f(ent));
                }
            }
        }
        None
    }

    pub fn allocate_handle(&self) -> Handle {
        let offset = self.next_offset.fetch_add(1, Ordering::SeqCst);
        Handle::new(self.base_handle.saturating_add(offset))
    }

    pub fn add_pending_entity(&self, mut entity: EntityType) -> u64 {
        let handle = self.allocate_handle();
        if !handle.is_null() {
            entity.as_entity_mut().set_handle(handle);
        }
        let val = entity.common().handle.value();
        if let Ok(mut list) = self.coordinator.pending_entities.lock() {
            list.push(entity.clone());
        }
        if let Ok(mut guard) = self.doc_snapshot.lock() {
            if let Some(doc) = guard.as_mut() {
                let name = entity.common().layer.trim();
                if !name.is_empty() && name != "0" && !doc.layers.contains(name) {
                    let mut l = ocs_plugin_api::host::codec::tables::Layer::new(name);
                    l.handle = doc.allocate_handle();
                    let _ = doc.layers.add(l);
                }
                let _ = doc.add_entity(entity);
            }
        }
        val
    }


    fn check_advance_step(&self) {
        let mut started = self.has_started.lock().unwrap_or_else(|e| e.into_inner());
        if *started {
            self.coordinator.notify_waiting_for_input();
        } else {
            *started = true;
        }
    }

    fn switch_focus_to_cad_for_input(&self) -> bool {
        let active = crate::rust_lisp::is_currently_paused_in_debugger()
            || crate::rust_lisp::was_paused_during_task()
            || crate::rust_lisp::is_debug_task_running();
        if active {
            crate::ide::server::broadcast_to_ide(&crate::ide::protocol::PluginToIde::FocusCad);
            crate::ide::server::force_foreground_cad();
        }
        active
    }

    fn restore_focus_to_ide_after_input(&self, had_debug_session: bool) {
        if had_debug_session && crate::rust_lisp::is_currently_paused_in_debugger() {
            let _ = crate::ide::server::focus_or_check_alive();
            crate::ide::server::force_foreground_ocside();
        }
    }

    pub fn set_rubber_band(&self, spec: RubberBandSpec) {
        self.coordinator.set_rubber_band(spec);
    }

    pub fn get_point(&self, prompt: Option<&str>) -> Option<[f64; 3]> {
        self.get_point_with_rubber_band(prompt, None)
    }

    pub fn get_point_with_rubber_band(
        &self,
        prompt: Option<&str>,
        rubber_band: Option<RubberBandSpec>,
    ) -> Option<[f64; 3]> {
        if let Some(p) = prompt {
            self.coordinator.set_prompt(p.to_string());
        }
        self.coordinator.set_mode(InteractionMode::Point);
        if let Some(rb) = rubber_band {
            self.coordinator.set_rubber_band(rb);
        }
        self.check_advance_step();

        dbg_trace(&format!("get_point_with_rubber_band: prompt={:?}, waiting on rx.recv()...", prompt));
        let debug_active = self.switch_focus_to_cad_for_input();
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let res = match rx.recv() {
            Ok(InteractiveResponse::Point(pt)) => {
                dbg_trace(&format!("get_point_with_rubber_band: got Point({:?})", pt));
                Some(pt)
            }
            other => {
                dbg_trace(&format!("get_point_with_rubber_band: got other response: {:?}", other));
                None
            }
        };
        drop(rx);
        self.coordinator.set_rubber_band(RubberBandSpec::None);
        self.restore_focus_to_ide_after_input(debug_active);
        res
    }

    pub fn get_entity(&self, prompt: Option<&str>) -> Option<(u64, [f64; 3])> {
        if let Some(p) = prompt {
            self.coordinator.set_prompt(p.to_string());
        }
        self.coordinator.set_mode(InteractionMode::EntityPick);
        self.check_advance_step();

        let debug_active = self.switch_focus_to_cad_for_input();
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let res = match rx.recv() {
            Ok(InteractiveResponse::EntityPick { handle, pt }) => Some((handle, pt)),
            _ => None,
        };
        drop(rx);
        self.restore_focus_to_ide_after_input(debug_active);
        res
    }

    pub fn get_selection(&self, prompt: Option<&str>) -> Option<Vec<u64>> {
        if let Some(p) = prompt {
            self.coordinator.set_prompt(p.to_string());
        }
        self.coordinator.set_mode(InteractionMode::Selection);
        self.check_advance_step();

        dbg_trace(&format!("get_selection: prompt={:?}, waiting on rx.recv()...", prompt));
        let debug_active = self.switch_focus_to_cad_for_input();
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let res = match rx.recv() {
            Ok(InteractiveResponse::Selection(list)) => {
                dbg_trace(&format!("get_selection: got Selection with {} items", list.len()));
                Some(list)
            }
            Ok(InteractiveResponse::EntityPick { handle, .. }) => {
                dbg_trace(&format!("get_selection: got EntityPick with handle {:X}", handle));
                Some(vec![handle])
            }
            other => {
                dbg_trace(&format!("get_selection: got other response: {:?}", other));
                None
            }
        };
        drop(rx);
        self.restore_focus_to_ide_after_input(debug_active);
        res
    }

    pub fn show_dialog_interactive<F>(
        &self,
        dialog: ocs_plugin_api::dialog::DialogDef,
        mut action_handler: F,
    ) -> ocs_plugin_api::dialog::DialogResponse
    where
        F: FnMut(ocs_plugin_api::dialog::DialogEvent) -> ocs_plugin_api::dialog::DialogActionResponse,
    {
        self.coordinator.set_modal_dialog(Some(dialog));
        self.check_advance_step();

        let debug_active = self.switch_focus_to_cad_for_input();
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let res = loop {
            match rx.recv() {
                Ok(InteractiveResponse::DialogAction { event, resp_tx }) => {
                    let resp = action_handler(event);
                    let _ = resp_tx.send(resp);
                }
                Ok(InteractiveResponse::Dialog(resp)) => break resp,
                _ => break ocs_plugin_api::dialog::DialogResponse::Cancelled,
            }
        };
        drop(rx);
        self.restore_focus_to_ide_after_input(debug_active);
        res
    }

    pub fn show_dialog(&self, dialog: ocs_plugin_api::dialog::DialogDef) -> ocs_plugin_api::dialog::DialogResponse {
        self.show_dialog_interactive(dialog, |_| ocs_plugin_api::dialog::DialogActionResponse::None)
    }

    pub fn get_text(&self, prompt: &str, free_text: bool) -> Option<String> {
        if !prompt.is_empty() {
            self.coordinator.set_prompt(prompt.to_string());
        }
        self.coordinator.set_mode(InteractionMode::Text { free_text });
        self.check_advance_step();

        dbg_trace(&format!("get_text: prompt={:?}, free_text={}, waiting on rx.recv()...", prompt, free_text));
        let debug_active = self.switch_focus_to_cad_for_input();
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let res = match rx.recv() {
            Ok(InteractiveResponse::Text(text)) => Some(text),
            Ok(InteractiveResponse::Enter) => Some(String::new()),
            _ => None,
        };
        drop(rx);
        self.restore_focus_to_ide_after_input(debug_active);
        res
    }

    pub fn finish(&self) {
        self.coordinator.notify_finished();
    }

    pub fn fail(&self, err: &str) {
        self.coordinator.notify_error(err.to_string());
    }

    pub fn add_pending_panel_update(&self, panel_id: String, values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue>) {
        self.coordinator.add_panel_update(panel_id, values);
    }

    pub fn queue_command(&self, cmd: String) {
        self.coordinator.queue_command(cmd);
    }

    pub fn set_single_selection(&self, single: bool) {
        self.coordinator.set_single_selection(single);
    }

    pub fn set_reject_locked(&self, reject: bool) {
        self.coordinator.set_reject_locked(reject);
    }

    pub fn set_point_only(&self, point_only: bool) {
        self.coordinator.set_point_only(point_only);
    }

    pub fn set_mode_flags(&self, reject_locked: bool, single_select: bool, point_only: bool) {
        self.coordinator.set_mode_flags(reject_locked, single_select, point_only);
    }
}

static CURRENT_SELECTION: Mutex<Vec<u64>> = Mutex::new(Vec::new());

pub fn update_selection(handles: Vec<u64>) {
    if let Ok(mut sel) = CURRENT_SELECTION.lock() {
        *sel = handles;
    }
}

pub fn get_cached_selection() -> Vec<u64> {
    CURRENT_SELECTION.lock().map(|s| s.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_for_step_does_not_timeout_while_debug_paused() {
        let (bridge, _cmd_box) = LispInteractiveBridge::new(
            "Pick point: ".to_string(),
            InteractionMode::Point,
            100,
            None,
        );
        // Mark first prompt started
        bridge.check_advance_step();

        let pause_guard = crate::rust_lisp::DebugPauseGuard::enter();
        let bridge_worker = Arc::clone(&bridge);
        let handle = std::thread::spawn(move || {
            // Sleep 3.4s (> 3.0s normal timeout) while paused in debugger
            std::thread::sleep(Duration::from_millis(3400));
            drop(pause_guard);
            bridge_worker.coordinator.notify_waiting_for_input();
        });

        let step = bridge.coordinator.wait_for_step();
        handle.join().unwrap();
        assert!(
            matches!(step, CommandStep::NeedPoint),
            "wait_for_step should not time out while paused in debugger, got {:?}",
            match step {
                CommandStep::NeedPoint => "NeedPoint",
                CommandStep::Done => "Done",
                CommandStep::Cancel => "Cancel",
                _ => "Other",
            }
        );
    }
}
