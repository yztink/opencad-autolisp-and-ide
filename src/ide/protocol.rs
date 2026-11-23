use serde::{Deserialize, Serialize};

fn default_one() -> usize {
    1
}

/// A breakpoint placed on an opening parenthesis `(` of an S-expression (1-based line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParenBreakpoint {
    pub line: usize,
    pub col: usize,
}

impl ParenBreakpoint {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// Commands sent from the IDE to the running debugger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugCommand {
    Continue,
    StepOver,
    StepInto,
    StepOut,
    Stop,
    SetBreakpoints(Vec<ParenBreakpoint>),
}

/// Events sent from the debugger worker to the IDE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugEvent {
    Paused {
        filename: String,
        line: usize,
        #[serde(default = "default_one")]
        col: usize,
        #[serde(default)]
        end_line: usize,
        #[serde(default)]
        end_col: usize,
        #[serde(default)]
        expr_preview: String,
        locals: Vec<(String, String, String)>, // (name, type_name, repr_val)
        stack: Vec<(String, usize, String)>,   // (func_name, lineno, filename)
    },
    Finished,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReplMode {
    Python,
    #[default]
    Lisp,
}

/// Messages sent from the standalone OCIDE application to the CAD plugin runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdeToPlugin {
    Ping,
    RunScript {
        title: String,
        code: String,
    },
    DebugScript {
        title: String,
        code: String,
        breakpoints: Vec<ParenBreakpoint>,
        step_into: bool,
    },
    DebugCmd(DebugCommand),
    ReplEval {
        code: String,
        #[serde(default)]
        mode: ReplMode,
    },
    SyncBreakpoints {
        file: String,
        breakpoints: Vec<ParenBreakpoint>,
    },
    RestartScript {
        title: String,
        code: String,
        breakpoints: Vec<ParenBreakpoint>,
    },
}

/// Messages sent from the CAD plugin runner to the standalone OCIDE application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginToIde {
    Pong,
    FocusWindow,
    FocusCad,
    ConsoleOutput {
        text: String,
        is_error: bool,
    },
    DebugEvt(DebugEvent),
    SelectionChanged {
        handles: Vec<u64>,
    },
    HostClosing,
}

/// Returns the path to the file storing the active IDE IPC port.
pub fn port_file_path() -> std::path::PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        std::path::PathBuf::from(appdata).join("OpenCADStudio").join("ocide_port.txt")
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(d).join("OpenCADStudio").join("ocide_port.txt")
    } else if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home).join(".config").join("OpenCADStudio").join("ocide_port.txt")
    } else {
        std::path::PathBuf::from("ocide_port.txt")
    }
}
