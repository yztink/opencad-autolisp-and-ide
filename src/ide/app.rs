use std::sync::mpsc::Receiver;
use iced::widget::text_editor;
use iced::{keyboard, Element, Point, Task, Theme};

use crate::ide::client::IdeClient;
use crate::ide::highlight::{color_to_hex, hex_to_color};
use crate::ide::protocol::{DebugCommand, DebugEvent, IdeToPlugin, PluginToIde};
use crate::ide::view::{identifier_at_cursor, view_standalone_window};
use crate::ide::{
    compute_search_matches, ContextMenuState, FindReplaceState, IdeMessage, IdeSettings, IdeTab, SettingsTab,
    SyntaxColorTarget, SyntaxPreset,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveEditorFocus {
    #[default]
    Main,
    Repl,
    Console,
}

pub struct IdeApp {
    pub tabs: Vec<IdeTab>,
    pub active_tab: usize,
    pub console_log: text_editor::Content,
    pub repl_content: text_editor::Content,
    pub repl_mode: crate::ide::protocol::ReplMode,
    pub repl_history: Vec<String>,
    pub repl_history_index: Option<usize>,
    pub repl_draft: String,
    pub active_editor_focus: ActiveEditorFocus,
    pub api_drawer_open: bool,
    pub api_search: String,
    pub theme: Theme,
    pub is_debugging: bool,
    pub variables_open: bool,
    pub debug_locals: Vec<(String, String, String)>,
    pub debug_stack: Vec<(String, usize, String)>,
    pub is_transparent: bool,
    pub watch_vars: Vec<String>,
    pub console_height: f32,
    pub is_splitter_dragging: bool,
    pub splitter_last_y: Option<f32>,
    pub window_width: f32,
    pub window_height: f32,
    pub context_menu: Option<ContextMenuState>,
    pub last_mouse_pos: Point,
    pub autocomplete_selected_index: usize,
    pub autocomplete_dismissed: bool,
    pub autocomplete_matches: Vec<String>,
    pub settings: IdeSettings,
    pub settings_open: bool,
    pub help_menu_open: bool,
    pub settings_tab: SettingsTab,
    pub active_color_target: SyntaxColorTarget,
    pub syntax_hex_input: String,
    pub font_size_input: String,
    pub transparency_input: String,
    pub selected_theme_name: String,
    pub new_theme_name: String,
    pub selected_custom_func: Option<String>,
    pub new_custom_func_input: String,
    pub selected_ignored_func: Option<String>,
    pub new_ignored_func_input: String,
    pub save_prompt: Option<crate::ide::SavePromptState>,
    pub find_replace: FindReplaceState,
    pub client: IdeClient,
    pub rx: Option<Receiver<PluginToIde>>,
}

/// Locate `tutorials/index.html` next to the running executable, in parent
/// directories (for `target/{debug,release}` runs), or in the crate root.
fn find_tutorials_index() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent();
        for _ in 0..4 {
            let Some(d) = dir else { break };
            let candidate = d.join("tutorials").join("index.html");
            if candidate.is_file() {
                return Some(candidate);
            }
            dir = d.parent();
        }
    }

    let manifest_candidate = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tutorials")
        .join("index.html");
    if manifest_candidate.is_file() {
        return Some(manifest_candidate);
    }

    None
}

/// Open a local HTML file in the user's default web browser.
fn open_in_browser(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
        return Ok(());
    }
}

impl IdeApp {
    pub fn refresh_editor_highlighting(&mut self) {
        for tab in &mut self.tabs {
            let cursor = tab.content.cursor();
            let text = tab.content.text();
            let mut new_content = text_editor::Content::with_text(&text);
            new_content.move_to(cursor);
            tab.content = new_content;
        }
    }

    pub fn refresh_find_matches(&mut self) {
        if !self.find_replace.find_query.is_empty() {
            if let Some(tab) = self.tabs.get(self.active_tab) {
                let text = tab.content.text();
                self.find_replace.matches = compute_search_matches(
                    &text,
                    &self.find_replace.find_query,
                    self.find_replace.match_case,
                    self.find_replace.match_whole_word,
                );
                if self.find_replace.matches.is_empty() {
                    self.find_replace.current_match_idx = None;
                } else if let Some(curr) = self.find_replace.current_match_idx {
                    if curr >= self.find_replace.matches.len() {
                        self.find_replace.current_match_idx = Some(self.find_replace.matches.len() - 1);
                    }
                }
            } else {
                self.find_replace.matches.clear();
                self.find_replace.current_match_idx = None;
            }
        } else {
            self.find_replace.matches.clear();
            self.find_replace.current_match_idx = None;
        }
    }

    pub fn navigate_to_search_match(&mut self, idx: usize) {
        if let Some(m) = self.find_replace.matches.get(idx).cloned() {
            self.find_replace.current_match_idx = Some(idx);
            let visible_lines = self.visible_lines();
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                let target_top = m.line.saturating_sub(visible_lines / 3);
                tab.scroll_to_selection_exact(target_top, m.line, m.col + m.len, m.col, visible_lines);
                tab.is_scrolling_away = false;
            }
        }
    }

    pub fn visible_lines(&self) -> usize {
        let line_height = (self.settings.font_size() as f32 * 1.6).round().max(16.0);
        const CHROME_HEIGHT: f32 = 136.0;
        let editor_height = (self.window_height - (CHROME_HEIGHT + self.console_height)).max(60.0);
        ((editor_height / line_height).floor() as usize).max(1)
    }

    pub fn append_to_console(&mut self, text: &str) {
        let prev = self.console_log.text();
        let mut new_content = text_editor::Content::with_text(&format!("{prev}{text}"));
        new_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        let line_count = new_content.line_count() as i32;
        new_content.perform(text_editor::Action::Scroll { lines: line_count });
        self.console_log = new_content;
    }

    pub fn set_console_text(&mut self, text: &str) {
        let mut new_content = text_editor::Content::with_text(text);
        new_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        let line_count = new_content.line_count() as i32;
        new_content.perform(text_editor::Action::Scroll { lines: line_count });
        self.console_log = new_content;
    }

    pub fn select_word_matching_paren(content: &mut text_editor::Content) -> bool {
        let cursor = content.cursor();
        let text = content.text();
        if let Some((start, end)) = crate::ide::paren::find_matching_paren_range(
            &text,
            cursor.position.line,
            cursor.position.column,
        ) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: end.0,
                    column: end.1,
                },
                selection: Some(text_editor::Position {
                    line: start.0,
                    column: start.1,
                }),
            });
            true
        } else {
            false
        }
    }

    pub fn select_enclosing_paren(content: &mut text_editor::Content) -> bool {
        let cursor = content.cursor();
        let text = content.text();
        if let Some((start, end)) = crate::ide::paren::find_enclosing_paren_range(
            &text,
            cursor.position.line,
            cursor.position.column,
        ) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: end.0,
                    column: end.1,
                },
                selection: Some(text_editor::Position {
                    line: start.0,
                    column: start.1,
                }),
            });
            true
        } else {
            false
        }
    }

    pub fn jump_target_paren(content: &mut text_editor::Content, forward: bool) -> bool {
        let cursor = content.cursor();
        let text = content.text();
        if let Some(target) = crate::ide::paren::find_target_paren(
            &text,
            cursor.position.line,
            cursor.position.column,
            forward,
        ) {
            content.move_to(text_editor::Cursor {
                position: text_editor::Position {
                    line: target.0,
                    column: target.1,
                },
                selection: None,
            });
            true
        } else {
            false
        }
    }

    pub fn resolve_context_menu_target(
        tab: &IdeTab,
        click_pos: Point,
        settings: &IdeSettings,
    ) -> (
        Option<String>,
        Option<String>,
        Option<(crate::ide::protocol::ParenBreakpoint, String)>,
        usize,
        bool,
    ) {
        let cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
        let has_selection = cursor.selection.is_some();
        let text = tab.content.text();
        let lines: Vec<&str> = text.lines().collect();

        let font_size = settings.font_size();
        let line_height = (font_size as f32 * 1.6).round().max(16.0);
        let char_w = settings.editor_char_width();
        const TOP_CHROME: f32 = 68.0;
        const PADDING_Y: f32 = 6.0;
        // 32.0 (gutter) + 4.0 (row spacing) + 8.0 (editor padding) = 44.0
        const GUTTER_WIDTH: f32 = 44.0;

        let (line_idx, col_idx) = if !has_selection && click_pos.y >= TOP_CHROME + PADDING_Y {
            let rel_y = click_pos.y - (TOP_CHROME + PADDING_Y);
            let rel_line = (rel_y / line_height).floor() as usize;
            let line_num = tab.scroll_top_line + rel_line;
            if line_num < lines.len() {
                let rel_x = (click_pos.x - GUTTER_WIDTH).max(0.0);
                let visual_col = (rel_x / char_w).floor() as usize;
                let cur_line = lines.get(line_num).copied().unwrap_or("");
                let byte_col = crate::ide::highlight::byte_col_for_visual_col(cur_line, visual_col);
                (line_num, byte_col)
            } else {
                (cursor.position.line, cursor.position.column)
            }
        } else {
            (cursor.position.line, cursor.position.column)
        };

        let cur_line = lines.get(line_idx).copied().unwrap_or("");
        let target_ident = if has_selection {
            identifier_at_cursor(&text, cursor.position.line, cursor.position.column)
        } else {
            identifier_at_cursor(&text, line_idx, col_idx)
                .or_else(|| identifier_at_cursor(&text, cursor.position.line, cursor.position.column))
        };

        let active_doc = crate::ide::view::active_function_call_in_line(cur_line, col_idx)
            .or_else(|| crate::ide::view::active_function_call_at_cursor(&text, line_idx, col_idx));

        let target_func = target_ident
            .as_ref()
            .map(|id| id.trim().trim_start_matches('\'').to_lowercase())
            .filter(|name| {
                !name.is_empty() && (
                    crate::ide::api_docs::API_DOCS.iter().any(|d| d.name.eq_ignore_ascii_case(name))
                        || settings.custom_functions_set.contains(name.as_str())
                )
            })
            .or_else(|| active_doc.map(|d| d.name.to_lowercase()))
            .or_else(|| {
                crate::ide::view::primary_function_in_line(cur_line).map(|d| d.name.to_lowercase())
            });

        let target_bp = crate::ide::paren::resolve_breakpoint_paren_at_cursor(&text, line_idx, col_idx)
            .or_else(|| {
                crate::ide::paren::resolve_breakpoint_paren_at_cursor(
                    &text,
                    cursor.position.line,
                    cursor.position.column,
                )
            })
            .map(|bp| {
                let preview = crate::ide::paren::sexpr_preview_at_bp(&text, bp)
                    .unwrap_or_else(|| format!("Ln {}:{}", bp.line, bp.col));
                (bp, preview)
            });

        let cursor_line = line_idx + 1;

        (target_ident, target_func, target_bp, cursor_line, has_selection)
    }

    pub fn current_autocomplete_matches(&self) -> Option<(usize, usize, Vec<String>)> {
        if self.autocomplete_dismissed {
            return None;
        }
        let tab = self.tabs.get(self.active_tab)?;
        let cursor = tab.content.cursor();
        if cursor.selection.is_some() {
            return None;
        }
        let text = tab.content.text();
        let lines: Vec<&str> = text.lines().collect();
        let line = lines.get(cursor.position.line).copied()?;
        let col = cursor.position.column.min(line.len());
        let prefix_slice = &line[..col];

        // Do not autocomplete inside a comment
        if let Some(comment_pos) = prefix_slice.find(';') {
            let before_comment = &prefix_slice[..comment_pos];
            if before_comment.chars().filter(|&c| c == '"').count() % 2 == 0 {
                return None;
            }
        }
        // Do not autocomplete inside a string literal
        if prefix_slice.chars().filter(|&c| c == '"').count() % 2 != 0 {
            return None;
        }

        // Do not show autocomplete tooltips if the caret is on a valid function
        if let Some(ident_at_caret) = crate::ide::view::identifier_in_line(line, col) {
            if self.settings.is_valid_function(&ident_at_caret) {
                return None;
            }
        }

        // Find the start of the identifier directly before cursor
        let chars: Vec<char> = prefix_slice.chars().collect();
        let mut start_idx = chars.len();
        while start_idx > 0 && crate::ide::api_docs::is_lisp_ident_char(chars[start_idx - 1]) {
            start_idx -= 1;
        }
        if start_idx == chars.len() {
            return None;
        }

        // REQUIREMENT: Only match a function if there is a left parenthesis '(' to the left of the function.
        // Look backwards from start_idx, skipping horizontal whitespace.
        let mut check_idx = start_idx;
        while check_idx > 0 && chars[check_idx - 1].is_whitespace() {
            check_idx -= 1;
        }

        let has_left_paren = if check_idx > 0 {
            chars[check_idx - 1] == '('
        } else {
            // Current line only contains whitespace before start_idx; check preceding lines for an unclosed '('
            let mut found = false;
            let mut line_idx = cursor.position.line;
            while line_idx > 0 {
                line_idx -= 1;
                let line_text = lines[line_idx];
                let clean = if let Some(semi) = line_text.find(';') {
                    &line_text[..semi]
                } else {
                    line_text
                }.trim_end();
                if clean.is_empty() {
                    continue;
                }
                if clean.ends_with('(') {
                    found = true;
                }
                break;
            }
            found
        };

        if !has_left_paren {
            return None;
        }

        let typed_ident: String = chars[start_idx..].iter().collect();
        if typed_ident.is_empty() || self.settings.is_valid_function(&typed_ident) {
            return None;
        }
        let prefix_lower = typed_ident.to_ascii_lowercase();

        let mut matches: Vec<String> = Vec::new();
        for doc in crate::ide::api_docs::API_DOCS {
            if doc.name.to_ascii_lowercase().starts_with(&prefix_lower) {
                matches.push(doc.name.to_string());
            }
        }
        for custom in self.settings.custom_functions_set.iter() {
            if custom.to_ascii_lowercase().starts_with(&prefix_lower)
                && !matches.iter().any(|m| m.eq_ignore_ascii_case(custom))
            {
                matches.push(custom.clone());
            }
        }

        if matches.is_empty() {
            return None;
        }

        matches.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        Some((start_idx, chars.len(), matches))
    }

    pub fn refresh_autocomplete_matches(&mut self) {
        if self.autocomplete_dismissed {
            self.autocomplete_matches.clear();
            return;
        }
        if let Some((_, _, matches)) = self.current_autocomplete_matches() {
            self.autocomplete_matches = matches;
            if !self.autocomplete_matches.is_empty() {
                self.autocomplete_selected_index = self.autocomplete_selected_index.min(self.autocomplete_matches.len() - 1);
            }
        } else {
            self.autocomplete_matches.clear();
        }
    }

    pub fn is_valid_function(&self, name: &str) -> bool {
        self.settings.is_valid_function(name)
    }

    pub fn new() -> (Self, Task<IdeMessage>) {
        let settings = IdeSettings::load();
        crate::i18n::init_for_ide(settings.language.as_deref());

        let (client, rx) = IdeClient::new();
        let initial_tab = IdeTab::new_empty("untitled.lsp".to_string());
        let welcome_text = format!("{}\n", crate::i18n::tr("ide-welcome"));
        let mut console_log = text_editor::Content::with_text(&welcome_text);
        console_log.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        let line_count = console_log.line_count() as i32;
        console_log.perform(text_editor::Action::Scroll { lines: line_count });

        let active_color_target = SyntaxColorTarget::Background;
        let syntax_hex_input = color_to_hex(settings.get_target_color(active_color_target));
        let font_size_input = settings.font_size().to_string();
        let transparency_input = settings.transparency_percent.to_string();
        let selected_theme_name = settings.detect_theme_name();

        let app = Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            console_log,
            repl_content: text_editor::Content::new(),
            repl_mode: crate::ide::protocol::ReplMode::Lisp,
            repl_history: Vec::new(),
            repl_history_index: None,
            repl_draft: String::new(),
            active_editor_focus: ActiveEditorFocus::Main,
            api_drawer_open: true,
            api_search: String::new(),
            theme: Theme::Dark,
            is_debugging: false,
            variables_open: false,
            debug_locals: Vec::new(),
            debug_stack: Vec::new(),
            is_transparent: false,
            watch_vars: Vec::new(),
            console_height: 220.0,
            is_splitter_dragging: false,
            splitter_last_y: None,
            window_width: 1000.0,
            window_height: 780.0,
            context_menu: None,
            last_mouse_pos: Point::ORIGIN,
            autocomplete_selected_index: 0,
            autocomplete_dismissed: false,
            autocomplete_matches: Vec::new(),
            settings,
            settings_open: false,
            help_menu_open: false,
            settings_tab: SettingsTab::General,
            active_color_target,
            syntax_hex_input,
            font_size_input,
            transparency_input,
            selected_theme_name,
            new_theme_name: String::new(),
            selected_custom_func: None,
            new_custom_func_input: String::new(),
            selected_ignored_func: None,
            new_ignored_func_input: String::new(),
            save_prompt: None,
            find_replace: FindReplaceState::default(),
            client,
            rx: Some(rx),
        };

        (app, Task::none())
    }

    pub fn title(&self) -> String {
        let active_title = self.tabs.get(self.active_tab)
            .map(|t| {
                let dot = if t.dirty { "● " } else { "" };
                format!("{}{}", dot, t.title)
            })
            .unwrap_or_else(|| "OCIDE".to_string());
        crate::i18n::tr_args("ide-window-title", &[("tab", active_title.as_str().into())])
    }

    pub fn update(&mut self, message: IdeMessage) -> Task<IdeMessage> {
        match message {
            IdeMessage::Tick => {
                let mut tasks = Vec::new();
                if let Some(rx) = &self.rx {
                    while let Ok(msg) = rx.try_recv() {
                        tasks.push(Task::done(IdeMessage::PluginEvent(msg)));
                    }
                }
                if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                }
            }
            IdeMessage::PluginEvent(event) => {
                match event {
                    PluginToIde::Pong => {}
                    PluginToIde::FocusWindow => {
                        #[cfg(windows)]
                        force_foreground_ocide();
                        return iced::window::latest().then(|id_opt| {
                            if let Some(id) = id_opt {
                                Task::batch([
                                    iced::window::minimize(id, false),
                                    iced::window::gain_focus(id),
                                ])
                            } else {
                                Task::none()
                            }
                        });
                    }
                    PluginToIde::FocusCad => {
                        force_foreground_cad();
                    }
                    PluginToIde::ConsoleOutput { text, is_error: _ } => {
                        self.append_to_console(&text);
                    }
                    PluginToIde::DebugEvt(debug_event) => {
                        match debug_event {
                            DebugEvent::Paused {
                                filename,
                                line,
                                col,
                                end_line,
                                end_col,
                                expr_preview,
                                locals,
                                stack,
                            } => {
                                self.is_debugging = true;
                                self.variables_open = true;
                                self.debug_locals = locals;
                                self.debug_stack = stack;

                                let norm_fname = filename.replace('\\', "/").to_lowercase();
                                let fname_stem = std::path::Path::new(&norm_fname).file_name().and_then(|n| n.to_str()).unwrap_or(&norm_fname);

                                let mut found_tab = None;
                                for (idx, tab) in self.tabs.iter().enumerate() {
                                    let tab_title_norm = tab.title.replace('\\', "/").to_lowercase();
                                    let tab_stem = std::path::Path::new(&tab_title_norm).file_name().and_then(|n| n.to_str()).unwrap_or(&tab_title_norm);
                                    let tab_path_norm = tab.path.as_ref().map(|p| p.to_string_lossy().replace('\\', "/").to_lowercase());

                                    if norm_fname == tab_title_norm
                                        || fname_stem == tab_stem
                                        || norm_fname.ends_with(&format!("/{}", tab_title_norm))
                                        || tab_path_norm.as_deref() == Some(norm_fname.as_str())
                                    {
                                        found_tab = Some(idx);
                                        break;
                                    }
                                }
                                let visible_lines = self.visible_lines();
                                for (idx, tab) in self.tabs.iter_mut().enumerate() {
                                    if Some(idx) == found_tab || (found_tab.is_none() && idx == self.active_tab) {
                                        tab.debug_paused_line = Some(line);
                                        tab.debug_paused_span = Some((
                                            line,
                                            col.max(1),
                                            end_line.max(line),
                                            end_col.max(col.max(1)),
                                        ));
                                        tab.debug_paused_expr = if expr_preview.is_empty() {
                                            None
                                        } else {
                                            Some(expr_preview.clone())
                                        };
                                        let target_line = line.saturating_sub(1);
                                        let target_col = col.saturating_sub(1);
                                        let margin = (visible_lines / 3).clamp(3, 10);
                                        let target_top = target_line.saturating_sub(margin);
                                        tab.scroll_to_line_exact(target_top, target_line, target_col, visible_lines);
                                    } else {
                                        tab.debug_paused_line = None;
                                        tab.debug_paused_span = None;
                                        tab.debug_paused_expr = None;
                                    }
                                }
                                if let Some(idx) = found_tab {
                                    self.active_tab = idx;
                                }

                                if expr_preview.is_empty() {
                                    let msg = crate::i18n::tr_args(
                                        "console-debug-paused",
                                        &[
                                            ("file", filename.as_str().into()),
                                            ("line", line.into()),
                                            ("col", col.into()),
                                        ],
                                    );
                                    self.append_to_console(&format!("{msg}\n"));
                                } else {
                                    let msg = crate::i18n::tr_args(
                                        "console-debug-paused-expr",
                                        &[
                                            ("file", filename.as_str().into()),
                                            ("line", line.into()),
                                            ("col", col.into()),
                                            ("expr", expr_preview.as_str().into()),
                                        ],
                                    );
                                    self.append_to_console(&format!("{msg}\n"));
                                }

                                #[cfg(windows)]
                                force_foreground_ocide();

                                return iced::window::latest().then(|id_opt| {
                                    if let Some(id) = id_opt {
                                        Task::batch([
                                            iced::window::minimize(id, false),
                                            iced::window::gain_focus(id),
                                        ])
                                    } else {
                                        Task::none()
                                    }
                                });
                            }
                            DebugEvent::Finished => {
                                self.is_debugging = false;
                                for tab in &mut self.tabs {
                                    tab.debug_paused_line = None;
                                    tab.debug_paused_span = None;
                                    tab.debug_paused_expr = None;
                                }
                                self.append_to_console(&format!("{}\n", crate::i18n::tr("console-debug-finished")));
                            }
                            DebugEvent::Error(err) => {
                                self.is_debugging = false;
                                for tab in &mut self.tabs {
                                    tab.debug_paused_line = None;
                                    tab.debug_paused_span = None;
                                    tab.debug_paused_expr = None;
                                }
                                let msg = crate::i18n::tr_args("console-debug-error", &[("error", err.as_str().into())]);
                                self.append_to_console(&format!("{msg}\n"));
                            }
                        }
                    }
                    PluginToIde::SelectionChanged { handles } => {
                        if !handles.is_empty() {
                            let handles_str = format!("{handles:?}");
                            let msg = crate::i18n::tr_args(
                                "console-cad-selection",
                                &[
                                    ("count", handles.len().into()),
                                    ("handles", handles_str.as_str().into()),
                                ],
                            );
                            self.append_to_console(&format!("{msg}\n"));
                        }
                    }
                    PluginToIde::HostClosing => {
                        let dirty_files: Vec<(usize, String)> = self.tabs.iter().enumerate()
                            .filter(|(_, t)| t.dirty)
                            .map(|(i, t)| (i, t.title.clone()))
                            .collect();
                        if dirty_files.is_empty() {
                            return iced::exit();
                        } else {
                            self.save_prompt = Some(crate::ide::SavePromptState {
                                trigger: crate::ide::SavePromptTrigger::HostClosing,
                                dirty_files,
                            });
                            return Task::none();
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::NewTab => {
                let count = self.tabs.len() + 1;
                let title = format!("untitled_{count}.lsp");
                self.tabs.push(IdeTab::new_empty(title));
                self.active_tab = self.tabs.len() - 1;
                self.repl_mode = crate::ide::protocol::ReplMode::Lisp;
                Task::none()
            }
            IdeMessage::SelectTab(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = idx;
                    let tab = &self.tabs[idx];
                    self.repl_mode = if tab.is_lisp() {
                        crate::ide::protocol::ReplMode::Lisp
                    } else {
                        crate::ide::protocol::ReplMode::Python
                    };
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    self.client.send(IdeToPlugin::SyncBreakpoints {
                        file: tab.title.clone(),
                        breakpoints: bps,
                    });
                    self.refresh_find_matches();
                }
                Task::none()
            }
            IdeMessage::CloseActiveTab => {
                self.update(IdeMessage::CloseTab(self.active_tab))
            }
            IdeMessage::CloseTab(idx) => {
                if idx < self.tabs.len() {
                    if self.tabs[idx].dirty {
                        self.save_prompt = Some(crate::ide::SavePromptState {
                            trigger: crate::ide::SavePromptTrigger::CloseTab(idx),
                            dirty_files: vec![(idx, self.tabs[idx].title.clone())],
                        });
                        return Task::none();
                    }
                    self.tabs.remove(idx);
                    if self.tabs.is_empty() {
                        self.tabs.push(IdeTab::new_empty("untitled.lsp".to_string()));
                    }
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len() - 1;
                    }
                }
                Task::none()
            }
            IdeMessage::OpenFile => {
                let start_dir = self.settings.resolved_scripts_dir();
                let filter_lisp = crate::i18n::tr("dialog-filter-lisp");
                let filter_all = crate::i18n::tr("dialog-filter-all");
                Task::perform(
                    async move {
                        let file = rfd::AsyncFileDialog::new()
                            .set_directory(&start_dir)
                            .add_filter(&filter_lisp, &["lsp", "lisp"])
                            .add_filter(&filter_all, &["*"])
                            .pick_file()
                            .await;
                        if let Some(f) = file {
                            let path = f.path().to_path_buf();
                            if let Ok(content) = crate::rust_lisp::read_lsp_text(&path) {
                                return Some((path, content));
                            }
                        }
                        None
                    },
                    IdeMessage::FileLoaded,
                )
            }
            IdeMessage::FileLoaded(opt) => {
                if let Some((path, content)) = opt {
                    let tab = IdeTab::from_file(path.clone(), &content);
                    let title = tab.title.clone();
                    self.repl_mode = if tab.is_lisp() {
                        crate::ide::protocol::ReplMode::Lisp
                    } else {
                        crate::ide::protocol::ReplMode::Python
                    };
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    self.client.send(IdeToPlugin::SyncBreakpoints {
                        file: title,
                        breakpoints: bps,
                    });
                    self.tabs.push(tab);
                    self.active_tab = self.tabs.len() - 1;
                    let path_str = path.display().to_string();
                    let msg = crate::i18n::tr_args("console-ide-opened", &[("path", path_str.as_str().into())]);
                    self.append_to_console(&format!("{msg}\n"));
                }
                Task::none()
            }
            IdeMessage::SaveTab => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    if tab.path.is_none() {
                        return self.update(IdeMessage::SaveTabAs);
                    }
                }
                let mut saved_title = None;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if let Some(path) = &tab.path {
                        let path = path.clone();
                        let code = tab.content.text();
                        if std::fs::write(&path, &code).is_ok() {
                            tab.dirty = false;
                            saved_title = Some(tab.title.clone());
                        }
                    }
                }
                if let Some(title) = saved_title {
                    let msg = crate::i18n::tr_args("console-ide-saved", &[("file", title.as_str().into())]);
                    self.append_to_console(&format!("{msg}\n"));
                }
                Task::none()
            }
            IdeMessage::SaveTabAs => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let active_idx = self.active_tab;
                    let (start_dir, file_name) = if let Some(p) = &tab.path {
                        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| self.settings.resolved_scripts_dir());
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("routine.lsp").to_string();
                        (dir, name)
                    } else {
                        let dir = self.settings.resolved_scripts_dir();
                        let name = if tab.title.ends_with(".lsp") || tab.title.ends_with(".lisp") {
                            tab.title.clone()
                        } else {
                            format!("{}.lsp", tab.title)
                        };
                        (dir, name)
                    };
                    let _ = std::fs::create_dir_all(&start_dir);
                    let filter_lisp = crate::i18n::tr("dialog-filter-lisp");
                    let filter_all = crate::i18n::tr("dialog-filter-all");
                    Task::perform(
                        async move {
                            let file = rfd::AsyncFileDialog::new()
                                .set_directory(&start_dir)
                                .set_file_name(&file_name)
                                .add_filter(&filter_lisp, &["lsp", "lisp"])
                                .add_filter(&filter_all, &["*"])
                                .save_file()
                                .await;
                            file.map(|f| (active_idx, f.path().to_path_buf()))
                        },
                        IdeMessage::FileSavedAs,
                    )
                } else {
                    Task::none()
                }
            }
            IdeMessage::FileSavedAs(opt) => {
                if let Some((tab_idx, path)) = opt {
                    if let Some(tab) = self.tabs.get_mut(tab_idx) {
                        let code = tab.content.text();
                        if std::fs::write(&path, &code).is_ok() {
                            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or(&tab.title).to_string();
                            tab.title = fname.clone();
                            tab.path = Some(path);
                            tab.dirty = false;
                            let msg = crate::i18n::tr_args("console-ide-saved", &[("file", fname.as_str().into())]);
                            self.append_to_console(&format!("{msg}\n"));
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::RestoreCursorAndAct(action) => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if let Some(saved) = tab.last_edit_cursor.take() {
                        tab.content.move_to(saved);
                        if saved.position.line < tab.scroll_top_line {
                            tab.scroll_top_line = saved.position.line;
                        } else if saved.position.line >= tab.scroll_top_line + visible_lines {
                            tab.scroll_top_line = saved.position.line.saturating_sub(visible_lines.saturating_sub(1));
                        }
                    }
                    tab.is_scrolling_away = false;
                    let is_edit = action.is_edit();
                    if is_edit {
                        tab.push_undo_snapshot_if_burst(matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter | text_editor::Edit::Backspace | text_editor::Edit::Delete | text_editor::Edit::Paste(_))));
                        tab.dirty = true;
                    }
                    tab.content.perform(action);
                    tab.last_edit_cursor = Some(tab.content.cursor());
                    if is_edit {
                        self.refresh_find_matches();
                    }
                    self.refresh_autocomplete_matches();
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::EditorAction(action) => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Main;
                let is_edit = action.is_edit();
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if let text_editor::Action::Scroll { lines } = action {
                        // 1. Preserve the user's editing cursor (including any active selection!) before scrolling away
                        if !tab.is_scrolling_away {
                            tab.last_edit_cursor = Some(tab.content.cursor());
                        }
                        let edit_cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
                        let edit_line = edit_cursor.position.line;

                        let total = tab.content.line_count().max(1);
                        let max_scroll = total.saturating_sub(visible_lines);

                        if (lines < 0 && tab.scroll_top_line == 0)
                            || (lines > 0 && tab.scroll_top_line >= max_scroll)
                        {
                            return Task::none();
                        }

                        let next = (tab.scroll_top_line as i32 + lines).clamp(0, max_scroll as i32) as usize;
                        if next == tab.scroll_top_line {
                            return Task::none();
                        }

                        if next <= edit_line && edit_line < next + visible_lines {
                            // User's actual cursor (and any active selection) is now ON-SCREEN!
                            tab.scroll_viewport_exact(next, edit_cursor, visible_lines, true);
                            tab.is_scrolling_away = false;
                            // Focus editor so the caret & selection at edit_cursor render natively
                            return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                        } else {
                            // User's actual cursor is OFF-SCREEN: keep edit_cursor (and its selection!) untouched in last_edit_cursor
                            tab.scroll_viewport_exact(
                                next,
                                text_editor::Cursor {
                                    position: text_editor::Position {
                                        line: next,
                                        column: 0,
                                    },
                                    selection: None,
                                },
                                visible_lines,
                                false,
                            );
                            tab.is_scrolling_away = true;
                            // Unfocus editor so the caret quad DISAPPEARS completely (view renders selection overlay if selected)
                            return iced::advanced::widget::operate(
                                iced::advanced::widget::operation::focusable::unfocus::<IdeMessage>(),
                            );
                        }
                    }

                    if action.is_edit() {
                        tab.push_undo_snapshot_if_burst(matches!(action, text_editor::Action::Edit(text_editor::Edit::Enter | text_editor::Edit::Backspace | text_editor::Edit::Delete | text_editor::Edit::Paste(_))));
                        tab.dirty = true;
                        self.autocomplete_dismissed = false;
                        self.autocomplete_selected_index = 0;
                        // If user was scrolled away and edits, jump back to edit cursor
                        if tab.is_scrolling_away && tab.last_edit_cursor.is_some() {
                            let saved = tab.last_edit_cursor.take().unwrap();
                            tab.content.move_to(saved);
                            if saved.position.line < tab.scroll_top_line {
                                tab.scroll_top_line = saved.position.line;
                            } else if saved.position.line >= tab.scroll_top_line + visible_lines {
                                tab.scroll_top_line = saved.position.line.saturating_sub(visible_lines.saturating_sub(1));
                            }
                            tab.is_scrolling_away = false;
                        }
                    } else if matches!(
                        action,
                        text_editor::Action::Click(_)
                            | text_editor::Action::Drag(_)
                            | text_editor::Action::SelectWord
                            | text_editor::Action::SelectLine
                            | text_editor::Action::SelectAll
                    ) {
                        tab.is_scrolling_away = false;
                        tab.last_edit_cursor = None;
                    } else if matches!(action, text_editor::Action::Move(_)) && tab.is_scrolling_away {
                        if let Some(saved) = tab.last_edit_cursor.take() {
                            tab.content.move_to(saved);
                            if saved.position.line < tab.scroll_top_line {
                                tab.scroll_top_line = saved.position.line;
                            } else if saved.position.line >= tab.scroll_top_line + visible_lines {
                                tab.scroll_top_line = saved.position.line.saturating_sub(visible_lines.saturating_sub(1));
                            }
                            tab.is_scrolling_away = false;
                        }
                    }

                    if matches!(action, text_editor::Action::SelectWord) && tab.is_lisp() {
                        if Self::select_word_matching_paren(&mut tab.content) {
                            tab.last_edit_cursor = Some(tab.content.cursor());
                            return Task::none();
                        }
                    }

                    tab.content.perform(action);

                    if !tab.is_scrolling_away {
                        tab.last_edit_cursor = Some(tab.content.cursor());
                        let cursor_line = tab.content.cursor().position.line;
                        if cursor_line < tab.scroll_top_line {
                            tab.scroll_top_line = cursor_line;
                        } else if cursor_line >= tab.scroll_top_line + visible_lines {
                            tab.scroll_top_line = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
                        }
                    }
                }
                if is_edit {
                    self.refresh_find_matches();
                }
                self.refresh_autocomplete_matches();
                Task::none()
            }
            IdeMessage::ScrollToLine(target_line) => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if !tab.is_scrolling_away {
                        tab.last_edit_cursor = Some(tab.content.cursor());
                    }
                    let edit_cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
                    let edit_line = edit_cursor.position.line;

                    let total = tab.content.line_count().max(1);
                    let max_scroll = total.saturating_sub(visible_lines);
                    let target = target_line.clamp(0, max_scroll);

                    if target <= edit_line && edit_line < target + visible_lines {
                        tab.scroll_viewport_exact(target, edit_cursor, visible_lines, true);
                        tab.is_scrolling_away = false;
                        return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                    } else {
                        tab.scroll_viewport_exact(
                            target,
                            text_editor::Cursor {
                                position: text_editor::Position {
                                    line: target,
                                    column: 0,
                                },
                                selection: None,
                            },
                            visible_lines,
                            false,
                        );
                        tab.is_scrolling_away = true;
                        return iced::advanced::widget::operate(
                            iced::advanced::widget::operation::focusable::unfocus::<IdeMessage>(),
                        );
                    }
                }
                Task::none()
            }
            IdeMessage::ConsoleAction(action) => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Console;
                if matches!(action, text_editor::Action::SelectWord) {
                    if Self::select_word_matching_paren(&mut self.console_log) {
                        return Task::none();
                    }
                }
                self.console_log.perform(action);
                Task::none()
            }
            IdeMessage::ReplAction(action) => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Repl;
                if matches!(action, text_editor::Action::SelectWord) {
                    if Self::select_word_matching_paren(&mut self.repl_content) {
                        return Task::none();
                    }
                }
                self.repl_content.perform(action);
                Task::none()
            }
            IdeMessage::ReplInsertNewline => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Repl;
                let text = self.repl_content.text();
                let cursor = self.repl_content.cursor();
                let offset = crate::ide::paren::pos_to_offset(&text, cursor.position.line, cursor.position.column);
                let indent = crate::ide::paren::compute_lisp_indent_at_offset(&text, offset);
                let insert_str = format!("\n{}", " ".repeat(indent));
                self.repl_content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    std::sync::Arc::new(insert_str),
                )));
                Task::none()
            }
            IdeMessage::ReplSubmit => {
                self.active_editor_focus = ActiveEditorFocus::Repl;
                let code = self.repl_content.text().trim().to_string();
                if !code.is_empty() {
                    let prompt_prefix = match self.repl_mode {
                        crate::ide::protocol::ReplMode::Python => ">>>",
                        crate::ide::protocol::ReplMode::Lisp => "LISP>",
                    };
                    self.append_to_console(&format!("{prompt_prefix} {code}\n"));
                    if self.repl_history.last().map(|s| s.as_str()) != Some(code.as_str()) {
                        self.repl_history.push(code.clone());
                    }
                    self.repl_history_index = None;
                    self.repl_draft.clear();
                    self.client.send(IdeToPlugin::ReplEval {
                        code,
                        mode: self.repl_mode,
                    });
                    self.repl_content = text_editor::Content::new();
                }
                iced::widget::operation::focus(crate::ide::view::REPL_EDITOR_ID)
            }
            IdeMessage::ReplClear => {
                self.active_editor_focus = ActiveEditorFocus::Repl;
                self.repl_content = text_editor::Content::new();
                self.repl_history_index = None;
                self.repl_draft.clear();
                Task::none()
            }
            IdeMessage::ReplHistoryPrev => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Repl;
                let cursor_line = self.repl_content.cursor().position.line;
                if cursor_line == 0 || self.repl_content.line_count() <= 1 {
                    if !self.repl_history.is_empty() {
                        if self.repl_history_index.is_none() {
                            self.repl_draft = self.repl_content.text();
                            self.repl_history_index = Some(self.repl_history.len() - 1);
                        } else if let Some(idx) = self.repl_history_index {
                            if idx > 0 {
                                self.repl_history_index = Some(idx - 1);
                            }
                        }
                        if let Some(idx) = self.repl_history_index {
                            if let Some(cmd) = self.repl_history.get(idx) {
                                self.repl_content = text_editor::Content::with_text(cmd);
                                self.repl_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                            }
                        }
                    }
                } else {
                    self.repl_content.perform(text_editor::Action::Move(text_editor::Motion::Up));
                }
                Task::none()
            }
            IdeMessage::ReplHistoryNext => {
                if self.settings_open {
                    return Task::none();
                }
                self.active_editor_focus = ActiveEditorFocus::Repl;
                let cursor_line = self.repl_content.cursor().position.line;
                let last_line = self.repl_content.line_count().saturating_sub(1);
                if cursor_line == last_line || self.repl_history_index.is_some() {
                    if let Some(idx) = self.repl_history_index {
                        if idx + 1 < self.repl_history.len() {
                            self.repl_history_index = Some(idx + 1);
                            if let Some(cmd) = self.repl_history.get(idx + 1) {
                                self.repl_content = text_editor::Content::with_text(cmd);
                                self.repl_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                            }
                        } else {
                            self.repl_history_index = None;
                            self.repl_content = text_editor::Content::with_text(&self.repl_draft);
                            self.repl_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                        }
                    }
                } else {
                    self.repl_content.perform(text_editor::Action::Move(text_editor::Motion::Down));
                }
                Task::none()
            }
            IdeMessage::ToggleReplMode => {
                self.repl_mode = match self.repl_mode {
                    crate::ide::protocol::ReplMode::Python => crate::ide::protocol::ReplMode::Lisp,
                    crate::ide::protocol::ReplMode::Lisp => crate::ide::protocol::ReplMode::Python,
                };
                let mode_str = match self.repl_mode {
                    crate::ide::protocol::ReplMode::Python => "Python",
                    crate::ide::protocol::ReplMode::Lisp => "OpenCAD LISP",
                };
                let msg = crate::i18n::tr_args("console-repl-mode-switched", &[("mode", mode_str.into())]);
                self.append_to_console(&format!("{msg}\n"));
                Task::none()
            }
            IdeMessage::SetReplMode(mode) => {
                self.repl_mode = mode;
                let mode_str = match self.repl_mode {
                    crate::ide::protocol::ReplMode::Python => "Python",
                    crate::ide::protocol::ReplMode::Lisp => "OpenCAD LISP",
                };
                let msg = crate::i18n::tr_args("console-repl-mode-switched", &[("mode", mode_str.into())]);
                self.append_to_console(&format!("{msg}\n"));
                Task::none()
            }
            IdeMessage::ClearConsole => {
                self.set_console_text(&format!("{}\n", crate::i18n::tr("console-cleared")));
                Task::none()
            }
            IdeMessage::SelectMatchingParen { forward: _ } => {
                match self.active_editor_focus {
                    ActiveEditorFocus::Repl => {
                        Self::select_enclosing_paren(&mut self.repl_content);
                    }
                    ActiveEditorFocus::Console => {
                        Self::select_enclosing_paren(&mut self.console_log);
                    }
                    ActiveEditorFocus::Main => {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            if tab.is_lisp() {
                                Self::select_enclosing_paren(&mut tab.content);
                            }
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::JumpToMatchingParen { forward } => {
                match self.active_editor_focus {
                    ActiveEditorFocus::Repl => {
                        Self::jump_target_paren(&mut self.repl_content, forward);
                    }
                    ActiveEditorFocus::Console => {
                        Self::jump_target_paren(&mut self.console_log, forward);
                    }
                    ActiveEditorFocus::Main => {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            if tab.is_lisp() {
                                Self::jump_target_paren(&mut tab.content, forward);
                            }
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::SmartEnter => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.is_scrolling_away {
                        if let Some(saved) = tab.last_edit_cursor.take() {
                            tab.content.move_to(saved);
                        }
                        tab.is_scrolling_away = false;
                    }
                    tab.push_undo_snapshot();
                    if tab.content.selection().is_some() {
                        tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                    }

                    // First normalize intra-line whitespace on the current line before the cursor
                    {
                        let cursor0 = tab.content.cursor();
                        let text0 = tab.content.text();
                        let line_idx = cursor0.position.line;
                        let line_start = crate::ide::paren::pos_to_offset(&text0, line_idx, 0).min(text0.len());
                        let offset0 = crate::ide::paren::pos_to_offset(&text0, line_idx, cursor0.position.column).min(text0.len());
                        let prefix_before_line = &text0[..line_start];
                        let before_cursor_on_line = &text0[line_start..offset0];
                        let trimmed_before = before_cursor_on_line.trim_start_matches([' ', '\t']);
                        if !trimmed_before.is_empty()
                            && !crate::ide::paren::is_in_multiline_string_or_block_comment(prefix_before_line)
                        {
                            let line_indent = crate::ide::paren::compute_lisp_line_indent(
                                prefix_before_line,
                                trimmed_before,
                            )
                            .unwrap_or(before_cursor_on_line.len() - trimmed_before.len());
                            let norm_before = crate::ide::paren::normalize_lisp_line_slice(trimmed_before);
                            let formatted_before = format!("{}{}", " ".repeat(line_indent), norm_before);
                            if formatted_before != before_cursor_on_line {
                                tab.content.move_to(text_editor::Cursor {
                                    position: cursor0.position,
                                    selection: Some(text_editor::Position {
                                        line: line_idx,
                                        column: 0,
                                    }),
                                });
                                tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                                    std::sync::Arc::new(formatted_before),
                                )));
                            }
                        }
                    }

                    let cursor = tab.content.cursor();
                    let text = tab.content.text();
                    let offset = crate::ide::paren::pos_to_offset(
                        &text,
                        cursor.position.line,
                        cursor.position.column,
                    );
                    let indent = crate::ide::paren::compute_lisp_indent_at_offset(&text, offset);

                    let after = &text[offset.min(text.len())..];
                    let line_end = after.find(['\r', '\n']).unwrap_or(after.len());
                    let trailing_ws = after[..line_end]
                        .bytes()
                        .take_while(|&b| b == b' ' || b == b'\t')
                        .count();
                    if trailing_ws > 0 {
                        tab.content.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: cursor.position.line,
                                column: cursor.position.column + trailing_ws,
                            },
                            selection: Some(cursor.position),
                        });
                    }

                    let insert_str = format!("\n{}", " ".repeat(indent));
                    tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        std::sync::Arc::new(insert_str),
                    )));
                    tab.dirty = true;
                    tab.last_edit_cursor = Some(tab.content.cursor());
                    let cursor_line = tab.content.cursor().position.line;
                    if cursor_line < tab.scroll_top_line {
                        tab.scroll_top_line = cursor_line;
                    } else if cursor_line >= tab.scroll_top_line + visible_lines {
                        tab.scroll_top_line = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
                    }
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::SmartTab => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.is_scrolling_away {
                        if let Some(saved) = tab.last_edit_cursor.take() {
                            tab.content.move_to(saved);
                        }
                        tab.is_scrolling_away = false;
                    }
                    tab.push_undo_snapshot();
                    let cursor = tab.content.cursor();
                    let text = tab.content.text();
                    let line_idx = cursor.position.line;
                    let line_start_offset = crate::ide::paren::pos_to_offset(&text, line_idx, 0);
                    let prefix = &text[..line_start_offset.min(text.len())];
                    let after = &text[line_start_offset.min(text.len())..];
                    let line_end = after.find(['\r', '\n']).unwrap_or(after.len());
                    let raw_line = &after[..line_end];
                    let current_ws = raw_line
                        .bytes()
                        .take_while(|&b| b == b' ' || b == b'\t')
                        .count();
                    let trimmed_rest = &raw_line[current_ws..];
                    let target_indent = crate::ide::paren::compute_lisp_line_indent(prefix, trimmed_rest)
                        .unwrap_or(current_ws);
                    let norm_rest = if !crate::ide::paren::is_in_multiline_string_or_block_comment(prefix) {
                        crate::ide::paren::normalize_lisp_line_slice(trimmed_rest)
                    } else {
                        trimmed_rest.to_string()
                    };
                    let new_line_str = format!("{}{}", " ".repeat(target_indent), norm_rest);
                    let new_line_len = new_line_str.len();

                    tab.content.move_to(text_editor::Cursor {
                        position: text_editor::Position {
                            line: line_idx,
                            column: raw_line.len(),
                        },
                        selection: Some(text_editor::Position {
                            line: line_idx,
                            column: 0,
                        }),
                    });
                    tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        std::sync::Arc::new(new_line_str),
                    )));

                    let new_col = if cursor.position.column <= current_ws {
                        target_indent
                    } else {
                        (target_indent + (cursor.position.column - current_ws)).min(new_line_len)
                    };
                    tab.content.move_to(text_editor::Cursor {
                        position: text_editor::Position {
                            line: line_idx,
                            column: new_col,
                        },
                        selection: None,
                    });
                    tab.dirty = true;
                    tab.last_edit_cursor = Some(tab.content.cursor());
                    if line_idx < tab.scroll_top_line {
                        tab.scroll_top_line = line_idx;
                    } else if line_idx >= tab.scroll_top_line + visible_lines {
                        tab.scroll_top_line = line_idx.saturating_sub(visible_lines.saturating_sub(1));
                    }
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::FormatDocument => {
                self.active_editor_focus = ActiveEditorFocus::Main;
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.is_scrolling_away {
                        if let Some(saved) = tab.last_edit_cursor.take() {
                            tab.content.move_to(saved);
                        }
                        tab.is_scrolling_away = false;
                    }
                    let saved_scroll = tab.scroll_top_line;
                    let cursor = tab.content.cursor();
                    let text = tab.content.text();
                    let formatted = crate::ide::paren::format_lisp_code_with_options(&text, &self.settings.format_options);
                    if formatted != text {
                        tab.push_undo_snapshot();
                        tab.content = text_editor::Content::with_text(&formatted);
                        let total = tab.content.line_count().max(1);
                        let target_line = cursor.position.line.min(total.saturating_sub(1));
                        let line_len = tab
                            .content
                            .line(target_line)
                            .map(|l| l.text.len())
                            .unwrap_or(0);
                        let target_col = cursor.position.column.min(line_len);
                        let new_cursor = text_editor::Cursor {
                            position: text_editor::Position {
                                line: target_line,
                                column: target_col,
                            },
                            selection: None,
                        };
                        let max_scroll = total.saturating_sub(visible_lines.max(1));
                        let target_scroll = if target_line < saved_scroll {
                            target_line
                        } else if target_line >= saved_scroll + visible_lines {
                            target_line.saturating_sub(visible_lines / 2)
                        } else {
                            saved_scroll
                        }
                        .min(max_scroll);
                        tab.scroll_viewport_exact(target_scroll, new_cursor, visible_lines, true);
                        tab.is_scrolling_away = false;
                        tab.dirty = true;
                    }
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::FormatSelection => {
                let tab_has_sel = self.tabs.get(self.active_tab).map_or(false, |tab| {
                    tab.content.selection().is_some()
                        || tab.content.cursor().selection.is_some()
                        || tab.last_edit_cursor.and_then(|c| c.selection).is_some()
                });

                if self.active_editor_focus == ActiveEditorFocus::Repl && !tab_has_sel {
                    let text = self.repl_content.text();
                    let cursor = self.repl_content.cursor();
                    let (start_line, end_line) = if let Some(sel) = cursor.selection {
                        let (top_pos, bot_pos) = if cursor.position.line < sel.line
                            || (cursor.position.line == sel.line && cursor.position.column <= sel.column)
                        {
                            (cursor.position, sel)
                        } else {
                            (sel, cursor.position)
                        };
                        let eff_end = if bot_pos.line > top_pos.line && bot_pos.column == 0 {
                            bot_pos.line.saturating_sub(1)
                        } else {
                            bot_pos.line
                        };
                        (top_pos.line, eff_end)
                    } else if let Some((s, e)) = crate::ide::paren::find_enclosing_paren_range(
                        &text,
                        cursor.position.line,
                        cursor.position.column,
                    ) {
                        (s.0, e.0)
                    } else {
                        (0, self.repl_content.line_count().saturating_sub(1))
                    };
                    let formatted = crate::ide::paren::format_lisp_line_range_with_options(&text, start_line, end_line, &self.settings.format_options);
                    if formatted != text {
                        self.repl_content = text_editor::Content::with_text(&formatted);
                    }
                    return iced::widget::operation::focus(crate::ide::view::REPL_EDITOR_ID);
                }

                self.active_editor_focus = ActiveEditorFocus::Main;
                let visible_lines = self.visible_lines();

                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.is_scrolling_away {
                        if let Some(saved) = tab.last_edit_cursor.take() {
                            tab.content.move_to(saved);
                        }
                        tab.is_scrolling_away = false;
                    }
                    let saved_scroll = tab.scroll_top_line;
                    let cursor = tab.content.cursor();
                    let sel_opt = cursor.selection.or_else(|| tab.last_edit_cursor.and_then(|c| c.selection));
                    let had_selection = sel_opt.is_some() || tab.content.selection().is_some();
                    let text = tab.content.text();
                    let (start_line, end_line) = if let Some(sel) = sel_opt {
                        let (top_pos, bot_pos) = if cursor.position.line < sel.line
                            || (cursor.position.line == sel.line && cursor.position.column <= sel.column)
                        {
                            (cursor.position, sel)
                        } else {
                            (sel, cursor.position)
                        };
                        let eff_end = if bot_pos.line > top_pos.line && bot_pos.column == 0 {
                            bot_pos.line.saturating_sub(1)
                        } else {
                            bot_pos.line
                        };
                        (top_pos.line, eff_end)
                    } else if let Some((s, e)) = crate::ide::paren::find_enclosing_paren_range(
                        &text,
                        cursor.position.line,
                        cursor.position.column,
                    ) {
                        (s.0, e.0)
                    } else {
                        (cursor.position.line, cursor.position.line)
                    };
                    let formatted = crate::ide::paren::format_lisp_line_range_with_options(&text, start_line, end_line, &self.settings.format_options);
                    if formatted != text {
                        tab.push_undo_snapshot();
                        tab.content = text_editor::Content::with_text(&formatted);
                        let total = tab.content.line_count().max(1);
                        let clamped_start = start_line.min(total.saturating_sub(1));
                        let clamped_end = end_line.min(total.saturating_sub(1));
                        let end_line_len = tab
                            .content
                            .line(clamped_end)
                            .map(|l| l.text.len())
                            .unwrap_or(0);

                        let new_cursor = if had_selection {
                            let start_pos = text_editor::Position {
                                line: clamped_start,
                                column: 0,
                            };
                            let end_pos = text_editor::Position {
                                line: clamped_end,
                                column: end_line_len,
                            };
                            text_editor::Cursor {
                                position: end_pos,
                                selection: Some(start_pos),
                            }
                        } else {
                            let target_line = cursor.position.line.min(total.saturating_sub(1));
                            let line_len = tab
                                .content
                                .line(target_line)
                                .map(|l| l.text.len())
                                .unwrap_or(0);
                            text_editor::Cursor {
                                position: text_editor::Position {
                                    line: target_line,
                                    column: cursor.position.column.min(line_len),
                                },
                                selection: None,
                            }
                        };

                        let max_scroll = total.saturating_sub(visible_lines.max(1));
                        let target_line = new_cursor.position.line;
                        let target_scroll = if target_line < saved_scroll {
                            target_line
                        } else if target_line >= saved_scroll + visible_lines {
                            target_line.saturating_sub(visible_lines / 2)
                        } else {
                            saved_scroll
                        }
                        .min(max_scroll);
                        tab.scroll_viewport_exact(target_scroll, new_cursor, visible_lines, true);
                        tab.is_scrolling_away = false;
                        tab.dirty = true;
                    }
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::RunAll => {
                let to_run = if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if let Some(path) = &tab.path {
                        let _ = std::fs::write(path, tab.content.text());
                        tab.dirty = false;
                    }
                    let code = tab.content.text();
                    let title = tab.title.clone();
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    let is_lisp = tab.is_lisp();
                    Some((code, title, bps, is_lisp))
                } else {
                    None
                };

                if let Some((code, title, bps, is_lisp)) = to_run {
                    if !title.starts_with("untitled") {
                        let msg = crate::i18n::tr_args("console-loading-script", &[("file", title.as_str().into())]);
                        self.append_to_console(&format!("\n{msg}\n"));
                    }

                    self.client.send(IdeToPlugin::SyncBreakpoints {
                        file: title.clone(),
                        breakpoints: bps.clone(),
                    });

                    self.is_debugging = !is_lisp || !bps.is_empty();
                    self.client.send(IdeToPlugin::DebugScript {
                        title,
                        code,
                        breakpoints: bps,
                        step_into: false,
                    });
                }
                Task::none()
            }
            IdeMessage::RunSelection => {
                let to_run = if let Some(tab) = self.tabs.get(self.active_tab) {
                    let full = tab.content.text();
                    let title = tab.title.clone();
                    let code_to_run = if let Some(sel) = tab.content.selection() {
                        if !sel.trim().is_empty() { sel } else { full }
                    } else {
                        full
                    };
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    let is_lisp = tab.is_lisp();
                    Some((title, code_to_run, bps, is_lisp))
                } else {
                    None
                };

                if let Some((title, code_to_run, bps, is_lisp)) = to_run {
                    self.is_debugging = !is_lisp || !bps.is_empty();
                    self.client.send(IdeToPlugin::DebugScript {
                        title,
                        code: code_to_run,
                        breakpoints: bps,
                        step_into: false,
                    });
                }
                Task::none()
            }
            IdeMessage::ToggleBreakpoint(line) => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.has_breakpoint_on_line(line) {
                        tab.breakpoints.retain(|bp| bp.line != line);
                    } else {
                        let text = tab.content.text();
                        let bp = crate::ide::paren::first_open_paren_on_line(&text, line)
                            .unwrap_or_else(|| crate::ide::protocol::ParenBreakpoint::new(line, 1));
                        tab.breakpoints.insert(bp);
                    }
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    let title = tab.title.clone();
                    self.client.send(IdeToPlugin::SyncBreakpoints {
                        file: title,
                        breakpoints: bps.clone(),
                    });
                    if self.is_debugging {
                        self.client.send(IdeToPlugin::DebugCmd(DebugCommand::SetBreakpoints(bps)));
                    }
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::ToggleBreakpointAtCursor => {
                self.context_menu = None;
                let target_bp = if let Some(tab) = self.tabs.get(self.active_tab) {
                    let cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
                    let text = tab.content.text();
                    crate::ide::paren::resolve_breakpoint_paren_at_cursor(
                        &text,
                        cursor.position.line,
                        cursor.position.column,
                    )
                    .or_else(|| {
                        crate::ide::paren::first_open_paren_on_line(&text, cursor.position.line + 1)
                    })
                } else {
                    None
                };
                if let Some(bp) = target_bp {
                    return self.update(IdeMessage::ToggleParenBreakpoint(bp));
                }
                Task::none()
            }
            IdeMessage::ToggleParenBreakpoint(bp) => {
                self.context_menu = None;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.breakpoints.contains(&bp) {
                        tab.breakpoints.remove(&bp);
                    } else {
                        tab.breakpoints.insert(bp);
                    }
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    let title = tab.title.clone();
                    self.client.send(IdeToPlugin::SyncBreakpoints {
                        file: title,
                        breakpoints: bps.clone(),
                    });
                    if self.is_debugging {
                        self.client.send(IdeToPlugin::DebugCmd(DebugCommand::SetBreakpoints(bps)));
                    }
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::AddWatch(var) => {
                let trimmed = var.trim().to_string();
                if !trimmed.is_empty() && !self.watch_vars.iter().any(|w| w.eq_ignore_ascii_case(&trimmed)) {
                    self.watch_vars.push(trimmed);
                }
                Task::none()
            }
            IdeMessage::AddWatchAtCursor => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let sel = tab.content.selection();
                    let ident = sel.and_then(|s| {
                        let t = s.trim().to_string();
                        if !t.is_empty() && (t.chars().next().unwrap().is_alphabetic() || t.starts_with('_')) {
                            Some(t)
                        } else {
                            None
                        }
                    }).or_else(|| {
                        let cursor = tab.content.cursor();
                        identifier_at_cursor(
                            &tab.content.text(),
                            cursor.position.line,
                            cursor.position.column,
                        )
                    });
                    if let Some(var) = ident {
                        return self.update(IdeMessage::AddWatch(var));
                    }
                }
                Task::none()
            }
            IdeMessage::RemoveWatch(idx) => {
                if idx < self.watch_vars.len() {
                    self.watch_vars.remove(idx);
                }
                Task::none()
            }
            IdeMessage::ClearWatch => {
                self.watch_vars.clear();
                Task::none()
            }
            IdeMessage::EditorCopy => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    if let Some(sel) = tab.content.selection() {
                        return iced::clipboard::write(sel).discard();
                    }
                }
                Task::none()
            }
            IdeMessage::EditorPasteRequest => {
                iced::clipboard::read_text().map(|res| IdeMessage::EditorPaste(res.ok().map(|s| (*s).clone())))
            }
            IdeMessage::EditorPaste(text_opt) => {
                if let Some(text) = text_opt {
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.push_undo_snapshot();
                        tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text))));
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            IdeMessage::ToggleApiDrawer => {
                self.api_drawer_open = !self.api_drawer_open;
                Task::none()
            }
            IdeMessage::ApiSearch(q) => {
                self.api_search = q;
                Task::none()
            }
            IdeMessage::ShowInApiDrawer(name) => {
                self.api_drawer_open = true;
                self.api_search = name;
                Task::none()
            }
            IdeMessage::InsertSnippet(snippet) => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.push_undo_snapshot();
                    tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(snippet))));
                    tab.dirty = true;
                }
                Task::none()
            }
            IdeMessage::ToggleVariables => {
                self.variables_open = !self.variables_open;
                Task::none()
            }
            IdeMessage::ToggleSettings => {
                self.help_menu_open = false;
                self.settings_open = !self.settings_open;
                if !self.settings_open {
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::ToggleHelpMenu => {
                self.help_menu_open = !self.help_menu_open;
                Task::none()
            }
            IdeMessage::OpenTutorials => {
                self.help_menu_open = false;
                if let Some(path) = find_tutorials_index() {
                    if let Err(err) = open_in_browser(&path) {
                        let path_str = path.display().to_string();
                        let err_str = err.to_string();
                        let msg = crate::i18n::tr_args(
                            "console-tutorials-open-failed",
                            &[
                                ("path", path_str.as_str().into()),
                                ("error", err_str.as_str().into()),
                            ],
                        );
                        self.append_to_console(&format!("{msg}\n"));
                    }
                } else {
                    self.append_to_console(&format!("{}\n", crate::i18n::tr("console-tutorials-not-found")));
                }
                Task::none()
            }
            IdeMessage::SetSettingsTab(tab) => {
                self.settings_tab = tab;
                Task::none()
            }
            IdeMessage::SetLanguage(lang_opt) => {
                self.settings.language = lang_opt;
                self.settings.save();
                let _ = crate::i18n::set_language(self.settings.language.as_deref());
                Task::none()
            }
            IdeMessage::SetFormatIndentSize(size) => {
                self.settings.format_options.indent_size = size;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ToggleFormatDowncaseBuiltins(val) => {
                self.settings.format_options.downcase_builtins = val;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ToggleFormatWideStyle(val) => {
                self.settings.format_options.wide_style_calls = val;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ToggleFormatAlignSetq(val) => {
                self.settings.format_options.align_setq_pairs = val;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ToggleFormatStructuralReflow(val) => {
                self.settings.format_options.structural_reflow = val;
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetEditorFont(font_opt) => {
                self.settings.font_name = font_opt;
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetEditorFontSize(size) => {
                let clamped = size.clamp(8, 72);
                self.settings.font_size = clamped;
                self.font_size_input = clamped.to_string();
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetFontSizeInput(input) => {
                self.font_size_input = input;
                Task::none()
            }
            IdeMessage::ApplyFontSizeInput => {
                if let Ok(parsed) = self.font_size_input.trim().parse::<u8>() {
                    let clamped = parsed.clamp(8, 72);
                    self.settings.font_size = clamped;
                    self.font_size_input = clamped.to_string();
                    self.settings.save();
                } else {
                    self.font_size_input = self.settings.font_size().to_string();
                }
                Task::none()
            }
            IdeMessage::SelectSyntaxColorTarget(target) => {
                self.active_color_target = target;
                let c = self.settings.get_target_color(target);
                self.syntax_hex_input = color_to_hex(c);
                Task::none()
            }
            IdeMessage::SetSyntaxColor(target, color) => {
                self.settings.set_target_color(target, color);
                self.syntax_hex_input = color_to_hex(color);
                self.selected_theme_name = "Custom".to_string();
                self.settings.current_theme = Some("Custom".to_string());
                self.settings.save();
                self.refresh_editor_highlighting();
                Task::none()
            }
            IdeMessage::SetSyntaxHexInput(hex) => {
                self.syntax_hex_input = hex.clone();
                if let Some(col) = hex_to_color(&hex) {
                    self.settings.set_target_color(self.active_color_target, col);
                    self.selected_theme_name = "Custom".to_string();
                    self.settings.current_theme = Some("Custom".to_string());
                    self.settings.save();
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::ApplySyntaxHexInput => {
                if let Some(col) = hex_to_color(&self.syntax_hex_input) {
                    self.settings.set_target_color(self.active_color_target, col);
                    self.syntax_hex_input = color_to_hex(col);
                    self.selected_theme_name = "Custom".to_string();
                    self.settings.current_theme = Some("Custom".to_string());
                    self.settings.save();
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::SetSyntaxRgbChannel(channel, val) => {
                let mut current = self.settings.get_target_color(self.active_color_target);
                let f = (val as f32) / 255.0;
                match channel {
                    0 => current.r = f,
                    1 => current.g = f,
                    2 => current.b = f,
                    _ => {}
                }
                self.settings.set_target_color(self.active_color_target, current);
                self.syntax_hex_input = color_to_hex(current);
                self.selected_theme_name = "Custom".to_string();
                self.settings.current_theme = Some("Custom".to_string());
                self.settings.save();
                self.refresh_editor_highlighting();
                Task::none()
            }
            IdeMessage::SetCustomFunctionColorMode(use_native) => {
                self.settings.set_custom_function_color_mode(use_native);
                let c = self.settings.get_target_color(self.active_color_target);
                self.syntax_hex_input = color_to_hex(c);
                self.selected_theme_name = "Custom".to_string();
                self.settings.current_theme = Some("Custom".to_string());
                self.settings.save();
                self.refresh_editor_highlighting();
                Task::none()
            }
            IdeMessage::ApplySyntaxPreset(preset) => {
                let (colors, name) = match preset {
                    SyntaxPreset::Base16Mocha => (crate::ide::highlight::SyntaxColors::base16_mocha(), "Base16 Mocha (Default)"),
                    SyntaxPreset::ClassicLight => (crate::ide::highlight::SyntaxColors::classic_light(), "Classic Light"),
                    SyntaxPreset::Monokai => (crate::ide::highlight::SyntaxColors::monokai(), "Monokai"),
                    SyntaxPreset::SolarizedDark => (crate::ide::highlight::SyntaxColors::solarized_dark(), "Solarized Dark"),
                };
                self.selected_theme_name = name.to_string();
                self.settings.syntax_colors = colors;
                self.settings.current_theme = Some(name.to_string());
                if self.settings.custom_function_use_native_color {
                    self.settings.syntax_colors.custom_function = None;
                }
                let c = self.settings.get_target_color(self.active_color_target);
                self.syntax_hex_input = color_to_hex(c);
                self.settings.save();
                self.refresh_editor_highlighting();
                Task::none()
            }
            IdeMessage::SelectTheme(name) => {
                self.selected_theme_name = name.clone();
                let colors_opt = match name.as_str() {
                    "Base16 Mocha (Default)" => Some(crate::ide::highlight::SyntaxColors::base16_mocha()),
                    "Classic Light" => Some(crate::ide::highlight::SyntaxColors::classic_light()),
                    "Monokai" => Some(crate::ide::highlight::SyntaxColors::monokai()),
                    "Solarized Dark" => Some(crate::ide::highlight::SyntaxColors::solarized_dark()),
                    "Custom" => None,
                    other => {
                        self.settings.user_themes.iter().find(|t| t.name == other).map(|t| t.colors.clone())
                    }
                };
                if let Some(colors) = colors_opt {
                    self.settings.syntax_colors = colors;
                    if self.settings.custom_function_use_native_color {
                        self.settings.syntax_colors.custom_function = None;
                    }
                    let c = self.settings.get_target_color(self.active_color_target);
                    self.syntax_hex_input = color_to_hex(c);
                    self.settings.current_theme = Some(name);
                    self.settings.save();
                    self.refresh_editor_highlighting();
                } else if name == "Custom" {
                    self.settings.current_theme = Some("Custom".to_string());
                    self.settings.save();
                }
                Task::none()
            }
            IdeMessage::SetNewThemeName(name) => {
                self.new_theme_name = name;
                Task::none()
            }
            IdeMessage::SaveCurrentTheme => {
                let saved_name = self.settings.save_user_theme(self.new_theme_name.clone(), self.settings.syntax_colors.clone());
                self.selected_theme_name = saved_name.clone();
                self.settings.current_theme = Some(saved_name);
                self.new_theme_name.clear();
                self.settings.save();
                Task::none()
            }
            IdeMessage::DeleteUserTheme(name) => {
                self.settings.delete_user_theme(&name);
                if self.selected_theme_name == name {
                    self.selected_theme_name = "Base16 Mocha (Default)".to_string();
                }
                self.settings.save();
                Task::none()
            }
            IdeMessage::SelectCustomFunction(opt) => {
                self.selected_custom_func = opt;
                Task::none()
            }
            IdeMessage::SetNewCustomFunctionInput(text) => {
                self.new_custom_func_input = text;
                Task::none()
            }
            IdeMessage::AddCustomFunction => {
                let name = std::mem::take(&mut self.new_custom_func_input);
                if self.settings.add_custom_function(name.clone()) {
                    self.selected_custom_func = Some(name.trim().to_string());
                    self.settings.save();
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::RemoveCustomFunction => {
                if let Some(name) = self.selected_custom_func.take() {
                    self.settings.remove_custom_function(&name);
                    self.settings.save();
                    self.refresh_editor_highlighting();
                }
                Task::none()
            }
            IdeMessage::ToggleShowTooltips(enabled) => {
                self.settings.show_function_tooltips = enabled;
                self.settings.save();
                Task::none()
            }
            IdeMessage::AddIgnoredTooltipFunction(name) => {
                self.context_menu = None;
                let trimmed = name.trim().to_string();
                if self.settings.add_ignored_tooltip_function(trimmed.clone()) {
                    self.selected_ignored_func = Some(trimmed);
                    self.settings.save();
                }
                Task::none()
            }
            IdeMessage::RemoveIgnoredTooltipFunction(name) => {
                self.context_menu = None;
                let trimmed = name.trim().to_string();
                if self.settings.remove_ignored_tooltip_function(&trimmed) {
                    if self.selected_ignored_func.as_deref() == Some(trimmed.as_str()) {
                        self.selected_ignored_func = None;
                    }
                    self.settings.save();
                }
                Task::none()
            }
            IdeMessage::SelectIgnoredFunction(func) => {
                self.selected_ignored_func = func;
                Task::none()
            }
            IdeMessage::SetNewIgnoredFunctionInput(text) => {
                self.new_ignored_func_input = text;
                Task::none()
            }
            IdeMessage::AddIgnoredFunction => {
                let name = std::mem::take(&mut self.new_ignored_func_input);
                if self.settings.add_ignored_tooltip_function(name.clone()) {
                    self.selected_ignored_func = Some(name.trim().to_string());
                    self.settings.save();
                }
                Task::none()
            }
            IdeMessage::RemoveIgnoredFunction => {
                if let Some(name) = self.selected_ignored_func.take() {
                    self.settings.remove_ignored_tooltip_function(&name);
                    self.settings.save();
                }
                Task::none()
            }
            IdeMessage::ClearIgnoredFunctions => {
                self.settings.ignored_tooltip_functions.clear();
                self.settings.rebuild_ignored_tooltip_functions_set();
                self.selected_ignored_func = None;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ResetEditorSyntaxDefaults => {
                self.settings.font_name = None;
                self.settings.font_size = 13;
                self.font_size_input = "13".to_string();
                self.settings.syntax_colors = crate::ide::highlight::SyntaxColors::base16_mocha();
                self.settings.custom_function_use_native_color = true;
                self.settings.syntax_colors.custom_function = None;
                self.selected_theme_name = "Base16 Mocha (Default)".to_string();
                self.settings.current_theme = Some("Base16 Mocha (Default)".to_string());
                let c = self.settings.get_target_color(self.active_color_target);
                self.syntax_hex_input = color_to_hex(c);
                self.settings.save();
                self.refresh_editor_highlighting();
                Task::none()
            }
            IdeMessage::SetCustomFunctions(funcs) => {
                self.settings.update_custom_functions(funcs);
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetBreakpointColor(c) => {
                self.settings.breakpoint_color = c;
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetExecutionColor(c) => {
                self.settings.execution_color = c;
                self.settings.save();
                Task::none()
            }
            IdeMessage::SetTransparencyPercent(p) => {
                let clamped = p.clamp(0, 90);
                self.settings.transparency_percent = clamped;
                self.transparency_input = clamped.to_string();
                self.settings.save();
                if self.is_transparent {
                    #[cfg(windows)]
                    apply_window_transparency(true, clamped);
                }
                Task::none()
            }
            IdeMessage::SetTransparencyInput(input) => {
                self.transparency_input = input;
                Task::none()
            }
            IdeMessage::ApplyTransparencyInput => {
                if let Ok(parsed) = self.transparency_input.trim().trim_end_matches('%').parse::<u8>() {
                    let clamped = parsed.clamp(0, 90);
                    self.update(IdeMessage::SetTransparencyPercent(clamped))
                } else {
                    self.transparency_input = self.settings.transparency_percent.to_string();
                    Task::none()
                }
            }
            IdeMessage::SetAutoTransparency(b) => {
                self.settings.auto_transparency = b;
                self.settings.save();
                Task::none()
            }
            IdeMessage::BrowseScriptsDir => {
                let start_dir = self.settings.resolved_scripts_dir();
                Task::perform(
                    async move {
                        let folder = rfd::AsyncFileDialog::new()
                            .set_directory(&start_dir)
                            .pick_folder()
                            .await;
                        folder.map(|f| f.path().to_path_buf())
                    },
                    |opt| IdeMessage::SetScriptsDir(opt),
                )
            }
            IdeMessage::SetScriptsDir(opt) => {
                self.settings.scripts_dir = opt;
                self.settings.save();
                Task::none()
            }
            IdeMessage::ToggleTransparency => {
                self.is_transparent = !self.is_transparent;
                #[cfg(windows)]
                apply_window_transparency(self.is_transparent, self.settings.transparency_percent);
                Task::none()
            }
            IdeMessage::WheelScrolled(dy) => {
                let lines = (-dy / 35.0).round() as i32;
                if lines != 0 {
                    return self.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines }));
                }
                Task::none()
            }
            IdeMessage::WindowResized(size) => {
                self.window_width = size.width;
                self.window_height = size.height;
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let total = tab.content.line_count().max(1);
                    let max_scroll = total.saturating_sub(visible_lines);
                    if tab.scroll_top_line > max_scroll {
                        let cursor = tab.content.cursor();
                        tab.scroll_viewport_exact(
                            max_scroll,
                            cursor,
                            visible_lines,
                            !tab.is_scrolling_away,
                        );
                    }
                }
                Task::none()
            }
            IdeMessage::MouseMoved(pos) => {
                self.last_mouse_pos = pos;
                Task::none()
            }
            IdeMessage::CloseContextMenu => {
                let had_menu = self.context_menu.take().is_some();
                if !had_menu && self.find_replace.is_open {
                    self.find_replace.is_open = false;
                    return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
                }
                Task::none()
            }
            IdeMessage::AutocompleteNext => {
                if !self.autocomplete_matches.is_empty() {
                    self.autocomplete_selected_index = (self.autocomplete_selected_index + 1) % self.autocomplete_matches.len();
                }
                Task::none()
            }
            IdeMessage::AutocompletePrev => {
                if !self.autocomplete_matches.is_empty() {
                    self.autocomplete_selected_index = if self.autocomplete_selected_index == 0 {
                        self.autocomplete_matches.len() - 1
                    } else {
                        self.autocomplete_selected_index - 1
                    };
                }
                Task::none()
            }
            IdeMessage::AutocompleteDismiss => {
                self.autocomplete_dismissed = true;
                self.autocomplete_matches.clear();
                Task::none()
            }
            IdeMessage::AutocompleteAccept => {
                if !self.autocomplete_matches.is_empty() {
                    let idx = self.autocomplete_selected_index.min(self.autocomplete_matches.len() - 1);
                    if let Some(func_name) = self.autocomplete_matches.get(idx).cloned() {
                        return self.update(IdeMessage::AutocompleteSelect(func_name));
                    }
                }
                Task::none()
            }
            IdeMessage::AutocompleteSelect(func_name) => {
                if let Some((start_col, end_col, _)) = self.current_autocomplete_matches() {
                    let replacement = format!("{func_name} ");
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        let cursor = tab.content.cursor();
                        tab.content.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: cursor.position.line,
                                column: end_col,
                            },
                            selection: Some(text_editor::Position {
                                line: cursor.position.line,
                                column: start_col,
                            }),
                        });
                        tab.content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            std::sync::Arc::new(replacement),
                        )));
                        tab.dirty = true;
                        tab.last_edit_cursor = Some(tab.content.cursor());
                    }
                }
                self.autocomplete_dismissed = true;
                self.autocomplete_matches.clear();
                self.autocomplete_selected_index = 0;
                Task::none()
            }
            IdeMessage::EditorRightClicked(mut pos) => {
                if pos == Point::ORIGIN {
                    pos = self.last_mouse_pos;
                }
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let (target_ident, target_func, target_bp, cursor_line, has_selection) =
                        Self::resolve_context_menu_target(tab, pos, &self.settings);

                    self.context_menu = Some(ContextMenuState {
                        position: pos,
                        target_func,
                        target_ident,
                        target_bp,
                        cursor_line,
                        has_selection,
                    });
                }
                Task::none()
            }
            IdeMessage::FocusCad => {
                force_foreground_cad();
                Task::none()
            }
            IdeMessage::SplitterGrab => {
                self.is_splitter_dragging = true;
                self.splitter_last_y = None;
                Task::none()
            }
            IdeMessage::SplitterMove(point) => {
                if self.is_splitter_dragging {
                    if let Some(last_y) = self.splitter_last_y {
                        let dy = point.y - last_y;
                        self.console_height = (self.console_height - dy).clamp(70.0, 750.0);
                    }
                    self.splitter_last_y = Some(point.y);
                }
                Task::none()
            }
            IdeMessage::SplitterRelease => {
                self.is_splitter_dragging = false;
                self.splitter_last_y = None;
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let total = tab.content.line_count().max(1);
                    let max_scroll = total.saturating_sub(visible_lines);
                    if tab.scroll_top_line > max_scroll {
                        let cursor = tab.content.cursor();
                        tab.scroll_viewport_exact(
                            max_scroll,
                            cursor,
                            visible_lines,
                            !tab.is_scrolling_away,
                        );
                    }
                }
                Task::none()
            }
            IdeMessage::SplitterReset => {
                self.console_height = 220.0;
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let total = tab.content.line_count().max(1);
                    let max_scroll = total.saturating_sub(visible_lines);
                    if tab.scroll_top_line > max_scroll {
                        let cursor = tab.content.cursor();
                        tab.scroll_viewport_exact(
                            max_scroll,
                            cursor,
                            visible_lines,
                            !tab.is_scrolling_away,
                        );
                    }
                }
                Task::none()
            }
            IdeMessage::DebugStart => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    self.is_debugging = true;
                    self.client.send(IdeToPlugin::DebugScript {
                        title: tab.title.clone(),
                        code: tab.content.text(),
                        breakpoints: bps,
                        step_into: false,
                    });
                }
                Task::none()
            }
            IdeMessage::DebugStartStep => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    self.is_debugging = true;
                    self.client.send(IdeToPlugin::DebugScript {
                        title: tab.title.clone(),
                        code: tab.content.text(),
                        breakpoints: bps,
                        step_into: true,
                    });
                }
                Task::none()
            }
            IdeMessage::DebugContinue => {
                self.client.send(IdeToPlugin::DebugCmd(DebugCommand::Continue));
                Task::none()
            }
            IdeMessage::DebugStepOver => {
                self.client.send(IdeToPlugin::DebugCmd(DebugCommand::StepOver));
                Task::none()
            }
            IdeMessage::DebugStepInto => {
                self.client.send(IdeToPlugin::DebugCmd(DebugCommand::StepInto));
                Task::none()
            }
            IdeMessage::DebugStepOut => {
                self.client.send(IdeToPlugin::DebugCmd(DebugCommand::StepOut));
                Task::none()
            }
            IdeMessage::DebugStop => {
                self.client.send(IdeToPlugin::DebugCmd(DebugCommand::Stop));
                self.is_debugging = false;
                for tab in &mut self.tabs {
                    tab.debug_paused_line = None;
                    tab.debug_paused_span = None;
                    tab.debug_paused_expr = None;
                }
                Task::none()
            }
            IdeMessage::DebugRestart => {
                let to_restart = if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if let Some(path) = &tab.path {
                        let _ = std::fs::write(path, tab.content.text());
                        tab.dirty = false;
                    }
                    tab.debug_paused_line = None;
                    tab.debug_paused_span = None;
                    tab.debug_paused_expr = None;
                    let code = tab.content.text();
                    let title = tab.title.clone();
                    let bps: Vec<crate::ide::protocol::ParenBreakpoint> =
                        tab.breakpoints.iter().copied().collect();
                    Some((code, title, bps))
                } else {
                    None
                };

                if let Some((code, title, bps)) = to_restart {
                    self.append_to_console(&format!("\n--- Restarting Script: {title} (Ctrl+Shift+F5) ---\n"));

                    self.is_debugging = true;
                    self.variables_open = false;
                    self.debug_locals.clear();
                    self.debug_stack.clear();
                    self.client.send(IdeToPlugin::RestartScript {
                        title,
                        code,
                        breakpoints: bps,
                    });
                }
                Task::none()
            }
            IdeMessage::F5Pressed => {
                if self.is_debugging {
                    self.update(IdeMessage::DebugContinue)
                } else {
                    self.update(IdeMessage::RunAll)
                }
            }
            IdeMessage::Undo => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.undo() {
                        let cursor = tab.content.cursor();
                        let total = tab.content.line_count().max(1);
                        let max_scroll = total.saturating_sub(visible_lines.max(1));
                        let target_scroll = if cursor.position.line < tab.scroll_top_line {
                            cursor.position.line
                        } else if cursor.position.line >= tab.scroll_top_line + visible_lines {
                            cursor.position.line.saturating_sub(visible_lines / 2)
                        } else {
                            tab.scroll_top_line
                        }
                        .min(max_scroll);
                        tab.scroll_viewport_exact(target_scroll, cursor, visible_lines, true);
                        tab.is_scrolling_away = false;
                        self.refresh_find_matches();
                        self.refresh_autocomplete_matches();
                    }
                }
                Task::none()
            }
            IdeMessage::Redo => {
                let visible_lines = self.visible_lines();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.redo() {
                        let cursor = tab.content.cursor();
                        let total = tab.content.line_count().max(1);
                        let max_scroll = total.saturating_sub(visible_lines.max(1));
                        let target_scroll = if cursor.position.line < tab.scroll_top_line {
                            cursor.position.line
                        } else if cursor.position.line >= tab.scroll_top_line + visible_lines {
                            cursor.position.line.saturating_sub(visible_lines / 2)
                        } else {
                            tab.scroll_top_line
                        }
                        .min(max_scroll);
                        tab.scroll_viewport_exact(target_scroll, cursor, visible_lines, true);
                        tab.is_scrolling_away = false;
                        self.refresh_find_matches();
                        self.refresh_autocomplete_matches();
                    }
                }
                Task::none()
            }
            IdeMessage::WindowCloseRequested => {
                let dirty_files: Vec<(usize, String)> = self.tabs.iter().enumerate()
                    .filter(|(_, t)| t.dirty)
                    .map(|(i, t)| (i, t.title.clone()))
                    .collect();
                if dirty_files.is_empty() {
                    iced::exit()
                } else {
                    self.save_prompt = Some(crate::ide::SavePromptState {
                        trigger: crate::ide::SavePromptTrigger::CloseWindow,
                        dirty_files,
                    });
                    Task::none()
                }
            }
            IdeMessage::ConfirmSaveAndProceed => {
                if let Some(state) = self.save_prompt.take() {
                    let mut cancel = false;
                    for &(tab_idx, _) in &state.dirty_files {
                        if let Some(tab) = self.tabs.get_mut(tab_idx) {
                            if let Some(path) = &tab.path {
                                let code = tab.content.text();
                                if std::fs::write(path, &code).is_ok() {
                                    tab.dirty = false;
                                }
                            } else {
                                let start_dir = self.settings.resolved_scripts_dir();
                                let file_name = if tab.title.ends_with(".lsp") || tab.title.ends_with(".lisp") {
                                    tab.title.clone()
                                } else {
                                    format!("{}.lsp", tab.title)
                                };
                                let picked = rfd::FileDialog::new()
                                    .set_directory(&start_dir)
                                    .set_file_name(&file_name)
                                    .add_filter("OpenCAD LISP Routine (*.lsp, *.lisp)", &["lsp", "lisp"])
                                    .add_filter("All Files (*.*)", &["*"])
                                    .save_file();
                                if let Some(path) = picked {
                                    let code = tab.content.text();
                                    if std::fs::write(&path, &code).is_ok() {
                                        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or(&tab.title).to_string();
                                        tab.title = fname;
                                        tab.path = Some(path);
                                        tab.dirty = false;
                                    }
                                } else {
                                    cancel = true;
                                    break;
                                }
                            }
                        }
                    }
                    if cancel {
                        self.save_prompt = Some(state);
                        return Task::none();
                    }
                    match state.trigger {
                        crate::ide::SavePromptTrigger::CloseWindow | crate::ide::SavePromptTrigger::HostClosing => {
                            return iced::exit();
                        }
                        crate::ide::SavePromptTrigger::CloseTab(idx) => {
                            if idx < self.tabs.len() {
                                self.tabs.remove(idx);
                                if self.tabs.is_empty() {
                                    self.tabs.push(IdeTab::new_empty("untitled.lsp".to_string()));
                                }
                                if self.active_tab >= self.tabs.len() {
                                    self.active_tab = self.tabs.len() - 1;
                                }
                            }
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::DiscardAndProceed => {
                if let Some(state) = self.save_prompt.take() {
                    match state.trigger {
                        crate::ide::SavePromptTrigger::CloseWindow | crate::ide::SavePromptTrigger::HostClosing => {
                            return iced::exit();
                        }
                        crate::ide::SavePromptTrigger::CloseTab(idx) => {
                            if idx < self.tabs.len() {
                                self.tabs.remove(idx);
                                if self.tabs.is_empty() {
                                    self.tabs.push(IdeTab::new_empty("untitled.lsp".to_string()));
                                }
                                if self.active_tab >= self.tabs.len() {
                                    self.active_tab = self.tabs.len() - 1;
                                }
                            }
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::CancelSavePrompt => {
                self.save_prompt = None;
                Task::none()
            }
            IdeMessage::OpenFind => {
                self.find_replace.is_open = true;
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    if let Some(sel) = tab.content.selection() {
                        let trimmed = sel.trim();
                        if !trimmed.is_empty() && !trimmed.contains('\n') {
                            self.find_replace.find_query = trimmed.to_string();
                        }
                    } else if self.find_replace.find_query.is_empty() {
                        let cursor = tab.content.cursor();
                        if let Some(ident) = identifier_at_cursor(&tab.content.text(), cursor.position.line, cursor.position.column) {
                            self.find_replace.find_query = ident;
                        }
                    }
                }
                self.refresh_find_matches();
                if !self.find_replace.matches.is_empty() {
                    let cursor_line = self.tabs.get(self.active_tab).map_or(0, |t| t.content.cursor().position.line);
                    let target_idx = self.find_replace.matches
                        .iter()
                        .position(|m| m.line >= cursor_line)
                        .unwrap_or(0);
                    self.navigate_to_search_match(target_idx);
                }
                return iced::widget::operation::focus(crate::ide::view::FIND_INPUT_ID);
            }
            IdeMessage::OpenReplace => {
                self.find_replace.is_open = true;
                self.find_replace.show_replace = true;
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    if let Some(sel) = tab.content.selection() {
                        let trimmed = sel.trim();
                        if !trimmed.is_empty() && !trimmed.contains('\n') {
                            self.find_replace.find_query = trimmed.to_string();
                        }
                    } else if self.find_replace.find_query.is_empty() {
                        let cursor = tab.content.cursor();
                        if let Some(ident) = identifier_at_cursor(&tab.content.text(), cursor.position.line, cursor.position.column) {
                            self.find_replace.find_query = ident;
                        }
                    }
                }
                self.refresh_find_matches();
                if !self.find_replace.matches.is_empty() {
                    let cursor_line = self.tabs.get(self.active_tab).map_or(0, |t| t.content.cursor().position.line);
                    let target_idx = self.find_replace.matches
                        .iter()
                        .position(|m| m.line >= cursor_line)
                        .unwrap_or(0);
                    self.navigate_to_search_match(target_idx);
                }
                if self.find_replace.find_query.is_empty() {
                    return iced::widget::operation::focus(crate::ide::view::FIND_INPUT_ID);
                } else {
                    return iced::widget::operation::focus(crate::ide::view::REPLACE_INPUT_ID);
                }
            }
            IdeMessage::CloseFind => {
                self.find_replace.is_open = false;
                return iced::widget::operation::focus(crate::ide::view::EDITOR_ID);
            }
            IdeMessage::FindQueryChanged(q) => {
                self.find_replace.find_query = q;
                self.refresh_find_matches();
                if !self.find_replace.matches.is_empty() {
                    let cursor_line = self.tabs.get(self.active_tab).map_or(0, |t| t.content.cursor().position.line);
                    let target_idx = self.find_replace.matches
                        .iter()
                        .position(|m| m.line >= cursor_line)
                        .unwrap_or(0);
                    self.navigate_to_search_match(target_idx);
                }
                Task::none()
            }
            IdeMessage::ReplaceQueryChanged(q) => {
                self.find_replace.replace_query = q;
                Task::none()
            }
            IdeMessage::ToggleFindMatchCase => {
                self.find_replace.match_case = !self.find_replace.match_case;
                self.refresh_find_matches();
                if !self.find_replace.matches.is_empty() {
                    self.navigate_to_search_match(0);
                }
                Task::none()
            }
            IdeMessage::ToggleFindWholeWord => {
                self.find_replace.match_whole_word = !self.find_replace.match_whole_word;
                self.refresh_find_matches();
                if !self.find_replace.matches.is_empty() {
                    self.navigate_to_search_match(0);
                }
                Task::none()
            }
            IdeMessage::ToggleFindShowReplace => {
                self.find_replace.show_replace = !self.find_replace.show_replace;
                if self.find_replace.show_replace {
                    return iced::widget::operation::focus(crate::ide::view::REPLACE_INPUT_ID);
                }
                Task::none()
            }
            IdeMessage::FindNext => {
                if !self.find_replace.matches.is_empty() {
                    let next = match self.find_replace.current_match_idx {
                        Some(i) => (i + 1) % self.find_replace.matches.len(),
                        None => 0,
                    };
                    self.navigate_to_search_match(next);
                }
                Task::none()
            }
            IdeMessage::FindPrev => {
                if !self.find_replace.matches.is_empty() {
                    let len = self.find_replace.matches.len();
                    let prev = match self.find_replace.current_match_idx {
                        Some(0) | None => len - 1,
                        Some(i) => i - 1,
                    };
                    self.navigate_to_search_match(prev);
                }
                Task::none()
            }
            IdeMessage::ReplaceCurrent => {
                let mut next_target_pos = None;
                if let Some(curr_idx) = self.find_replace.current_match_idx {
                    if let Some(m) = self.find_replace.matches.get(curr_idx).cloned() {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            let text = tab.content.text();
                            let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
                            if m.line < lines.len() {
                                let line_str = &lines[m.line];
                                if m.col + m.len <= line_str.len() {
                                    tab.push_undo_snapshot();
                                    let mut new_line = String::new();
                                    new_line.push_str(&line_str[..m.col]);
                                    new_line.push_str(&self.find_replace.replace_query);
                                    new_line.push_str(&line_str[m.col + m.len..]);
                                    lines[m.line] = new_line;
                                    let new_text = lines.join("\n");
                                    let saved_scroll = tab.scroll_top_line;
                                    tab.content = text_editor::Content::with_text(&new_text);
                                    tab.dirty = true;
                                    tab.scroll_top_line = saved_scroll;

                                    let after_replace_col = m.col + self.find_replace.replace_query.len();
                                    next_target_pos = Some((m.line, after_replace_col));
                                }
                            }
                        }
                        self.refresh_find_matches();
                        if !self.find_replace.matches.is_empty() {
                            // Find the first match that is strictly at or after the replacement point
                            // to prevent infinite loops if replace_query contains find_query!
                            let next_idx = if let Some((rep_line, rep_col)) = next_target_pos {
                                self.find_replace.matches
                                    .iter()
                                    .position(|candidate| {
                                        candidate.line > rep_line
                                            || (candidate.line == rep_line && candidate.col >= rep_col)
                                    })
                                    .unwrap_or(0)
                            } else {
                                curr_idx.min(self.find_replace.matches.len() - 1)
                            };
                            self.navigate_to_search_match(next_idx);
                        } else if let Some((rep_line, rep_col)) = next_target_pos {
                            // If no further matches exist, position cursor right after replacement
                            // and maintain current scroll position instead of jumping to line 0!
                            let visible_lines = self.visible_lines();
                            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                                let scroll_top = tab.scroll_top_line;
                                tab.scroll_to_line_exact(scroll_top, rep_line, rep_col, visible_lines);
                            }
                        }
                    }
                }
                Task::none()
            }
            IdeMessage::ReplaceAll => {
                if !self.find_replace.matches.is_empty() {
                    let visible_lines = self.visible_lines();
                    let mut last_rep_pos = None;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.push_undo_snapshot();
                        let text = tab.content.text();
                        let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
                        let saved_scroll = tab.scroll_top_line;
                        for m in self.find_replace.matches.iter().rev() {
                            if m.line < lines.len() {
                                let line_str = &lines[m.line];
                                if m.col + m.len <= line_str.len() {
                                    let mut new_line = String::new();
                                    new_line.push_str(&line_str[..m.col]);
                                    new_line.push_str(&self.find_replace.replace_query);
                                    new_line.push_str(&line_str[m.col + m.len..]);
                                    lines[m.line] = new_line;
                                    last_rep_pos = Some((m.line, m.col + self.find_replace.replace_query.len()));
                                }
                            }
                        }
                        let new_text = lines.join("\n");
                        tab.content = text_editor::Content::with_text(&new_text);
                        tab.dirty = true;
                        tab.scroll_top_line = saved_scroll;
                        if let Some((line, col)) = last_rep_pos {
                            tab.scroll_to_line_exact(saved_scroll, line, col, visible_lines);
                        }
                    }
                    self.refresh_find_matches();
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, IdeMessage> {
        let matches_ref = if self.autocomplete_dismissed || self.autocomplete_matches.is_empty() {
            None
        } else {
            Some(self.autocomplete_matches.as_slice())
        };

        view_standalone_window(
            &self.tabs,
            self.active_tab,
            &self.console_log,
            &self.repl_content,
            self.repl_mode,
            self.api_drawer_open,
            &self.api_search,
            &self.theme,
            self.is_debugging,
            self.variables_open,
            &self.debug_locals,
            &self.debug_stack,
            self.is_transparent,
            &self.watch_vars,
            self.console_height,
            self.is_splitter_dragging,
            self.visible_lines(),
            &self.settings,
            self.settings_open,
            self.help_menu_open,
            self.settings_tab,
            self.active_color_target,
            &self.syntax_hex_input,
            &self.font_size_input,
            &self.transparency_input,
            &self.selected_theme_name,
            &self.new_theme_name,
            self.selected_custom_func.as_deref(),
            &self.new_custom_func_input,
            matches_ref,
            self.autocomplete_selected_index,
            self.context_menu.as_ref(),
            self.save_prompt.as_ref(),
            &self.find_replace,
            self.window_width,
            self.window_height,
        )
    }

    pub fn subscription(&self) -> iced::Subscription<IdeMessage> {
        let timer = iced::time::every(std::time::Duration::from_millis(100)).map(|_| IdeMessage::Tick);

        let events = iced::event::listen_with(|ev, status, _win_id| {
            match ev {
                iced::Event::Window(iced::window::Event::CloseRequested) => {
                    Some(IdeMessage::WindowCloseRequested)
                }
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(IdeMessage::WindowResized(size))
                }
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, physical_key, .. }) => {
                    let is_f = match key.as_ref() {
                        keyboard::Key::Character(c) => c.eq_ignore_ascii_case("f"),
                        _ => matches!(physical_key, keyboard::key::Physical::Code(keyboard::key::Code::KeyF)),
                    };
                    let is_cmd = modifiers.control() || modifiers.command();
                    if is_f {
                        if modifiers.shift() && modifiers.alt() {
                            return Some(IdeMessage::FormatDocument);
                        }
                        if (modifiers.shift() && is_cmd) || (modifiers.alt() && is_cmd) {
                            return Some(IdeMessage::FormatSelection);
                        }
                        if is_cmd {
                            return Some(IdeMessage::OpenFind);
                        }
                    }

                    let is_h = match key.as_ref() {
                        keyboard::Key::Character(c) => c.eq_ignore_ascii_case("h"),
                        _ => matches!(physical_key, keyboard::key::Physical::Code(keyboard::key::Code::KeyH)),
                    };
                    if is_h && is_cmd {
                        return Some(IdeMessage::OpenReplace);
                    }

                    match key.as_ref() {
                        keyboard::Key::Named(keyboard::key::Named::F2) => {
                            Some(IdeMessage::ToggleApiDrawer)
                        }
                        keyboard::Key::Named(keyboard::key::Named::F3) => {
                            if modifiers.shift() {
                                Some(IdeMessage::FindPrev)
                            } else {
                                Some(IdeMessage::FindNext)
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            Some(IdeMessage::CloseContextMenu)
                        }
                        keyboard::Key::Named(keyboard::key::Named::F5) => {
                            if modifiers.shift() && (modifiers.control() || modifiers.command()) {
                                Some(IdeMessage::DebugRestart)
                            } else if modifiers.shift() {
                                Some(IdeMessage::DebugStop)
                            } else {
                                Some(IdeMessage::F5Pressed)
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::F8) => {
                            Some(IdeMessage::RunSelection)
                        }
                        keyboard::Key::Named(keyboard::key::Named::F9) => {
                            if modifiers.shift() {
                                Some(IdeMessage::AddWatchAtCursor)
                            } else {
                                None
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::F10) => {
                            Some(IdeMessage::DebugStepOver)
                        }
                        keyboard::Key::Named(keyboard::key::Named::F11) => {
                            if modifiers.shift() {
                                Some(IdeMessage::DebugStepOut)
                            } else {
                                Some(IdeMessage::DebugStepInto)
                            }
                        }
                        keyboard::Key::Character("n") if is_cmd => {
                            Some(IdeMessage::NewTab)
                        }
                        keyboard::Key::Character("o") if is_cmd => {
                            Some(IdeMessage::OpenFile)
                        }
                        keyboard::Key::Character("s") if is_cmd => {
                            if modifiers.shift() {
                                Some(IdeMessage::SaveTabAs)
                            } else {
                                Some(IdeMessage::SaveTab)
                            }
                        }
                        keyboard::Key::Character("w") if is_cmd => {
                            Some(IdeMessage::CloseActiveTab)
                        }
                        keyboard::Key::Character("z") if is_cmd => {
                            if modifiers.shift() {
                                Some(IdeMessage::Redo)
                            } else {
                                Some(IdeMessage::Undo)
                            }
                        }
                        keyboard::Key::Character("y") if is_cmd => {
                            Some(IdeMessage::Redo)
                        }
                        _ => None,
                    }
                }
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(IdeMessage::MouseMoved(position))
                }
                iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) if status == iced::event::Status::Ignored => {
                    let dy = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => y * 35.0,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => y,
                    };
                    Some(IdeMessage::WheelScrolled(dy))
                }
                _ => None,
            }
        });

        iced::Subscription::batch([timer, events])
    }
}

#[cfg(windows)]
pub fn apply_window_transparency(transparent: bool, percent: u8) {
    unsafe {
        extern "system" {
            fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32, lParam: isize) -> i32;
            fn GetWindowTextW(hWnd: *mut std::ffi::c_void, lpString: *mut u16, nMaxCount: i32) -> i32;
            fn GetWindowLongPtrW(hWnd: *mut std::ffi::c_void, nIndex: i32) -> isize;
            fn SetWindowLongPtrW(hWnd: *mut std::ffi::c_void, nIndex: i32, dwNewLong: isize) -> isize;
            fn SetLayeredWindowAttributes(hWnd: *mut std::ffi::c_void, crKey: u32, bAlpha: u8, dwFlags: u32) -> i32;
        }

        const GWL_EXSTYLE: i32 = -20;
        const WS_EX_LAYERED: isize = 0x00080000;
        const LWA_ALPHA: u32 = 0x00000002;

        struct TargetData {
            transparent: bool,
            percent: u8,
        }

        let data = TargetData { transparent, percent };

        unsafe extern "system" fn enum_proc(hwnd: *mut std::ffi::c_void, lparam: isize) -> i32 {
            let data = &*(lparam as *const TargetData);
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                if title.contains("(OCIDE)") {
                    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                    if data.transparent {
                        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED);
                        let opacity_pct = (100u8.saturating_sub(data.percent)).max(15);
                        let alpha = ((opacity_pct as f32 / 100.0) * 255.0).round() as u8;
                        SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
                    } else {
                        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !WS_EX_LAYERED);
                    }
                    return 0;
                }
            }
            1
        }

        EnumWindows(enum_proc, &data as *const _ as isize);
    }
}

pub use crate::ide::server::{force_foreground_cad, force_foreground_ide, force_foreground_ocide};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_history_navigation_and_draft_preservation() {
        let (mut app, _) = IdeApp::new();

        // 1. Submit first command
        app.repl_content = text_editor::Content::with_text("(+ 1 2)");
        let _ = app.update(IdeMessage::ReplSubmit);
        assert_eq!(app.repl_history, vec!["(+ 1 2)"]);
        assert_eq!(app.repl_content.text(), "");

        // 2. Submit second command
        app.repl_content = text_editor::Content::with_text("(defun foo (x) (* x 2))");
        let _ = app.update(IdeMessage::ReplSubmit);
        assert_eq!(app.repl_history, vec!["(+ 1 2)", "(defun foo (x) (* x 2))"]);
        assert_eq!(app.repl_content.text(), "");

        // 3. User types an unfinished draft in REPL
        app.repl_content = text_editor::Content::with_text("(my-unfin");

        // 4. Press Up Arrow (ReplHistoryPrev) -> stashes draft, shows most recent command
        let _ = app.update(IdeMessage::ReplHistoryPrev);
        assert_eq!(app.repl_draft, "(my-unfin");
        assert_eq!(app.repl_content.text(), "(defun foo (x) (* x 2))");
        assert_eq!(app.repl_history_index, Some(1));

        // 5. Press Up Arrow again -> shows older command
        let _ = app.update(IdeMessage::ReplHistoryPrev);
        assert_eq!(app.repl_content.text(), "(+ 1 2)");
        assert_eq!(app.repl_history_index, Some(0));

        // 6. Press Up Arrow again -> clamps at oldest command
        let _ = app.update(IdeMessage::ReplHistoryPrev);
        assert_eq!(app.repl_content.text(), "(+ 1 2)");
        assert_eq!(app.repl_history_index, Some(0));

        // 7. Press Down Arrow (ReplHistoryNext) -> moves forward to newer command
        let _ = app.update(IdeMessage::ReplHistoryNext);
        assert_eq!(app.repl_content.text(), "(defun foo (x) (* x 2))");
        assert_eq!(app.repl_history_index, Some(1));

        // 8. Press Down Arrow past newest command -> restores the unfinished draft!
        let _ = app.update(IdeMessage::ReplHistoryNext);
        assert_eq!(app.repl_content.text(), "(my-unfin");
        assert_eq!(app.repl_history_index, None);
    }

    #[test]
    fn test_repl_and_console_paren_selection() {
        let (mut app, _) = IdeApp::new();

        // 1. REPL paren double click selection
        app.repl_content = text_editor::Content::with_text("(setq a (+ 1 2))");
        // Place cursor at the inner '(' of (+ 1 2) which is line 0 col 8
        app.repl_content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 0, column: 8 },
            selection: None,
        });
        let selected = IdeApp::select_word_matching_paren(&mut app.repl_content);
        assert!(selected);
        let sel = app.repl_content.selection();
        assert!(sel.is_some());
        assert_eq!(sel.unwrap(), "(+ 1 2)");

        // 2. Console paren double click selection
        app.console_log = text_editor::Content::with_text("LISP> (princ \"hello\")\nLISP> (list 10 20 30)\n");
        // Place cursor at '(' of (list 10 20 30) on line 1 col 6
        app.console_log.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 1, column: 6 },
            selection: None,
        });
        let console_selected = IdeApp::select_word_matching_paren(&mut app.console_log);
        assert!(console_selected);
        assert_eq!(app.console_log.selection().unwrap(), "(list 10 20 30)");

        // 3. Jump to matching paren via jump_target_paren
        app.repl_content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 0, column: 8 },
            selection: None,
        });
        let jumped = IdeApp::jump_target_paren(&mut app.repl_content, true);
        assert!(jumped);
        assert_eq!(app.repl_content.cursor().position, text_editor::Position { line: 0, column: 15 });
    }

    #[test]
    fn test_repl_smart_indent_on_newline() {
        let (mut app, _) = IdeApp::new();
        // Type defun with open parenthesis
        app.repl_content = text_editor::Content::with_text("(defun c:test ()");
        app.repl_content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));

        // Insert newline (Ctrl+Enter)
        let _ = app.update(IdeMessage::ReplInsertNewline);
        // Next line should be indented 2 spaces
        assert_eq!(app.repl_content.text(), "(defun c:test ()\n  ");
    }

    #[test]
    fn test_format_selection() {
        let (mut app, _) = IdeApp::new();
        let unformatted = "(defun foo ()\n(setq a 1)\n(setq b 2)\n)";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(unformatted);
            // Select lines 1 to 2
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 2, column: 10 },
                selection: Some(text_editor::Position { line: 1, column: 0 }),
            });
        }
        let _ = app.update(IdeMessage::FormatSelection);
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.content.text(), "(defun foo ()\n  (setq a 1)\n  (setq b 2)\n)");
    }

    #[test]
    fn test_format_document_and_selection_recover_focus_from_repl() {
        let (mut app, _) = IdeApp::new();
        app.active_editor_focus = ActiveEditorFocus::Repl;

        let unformatted = "(defun foo ()\n(setq a 1)\n(setq b 2)\n)";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(unformatted);
            // Select ONLY line 1: `(setq a 1)`
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 10 },
                selection: Some(text_editor::Position { line: 1, column: 0 }),
            });
        }

        // Even if active_editor_focus was Repl, tab selection causes FormatSelection to target tab!
        let _ = app.update(IdeMessage::FormatSelection);
        assert_eq!(app.active_editor_focus, ActiveEditorFocus::Main);
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.content.text(), "(defun foo ()\n  (setq a 1)\n(setq b 2)\n)");

        // Now test FormatDocument recovers focus and formats entire document
        app.active_editor_focus = ActiveEditorFocus::Repl;
        let _ = app.update(IdeMessage::FormatDocument);
        assert_eq!(app.active_editor_focus, ActiveEditorFocus::Main);
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.content.text(), "(defun foo ()\n  (setq a 1)\n  (setq b 2)\n)");
    }

    #[test]
    fn test_format_selection_in_repl() {
        let (mut app, _) = IdeApp::new();
        app.active_editor_focus = ActiveEditorFocus::Repl;
        app.repl_content = text_editor::Content::with_text("(progn\n(princ \"hi\")\n)");
        // Select line 1: `(princ "hi")`
        app.repl_content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 1, column: 12 },
            selection: Some(text_editor::Position { line: 1, column: 0 }),
        });
        let _ = app.update(IdeMessage::FormatSelection);
        assert_eq!(app.repl_content.text(), "(progn\n  (princ \"hi\")\n)");
    }

    #[test]
    fn test_format_document_multiline_with_intraline_spaces() {
        let (mut app, _) = IdeApp::new();
        let unformatted = "(defun c:test ( / a b )\n(setq a   10)\n(setq b   20)\n(princ)\n)";
        let expected = "(defun c:test (/ a b)\n  (setq a 10)\n  (setq b 20)\n  (princ)\n)";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(unformatted);
        }
        let _ = app.update(IdeMessage::FormatDocument);
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.content.text(), expected);
    }

    #[test]
    fn test_paren_breakpoints_and_debug_paused_span_in_app() {
        use crate::ide::protocol::ParenBreakpoint;
        let (mut app, _) = IdeApp::new();
        let code = "(defun c:demo (/ a b)\n  (setq a (+ 1 2) b (* a 3))\n  (princ b)\n)";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(code);
            // Put cursor on `+` inside `(+ 1 2)` at 0-based line 1, col 11
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 11 },
                selection: None,
            });
            tab.last_edit_cursor = Some(tab.content.cursor());
        }

        // 1. Toggle breakpoint at cursor (F9) -> sets breakpoint at (2, 11) on `(+ 1 2)`
        let _ = app.update(IdeMessage::ToggleBreakpointAtCursor);
        assert!(app.tabs[0].breakpoints.contains(&ParenBreakpoint::new(2, 11)));

        // 2. Move cursor to `*` inside `(* a 3)` (0-based line 1, col 21) and press F9 -> second breakpoint on same line!
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 21 },
                selection: None,
            });
            tab.last_edit_cursor = Some(tab.content.cursor());
        }
        let _ = app.update(IdeMessage::ToggleBreakpointAtCursor);
        assert_eq!(
            app.tabs[0].breakpoints_on_line(2),
            vec![ParenBreakpoint::new(2, 11), ParenBreakpoint::new(2, 21)]
        );

        // 3. Simulate DebugEvent::Paused at `(+ 1 2)` (line 2, col 11..17)
        let _ = app.update(IdeMessage::PluginEvent(PluginToIde::DebugEvt(
            DebugEvent::Paused {
                filename: "untitled.lsp".to_string(),
                line: 2,
                col: 11,
                end_line: 2,
                end_col: 17,
                expr_preview: "(+ 1 2)".to_string(),
                locals: vec![("A".to_string(), "INT".to_string(), "3".to_string())],
                stack: vec![("C:DEMO".to_string(), 2, "untitled.lsp".to_string())],
            },
        )));
        assert!(app.is_debugging);
        assert_eq!(app.tabs[0].debug_paused_span, Some((2, 11, 2, 17)));
        assert_eq!(app.tabs[0].debug_paused_expr.as_deref(), Some("(+ 1 2)"));

        // 4. Gutter click on line 2 clears all breakpoints on line 2
        let _ = app.update(IdeMessage::ToggleBreakpoint(2));
        assert!(!app.tabs[0].has_breakpoint_on_line(2));

        // 5. DebugEvent::Finished clears paused span
        let _ = app.update(IdeMessage::PluginEvent(PluginToIde::DebugEvt(
            DebugEvent::Finished,
        )));
        assert!(!app.is_debugging);
        assert_eq!(app.tabs[0].debug_paused_span, None);
    }

    #[test]
    fn test_debug_paused_scroll_synchronization_long_document() {
        let (mut app, _) = IdeApp::new();
        // Generate a 1,000-line OpenCAD LISP document
        let mut long_code = String::new();
        for i in 1..=1000 {
            long_code.push_str(&format!("(defun func_{i} () (princ {i}))\n"));
        }

        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(&long_code);
            tab.scroll_top_line = 0;
        }

        // Simulate DebugEvent::Paused at line 594, col 10
        let visible_lines = app.visible_lines();
        let margin = (visible_lines / 3).clamp(3, 10);
        let expected_top = 593 - margin;

        let _ = app.update(IdeMessage::PluginEvent(PluginToIde::DebugEvt(
            DebugEvent::Paused {
                filename: "untitled.lsp".to_string(),
                line: 594,
                col: 10,
                end_line: 594,
                end_col: 28,
                expr_preview: "(princ 594)".to_string(),
                locals: vec![],
                stack: vec![("FUNC_594".to_string(), 594, "untitled.lsp".to_string())],
            },
        )));

        assert!(app.is_debugging);
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.debug_paused_span, Some((594, 10, 594, 28)));
        assert_eq!(tab.scroll_top_line, expected_top);
        assert_eq!(tab.content.cursor().position.line, 593);
        assert_eq!(tab.content.cursor().position.column, 9);
        assert!(!tab.is_scrolling_away);

        // Test ScrollToLine to line 800
        let _ = app.update(IdeMessage::ScrollToLine(800));
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.scroll_top_line, 800);
        assert!(tab.is_scrolling_away);

        // Test mouse wheel scroll down by 10 lines
        let _ = app.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: 10 }));
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.scroll_top_line, 810);

        // Test mouse wheel scroll up by 25 lines
        let _ = app.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: -25 }));
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.scroll_top_line, 785);
    }

    #[test]
    fn test_find_and_replace_navigation_and_replace_all() {
        let (mut app, _) = IdeApp::new();
        let code = "(defun c:test ()\n  (setq my_var 10)\n  (setq b (+ my_var 20))\n  (princ my_var)\n)";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(code);
        }

        // 1. Open Find and query "my_var"
        let _ = app.update(IdeMessage::OpenFind);
        assert!(app.find_replace.is_open);
        let _ = app.update(IdeMessage::FindQueryChanged("my_var".to_string()));
        assert_eq!(app.find_replace.matches.len(), 3);
        assert_eq!(app.find_replace.current_match_idx, Some(0));

        // 2. Next & Prev navigation
        let _ = app.update(IdeMessage::FindNext);
        assert_eq!(app.find_replace.current_match_idx, Some(1));
        let _ = app.update(IdeMessage::FindNext);
        assert_eq!(app.find_replace.current_match_idx, Some(2));
        let _ = app.update(IdeMessage::FindNext);
        assert_eq!(app.find_replace.current_match_idx, Some(0)); // wraps
        let _ = app.update(IdeMessage::FindPrev);
        assert_eq!(app.find_replace.current_match_idx, Some(2)); // wraps backwards

        // 3. Replace single match at idx 2
        let _ = app.update(IdeMessage::ReplaceQueryChanged("new_val".to_string()));
        let _ = app.update(IdeMessage::ReplaceCurrent);
        assert_eq!(app.find_replace.matches.len(), 2);
        let text_after_single = app.tabs[app.active_tab].content.text();
        assert!(text_after_single.contains("(princ new_val)"));

        // 4. Replace All remaining
        let _ = app.update(IdeMessage::ReplaceAll);
        assert_eq!(app.find_replace.matches.len(), 0);
        let text_after_all = app.tabs[app.active_tab].content.text();
        assert!(!text_after_all.contains("my_var"));
        assert!(text_after_all.contains("(setq new_val 10)"));
        assert!(text_after_all.contains("(setq b (+ new_val 20))"));

        // 5. Test Undo restores replaced text
        let _ = app.update(IdeMessage::Undo);
        let text_after_undo = app.tabs[app.active_tab].content.text();
        assert_eq!(text_after_undo, text_after_single);

        // 6. Close Find
        let _ = app.update(IdeMessage::CloseFind);
        assert!(!app.find_replace.is_open);
    }

    #[test]
    fn test_find_and_replace_recursive_target_progression_and_open_replace() {
        let (mut app, _) = IdeApp::new();
        let code = "(setq oc-1 10)\n(setq oc-2 (+ oc-1 20))\n(princ oc-1)\n";
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(code);
        }

        // Test OpenReplace opens find overlay with replace expanded
        let _ = app.update(IdeMessage::OpenReplace);
        assert!(app.find_replace.is_open);
        assert!(app.find_replace.show_replace);

        // Search for "oc-" and replace with "ocad-"
        let _ = app.update(IdeMessage::FindQueryChanged("oc-".to_string()));
        let _ = app.update(IdeMessage::ReplaceQueryChanged("ocad-".to_string()));
        assert_eq!(app.find_replace.matches.len(), 4);
        assert_eq!(app.find_replace.current_match_idx, Some(0));

        // Replace first match (on line 0): progresses to match on line 1 (now index 0)
        let _ = app.update(IdeMessage::ReplaceCurrent);
        assert_eq!(app.find_replace.matches.len(), 3);
        assert_eq!(app.find_replace.current_match_idx, Some(0));
        assert_eq!(app.find_replace.matches[0].line, 1);
        let text_1 = app.tabs[app.active_tab].content.text();
        assert!(text_1.starts_with("(setq ocad-1 10)"));

        // Replace second match (on line 1, first oc-)
        let _ = app.update(IdeMessage::ReplaceCurrent);
        assert_eq!(app.find_replace.matches.len(), 2);
        assert_eq!(app.find_replace.matches[0].line, 1);
        let text_2 = app.tabs[app.active_tab].content.text();
        assert!(text_2.contains("(setq ocad-2 (+ oc-1 20))"));

        // Replace third match (on line 1, second oc-)
        let _ = app.update(IdeMessage::ReplaceCurrent);
        assert_eq!(app.find_replace.matches.len(), 1);
        assert_eq!(app.find_replace.matches[0].line, 2);
        let text_3 = app.tabs[app.active_tab].content.text();
        assert!(text_3.contains("(+ ocad-1 20)"));

        // Replace fourth match (on line 2, last match in file)
        let _ = app.update(IdeMessage::ReplaceCurrent);
        let text_4 = app.tabs[app.active_tab].content.text();
        assert!(text_4.contains("(princ ocad-1)"));
        assert_eq!(app.find_replace.matches.len(), 0);
        // Cursor remains after the replacement rather than jumping to document start
        let tab = &app.tabs[app.active_tab];
        assert_eq!(tab.content.cursor().position.line, 2);
    }

    #[test]
    fn test_watch_variables_case_insensitive_matching() {
        let (mut app, _) = IdeApp::new();
        // Add lowercase watch variable
        let _ = app.update(IdeMessage::AddWatch("ch_width".to_string()));
        assert_eq!(app.watch_vars, vec!["ch_width"]);

        // Deduplication is case-insensitive
        let _ = app.update(IdeMessage::AddWatch("CH_WIDTH".to_string()));
        assert_eq!(app.watch_vars.len(), 1);

        // When paused with uppercase symbol binding from OpenCAD LISP engine
        let _ = app.update(IdeMessage::PluginEvent(PluginToIde::DebugEvt(
            DebugEvent::Paused {
                filename: "untitled.lsp".to_string(),
                line: 1,
                col: 1,
                end_line: 1,
                end_col: 10,
                expr_preview: "(test)".to_string(),
                locals: vec![("CH_WIDTH".to_string(), "REAL".to_string(), "12.5".to_string())],
                stack: vec![],
            },
        )));
        assert_eq!(app.debug_locals.len(), 1);
        assert!(app.watch_vars.iter().any(|w| app.debug_locals.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(w))));
    }

    #[test]
    fn test_selection_preserved_when_scrolling_on_and_off_screen() {
        let (mut app, _) = IdeApp::new();
        let long_code: String = (0..200)
            .map(|i| format!("(setq var_{i} (+ {i} 1))"))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(&long_code);
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 5, column: 12 },
                selection: Some(text_editor::Position { line: 5, column: 6 }),
            });
            tab.last_edit_cursor = Some(tab.content.cursor());
        }

        let visible_lines = app.visible_lines();
        assert!(visible_lines > 5);

        // 1. Scroll 2 lines down while cursor/selection line 5 stays visible on-screen
        let _ = app.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: 2 }));
        {
            let tab = &app.tabs[app.active_tab];
            assert!(!tab.is_scrolling_away);
            assert_eq!(tab.scroll_top_line, 2);
            let cur = tab.content.cursor();
            assert_eq!(cur.position, text_editor::Position { line: 5, column: 12 });
            assert_eq!(cur.selection, Some(text_editor::Position { line: 5, column: 6 }));
        }

        // 2. Scroll far enough down that line 5 goes completely off-screen
        let _ = app.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: 20 }));
        {
            let tab = &app.tabs[app.active_tab];
            assert!(tab.is_scrolling_away);
            assert_eq!(tab.scroll_top_line, 22);
            // Saved edit cursor retains the active selection while scrolled away
            let saved = tab.last_edit_cursor.expect("last_edit_cursor preserved");
            assert_eq!(saved.position, text_editor::Position { line: 5, column: 12 });
            assert_eq!(saved.selection, Some(text_editor::Position { line: 5, column: 6 }));
        }

        // 3. Scroll back up until line 5 returns to the viewport
        let _ = app.update(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: -20 }));
        {
            let tab = &app.tabs[app.active_tab];
            assert!(!tab.is_scrolling_away);
            assert_eq!(tab.scroll_top_line, 2);
            let cur = tab.content.cursor();
            assert_eq!(cur.position, text_editor::Position { line: 5, column: 12 });
            assert_eq!(cur.selection, Some(text_editor::Position { line: 5, column: 6 }));
        }
    }

    #[test]
    fn test_find_and_paused_scroll_on_unrendered_content() {
        let (mut app, _) = IdeApp::new();
        let long_code: String = (0..250)
            .map(|i| {
                if i == 150 {
                    "(setq target_needle 42)".to_string()
                } else {
                    format!("(setq filler_{i} {i})")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text(&long_code);
        }

        let visible_lines = app.visible_lines();

        // Find navigates to match on line 150 (never laid out yet)
        let _ = app.update(IdeMessage::FindQueryChanged("target_needle".to_string()));
        {
            let tab = &app.tabs[app.active_tab];
            assert!(!tab.is_scrolling_away);
            assert_eq!(tab.content.cursor().position.line, 150);
            assert!(tab.scroll_top_line <= 150 && 150 < tab.scroll_top_line + visible_lines);
            assert!(tab.content.cursor().selection.is_some());
        }

        // ReplaceCurrent on match resets Content::with_text (height_opt == None) and keeps position synced
        let _ = app.update(IdeMessage::ReplaceQueryChanged("target_updated".to_string()));
        let _ = app.update(IdeMessage::ReplaceCurrent);
        {
            let tab = &app.tabs[app.active_tab];
            assert_eq!(tab.content.cursor().position.line, 150);
            assert!(tab.scroll_top_line <= 150 && 150 < tab.scroll_top_line + visible_lines);
        }
    }
}
