pub mod protocol;
pub mod server;
#[cfg(feature = "ide-gui")]
pub mod client;
#[cfg(feature = "ide-gui")]
pub mod view;
#[cfg(feature = "ide-gui")]
pub mod app;
#[cfg(feature = "ide-gui")]
pub mod paren;
#[cfg(feature = "ide-gui")]
pub mod highlight;
#[cfg(feature = "ide-gui")]
pub mod api_docs;

pub use protocol::ReplMode;

#[cfg(feature = "ide-gui")]
mod gui_state {
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use iced::widget::text_editor;
use iced::{Color, Point, Size};
use serde::{Deserialize, Serialize};
use crate::ide::highlight::{color_serde, SyntaxColors, UserTheme};
use crate::ide::protocol::{self, PluginToIde};

pub mod custom_funcs_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        List(Vec<String>),
        Single(String),
    }

    pub fn serialize<S>(list: &Vec<String>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        list.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Helper::deserialize(deserializer)? {
            Helper::List(v) => Ok(v),
            Helper::Single(s) => {
                let list = s
                    .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect();
                Ok(list)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    EditorSyntax,
    Formatting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyntaxColorTarget {
    #[default]
    Background,
    DefaultText,
    Comment,
    StringLiteral,
    Keyword,
    BuiltinFunction,
    CustomFunction,
    Number,
    Paren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxPreset {
    Base16Mocha,
    ClassicLight,
    Monokai,
    SolarizedDark,
}

pub fn parse_custom_functions(raw: &str) -> Arc<BTreeSet<String>> {
    let set: BTreeSet<String> = raw
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    Arc::new(set)
}

pub fn default_font_size() -> u8 {
    13
}

pub fn default_true() -> bool {
    true
}

/// User-customizable settings for the OpenCAD LISP IDE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdeSettings {
    #[serde(with = "color_serde")]
    pub breakpoint_color: Color,
    #[serde(with = "color_serde")]
    pub execution_color: Color,
    pub transparency_percent: u8,
    pub auto_transparency: bool,
    pub scripts_dir: Option<PathBuf>,
    pub font_name: Option<String>,
    #[serde(default = "default_font_size")]
    pub font_size: u8,
    pub syntax_colors: SyntaxColors,
    #[serde(default, with = "custom_funcs_serde")]
    pub custom_functions: Vec<String>,
    #[serde(skip)]
    pub custom_functions_set: Arc<BTreeSet<String>>,
    pub custom_function_use_native_color: bool,
    #[serde(default = "default_true")]
    pub show_function_tooltips: bool,
    #[serde(default, with = "custom_funcs_serde")]
    pub ignored_tooltip_functions: Vec<String>,
    #[serde(skip)]
    pub ignored_tooltip_functions_set: Arc<BTreeSet<String>>,
    #[serde(default)]
    pub user_themes: Vec<UserTheme>,
    #[serde(default)]
    pub current_theme: Option<String>,
    #[serde(default)]
    pub format_options: crate::ide::paren::LispFormatOptions,
    #[serde(default)]
    pub language: Option<String>,
}

pub fn default_scripts_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("OpenCADStudio").join("scripts")
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d).join("OpenCADStudio").join("scripts")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("OpenCADStudio").join("scripts")
    } else {
        PathBuf::from("scripts")
    }
}

impl IdeSettings {
    pub fn resolved_scripts_dir(&self) -> PathBuf {
        self.scripts_dir
            .as_ref()
            .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p.clone()) })
            .unwrap_or_else(default_scripts_dir)
    }

    pub fn editor_font(&self) -> iced::Font {
        crate::ide::highlight::resolve_monospace_font(self.font_name.as_deref())
    }

    pub fn font_size(&self) -> u8 {
        if self.font_size == 0 {
            13
        } else {
            self.font_size
        }
    }

    pub fn editor_char_width(&self) -> f32 {
        let ratio = crate::ide::highlight::monospace_advance_ratio(self.font_name.as_deref());
        (self.font_size() as f32 * ratio).max(4.0)
    }

    pub fn detect_theme_name(&self) -> String {
        if let Some(ref name) = self.current_theme {
            if !name.trim().is_empty() {
                return name.clone();
            }
        }
        if self.syntax_colors == SyntaxColors::base16_mocha() {
            "Base16 Mocha (Default)".to_string()
        } else if self.syntax_colors == SyntaxColors::classic_light() {
            "Classic Light".to_string()
        } else if self.syntax_colors == SyntaxColors::monokai() {
            "Monokai".to_string()
        } else if self.syntax_colors == SyntaxColors::solarized_dark() {
            "Solarized Dark".to_string()
        } else if let Some(ut) = self.user_themes.iter().find(|t| t.colors == self.syntax_colors) {
            ut.name.clone()
        } else {
            "Custom".to_string()
        }
    }

    pub fn get_target_color(&self, target: SyntaxColorTarget) -> Color {
        match target {
            SyntaxColorTarget::Background => self.syntax_colors.background,
            SyntaxColorTarget::DefaultText => self.syntax_colors.default_text,
            SyntaxColorTarget::Comment => self.syntax_colors.comment,
            SyntaxColorTarget::StringLiteral => self.syntax_colors.string,
            SyntaxColorTarget::Keyword => self.syntax_colors.keyword,
            SyntaxColorTarget::BuiltinFunction => self.syntax_colors.builtin_function,
            SyntaxColorTarget::CustomFunction => self.syntax_colors.effective_custom_func_color(),
            SyntaxColorTarget::Number => self.syntax_colors.number,
            SyntaxColorTarget::Paren => self.syntax_colors.paren,
        }
    }

    pub fn set_target_color(&mut self, target: SyntaxColorTarget, color: Color) {
        match target {
            SyntaxColorTarget::Background => self.syntax_colors.background = color,
            SyntaxColorTarget::DefaultText => self.syntax_colors.default_text = color,
            SyntaxColorTarget::Comment => self.syntax_colors.comment = color,
            SyntaxColorTarget::StringLiteral => self.syntax_colors.string = color,
            SyntaxColorTarget::Keyword => self.syntax_colors.keyword = color,
            SyntaxColorTarget::BuiltinFunction => self.syntax_colors.builtin_function = color,
            SyntaxColorTarget::CustomFunction => {
                self.custom_function_use_native_color = false;
                self.syntax_colors.custom_function = Some(color);
            }
            SyntaxColorTarget::Number => self.syntax_colors.number = color,
            SyntaxColorTarget::Paren => self.syntax_colors.paren = color,
        }
    }

    pub fn rebuild_custom_functions_set(&mut self) {
        let set: BTreeSet<String> = self
            .custom_functions
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        self.custom_functions_set = Arc::new(set);
    }

    pub fn is_custom_function(&self, name: &str) -> bool {
        self.custom_functions_set.contains(&name.to_ascii_lowercase())
    }

    /// Check whether an identifier is an exact match for a recognized valid OpenCAD LISP function,
    /// special form, or custom user function.
    pub fn is_valid_function(&self, name: &str) -> bool {
        let trimmed = name.trim().trim_start_matches('\'');
        if trimmed.is_empty() {
            return false;
        }
        // 1. Built-in API documentation table (case-insensitive)
        if crate::ide::api_docs::API_DOCS.iter().any(|d| d.name.eq_ignore_ascii_case(trimmed)) {
            return true;
        }
        // 2. Syntax highlighter builtins (exact match, excluding generic prefix patterns)
        let lower = trimmed.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "+" | "-" | "*" | "/" | "1+" | "1-" | "abs" | "max" | "min" | "rem" | "gcd" | "sqrt"
                | "expt" | "sin" | "cos" | "atan" | "log" | "exp" | "=" | "/=" | "<" | "<=" | ">"
                | ">=" | "eq" | "equal" | "null" | "atom" | "listp" | "numberp" | "minusp" | "zerop"
                | "car" | "cdr" | "caar" | "cadr" | "cdar" | "cddr" | "caaar" | "caadr" | "cadar"
                | "caddr" | "cdaar" | "cdadr" | "cddar" | "cdddr" | "cons" | "list" | "append"
                | "reverse" | "length" | "last" | "nth" | "assoc" | "subst" | "member" | "mapcar"
                | "apply" | "strcat" | "strlen" | "substr" | "strcase" | "ascii" | "chr" | "read"
                | "rtos" | "angtos" | "distof" | "angtof" | "atof" | "atoi" | "itoa" | "wcmatch"
                | "princ" | "prin1" | "print" | "prompt" | "terpri" | "open" | "close" | "read-line"
                | "write-line" | "read-char" | "write-char" | "load" | "findfile" | "getenv" | "setenv"
                | "command" | "command-s" | "vl-cmdf" | "getvar" | "setvar" | "alert" | "add-line"
                | "add-circle" | "add-arc" | "add-point" | "add-text" | "entget" | "entmod" | "entmake"
                | "entdel" | "entnext" | "entlast" | "entsel" | "nentsel" | "nentselp" | "ssget"
                | "ssgetfirst" | "sssetfirst" | "sslength" | "ssname" | "ssadd" | "ssdel" | "ssmemb"
                | "getpoint" | "getdist" | "getangle" | "getorient" | "getcorner" | "getstring"
                | "getint" | "getreal" | "getkword" | "initget" | "polar" | "inters" | "distance"
                | "angle" | "trans" | "boole" | "logand" | "logior" | "lsh" | "boundp" | "type"
                | "cvunit" | "acad_colordlg" | "acad_strlsort" | "menucmd" | "graphscr" | "textscr"
                | "redraw" | "vports" | "ver" | "vl-version"
        ) {
            return true;
        }
        // 3. OpenCAD LISP special forms / keyword functions
        if matches!(
            lower.as_str(),
            "defun"
                | "defun-q"
                | "setq"
                | "set"
                | "lambda"
                | "if"
                | "cond"
                | "while"
                | "repeat"
                | "foreach"
                | "progn"
                | "and"
                | "or"
                | "not"
                | "quote"
                | "function"
                | "eval"
        ) {
            return true;
        }
        // 4. User-defined custom functions from settings
        self.is_custom_function(trimmed)
    }

    pub fn add_custom_function(&mut self, name: String) -> bool {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return false;
        }
        if !self.custom_functions.iter().any(|f| f.eq_ignore_ascii_case(&trimmed)) {
            self.custom_functions.push(trimmed);
            self.rebuild_custom_functions_set();
            true
        } else {
            false
        }
    }

    pub fn remove_custom_function(&mut self, name: &str) -> bool {
        let before = self.custom_functions.len();
        self.custom_functions.retain(|f| !f.eq_ignore_ascii_case(name));
        if self.custom_functions.len() != before {
            self.rebuild_custom_functions_set();
            true
        } else {
            false
        }
    }

    pub fn rebuild_ignored_tooltip_functions_set(&mut self) {
        let set: BTreeSet<String> = self
            .ignored_tooltip_functions
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        self.ignored_tooltip_functions_set = Arc::new(set);
    }

    pub fn is_tooltip_ignored(&self, name: &str) -> bool {
        self.ignored_tooltip_functions_set.contains(&name.to_ascii_lowercase())
    }

    pub fn add_ignored_tooltip_function(&mut self, name: String) -> bool {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return false;
        }
        if !self.ignored_tooltip_functions.iter().any(|f| f.eq_ignore_ascii_case(&trimmed)) {
            self.ignored_tooltip_functions.push(trimmed);
            self.rebuild_ignored_tooltip_functions_set();
            true
        } else {
            false
        }
    }

    pub fn remove_ignored_tooltip_function(&mut self, name: &str) -> bool {
        let before = self.ignored_tooltip_functions.len();
        self.ignored_tooltip_functions.retain(|f| !f.eq_ignore_ascii_case(name));
        if self.ignored_tooltip_functions.len() != before {
            self.rebuild_ignored_tooltip_functions_set();
            true
        } else {
            false
        }
    }

    pub fn save_user_theme(&mut self, name: String, colors: SyntaxColors) -> String {
        let trimmed = name.trim();
        let theme_name = if trimmed.is_empty() {
            format!("Custom Theme {}", self.user_themes.len() + 1)
        } else {
            trimmed.to_string()
        };
        if let Some(existing) = self.user_themes.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&theme_name)) {
            existing.colors = colors;
        } else {
            self.user_themes.push(UserTheme {
                name: theme_name.clone(),
                colors,
            });
        }
        theme_name
    }

    pub fn delete_user_theme(&mut self, name: &str) -> bool {
        let before = self.user_themes.len();
        self.user_themes.retain(|t| !t.name.eq_ignore_ascii_case(name));
        self.user_themes.len() != before
    }

    pub fn update_custom_functions(&mut self, text: String) {
        self.custom_functions = text
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.rebuild_custom_functions_set();
    }

    pub fn set_custom_function_color_mode(&mut self, use_native: bool) {
        self.custom_function_use_native_color = use_native;
        if use_native {
            self.syntax_colors.custom_function = None;
        } else if self.syntax_colors.custom_function.is_none() {
            self.syntax_colors.custom_function = Some(Color::from_rgb8(0x82, 0xAA, 0xFF));
        }
    }

    pub fn settings_file_path() -> PathBuf {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            PathBuf::from(appdata).join("OpenCADStudio").join("ocide_settings.json")
        } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(d).join("OpenCADStudio").join("ocide_settings.json")
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".config").join("OpenCADStudio").join("ocide_settings.json")
        } else {
            PathBuf::from("ocide_settings.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::settings_file_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut settings) = serde_json::from_str::<IdeSettings>(&content) {
                settings.rebuild_custom_functions_set();
                settings.rebuild_ignored_tooltip_functions_set();
                if settings.custom_function_use_native_color {
                    settings.syntax_colors.custom_function = None;
                } else if settings.syntax_colors.custom_function.is_none() {
                    settings.syntax_colors.custom_function = Some(Color::from_rgb8(0x82, 0xAA, 0xFF));
                }
                return settings;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::settings_file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

impl Default for IdeSettings {
    fn default() -> Self {
        let custom_funcs = vec!["my-func".to_string(), "draw-plate".to_string(), "c:test".to_string()];
        let custom_set: BTreeSet<String> = custom_funcs.iter().map(|s| s.to_ascii_lowercase()).collect();
        Self {
            breakpoint_color: Color::from_rgba(0.55, 0.12, 0.12, 0.45),
            execution_color: Color::from_rgba(0.12, 0.55, 0.20, 0.50),
            transparency_percent: 50,
            auto_transparency: false,
            scripts_dir: None,
            font_name: None,
            font_size: 13,
            syntax_colors: SyntaxColors::base16_mocha(),
            custom_functions: custom_funcs,
            custom_functions_set: Arc::new(custom_set),
            custom_function_use_native_color: true,
            show_function_tooltips: true,
            ignored_tooltip_functions: Vec::new(),
            ignored_tooltip_functions_set: Arc::new(BTreeSet::new()),
            user_themes: Vec::new(),
            current_theme: None,
            format_options: crate::ide::paren::LispFormatOptions::default(),
            language: None,
        }
    }
}

/// A snapshot of an editor tab's text and cursor state for temporary undo/redo.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSnapshot {
    pub text: String,
    pub cursor: text_editor::Cursor,
}

/// A matched search occurrence in an editor buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

/// State for the Find & Replace toolbar and search engine.
#[derive(Debug, Clone, Default)]
pub struct FindReplaceState {
    pub is_open: bool,
    pub show_replace: bool,
    pub find_query: String,
    pub replace_query: String,
    pub match_case: bool,
    pub match_whole_word: bool,
    pub matches: Vec<SearchMatch>,
    pub current_match_idx: Option<usize>,
}

/// Computes all search match spans for a query within editor buffer text.
pub fn compute_search_matches(
    text: &str,
    query: &str,
    match_case: bool,
    match_whole_word: bool,
) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let query_len = query.len();
    let query_lower = if !match_case {
        query.to_ascii_lowercase()
    } else {
        String::new()
    };

    let is_word_char = |c: char| crate::ide::api_docs::is_lisp_ident_char(c);

    for (line_idx, line) in text.split('\n').enumerate() {
        let line_trimmed = line.strip_suffix('\r').unwrap_or(line);
        if line_trimmed.is_empty() {
            continue;
        }

        let line_lower = if !match_case {
            line_trimmed.to_ascii_lowercase()
        } else {
            String::new()
        };

        let search_haystack = if match_case { line_trimmed } else { &line_lower };
        let search_needle = if match_case { query } else { &query_lower };

        let mut start = 0;
        while let Some(pos) = search_haystack[start..].find(search_needle) {
            let match_col = start + pos;
            let end_col = match_col + query_len;

            let word_ok = if match_whole_word {
                let char_before = line_trimmed[..match_col].chars().next_back();
                let char_after = line_trimmed[end_col..].chars().next();
                let prev_ok = char_before.map_or(true, |c| !is_word_char(c));
                let next_ok = char_after.map_or(true, |c| !is_word_char(c));
                prev_ok && next_ok
            } else {
                true
            };

            if word_ok {
                matches.push(SearchMatch {
                    line: line_idx,
                    col: match_col,
                    len: query_len,
                });
            }

            start = match_col + 1.max(query_len);
            if start >= line_trimmed.len() {
                break;
            }
        }
    }

    matches
}

/// In-memory temporary undo and redo stacks for an editor tab.
#[derive(Debug, Clone, Default)]
pub struct UndoHistory {
    pub undo_stack: Vec<TextSnapshot>,
    pub redo_stack: Vec<TextSnapshot>,
    pub last_edit_instant: Option<std::time::Instant>,
}

/// Represents one open file buffer in the IDE.
#[derive(Debug, Clone)]
pub struct IdeTab {
    pub path: Option<PathBuf>,
    pub title: String,
    pub dirty: bool,
    pub content: text_editor::Content,
    pub breakpoints: BTreeSet<protocol::ParenBreakpoint>,
    pub debug_paused_line: Option<usize>,
    pub debug_paused_span: Option<(usize, usize, usize, usize)>,
    pub debug_paused_expr: Option<String>,
    pub scroll_top_line: usize,
    pub last_edit_cursor: Option<text_editor::Cursor>,
    pub is_scrolling_away: bool,
    pub undo_history: UndoHistory,
}

impl IdeTab {
    pub const MAX_UNDO_DEPTH: usize = 200;

    pub fn new_empty(title: String) -> Self {
        let default_code = ";; OpenCAD LISP script\n";
        Self {
            path: None,
            title,
            dirty: false,
            content: text_editor::Content::with_text(default_code),
            breakpoints: BTreeSet::new(),
            debug_paused_line: None,
            debug_paused_span: None,
            debug_paused_expr: None,
            scroll_top_line: 0,
            last_edit_cursor: None,
            is_scrolling_away: false,
            undo_history: UndoHistory::default(),
        }
    }

    pub fn from_file(path: PathBuf, content: &str) -> Self {
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled.lsp")
            .to_string();
        Self {
            path: Some(path),
            title,
            dirty: false,
            content: text_editor::Content::with_text(content),
            breakpoints: BTreeSet::new(),
            debug_paused_line: None,
            debug_paused_span: None,
            debug_paused_expr: None,
            scroll_top_line: 0,
            last_edit_cursor: None,
            is_scrolling_away: false,
            undo_history: UndoHistory::default(),
        }
    }

    pub fn has_breakpoint_on_line(&self, line_1based: usize) -> bool {
        self.breakpoints.iter().any(|bp| bp.line == line_1based)
    }

    pub fn breakpoints_on_line(&self, line_1based: usize) -> Vec<protocol::ParenBreakpoint> {
        self.breakpoints
            .iter()
            .copied()
            .filter(|bp| bp.line == line_1based)
            .collect()
    }

    pub fn snapshot(&self) -> TextSnapshot {
        TextSnapshot {
            text: self.content.text(),
            cursor: self.content.cursor(),
        }
    }

    pub fn push_undo_snapshot(&mut self) {
        let snap = self.snapshot();
        if self.undo_history.undo_stack.last() != Some(&snap) {
            self.undo_history.undo_stack.push(snap);
            if self.undo_history.undo_stack.len() > Self::MAX_UNDO_DEPTH {
                self.undo_history.undo_stack.remove(0);
            }
        }
        self.undo_history.redo_stack.clear();
        self.undo_history.last_edit_instant = Some(std::time::Instant::now());
    }

    pub fn push_undo_snapshot_if_burst(&mut self, is_boundary: bool) {
        let now = std::time::Instant::now();
        let is_new_burst = is_boundary || self.undo_history.last_edit_instant.map_or(true, |last| {
            now.duration_since(last).as_millis() > 600
        });

        if is_new_burst {
            self.push_undo_snapshot();
        } else {
            self.undo_history.last_edit_instant = Some(now);
            self.undo_history.redo_stack.clear();
        }
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_history.undo_stack.pop() {
            let current = self.snapshot();
            self.undo_history.redo_stack.push(current);
            let mut new_content = text_editor::Content::with_text(&prev.text);
            new_content.move_to(prev.cursor);
            self.content = new_content;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.undo_history.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_history.undo_stack.push(current);
            let mut new_content = text_editor::Content::with_text(&next.text);
            new_content.move_to(next.cursor);
            self.content = new_content;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn is_lisp(&self) -> bool {
        self.path.as_ref().map_or_else(
            || self.title.ends_with(".lsp") || self.title.ends_with(".lisp"),
            |p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp") || e.eq_ignore_ascii_case("lisp")),
        )
    }

    /// Deterministically scroll to `target_top_line` and position the cursor without
    /// sub-pixel drift or desynchronization between iced's text buffer and the gutter.
    ///
    /// Uses two-phase scroll anchoring:
    /// 1. Force cosmic-text's internal scroll.line >= target_top via DocumentEnd if needed.
    /// 2. Move cursor to target_top (col 0).
    /// 3. Trigger shape_until_cursor via Action::Scroll { lines: 0 }. Since scroll.line >= target_top == cursor,
    ///    cosmic-text executes: scroll.line = target_top; scroll.vertical = 0.0;
    /// 4. Position cursor at clamped_line within [target_top, target_top + visible_lines)
    ///    so cosmic-text during draw will neither clamp backwards nor clamp forwards.
    /// Low-level exact viewport scroll helper.
    /// Positions `scroll.line` at `target_top_line` (clamped to `max_scroll`) using
    /// top-down scrolling so it remains deterministic even before first layout
    /// (`height_opt == None` after `Content::with_text` or tab switch), then places
    /// the editor cursor/selection and updates `scroll_top_line`.
    pub fn scroll_viewport_exact(
        &mut self,
        target_top_line: usize,
        cursor: text_editor::Cursor,
        visible_lines: usize,
        update_last_edit_cursor: bool,
    ) {
        let total = self.content.line_count().max(1);
        let max_scroll = total.saturating_sub(visible_lines.max(1));
        let target_top = target_top_line.min(max_scroll);

        // 1. Reset cosmic-text scroll deterministically to line 0 regardless of height_opt
        self.content.perform(text_editor::Action::Move(text_editor::Motion::DocumentStart));
        self.content.perform(text_editor::Action::Scroll { lines: -100_000 });

        // 2. Advance scroll.line to target_top without relying on DocumentEnd (which is a no-op when height_opt is None)
        if target_top > 0 {
            self.content.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: target_top,
                    column: 0,
                },
                selection: None,
            });
            self.content.perform(text_editor::Action::Scroll { lines: target_top as i32 });
        }

        self.scroll_top_line = target_top;

        // 3. Position cursor (and optional selection) comfortably within [target_top, target_top + visible_lines)
        let clamped_line = cursor.position.line.clamp(
            target_top,
            (target_top + visible_lines.saturating_sub(1)).min(total.saturating_sub(1)),
        );
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: clamped_line,
                column: cursor.position.column,
            },
            selection: cursor.selection,
        });
        if update_last_edit_cursor {
            self.last_edit_cursor = Some(self.content.cursor());
        }
    }

    pub fn scroll_to_line_exact(
        &mut self,
        target_top_line: usize,
        cursor_line: usize,
        cursor_col: usize,
        visible_lines: usize,
    ) {
        self.scroll_viewport_exact(
            target_top_line,
            text_editor::Cursor {
                position: text_editor::Position {
                    line: cursor_line,
                    column: cursor_col,
                },
                selection: None,
            },
            visible_lines,
            true,
        );
        self.is_scrolling_away = false;
    }

    /// Positions viewport scroll at target_top_line, places cursor at (cursor_line, cursor_col),
    /// and selects text back to selection_start_col on cursor_line.
    pub fn scroll_to_selection_exact(
        &mut self,
        target_top_line: usize,
        cursor_line: usize,
        cursor_col: usize,
        selection_start_col: usize,
        visible_lines: usize,
    ) {
        let total = self.content.line_count().max(1);
        let max_scroll = total.saturating_sub(visible_lines.max(1));
        let target_top = target_top_line.min(max_scroll);
        let clamped_line = cursor_line.clamp(
            target_top,
            (target_top + visible_lines.saturating_sub(1)).min(total.saturating_sub(1)),
        );
        self.scroll_viewport_exact(
            target_top,
            text_editor::Cursor {
                position: text_editor::Position {
                    line: clamped_line,
                    column: cursor_col,
                },
                selection: Some(text_editor::Position {
                    line: clamped_line,
                    column: selection_start_col,
                }),
            },
            visible_lines,
            true,
        );
        self.is_scrolling_away = false;
    }
}

/// What action triggered the modal save confirmation prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavePromptTrigger {
    CloseWindow,
    CloseTab(usize),
    HostClosing,
}

/// Active state for the modal "Do you want to save changes?" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePromptState {
    pub trigger: SavePromptTrigger,
    pub dirty_files: Vec<(usize, String)>,
}

/// State of the active editor context menu overlay.
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub position: Point,
    pub target_func: Option<String>,
    pub target_ident: Option<String>,
    pub target_bp: Option<(protocol::ParenBreakpoint, String)>,
    pub cursor_line: usize,
    pub has_selection: bool,
}

/// All interactive messages handled by OCIDE.
#[derive(Debug, Clone)]
pub enum IdeMessage {
    AutocompleteNext,
    AutocompletePrev,
    AutocompleteAccept,
    AutocompleteSelect(String),
    AutocompleteDismiss,
    EditorRightClicked(Point),
    CloseContextMenu,
    MouseMoved(Point),
    NewTab,
    SelectTab(usize),
    CloseActiveTab,
    CloseTab(usize),
    OpenFile,
    FileLoaded(Option<(PathBuf, String)>),
    SaveTab,
    SaveTabAs,
    FileSavedAs(Option<(usize, PathBuf)>),
    EditorAction(text_editor::Action),
    RestoreCursorAndAct(text_editor::Action),
    ScrollToLine(usize),
    ConsoleAction(text_editor::Action),
    ReplAction(text_editor::Action),
    ReplSubmit,
    ReplInsertNewline,
    ReplClear,
    ReplHistoryPrev,
    ReplHistoryNext,
    ToggleReplMode,
    SetReplMode(protocol::ReplMode),
    ClearConsole,
    RunAll,
    RunSelection,
    ToggleBreakpoint(usize),
    ToggleBreakpointAtCursor,
    ToggleParenBreakpoint(protocol::ParenBreakpoint),
    AddWatch(String),
    AddWatchAtCursor,
    RemoveWatch(usize),
    ClearWatch,
    EditorCopy,
    EditorPasteRequest,
    EditorPaste(Option<String>),
    ToggleApiDrawer,
    ApiSearch(String),
    ShowInApiDrawer(String),
    InsertSnippet(String),
    ToggleVariables,
    ToggleSettings,
    ToggleHelpMenu,
    OpenTutorials,
    SetSettingsTab(SettingsTab),
    SetLanguage(Option<String>),
    SetEditorFont(Option<String>),
    SetEditorFontSize(u8),
    SetFontSizeInput(String),
    ApplyFontSizeInput,
    SelectSyntaxColorTarget(SyntaxColorTarget),
    SetSyntaxColor(SyntaxColorTarget, Color),
    SetSyntaxHexInput(String),
    ApplySyntaxHexInput,
    SetSyntaxRgbChannel(u8, u8),
    SetCustomFunctionColorMode(bool),
    ApplySyntaxPreset(SyntaxPreset),
    SelectTheme(String),
    SetNewThemeName(String),
    SaveCurrentTheme,
    DeleteUserTheme(String),
    SelectCustomFunction(Option<String>),
    SetNewCustomFunctionInput(String),
    AddCustomFunction,
    RemoveCustomFunction,
    ToggleShowTooltips(bool),
    AddIgnoredTooltipFunction(String),
    RemoveIgnoredTooltipFunction(String),
    SelectIgnoredFunction(Option<String>),
    SetNewIgnoredFunctionInput(String),
    AddIgnoredFunction,
    RemoveIgnoredFunction,
    ClearIgnoredFunctions,
    ResetEditorSyntaxDefaults,
    SetCustomFunctions(String),
    SetBreakpointColor(Color),
    SetExecutionColor(Color),
    SetTransparencyPercent(u8),
    SetTransparencyInput(String),
    ApplyTransparencyInput,
    SetAutoTransparency(bool),
    BrowseScriptsDir,
    SetScriptsDir(Option<PathBuf>),
    ToggleTransparency,
    FocusCad,
    SplitterGrab,
    SplitterMove(Point),
    SplitterRelease,
    SplitterReset,
    DebugStart,
    DebugStartStep,
    DebugContinue,
    DebugStepOver,
    DebugStepInto,
    DebugStepOut,
    DebugRestart,
    DebugStop,
    F5Pressed,
    SmartEnter,
    SmartTab,
    FormatDocument,
    FormatSelection,
    SetFormatIndentSize(usize),
    ToggleFormatDowncaseBuiltins(bool),
    ToggleFormatWideStyle(bool),
    ToggleFormatAlignSetq(bool),
    ToggleFormatStructuralReflow(bool),
    SelectMatchingParen { forward: bool },
    JumpToMatchingParen { forward: bool },
    PluginEvent(PluginToIde),
    WheelScrolled(f32),
    WindowResized(Size),
    WindowCloseRequested,
    Undo,
    Redo,
    ConfirmSaveAndProceed,
    DiscardAndProceed,
    CancelSavePrompt,
    OpenFind,
    OpenReplace,
    CloseFind,
    FindQueryChanged(String),
    ReplaceQueryChanged(String),
    ToggleFindMatchCase,
    ToggleFindWholeWord,
    ToggleFindShowReplace,
    FindNext,
    FindPrev,
    ReplaceCurrent,
    ReplaceAll,
    Tick,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_undo_redo_stack() {
        let mut tab = IdeTab::new_empty("test.lsp".to_string());
        let initial_text = tab.content.text();

        // Snapshot 1 (initial)
        tab.push_undo_snapshot();
        tab.content = text_editor::Content::with_text("(defun c:hello ()\n  (princ \"hi\")\n)");

        // Snapshot 2 (after typing hello)
        tab.push_undo_snapshot();
        tab.content = text_editor::Content::with_text("(defun c:hello ()\n  (princ \"hello world!\")\n)");

        assert_eq!(tab.content.text(), "(defun c:hello ()\n  (princ \"hello world!\")\n)");

        // Undo 1 -> should revert to (princ "hi")
        assert!(tab.undo());
        assert_eq!(tab.content.text(), "(defun c:hello ()\n  (princ \"hi\")\n)");

        // Undo 2 -> should revert to initial
        assert!(tab.undo());
        assert_eq!(tab.content.text(), initial_text);

        // Undo 3 -> nothing left to undo
        assert!(!tab.undo());

        // Redo 1 -> should restore (princ "hi")
        assert!(tab.redo());
        assert_eq!(tab.content.text(), "(defun c:hello ()\n  (princ \"hi\")\n)");

        // Redo 2 -> should restore (princ "hello world!")
        assert!(tab.redo());
        assert_eq!(tab.content.text(), "(defun c:hello ()\n  (princ \"hello world!\")\n)");

        // Redo 3 -> nothing left to redo
        assert!(!tab.redo());
    }

    #[test]
    fn test_ide_settings_theme_detection() {
        let mut settings = IdeSettings::default();
        // Default starts as base16 mocha
        assert_eq!(settings.detect_theme_name(), "Base16 Mocha (Default)");

        // Explicit preset
        settings.syntax_colors = crate::ide::highlight::SyntaxColors::monokai();
        settings.current_theme = Some("Monokai".to_string());
        assert_eq!(settings.detect_theme_name(), "Monokai");

        // Custom modification
        settings.syntax_colors.background = iced::Color::from_rgb(0.9, 0.1, 0.1);
        settings.current_theme = Some("Custom".to_string());
        assert_eq!(settings.detect_theme_name(), "Custom");
    }

    #[test]
    fn test_save_prompt_triggers() {
        let prompt_close_win = SavePromptState {
            trigger: SavePromptTrigger::CloseWindow,
            dirty_files: vec![(0, "script.lsp".to_string()), (1, "drawing.lsp".to_string())],
        };
        assert_eq!(prompt_close_win.dirty_files.len(), 2);
        assert_eq!(prompt_close_win.trigger, SavePromptTrigger::CloseWindow);

        let prompt_close_tab = SavePromptState {
            trigger: SavePromptTrigger::CloseTab(2),
            dirty_files: vec![(2, "untitled_3.lsp".to_string())],
        };
        assert_eq!(prompt_close_tab.trigger, SavePromptTrigger::CloseTab(2));
    }

    #[test]
    fn test_compute_search_matches() {
        let code = "(defun c:test ()\n  (setq a 10)\n  (setq b (+ a 20))\n  (setq a_val (* a b))\n)";

        // 1. Case-insensitive search for "setq"
        let m1 = compute_search_matches(code, "setq", false, false);
        assert_eq!(m1.len(), 3);
        assert_eq!(m1[0], SearchMatch { line: 1, col: 3, len: 4 });
        assert_eq!(m1[1], SearchMatch { line: 2, col: 3, len: 4 });
        assert_eq!(m1[2], SearchMatch { line: 3, col: 3, len: 4 });

        // 2. Case-sensitive search for "SETQ" -> 0 matches because code has lowercase
        let m2 = compute_search_matches(code, "SETQ", true, false);
        assert_eq!(m2.len(), 0);

        // 3. Whole word search for "a"
        // line 1: "(setq a 10)" -> "a" is whole word (col 8)
        // line 2: "(setq b (+ a 20))" -> "a" is whole word (col 13)
        // line 3: "(setq a_val (* a b))" -> "a_val" should NOT match whole word "a", but "(* a b)" should (col 17)
        let m3 = compute_search_matches(code, "a", false, true);
        assert_eq!(m3.len(), 3);
        assert_eq!(m3[0], SearchMatch { line: 1, col: 8, len: 1 });
        assert_eq!(m3[1], SearchMatch { line: 2, col: 13, len: 1 });
        assert_eq!(m3[2], SearchMatch { line: 3, col: 17, len: 1 });

        // 4. Non-whole word search for "a" -> should include both 'a's in "a_val" as well
        let m4 = compute_search_matches(code, "a", false, false);
        assert_eq!(m4.len(), 5);
    }

    #[test]
    fn test_ide_settings_language_roundtrip_and_default() {
        let legacy_json = r#"{"transparency_percent": 40}"#;
        let parsed: IdeSettings = serde_json::from_str(legacy_json).expect("legacy settings deserialize");
        assert_eq!(parsed.language, None);

        let mut custom = IdeSettings::default();
        custom.language = Some("es-ES".to_string());
        let json = serde_json::to_string(&custom).expect("serialize settings");
        let roundtrip: IdeSettings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(roundtrip.language.as_deref(), Some("es-ES"));
    }
}
}

#[cfg(feature = "ide-gui")]
pub use gui_state::*;
