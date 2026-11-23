//! OpenCAD LISP IDE (OCIDE) — an integrated development environment
//! for writing, testing, and debugging OpenCAD LISP scripts directly in OpenCAD Studio.
//! Supports both a top-level native OS window (with Alt-Tab multitasking) and an in-canvas modal.

use crate::ide::highlight::{available_monospace_fonts, DEFAULT_FONT_NAME};
use crate::ide::{ContextMenuState, IdeMessage, IdeSettings, IdeTab, SettingsTab, SyntaxColorTarget};
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, slider, stack, text, text_editor, text_input, Space,
};
use iced::{keyboard, Background, Border, Element, Fill, Length, Point, Theme};

/// Widget ID for the unified IDE editor and gutter scrollable.
pub const EDITOR_SCROLL_ID: &str = "ocide_editor_scroll";
pub const GUTTER_SCROLL_ID: &str = "ocide_editor_scroll";
pub const EDITOR_ID: &str = "ocide_main_editor";
pub const REPL_EDITOR_ID: &str = "ocide_repl_editor";
pub const CONSOLE_EDITOR_ID: &str = "ocide_console_editor";
pub const FIND_INPUT_ID: &str = "find_query_input";
pub const REPLACE_INPUT_ID: &str = "replace_query_input";

pub use crate::ide::api_docs::{is_lisp_ident_char, ApiDoc, API_DOCS, ENTITY_DXF_SUMMARY, ENTMOD_DXF_SUMMARY};

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

/// Dark charcoal gray color (#2A2A2A) used throughout OCIDE for containers, borders, and controls.
pub const IDE_DARK_GRAY: iced::Color = iced::Color::from_rgb(
    42.0 / 255.0, // 0x2A
    42.0 / 255.0, // 0x2A
    42.0 / 255.0, // 0x2A
);

pub fn ide_secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.palette();
    let bg = match status {
        button::Status::Active => IDE_DARK_GRAY,
        button::Status::Hovered => iced::Color::from_rgb(56.0 / 255.0, 56.0 / 255.0, 56.0 / 255.0),
        button::Status::Pressed => iced::Color::from_rgb(34.0 / 255.0, 34.0 / 255.0, 34.0 / 255.0),
        button::Status::Disabled => IDE_DARK_GRAY.scale_alpha(0.5),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.background.base.text,
        border: Border {
            color: IDE_DARK_GRAY,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}


/// Helper to extract an identifier under or immediately adjacent to a cursor column in a line.
pub fn identifier_in_line(line: &str, cursor_col: usize) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let col = cursor_col.min(chars.len());

    let check_idx = if col < chars.len() && is_lisp_ident_char(chars[col]) {
        col
    } else if col > 0 && is_lisp_ident_char(chars[col - 1]) {
        col - 1
    } else {
        return None;
    };

    let mut start = check_idx;
    while start > 0 && is_lisp_ident_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = check_idx;
    while end < chars.len() && is_lisp_ident_char(chars[end]) {
        end += 1;
    }

    let ident: String = chars[start..end].iter().collect();
    if ident.is_empty() {
        return None;
    }
    // Filter out purely numeric literals like "123", "45.6", but allow "1+" or "1-" which are Lisp builtins
    if (ident != "1+" && ident != "1-") && ident.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some(ident)
}

/// Helper to extract an identifier under or immediately adjacent to a cursor position.
pub fn identifier_at_cursor(text: &str, cursor_line: usize, cursor_col: usize) -> Option<String> {
    let line = text.lines().nth(cursor_line)?;
    identifier_in_line(line, cursor_col)
}

/// Helper to identify an active function call at or immediately before cursor column in a single line.
pub fn active_function_call_in_line(
    line: &str,
    cursor_col: usize,
) -> Option<&'static ApiDoc> {
    let chars: Vec<char> = line.chars().collect();
    let col = cursor_col.min(chars.len());
    let prefix = &chars[..col];

    // 1. Look backwards from cursor for an unclosed '(':
    let mut paren_depth = 0;
    let mut open_paren_idx = None;
    for i in (0..prefix.len()).rev() {
        match prefix[i] {
            ')' => paren_depth += 1,
            '(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    open_paren_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    if let Some(idx) = open_paren_idx {
        // In OpenCAD LISP prefix syntax: (func-name arg1 arg2 ...)
        // The function name is the first token immediately following '(' (skipping whitespace/quote)
        let mut start = idx + 1;
        while start < chars.len() && (chars[start].is_whitespace() || chars[start] == '\'') {
            start += 1;
        }
        let mut end = start;
        while end < chars.len() && is_lisp_ident_char(chars[end]) {
            end += 1;
        }
        if start < end {
            let ident: String = chars[start..end].iter().collect();
            let func_name = ident.trim().trim_start_matches('\'');
            if !func_name.is_empty() {
                if let Some(doc) = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(func_name)) {
                    return Some(doc);
                }
            }
        }
    }

    // 2. If not inside parentheses or function doc not found, check if identifier at cursor is an API function
    if let Some(ident) = identifier_in_line(line, cursor_col) {
        let trimmed = ident.trim().trim_start_matches('\'');
        let is_after_close = col > 0 && chars[col - 1] == ')';
        if !is_after_close {
            if let Some(doc) = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(trimmed)) {
                return Some(doc);
            }
        }
    }

    None
}

/// Helper to extract the primary function call initiated on the given line,
/// even when parentheses on that line are already closed/balanced.
pub fn primary_function_in_line(line: &str) -> Option<&'static ApiDoc> {
    let trimmed = line.trim();
    if !trimmed.starts_with('(') {
        return None;
    }
    let after_paren = trimmed.trim_start_matches('(').trim_start();
    let word: String = after_paren
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')' && *c != ';')
        .collect();
    if word.is_empty() {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(&lower))
}

/// Helper to identify an active function call at or immediately before the cursor,
/// scanning backwards across lines if necessary to locate the enclosing unclosed '('.
pub fn active_function_call_at_cursor(
    text: &str,
    cursor_line: usize,
    cursor_col: usize,
) -> Option<&'static ApiDoc> {
    let lines: Vec<&str> = text.lines().collect();
    if cursor_line >= lines.len() {
        return None;
    }

    // 1. First check the current line
    let cur_line = lines[cursor_line];
    let chars: Vec<char> = cur_line.chars().collect();
    let col = cursor_col.min(chars.len());
    let prefix = &chars[..col];

    let mut paren_depth = 0;
    for i in (0..prefix.len()).rev() {
        match prefix[i] {
            ')' => paren_depth += 1,
            '(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    let mut start = i + 1;
                    while start < chars.len() && (chars[start].is_whitespace() || chars[start] == '\'') {
                        start += 1;
                    }
                    let mut end = start;
                    while end < chars.len() && is_lisp_ident_char(chars[end]) {
                        end += 1;
                    }
                    if start < end {
                        let ident: String = chars[start..end].iter().collect();
                        let func_name = ident.trim().trim_start_matches('\'');
                        if let Some(doc) = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(func_name)) {
                            return Some(doc);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if paren_depth == 0 {
        // If parens were completely balanced on this line, check if word at cursor is an API function
        if let Some(ident) = identifier_in_line(cur_line, cursor_col) {
            let trimmed = ident.trim().trim_start_matches('\'');
            let is_after_close = col > 0 && chars[col - 1] == ')';
            if !is_after_close {
                if let Some(doc) = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(trimmed)) {
                    return Some(doc);
                }
            }
        }
    }

    // 2. If open_paren was not found on current line, scan backwards across preceding lines (up to 25 lines):
    let scan_start = cursor_line.saturating_sub(25);
    let mut net_depth = paren_depth;
    for prev_idx in (scan_start..cursor_line).rev() {
        let prev_line = lines[prev_idx];
        let p_chars: Vec<char> = prev_line.chars().collect();
        for i in (0..p_chars.len()).rev() {
            match p_chars[i] {
                ')' => net_depth += 1,
                '(' => {
                    if net_depth > 0 {
                        net_depth -= 1;
                    } else {
                        // Found the enclosing unclosed '(' on this previous line!
                        let mut start = i + 1;
                        while start < p_chars.len() && (p_chars[start].is_whitespace() || p_chars[start] == '\'') {
                            start += 1;
                        }
                        let mut end = start;
                        while end < p_chars.len() && is_lisp_ident_char(p_chars[end]) {
                            end += 1;
                        }
                        if start < end {
                            let ident: String = p_chars[start..end].iter().collect();
                            let func_name = ident.trim().trim_start_matches('\'');
                            if let Some(doc) = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(func_name)) {
                                return Some(doc);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn editor_context_menu(
    has_selection: bool,
    target_ident: Option<String>,
    target_bp: Option<(crate::ide::protocol::ParenBreakpoint, String)>,
    cursor_line: usize,
) -> Element<'static, IdeMessage> {
    const MENU_W: f32 = 260.0;

    let item = |label: String, msg: Option<IdeMessage>| -> Element<'static, IdeMessage> {
        let enabled = msg.is_some();

        let btn = button(text(label).size(12))
            .width(Fill)
            .padding([4, 10])
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let bg = match status {
                button::Status::Hovered if enabled => Background::Color(iced::Color::from_rgb(0.25, 0.25, 0.28)),
                button::Status::Pressed if enabled => Background::Color(iced::Color::from_rgb(0.20, 0.20, 0.22)),
                _ => Background::Color(iced::Color::TRANSPARENT),
            };
            let text_color = if enabled {
                palette.background.base.text
            } else {
                palette.background.base.text.scale_alpha(0.42)
            };
            button::Style {
                background: Some(bg),
                text_color,
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                shadow: iced::Shadow::default(),
                ..Default::default()
            }
        });

        if let Some(msg) = msg {
            btn.on_press(msg).into()
        } else {
            btn.into()
        }
    };

    let watch_label = if let Some(ref ident) = target_ident {
        format!("Add to Watch: '{ident}' (Shift+F9)")
    } else {
        "Add to Watch (Shift+F9)".to_string()
    };
    let watch_msg = target_ident
        .map(IdeMessage::AddWatch)
        .or(Some(IdeMessage::AddWatchAtCursor));

    let (bp_label, bp_msg) = if let Some((bp, preview)) = target_bp {
        (
            format!("Toggle Breakpoint: {preview} (F9)"),
            Some(IdeMessage::ToggleParenBreakpoint(bp)),
        )
    } else {
        (
            format!("Toggle Breakpoint (Line {cursor_line}, F9)"),
            Some(IdeMessage::ToggleBreakpointAtCursor),
        )
    };

    let menu_sep = || {
        container(Space::new())
            .width(Fill)
            .height(Length::Fixed(1.0))
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY)),
                ..Default::default()
            })
    };

    let menu = column![
        item("Copy (Ctrl+C)".to_string(), has_selection.then_some(IdeMessage::EditorCopy)),
        item("Paste (Ctrl+V)".to_string(), Some(IdeMessage::EditorPasteRequest)),
        menu_sep(),
        item("Find (Ctrl+F)".to_string(), Some(IdeMessage::OpenFind)),
        item("Replace (Ctrl+H)".to_string(), Some(IdeMessage::OpenReplace)),
        menu_sep(),
        item("Format Selection (Ctrl+Shift+F)".to_string(), Some(IdeMessage::FormatSelection)),
        item("Format Document (Shift+Alt+F)".to_string(), Some(IdeMessage::FormatDocument)),
        menu_sep(),
        item(watch_label, watch_msg),
        menu_sep(),
        item(bp_label, bp_msg),
    ]
    .spacing(2)
    .padding(4);

    container(menu)
        .width(Length::Fixed(MENU_W))
        .style(|_theme: &Theme| {
            container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY)),
                border: Border {
                    color: IDE_DARK_GRAY,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn render_context_menu_overlay<'a>(
    cm: &'a ContextMenuState,
    window_width: f32,
    window_height: f32,
) -> Element<'a, IdeMessage> {
    const MENU_W: f32 = 270.0;
    const MENU_APPROX_H: f32 = 280.0;

    let x = cm.position.x.clamp(8.0, (window_width - MENU_W - 8.0).max(8.0));
    let y = cm.position.y.clamp(8.0, (window_height - MENU_APPROX_H - 8.0).max(8.0));

    let backdrop = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Fill)
    )
    .on_press(IdeMessage::CloseContextMenu);

    let menu_content = editor_context_menu(
        cm.has_selection,
        cm.target_ident.clone(),
        cm.target_bp.clone(),
        cm.cursor_line,
    );

    let positioned_menu = column![
        Space::new().height(Length::Fixed(y)),
        row![
            Space::new().width(Length::Fixed(x)),
            menu_content,
        ],
    ]
    .width(Fill)
    .height(Fill);

    stack![backdrop, positioned_menu].into()
}

fn render_autocomplete_popup<'a>(
    matches: &'a [String],
    selected_index: usize,
    cursor_x: f32,
    top_offset: f32,
) -> Element<'a, IdeMessage> {
    const POPUP_W: f32 = 250.0;
    const MAX_VISIBLE_ITEMS: usize = 7;
    const ITEM_H: f32 = 25.0;

    let mut list_col = column![].spacing(1);
    for (idx, name) in matches.iter().enumerate() {
        let is_sel = idx == selected_index;
        let func_name = name.clone();

        let category = API_DOCS.iter().find(|d| d.name.eq_ignore_ascii_case(name))
            .map(|d| d.category)
            .unwrap_or("Custom");

        let row_item = button(
            row![
                text(format!("λ {name}")).size(12).font(iced::Font::MONOSPACE),
                Space::new().width(Fill),
                text(category).size(10).style(|_t: &Theme| text::Style {
                    color: Some(iced::Color::from_rgb(0.65, 0.75, 0.88)),
                }),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .width(Fill)
        .padding([3, 8])
        .on_press(IdeMessage::AutocompleteSelect(func_name))
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let (bg, text_color) = if is_sel {
                (
                    Background::Color(iced::Color::from_rgb(0.18, 0.38, 0.72)),
                    iced::Color::WHITE,
                )
            } else {
                match status {
                    button::Status::Hovered => (
                        Background::Color(iced::Color::from_rgb(0.24, 0.24, 0.28)),
                        palette.background.base.text,
                    ),
                    _ => (
                        Background::Color(iced::Color::TRANSPARENT),
                        palette.background.base.text,
                    ),
                }
            };
            button::Style {
                background: Some(bg),
                text_color,
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

        list_col = list_col.push(row_item);
    }

    let scroll_list = scrollable(list_col)
        .height(Length::Fixed((matches.len().min(MAX_VISIBLE_ITEMS) as f32 * ITEM_H).max(ITEM_H) + 4.0));

    let hint_footer = container(
        row![
            text("↑/↓ Navigate").size(9).style(muted_style),
            Space::new().width(Fill),
            text("Tab Complete").size(9).style(muted_style),
        ]
        .align_y(iced::Center),
    )
    .padding([2, 6])
    .style(|_t: &Theme| container::Style {
        background: Some(Background::Color(iced::Color::from_rgba(0.08, 0.08, 0.10, 0.95))),
        ..Default::default()
    });

    let card = container(
        column![
            scroll_list,
            hint_footer,
        ]
        .spacing(2)
        .padding(4),
    )
    .width(Length::Fixed(POPUP_W))
    .style(|_t: &Theme| container::Style {
        background: Some(Background::Color(iced::Color::from_rgba(0.12, 0.12, 0.14, 0.98))),
        border: Border {
            color: iced::Color::from_rgba(0.3, 0.5, 0.85, 0.8),
            width: 1.0,
            radius: 5.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
        ..Default::default()
    });

    column![
        Space::new().height(Length::Fixed(top_offset)),
        row![
            Space::new().width(Length::Fixed(cursor_x.min(450.0))),
            card,
        ],
    ]
    .into()
}

fn render_find_replace_overlay<'a>(
    find_replace: &'a crate::ide::FindReplaceState,
    _settings: &'a IdeSettings,
    alpha: f32,
) -> Element<'a, IdeMessage> {
    let match_count = find_replace.matches.len();
    let count_text = if find_replace.find_query.is_empty() {
        String::new()
    } else if match_count == 0 {
        "No results".to_string()
    } else if let Some(idx) = find_replace.current_match_idx {
        format!("{} of {}", idx + 1, match_count)
    } else {
        format!("{match_count} matches")
    };

    let toggle_replace_btn = button(
        text(if find_replace.show_replace { "▼" } else { "▶" }).size(10)
    )
    .on_press(IdeMessage::ToggleFindShowReplace)
    .padding([2, 5])
    .style(button::text);

    let find_input = text_input("Find...", &find_replace.find_query)
        .id(FIND_INPUT_ID)
        .on_input(IdeMessage::FindQueryChanged)
        .on_submit(IdeMessage::FindNext)
        .size(12)
        .padding([4, 6])
        .width(Length::Fixed(160.0));

    let count_label = text(count_text)
        .size(10)
        .style(|_t| text::Style { color: Some(iced::Color::from_rgb(0.65, 0.65, 0.65)) })
        .width(Length::Fixed(65.0));

    let prev_btn = button(text("▲").size(10))
        .on_press(IdeMessage::FindPrev)
        .padding([3, 6])
        .style(button::secondary);

    let next_btn = button(text("▼").size(10))
        .on_press(IdeMessage::FindNext)
        .padding([3, 6])
        .style(button::secondary);

    let case_btn = button(text("Aa").size(10).font(iced::Font::MONOSPACE))
        .on_press(IdeMessage::ToggleFindMatchCase)
        .padding([3, 5])
        .style(if find_replace.match_case { button::primary } else { button::secondary });

    let word_btn = button(text("ab").size(10).font(iced::Font::MONOSPACE))
        .on_press(IdeMessage::ToggleFindWholeWord)
        .padding([3, 5])
        .style(if find_replace.match_whole_word { button::primary } else { button::secondary });

    let close_btn = button(text("✕").size(10))
        .on_press(IdeMessage::CloseFind)
        .padding([2, 5])
        .style(button::text);

    let row1 = row![
        toggle_replace_btn,
        find_input,
        count_label,
        prev_btn,
        next_btn,
        case_btn,
        word_btn,
        close_btn,
    ]
    .spacing(4)
    .align_y(iced::Center);

    let mut content_col = column![row1].spacing(4);

    if find_replace.show_replace {
        let replace_input = text_input("Replace with...", &find_replace.replace_query)
            .id(REPLACE_INPUT_ID)
            .on_input(IdeMessage::ReplaceQueryChanged)
            .on_submit(IdeMessage::ReplaceCurrent)
            .size(12)
            .padding([4, 6])
            .width(Length::Fixed(160.0));

        let replace_btn = button(text("Replace").size(10))
            .on_press(IdeMessage::ReplaceCurrent)
            .padding([3, 8])
            .style(button::secondary);

        let replace_all_btn = button(text("All").size(10))
            .on_press(IdeMessage::ReplaceAll)
            .padding([3, 8])
            .style(button::secondary);

        let row2 = row![
            Space::new().width(Length::Fixed(20.0)),
            replace_input,
            replace_btn,
            replace_all_btn,
        ]
        .spacing(4)
        .align_y(iced::Center);

        content_col = content_col.push(row2);
    }

    let panel = container(content_col)
        .padding([6, 8])
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha * 0.96))),
            border: Border {
                color: iced::Color::from_rgb(0.3, 0.3, 0.35),
                width: 1.0,
                radius: 5.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45),
                offset: iced::Vector::new(0.0, 3.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    column![
        Space::new().height(Length::Fixed(4.0)),
        row![
            Space::new().width(Fill),
            panel,
            Space::new().width(Length::Fixed(16.0)),
        ]
        .width(Fill),
    ]
    .width(Fill)
    .into()
}

/// Render the standalone full-screen OS window for OCIDE.
pub fn view_standalone_window<'a>(
    tabs: &'a [IdeTab],
    active_tab: usize,
    console_log: &'a text_editor::Content,
    repl_content: &'a text_editor::Content,
    repl_mode: crate::ide::protocol::ReplMode,
    api_drawer_open: bool,
    api_search: &'a str,
    theme: &'a Theme,
    is_debugging: bool,
    variables_open: bool,
    debug_locals: &'a [(String, String, String)],
    debug_stack: &'a [(String, usize, String)],
    is_transparent: bool,
    watch_vars: &'a [String],
    console_height: f32,
    is_splitter_dragging: bool,
    visible_lines: usize,
    settings: &'a IdeSettings,
    settings_open: bool,
    help_menu_open: bool,
    settings_tab: SettingsTab,
    active_color_target: SyntaxColorTarget,
    syntax_hex_input: &'a str,
    font_size_input: &'a str,
    transparency_input: &'a str,
    selected_theme_name: &'a str,
    new_theme_name: &'a str,
    selected_custom_func: Option<&'a str>,
    new_custom_func_input: &'a str,
    autocomplete_matches: Option<&'a [String]>,
    autocomplete_selected_index: usize,
    context_menu: Option<&'a ContextMenuState>,
    save_prompt: Option<&'a crate::ide::SavePromptState>,
    find_replace: &'a crate::ide::FindReplaceState,
    window_width: f32,
    window_height: f32,
) -> Element<'a, IdeMessage> {
    render_ide_layout(
        tabs,
        active_tab,
        console_log,
        repl_content,
        repl_mode,
        api_drawer_open,
        api_search,
        theme,
        Fill,
        Fill,
        true,
        is_debugging,
        variables_open,
        debug_locals,
        debug_stack,
        is_transparent,
        watch_vars,
        console_height,
        is_splitter_dragging,
        visible_lines,
        settings,
        settings_open,
        help_menu_open,
        settings_tab,
        active_color_target,
        syntax_hex_input,
        font_size_input,
        transparency_input,
        selected_theme_name,
        new_theme_name,
        selected_custom_func,
        new_custom_func_input,
        autocomplete_matches,
        autocomplete_selected_index,
        context_menu,
        save_prompt,
        find_replace,
        window_width,
        window_height,
    )
}

fn ide_tooltip<'a>(
    content: impl Into<Element<'a, IdeMessage>>,
    tip: &'static str,
) -> Element<'a, IdeMessage> {
    iced::widget::tooltip(
        content,
        container(text(tip).size(12))
            .padding([4, 8])
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY)),
                border: Border {
                    color: iced::Color::from_rgb(0.3, 0.3, 0.3),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                text_color: Some(theme.palette().background.base.text),
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4.0)
    .into()
}

fn render_syntax_swatch<'a>(
    label: &'static str,
    color: iced::Color,
    is_sel: bool,
    msg: IdeMessage,
) -> Element<'a, IdeMessage> {
    ide_tooltip(
        button(
            container(Space::new())
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0))
                .style(move |_t: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        color: if is_sel {
                            iced::Color::from_rgb(1.0, 1.0, 1.0)
                        } else {
                            iced::Color::from_rgb(0.3, 0.3, 0.3)
                        },
                        width: if is_sel { 2.0 } else { 1.0 },
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
        )
        .on_press(msg)
        .padding(2)
        .style(button::text),
        label,
    )
}

fn render_settings_dialog<'a>(
    settings: &'a IdeSettings,
    settings_tab: SettingsTab,
    active_color_target: SyntaxColorTarget,
    syntax_hex_input: &'a str,
    font_size_input: &'a str,
    transparency_input: &'a str,
    selected_theme_name: &'a str,
    new_theme_name: &'a str,
    selected_custom_func: Option<&'a str>,
    new_custom_func_input: &'a str,
    window_height: f32,
    _theme: &'a Theme,
) -> Element<'a, IdeMessage> {
    let title_row = row![
        text("⚙ IDE Settings").size(15),
        Space::new().width(Fill),
        button(text("Done").size(12))
            .on_press(IdeMessage::ToggleSettings)
            .padding([4, 14])
            .style(button::primary),
        button(text("✕").size(12))
            .on_press(IdeMessage::ToggleSettings)
            .padding([2, 6])
            .style(button::text),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let tab_general_btn = button(text("⚙ General & Debugger").size(12))
        .on_press(IdeMessage::SetSettingsTab(SettingsTab::General))
        .padding([6, 12])
        .style(if settings_tab == SettingsTab::General { button::primary } else { ide_secondary_button });

    let tab_syntax_btn = button(text("🎨 Editor, Font & Syntax").size(12))
        .on_press(IdeMessage::SetSettingsTab(SettingsTab::EditorSyntax))
        .padding([6, 12])
        .style(if settings_tab == SettingsTab::EditorSyntax { button::primary } else { ide_secondary_button });

    let tab_format_btn = button(text("📐 OpenCAD LISP Formatting").size(12))
        .on_press(IdeMessage::SetSettingsTab(SettingsTab::Formatting))
        .padding([6, 12])
        .style(if settings_tab == SettingsTab::Formatting { button::primary } else { ide_secondary_button });

    let tabs_row = row![tab_general_btn, tab_syntax_btn, tab_format_btn].spacing(8);

    let dialog_sep = || {
        container(Space::new())
            .width(Fill)
            .height(Length::Fixed(1.0))
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY)),
                ..Default::default()
            })
    };

    let body_content: Element<'a, IdeMessage> = match settings_tab {
        SettingsTab::General => {
            // Breakpoint presets
            let bp_presets = [
                ("Dark Red (Default)", iced::Color::from_rgba(0.55, 0.12, 0.12, 0.45)),
                ("Crimson", iced::Color::from_rgba(0.80, 0.10, 0.15, 0.45)),
                ("Burgundy", iced::Color::from_rgba(0.45, 0.05, 0.25, 0.45)),
                ("Purple", iced::Color::from_rgba(0.50, 0.15, 0.65, 0.45)),
                ("Orange", iced::Color::from_rgba(0.85, 0.40, 0.05, 0.45)),
                ("Navy Blue", iced::Color::from_rgba(0.12, 0.25, 0.65, 0.45)),
                ("Charcoal", iced::Color::from_rgba(0.20, 0.20, 0.20, 0.45)),
                ("Teal", iced::Color::from_rgba(0.10, 0.50, 0.50, 0.45)),
            ];

            let mut bp_items = row![].spacing(4);
            for (name, col) in bp_presets {
                let is_sel = (settings.breakpoint_color.r - col.r).abs() < 0.02
                    && (settings.breakpoint_color.g - col.g).abs() < 0.02
                    && (settings.breakpoint_color.b - col.b).abs() < 0.02;
                bp_items = bp_items.push(render_syntax_swatch(name, col, is_sel, IdeMessage::SetBreakpointColor(col)));
            }
            let bp_row = bp_items.wrap();

            // Execution line presets
            let exec_presets = [
                ("Green (Default)", iced::Color::from_rgba(0.12, 0.55, 0.20, 0.50)),
                ("Emerald", iced::Color::from_rgba(0.05, 0.65, 0.40, 0.50)),
                ("Lime", iced::Color::from_rgba(0.45, 0.70, 0.10, 0.50)),
                ("Yellow / Gold", iced::Color::from_rgba(0.75, 0.65, 0.10, 0.50)),
                ("Cyan", iced::Color::from_rgba(0.10, 0.60, 0.75, 0.50)),
                ("Teal", iced::Color::from_rgba(0.10, 0.50, 0.50, 0.50)),
                ("Sky Blue", iced::Color::from_rgba(0.20, 0.55, 0.85, 0.50)),
                ("Purple", iced::Color::from_rgba(0.55, 0.25, 0.75, 0.50)),
            ];

            let mut exec_items = row![].spacing(4);
            for (name, col) in exec_presets {
                let is_sel = (settings.execution_color.r - col.r).abs() < 0.02
                    && (settings.execution_color.g - col.g).abs() < 0.02
                    && (settings.execution_color.b - col.b).abs() < 0.02;
                exec_items = exec_items.push(render_syntax_swatch(name, col, is_sel, IdeMessage::SetExecutionColor(col)));
            }
            let exec_row = exec_items.wrap();

            // Transparency stepper and text input
            let cur_trans = settings.transparency_percent;
            let dec_trans = cur_trans.saturating_sub(5).max(0);
            let inc_trans = cur_trans.saturating_add(5).min(90);

            let trans_box = row![
                text("Transparency Level:").size(12),
                button(text("−").size(12))
                    .on_press(IdeMessage::SetTransparencyPercent(dec_trans))
                    .padding([2, 6])
                    .style(ide_secondary_button),
                text_input("50", transparency_input)
                    .on_input(IdeMessage::SetTransparencyInput)
                    .on_submit(IdeMessage::ApplyTransparencyInput)
                    .padding([3, 4])
                    .size(12)
                    .width(Length::Fixed(36.0)),
                button(text("+").size(12))
                    .on_press(IdeMessage::SetTransparencyPercent(inc_trans))
                    .padding([2, 6])
                    .style(ide_secondary_button),
                text("%").size(12),
            ]
            .spacing(3)
            .align_y(iced::Center);

            // Auto-transparency toggle button
            let auto_trans_btn = button(
                row![
                    text(if settings.auto_transparency { "☑" } else { "☐" }).size(14),
                    text("Automatically switch IDE to transparent when debugging (and restore when done)").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::SetAutoTransparency(!settings.auto_transparency))
            .padding([4, 8])
            .style(if settings.auto_transparency { button::primary } else { ide_secondary_button });

            column![
                text("Breakpoint Line Color").size(13),
                bp_row,
                Space::new().height(Length::Fixed(4.0)),
                text("Current Execution Line Color").size(13),
                exec_row,
                Space::new().height(Length::Fixed(4.0)),
                dialog_sep(),
                trans_box,
                Space::new().height(Length::Fixed(4.0)),
                dialog_sep(),
                text("Debugger Automation").size(13),
                auto_trans_btn,
                Space::new().height(Length::Fixed(4.0)),
                dialog_sep(),
                text("Default Scripts Location").size(13),
                row![
                    container(
                        text(settings.resolved_scripts_dir().to_string_lossy().to_string())
                            .size(11)
                    )
                    .padding([6, 8])
                    .width(Fill)
                    .style(move |t: &Theme| {
                        let p = t.palette();
                        container::Style {
                            background: Some(Background::Color(p.background.base.color.scale_alpha(0.5))),
                            border: Border {
                                color: IDE_DARK_GRAY,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    }),
                    button(text("📂 Browse...").size(12))
                        .on_press(IdeMessage::BrowseScriptsDir)
                        .padding([6, 10])
                        .style(button::primary),
                    button(text("↺ Reset").size(12))
                        .on_press(IdeMessage::SetScriptsDir(None))
                        .padding([6, 8])
                        .style(ide_secondary_button),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(8)
            .into()
        }
        SettingsTab::EditorSyntax => {
            // 1. Font Selection
            let fonts = available_monospace_fonts();
            let selected_font: Option<&'static str> = match &settings.font_name {
                Some(name) => fonts.iter().copied().find(|&f| f.eq_ignore_ascii_case(name)),
                None => Some(DEFAULT_FONT_NAME),
            };

            let font_picker = pick_list(
                selected_font,
                fonts,
                |&f| f.to_string(),
            )
            .on_select(|f: &'static str| {
                if f == DEFAULT_FONT_NAME {
                    IdeMessage::SetEditorFont(None)
                } else {
                    IdeMessage::SetEditorFont(Some(f.to_string()))
                }
            })
            .text_size(12)
            .width(Length::Fixed(270.0));

            let cur_size = settings.font_size();
            let dec_size = cur_size.saturating_sub(1).max(8);
            let inc_size = cur_size.saturating_add(1).min(72);

            let size_box = row![
                text("Size:").size(12),
                button(text("−").size(12))
                    .on_press(IdeMessage::SetEditorFontSize(dec_size))
                    .padding([2, 6])
                    .style(ide_secondary_button),
                text_input("13", font_size_input)
                    .on_input(IdeMessage::SetFontSizeInput)
                    .on_submit(IdeMessage::ApplyFontSizeInput)
                    .padding([3, 4])
                    .size(12)
                    .width(Length::Fixed(36.0)),
                button(text("+").size(12))
                    .on_press(IdeMessage::SetEditorFontSize(inc_size))
                    .padding([2, 6])
                    .style(ide_secondary_button),
            ]
            .spacing(3)
            .align_y(iced::Center);

            let reset_font_btn = button(text("↺ Default Font").size(12))
                .on_press(IdeMessage::SetEditorFont(None))
                .padding([4, 8])
                .style(ide_secondary_button);

            let font_row = row![
                text("Editor Font:").size(12),
                font_picker,
                size_box,
                reset_font_btn,
            ]
            .spacing(8)
            .align_y(iced::Center);

            // 2. Presets Dropdown & Save Section
            let mut theme_options: Vec<String> = vec![
                "Base16 Mocha (Default)".to_string(),
                "Classic Light".to_string(),
                "Monokai".to_string(),
                "Solarized Dark".to_string(),
            ];
            for ut in &settings.user_themes {
                if !theme_options.contains(&ut.name) {
                    theme_options.push(ut.name.clone());
                }
            }
            if !selected_theme_name.is_empty() && !theme_options.contains(&selected_theme_name.to_string()) {
                theme_options.push(selected_theme_name.to_string());
            }

            let cur_theme_selected = if theme_options.iter().any(|t| t == selected_theme_name) {
                Some(selected_theme_name.to_string())
            } else {
                Some("Base16 Mocha (Default)".to_string())
            };

            let is_user_theme = settings.user_themes.iter().any(|t| t.name == selected_theme_name);

            let mut theme_row = row![
                text("Theme:").size(12),
                pick_list(
                    cur_theme_selected,
                    theme_options,
                    |s: &String| s.clone(),
                )
                .on_select(IdeMessage::SelectTheme)
                .text_size(12)
                .width(Length::Fixed(190.0)),
                text_input("New theme name...", new_theme_name)
                    .on_input(IdeMessage::SetNewThemeName)
                    .on_submit(IdeMessage::SaveCurrentTheme)
                    .padding([4, 6])
                    .size(12)
                    .width(Length::Fixed(150.0)),
                button(text("💾 Save").size(11))
                    .on_press(IdeMessage::SaveCurrentTheme)
                    .padding([4, 8])
                    .style(ide_secondary_button),
            ]
            .spacing(6)
            .align_y(iced::Center);

            if is_user_theme {
                theme_row = theme_row.push(
                    button(text("🗑").size(11))
                        .on_press(IdeMessage::DeleteUserTheme(selected_theme_name.to_string()))
                        .padding([4, 6])
                        .style(button::danger),
                );
            }

            theme_row = theme_row.push(Space::new().width(Fill));
            theme_row = theme_row.push(
                button(text("↺ Reset All Defaults").size(11))
                    .on_press(IdeMessage::ResetEditorSyntaxDefaults)
                    .padding([4, 8])
                    .style(button::danger),
            );

            // 3. Syntax Elements List Box & Stacked Sliders Section
            let c = &settings.syntax_colors;
            let targets: [(&'static str, SyntaxColorTarget, iced::Color); 9] = [
                ("Editor Background", SyntaxColorTarget::Background, c.background),
                ("Default Text", SyntaxColorTarget::DefaultText, c.default_text),
                ("Comments (;)", SyntaxColorTarget::Comment, c.comment),
                ("Strings (\"...\")", SyntaxColorTarget::StringLiteral, c.string),
                ("Keywords & Forms", SyntaxColorTarget::Keyword, c.keyword),
                ("Native Functions", SyntaxColorTarget::BuiltinFunction, c.builtin_function),
                ("Custom Functions", SyntaxColorTarget::CustomFunction, c.effective_custom_func_color()),
                ("Numbers", SyntaxColorTarget::Number, c.number),
                ("Parentheses ()", SyntaxColorTarget::Paren, c.paren),
            ];

            let mut syntax_items = column![].spacing(2);
            for (lbl, tgt, col) in targets {
                let is_sel = tgt == active_color_target;
                let item_btn = button(
                    row![
                        container(Space::new())
                            .width(Length::Fixed(18.0))
                            .height(Length::Fixed(18.0))
                            .style(move |_t| container::Style {
                                background: Some(Background::Color(col)),
                                border: Border {
                                    color: if is_sel {
                                        iced::Color::from_rgb(1.0, 1.0, 1.0)
                                    } else {
                                        iced::Color::from_rgb(0.4, 0.4, 0.4)
                                    },
                                    width: if is_sel { 2.0 } else { 1.0 },
                                    radius: 3.0.into(),
                                },
                                ..Default::default()
                            }),
                        text(lbl).size(12),
                    ]
                    .spacing(8)
                    .align_y(iced::Center)
                )
                .width(Fill)
                .padding([4, 6])
                .on_press(IdeMessage::SelectSyntaxColorTarget(tgt))
                .style(if is_sel { button::primary } else { button::text });

                syntax_items = syntax_items.push(item_btn);
            }

            let syntax_list_box = container(
                scrollable(syntax_items)
                    .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::default()))
                    .height(Length::Fixed(180.0))
                    .width(Length::Fixed(230.0))
            )
            .padding(4)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.25))),
                border: Border {
                    color: IDE_DARK_GRAY,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            let active_target_name = match active_color_target {
                SyntaxColorTarget::Background => "Editor Background",
                SyntaxColorTarget::DefaultText => "Default Text",
                SyntaxColorTarget::Comment => "Comments",
                SyntaxColorTarget::StringLiteral => "Strings",
                SyntaxColorTarget::Keyword => "Keywords & Forms",
                SyntaxColorTarget::BuiltinFunction => "Native Functions",
                SyntaxColorTarget::CustomFunction => "User Custom Functions",
                SyntaxColorTarget::Number => "Numbers",
                SyntaxColorTarget::Paren => "Parentheses",
            };

            let cur_col = settings.get_target_color(active_color_target);
            let cur_r = (cur_col.r.clamp(0.0, 1.0) * 255.0).round() as u8;
            let cur_g = (cur_col.g.clamp(0.0, 1.0) * 255.0).round() as u8;
            let cur_b = (cur_col.b.clamp(0.0, 1.0) * 255.0).round() as u8;

            let default_target_color = match active_color_target {
                SyntaxColorTarget::Background => crate::ide::highlight::SyntaxColors::base16_mocha().background,
                SyntaxColorTarget::DefaultText => crate::ide::highlight::SyntaxColors::base16_mocha().default_text,
                SyntaxColorTarget::Comment => crate::ide::highlight::SyntaxColors::base16_mocha().comment,
                SyntaxColorTarget::StringLiteral => crate::ide::highlight::SyntaxColors::base16_mocha().string,
                SyntaxColorTarget::Keyword => crate::ide::highlight::SyntaxColors::base16_mocha().keyword,
                SyntaxColorTarget::BuiltinFunction => crate::ide::highlight::SyntaxColors::base16_mocha().builtin_function,
                SyntaxColorTarget::CustomFunction => iced::Color::from_rgb8(0x82, 0xAA, 0xFF),
                SyntaxColorTarget::Number => crate::ide::highlight::SyntaxColors::base16_mocha().number,
                SyntaxColorTarget::Paren => crate::ide::highlight::SyntaxColors::base16_mocha().paren,
            };

            let curated_palette: &[(&'static str, iced::Color)] = &[
                ("Base16 Mocha Default", default_target_color),
                ("Pure White (#FFFFFF)", iced::Color::from_rgb8(0xFF, 0xFF, 0xFF)),
                ("Soft Cream (#F8F8F2)", iced::Color::from_rgb8(0xF8, 0xF8, 0xF2)),
                ("Light Gray (#D0C8C6)", iced::Color::from_rgb8(0xD0, 0xC8, 0xC6)),
                ("Muted Gray (#839496)", iced::Color::from_rgb8(0x83, 0x94, 0x96)),
                ("Charcoal (#272822)", iced::Color::from_rgb8(0x27, 0x28, 0x22)),
                ("Mocha BG (#202225)", iced::Color::from_rgb8(0x20, 0x22, 0x25)),
                ("Pure Black (#000000)", iced::Color::from_rgb8(0x00, 0x00, 0x00)),
                ("Solarized Dark (#002B36)", iced::Color::from_rgb8(0x00, 0x2B, 0x36)),
                ("Red / Coral (#FF5555)", iced::Color::from_rgb8(0xFF, 0x55, 0x55)),
                ("Orange (#FFB86C)", iced::Color::from_rgb8(0xFF, 0xB8, 0x6C)),
                ("Gold / Yellow (#F1FA8C)", iced::Color::from_rgb8(0xF1, 0xFA, 0x8C)),
                ("Emerald Green (#50FA7B)", iced::Color::from_rgb8(0x50, 0xFA, 0x7B)),
                ("Cyan / Teal (#8BE9FD)", iced::Color::from_rgb8(0x8B, 0xE9, 0xFD)),
                ("Classic Blue (#0000FF)", iced::Color::from_rgb8(0x00, 0x00, 0xFF)),
                ("Navy (#000080)", iced::Color::from_rgb8(0x00, 0x00, 0x80)),
                ("Sky Blue (#82AAFF)", iced::Color::from_rgb8(0x82, 0xAA, 0xFF)),
                ("Purple / Lavender (#BD93F9)", iced::Color::from_rgb8(0xBD, 0x93, 0xF9)),
                ("Magenta (#FF79C6)", iced::Color::from_rgb8(0xFF, 0x79, 0xC6)),
                ("Classic Purple (#800080)", iced::Color::from_rgb8(0x80, 0x00, 0x80)),
                ("Sienna Brown (#A0522D)", iced::Color::from_rgb8(0xA0, 0x52, 0x2D)),
            ];

            let mut swatch_row = row![].spacing(3);
            for (lbl, col) in curated_palette {
                let is_sel = (cur_col.r - col.r).abs() < 0.02
                    && (cur_col.g - col.g).abs() < 0.02
                    && (cur_col.b - col.b).abs() < 0.02;
                swatch_row = swatch_row.push(render_syntax_swatch(
                    lbl,
                    *col,
                    is_sel,
                    IdeMessage::SetSyntaxColor(active_color_target, *col),
                ));
            }
            let swatches_wrapped = swatch_row.wrap();

            let hex_rgb_row = row![
                text("Hex:").size(12),
                text_input("#RRGGBB", syntax_hex_input)
                    .on_input(IdeMessage::SetSyntaxHexInput)
                    .on_submit(IdeMessage::ApplySyntaxHexInput)
                    .padding([3, 6])
                    .size(12)
                    .width(Length::Fixed(85.0)),
                button(text("Apply").size(11))
                    .on_press(IdeMessage::ApplySyntaxHexInput)
                    .padding([3, 8])
                    .style(ide_secondary_button),
                Space::new().width(Length::Fixed(6.0)),
                container(Space::new())
                    .width(Length::Fixed(20.0))
                    .height(Length::Fixed(20.0))
                    .style(move |_t| container::Style {
                        background: Some(Background::Color(cur_col)),
                        border: Border {
                            color: iced::Color::from_rgb(0.6, 0.6, 0.6),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
                Space::new().width(Length::Fixed(6.0)),
                text(format!("RGB: ({cur_r}, {cur_g}, {cur_b})")).size(11),
            ]
            .spacing(4)
            .align_y(iced::Center);

            let slider_row = |name: &'static str, val: u8, channel: u8| {
                row![
                    text(format!("{name}: {val:3}")).size(11).width(Length::Fixed(44.0)),
                    slider(0..=255u8, val, move |v| IdeMessage::SetSyntaxRgbChannel(channel, v))
                        .width(Length::Fixed(190.0)),
                ]
                .spacing(4)
                .align_y(iced::Center)
            };

            let rgb_sliders_column = column![
                slider_row("R", cur_r, 0),
                slider_row("G", cur_g, 1),
                slider_row("B", cur_b, 2),
            ]
            .spacing(3);

            let color_adjuster_column = column![
                text(format!("Adjust Color: {active_target_name}")).size(12),
                rgb_sliders_column,
                hex_rgb_row,
                text("Quick Palette:").size(11).style(muted_style),
                swatches_wrapped,
            ]
            .spacing(5);

            let syntax_middle_section = row![
                column![
                    text("Syntax Elements").size(12),
                    syntax_list_box,
                ]
                .spacing(4),
                color_adjuster_column,
            ]
            .spacing(16);

            // 4. User Custom Functions Section
            let mut func_items = column![].spacing(2);
            if settings.custom_functions.is_empty() {
                func_items = func_items.push(
                    container(text("No custom functions added yet.").size(11).style(muted_style))
                        .padding([6, 8])
                );
            } else {
                for func in &settings.custom_functions {
                    let is_sel = selected_custom_func == Some(func.as_str());
                    let func_name = func.clone();
                    let row_btn = button(
                        row![
                            text("ƒ").size(12).style(move |_t| text::Style {
                                color: Some(settings.syntax_colors.effective_custom_func_color()),
                            }),
                            text(func_name.clone()).size(12),
                        ]
                        .spacing(6)
                        .align_y(iced::Center)
                    )
                    .width(Fill)
                    .padding([3, 6])
                    .on_press(IdeMessage::SelectCustomFunction(Some(func_name)))
                    .style(if is_sel { button::primary } else { button::text });

                    func_items = func_items.push(row_btn);
                }
            }

            let func_list_box = container(
                scrollable(func_items)
                    .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::default()))
                    .height(Length::Fixed(120.0))
                    .width(Length::Fixed(230.0))
            )
            .padding(4)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.25))),
                border: Border {
                    color: IDE_DARK_GRAY,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            let add_row = row![
                text_input("New function name...", new_custom_func_input)
                    .on_input(IdeMessage::SetNewCustomFunctionInput)
                    .on_submit(IdeMessage::AddCustomFunction)
                    .padding([4, 6])
                    .size(12)
                    .width(Length::Fixed(180.0)),
                button(text("+ Add").size(11))
                    .on_press(IdeMessage::AddCustomFunction)
                    .padding([4, 10])
                    .style(button::primary),
            ]
            .spacing(6)
            .align_y(iced::Center);

            let remove_btn = {
                let mut btn = button(text("− Remove").size(11))
                    .padding([4, 10]);
                if selected_custom_func.is_some() {
                    btn = btn.on_press(IdeMessage::RemoveCustomFunction).style(button::danger);
                } else {
                    btn = btn.style(ide_secondary_button);
                }
                btn
            };

            let remove_row = row![
                remove_btn,
                if let Some(sel) = selected_custom_func {
                    text(format!("Selected: {sel}")).size(11).style(muted_style)
                } else {
                    text("Select an item in list to remove").size(11).style(muted_style)
                }
            ]
            .spacing(8)
            .align_y(iced::Center);

            let color_mode_row = row![
                text("Color Mode:").size(12),
                button(
                    row![
                        text(if settings.custom_function_use_native_color { "● " } else { "○ " }).size(12),
                        text("Same as Native").size(12),
                    ]
                    .spacing(4)
                    .align_y(iced::Center)
                )
                .on_press(IdeMessage::SetCustomFunctionColorMode(true))
                .padding([4, 8])
                .style(if settings.custom_function_use_native_color { button::primary } else { ide_secondary_button }),
                button(
                    row![
                        text(if !settings.custom_function_use_native_color { "● " } else { "○ " }).size(12),
                        text("Dedicated Custom Color").size(12),
                    ]
                    .spacing(4)
                    .align_y(iced::Center)
                )
                .on_press(IdeMessage::SetCustomFunctionColorMode(false))
                .padding([4, 8])
                .style(if !settings.custom_function_use_native_color { button::primary } else { ide_secondary_button }),
                Space::new().width(Length::Fixed(4.0)),
                container(Space::new())
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0))
                    .style(move |_t| container::Style {
                        background: Some(Background::Color(settings.syntax_colors.effective_custom_func_color())),
                        border: Border {
                            color: iced::Color::from_rgb(0.6, 0.6, 0.6),
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }),
            ]
            .spacing(6)
            .align_y(iced::Center);

            let custom_controls = column![
                text("Manage Function Names:").size(11).style(muted_style),
                add_row,
                remove_row,
                Space::new().height(Length::Fixed(2.0)),
                color_mode_row,
            ]
            .spacing(6);

            let custom_funcs_section = column![
                text("User Custom Functions").size(13),
                row![func_list_box, custom_controls].spacing(16),
            ]
            .spacing(6);

            // 6. Live Mini OpenCAD LISP Preview
            let preview_font = settings.editor_font();
            let preview_bg = c.background;
            let preview_comment = c.comment;
            let preview_paren = c.paren;
            let preview_kw = c.keyword;
            let preview_builtin = c.builtin_function;
            let preview_custom = c.effective_custom_func_color();
            let preview_num = c.number;
            let preview_str = c.string;
            let preview_def = c.default_text;

            let ptoken = |t: &str, col: iced::Color| {
                text(t.to_string())
                    .font(preview_font)
                    .size(11)
                    .style(move |_t| text::Style { color: Some(col) })
            };

            let preview_box = container(
                column![
                    row![ptoken(";; OpenCAD LISP Syntax Preview", preview_comment)],
                    row![
                        ptoken("(", preview_paren),
                        ptoken("defun", preview_kw),
                        ptoken(" ", preview_def),
                        ptoken("my-custom-func", preview_custom),
                        ptoken(" (", preview_paren),
                        ptoken("x msg / r", preview_def),
                        ptoken(")", preview_paren),
                    ],
                    row![
                        ptoken("  (", preview_paren),
                        ptoken("setq", preview_kw),
                        ptoken(" r (", preview_paren),
                        ptoken("*", preview_builtin),
                        ptoken(" x ", preview_def),
                        ptoken("3.14159", preview_num),
                        ptoken("))", preview_paren),
                    ],
                    row![
                        ptoken("  (", preview_paren),
                        ptoken("princ", preview_builtin),
                        ptoken(" (", preview_paren),
                        ptoken("strcat", preview_builtin),
                        ptoken(" msg ", preview_def),
                        ptoken("\" radius: \"", preview_str),
                        ptoken("))", preview_paren),
                    ],
                    row![
                        ptoken("  (", preview_paren),
                        ptoken("command", preview_builtin),
                        ptoken(" ", preview_def),
                        ptoken("\"_circle\"", preview_str),
                        ptoken(" '(", preview_paren),
                        ptoken("0 0", preview_num),
                        ptoken(") r)", preview_paren),
                    ],
                    row![ptoken(")", preview_paren)],
                ]
                .spacing(2)
            )
            .padding(8)
            .width(Fill)
            .style(move |_t| container::Style {
                background: Some(Background::Color(preview_bg)),
                border: Border {
                    color: IDE_DARK_GRAY,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            column![
                font_row,
                dialog_sep(),
                theme_row,
                dialog_sep(),
                syntax_middle_section,
                dialog_sep(),
                custom_funcs_section,
                dialog_sep(),
                text("Live Syntax Preview").size(13),
                preview_box,
            ]
            .spacing(8)
            .into()
        }
        SettingsTab::Formatting => {
            let opts = &settings.format_options;

            // 1. Indent Size selector
            let indent_sizes = [2usize, 3, 4];
            let mut indent_row = row![
                text("Indentation Size:").size(12),
            ]
            .spacing(6)
            .align_y(iced::Center);

            for size in indent_sizes {
                let is_sel = opts.indent_size == size;
                let lbl = if size == 2 {
                    "2 spaces (default)"
                } else if size == 3 {
                    "3 spaces"
                } else {
                    "4 spaces"
                };
                let btn = button(
                    row![
                        text(if is_sel { "● " } else { "○ " }).size(12),
                        text(lbl).size(12),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(IdeMessage::SetFormatIndentSize(size))
                .padding([4, 8])
                .style(if is_sel { button::primary } else { ide_secondary_button });
                indent_row = indent_row.push(btn);
            }

            // 2. Downcase Built-in Keywords & Functions Toggle
            let downcase_btn = button(
                row![
                    text(if opts.downcase_builtins { "☑" } else { "☐" }).size(14),
                    text("Convert built-in keywords and functions to lowercase (defun, setq, getpoint, princ, etc.)").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::ToggleFormatDowncaseBuiltins(!opts.downcase_builtins))
            .padding([4, 8])
            .style(if opts.downcase_builtins { button::primary } else { ide_secondary_button });

            let downcase_note = text(
                "Standardizes language keywords and built-in functions to lowercase, and boolean constants to T and nil, while preserving original casing for user variables and strings."
            )
            .size(11)
            .style(muted_style);

            // 3. Wide-Style Multi-Line Function Calls Toggle
            let wide_btn = button(
                row![
                    text(if opts.wide_style_calls { "☑" } else { "☐" }).size(14),
                    text("Wide Style: Align multi-line function call arguments under the first argument").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::ToggleFormatWideStyle(!opts.wide_style_calls))
            .padding([4, 8])
            .style(if opts.wide_style_calls { button::primary } else { ide_secondary_button });

            let wide_note = text(
                "For regular function calls like (command \"_.LINE\" ...) or (strcat ...), subsequent lines align vertically under the first argument on line 1."
            )
            .size(11)
            .style(muted_style);

            // 4. Align setq/set pairs Toggle
            let align_setq_btn = button(
                row![
                    text(if opts.align_setq_pairs { "☑" } else { "☐" }).size(14),
                    text("Align variable-value pairs in (setq ...) blocks").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::ToggleFormatAlignSetq(!opts.align_setq_pairs))
            .padding([4, 8])
            .style(if opts.align_setq_pairs { button::primary } else { ide_secondary_button });

            let align_setq_note = text(
                "In multi-line (setq ...) assignments, subsequent variable names align vertically under the first variable."
            )
            .size(11)
            .style(muted_style);

            // 5. Structural Re-flow (Line Breaking & Line Joining) Toggle
            let reflow_btn = button(
                row![
                    text(if opts.structural_reflow { "☑" } else { "☐" }).size(14),
                    text("Structural Re-flow: Move expressions between lines (classic LISP breaks & joins)").size(12),
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::ToggleFormatStructuralReflow(!opts.structural_reflow))
            .padding([4, 8])
            .style(if opts.structural_reflow { button::primary } else { ide_secondary_button });

            let reflow_note = text(
                "Expands defun bodies, progn, while/repeat loops, multi-pair setq, and cond clauses onto separate lines; condenses fragmented atoms/parentheses into single lines."
            )
            .size(11)
            .style(muted_style);

            // 6. Actions & Shortcuts Info Box
            let format_doc_btn = button(
                row![
                    text("⚡ Format Active Document Now").size(12),
                ]
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::FormatDocument)
            .padding([6, 12])
            .style(button::primary);

            let shortcuts_info = column![
                text("Keyboard & Context Shortcuts:").size(12),
                row![
                    text("• Format Entire Document:").size(11).style(muted_style),
                    text(" Shift+Alt+F  or  Ctrl+Alt+F (default)").size(11),
                ]
                .spacing(4),
                row![
                    text("• Format Selection:").size(11).style(muted_style),
                    text(" Ctrl+Shift+F  or  Right-Click → Format Selection").size(11),
                ]
                .spacing(4),
                row![
                    text("• Smart Indent on Enter:").size(11).style(muted_style),
                    text(" Enter automatically computes indentation based on current rules").size(11),
                ]
                .spacing(4),
                row![
                    text("• Comments:").size(11).style(muted_style),
                    text(" ;;; stays at column 0  |  ;; aligns to code block").size(11),
                ]
                .spacing(4),
            ]
            .spacing(4);

            column![
                text("OpenCAD LISP Formatting Guidelines").size(13),
                text("Configure automatic indentation, symbol casing, and alignment rules matching classic LISP and standard CAD specifications.").size(11).style(muted_style),
                Space::new().height(Length::Fixed(4.0)),
                dialog_sep(),
                indent_row,
                dialog_sep(),
                column![reflow_btn, reflow_note].spacing(4),
                dialog_sep(),
                column![downcase_btn, downcase_note].spacing(4),
                dialog_sep(),
                column![wide_btn, wide_note].spacing(4),
                dialog_sep(),
                column![align_setq_btn, align_setq_note].spacing(4),
                dialog_sep(),
                shortcuts_info,
                Space::new().height(Length::Fixed(6.0)),
                format_doc_btn,
            ]
            .spacing(8)
            .into()
        }
    };

    let dialog_content = column![
        title_row,
        tabs_row,
        dialog_sep(),
        body_content,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fixed(700.0));

    let dialog_height = (window_height - 60.0).clamp(680.0, 860.0);

    container(
        scrollable(dialog_content)
            .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::default()))
            .height(Length::Fixed(dialog_height))
    )
    .style(move |_t: &Theme| {
        container::Style {
            background: Some(Background::Color(IDE_DARK_GRAY)),
            border: Border {
                color: IDE_DARK_GRAY,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}


fn render_ide_layout<'a>(
    tabs: &'a [IdeTab],
    active_tab: usize,
    console_log: &'a text_editor::Content,
    repl_content: &'a text_editor::Content,
    repl_mode: crate::ide::protocol::ReplMode,
    api_drawer_open: bool,
    api_search: &'a str,
    theme: &'a Theme,
    layout_width: Length,
    layout_height: Length,
    is_standalone_window: bool,
    is_debugging: bool,
    variables_open: bool,
    debug_locals: &'a [(String, String, String)],
    debug_stack: &'a [(String, usize, String)],
    is_transparent: bool,
    watch_vars: &'a [String],
    console_height: f32,
    is_splitter_dragging: bool,
    visible_lines: usize,
    settings: &'a IdeSettings,
    settings_open: bool,
    help_menu_open: bool,
    settings_tab: SettingsTab,
    active_color_target: SyntaxColorTarget,
    syntax_hex_input: &'a str,
    font_size_input: &'a str,
    transparency_input: &'a str,
    selected_theme_name: &'a str,
    new_theme_name: &'a str,
    selected_custom_func: Option<&'a str>,
    new_custom_func_input: &'a str,
    autocomplete_matches: Option<&'a [String]>,
    autocomplete_selected_index: usize,
    context_menu: Option<&'a ContextMenuState>,
    save_prompt: Option<&'a crate::ide::SavePromptState>,
    find_replace: &'a crate::ide::FindReplaceState,
    window_width: f32,
    window_height: f32,
) -> Element<'a, IdeMessage> {
    #[cfg(not(windows))]
    let trans_fraction = (100.0 - settings.transparency_percent as f32).max(10.0) / 100.0;
    #[cfg(windows)]
    let alpha = 1.0;
    #[cfg(not(windows))]
    let alpha = if is_transparent { trans_fraction } else { 1.0 };

    // ── 1. Top Toolbar ────────────────────────────────────────────────────────
    let vsep = move || {
        container(Space::new())
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(28.0))
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                ..Default::default()
            })
    };
    let hsep = move || {
        container(Space::new())
            .width(Fill)
            .height(Length::Fixed(1.0))
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                ..Default::default()
            })
    };
    let panel_vsep = move || {
        container(Space::new())
            .width(Length::Fixed(1.0))
            .height(Fill)
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                ..Default::default()
            })
    };
    let mut toolbar_items = row![].spacing(6).align_y(iced::Center);

    if is_debugging {
        let btn_continue = ide_tooltip(
            button(text("▶").size(20))
                .on_press(IdeMessage::DebugContinue)
                .padding([6, 12])
                .style(button::primary),
            "Continue (F5)",
        );

        let btn_step_over = ide_tooltip(
            button(text("↷").size(22))
                .on_press(IdeMessage::DebugStepOver)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Step Over (F10)",
        );

        let btn_step_into = ide_tooltip(
            button(text("⇲").size(22))
                .on_press(IdeMessage::DebugStepInto)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Step Into (F11)",
        );

        let btn_step_out = ide_tooltip(
            button(text("⇱").size(22))
                .on_press(IdeMessage::DebugStepOut)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Step Out (Shift+F11)",
        );

        let btn_restart = ide_tooltip(
            button(text("↻").size(22))
                .on_press(IdeMessage::DebugRestart)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Restart (Ctrl+Shift+F5)",
        );

        let btn_stop = ide_tooltip(
            button(text("■").size(20))
                .on_press(IdeMessage::DebugStop)
                .padding([6, 12])
                .style(button::danger),
            "Stop (Shift+F5)",
        );

        toolbar_items = toolbar_items
            .push(btn_continue)
            .push(btn_step_over)
            .push(btn_step_into)
            .push(btn_step_out)
            .push(btn_restart)
            .push(btn_stop)
            .push(text("● DEBUGGING").size(13).style(primary_style))
            .push(vsep());
    } else {
        let btn_run_all = ide_tooltip(
            button(text("▶").size(20))
                .on_press(IdeMessage::RunAll)
                .padding([6, 14])
                .style(button::primary),
            "Load Script (F5)",
        );

        let btn_run_sel = ide_tooltip(
            button(text("▷").size(20))
                .on_press(IdeMessage::RunSelection)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Load Selection (F8)",
        );

        let btn_debug = ide_tooltip(
            button(
                row![
                    text("▶").size(20),
                    text("●").size(10).style(move |_t| text::Style {
                        color: Some(iced::Color::from_rgb(0.95, 0.25, 0.25))
                    }),
                ]
                .spacing(3)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::DebugStart)
            .padding([6, 12])
            .style(ide_secondary_button),
            "Start Debugging",
        );

        let btn_step_over = ide_tooltip(
            button(text("↷").size(22))
                .on_press(IdeMessage::DebugStartStep)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Step Over (F10)",
        );

        let btn_step_into = ide_tooltip(
            button(text("⇲").size(22))
                .on_press(IdeMessage::DebugStartStep)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Step Into (F11)",
        );

        let btn_restart = ide_tooltip(
            button(text("↻").size(22))
                .on_press(IdeMessage::DebugRestart)
                .padding([6, 12])
                .style(ide_secondary_button),
            "Restart / Re-run (Ctrl+Shift+F5)",
        );

        toolbar_items = toolbar_items
            .push(btn_run_all)
            .push(btn_run_sel)
            .push(btn_debug)
            .push(btn_step_over)
            .push(btn_step_into)
            .push(btn_restart)
            .push(vsep());
    }

    let btn_new = ide_tooltip(
        button(text("+").size(20))
            .on_press(IdeMessage::NewTab)
            .padding([6, 12])
            .style(ide_secondary_button),
        "New Script Tab",
    );

    let btn_open = ide_tooltip(
        button(text("Open").size(14))
            .on_press(IdeMessage::OpenFile)
            .padding([6, 12])
            .style(ide_secondary_button),
        "Open Script File",
    );

    let btn_save = ide_tooltip(
        button(text("Save").size(14))
            .on_press(IdeMessage::SaveTab)
            .padding([6, 12])
            .style(ide_secondary_button),
        "Save Script File (Ctrl+S)",
    );

    let btn_save_as = ide_tooltip(
        button(text("Save As...").size(14))
            .on_press(IdeMessage::SaveTabAs)
            .padding([6, 12])
            .style(ide_secondary_button),
        "Save Script As New File (Ctrl+Shift+S)",
    );

    let btn_undo = ide_tooltip(
        button(text("↶").size(16))
            .on_press(IdeMessage::Undo)
            .padding([6, 10])
            .style(ide_secondary_button),
        "Undo (Ctrl+Z)",
    );

    let btn_redo = ide_tooltip(
        button(text("↷").size(16))
            .on_press(IdeMessage::Redo)
            .padding([6, 10])
            .style(ide_secondary_button),
        "Redo (Ctrl+Y / Ctrl+Shift+Z)",
    );

    let has_tab_selection = tabs.get(active_tab).map_or(false, |t| {
        t.content.selection().is_some() || t.last_edit_cursor.and_then(|c| c.selection).is_some()
    });

    let btn_format = ide_tooltip(
        button(text("Format").size(14))
            .on_press(if has_tab_selection {
                IdeMessage::FormatSelection
            } else {
                IdeMessage::FormatDocument
            })
            .padding([6, 12])
            .style(ide_secondary_button),
        "Format OpenCAD LISP Document (Shift+Alt+F) | Selection (Ctrl+Shift+F)",
    );

    let btn_find = ide_tooltip(
        button(
            row![
                text("🔍").size(13),
                text("Find").size(14),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::OpenFind)
        .padding([6, 12])
        .style(if find_replace.is_open {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Find (Ctrl+F) | Replace (Ctrl+H)",
    );

    let btn_variables = ide_tooltip(
        button(
            row![
                text(if variables_open { "▾" } else { "▸" }).size(12),
                text("Variables").size(14),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ToggleVariables)
        .padding([6, 12])
        .style(if variables_open {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Toggle Variables Panel",
    );

    let btn_api_drawer = ide_tooltip(
        button(
            row![
                text(if api_drawer_open { "◀" } else { "▶" }).size(12),
                text("λ Function List").size(14),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ToggleApiDrawer)
        .padding([6, 12])
        .style(if api_drawer_open {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Toggle OpenCAD LISP Function List & Reference (F2)",
    );

    let btn_clear = ide_tooltip(
        button(text("Clear Output").size(13))
            .on_press(IdeMessage::ClearConsole)
            .padding([6, 10])
            .style(button::text),
        "Clear Output Console",
    );

    let switch_to_cad_btn = if is_standalone_window {
        Some(
            button(
                row![
                    text("⮂").size(14),
                    text("Switch to CAD").size(14),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(IdeMessage::FocusCad)
            .padding([6, 12])
            .style(ide_secondary_button),
        )
    } else {
        None
    };

    let trans_label = format!("{}%", settings.transparency_percent);
    let btn_transparency = ide_tooltip(
        button(
            row![
                text("◐").size(14),
                text(trans_label).size(13),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ToggleTransparency)
        .padding([6, 12])
        .style(if is_transparent {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Toggle Window Transparency",
    );

    let btn_settings = ide_tooltip(
        button(
            row![
                text("⚙").size(14),
                text("Settings").size(13),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ToggleSettings)
        .padding([6, 12])
        .style(if settings_open {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Open IDE Settings",
    );

    let btn_help = ide_tooltip(
        button(
            row![
                text("?").size(14),
                text("Help ▾").size(13),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ToggleHelpMenu)
        .padding([6, 12])
        .style(if help_menu_open {
            button::primary
        } else {
            ide_secondary_button
        }),
        "Help & Tutorials",
    );

    toolbar_items = toolbar_items
        .push(btn_new)
        .push(btn_open)
        .push(btn_save)
        .push(btn_save_as)
        .push(btn_undo)
        .push(btn_redo)
        .push(btn_format)
        .push(btn_find)
        .push(vsep())
        .push(btn_variables)
        .push(btn_api_drawer)
        .push(Space::new().width(Fill));

    if let Some(btn) = switch_to_cad_btn {
        toolbar_items = toolbar_items.push(btn);
    }
    toolbar_items = toolbar_items.push(btn_transparency);
    toolbar_items = toolbar_items.push(btn_settings);
    toolbar_items = toolbar_items.push(btn_help);
    toolbar_items = toolbar_items.push(btn_clear);

    let toolbar_bar = container(toolbar_items)
        .padding([4, 6])
        .width(Fill)
        .height(Length::Shrink)
        .style(move |_t: &Theme| {
            container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                border: Border {
                    color: IDE_DARK_GRAY.scale_alpha(alpha),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

    // ── 2. File Tabs Header ───────────────────────────────────────────────────
    let mut tab_buttons = row![].spacing(4);
    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = idx == active_tab;
        let dirty_mark = if tab.dirty { "*" } else { "" };
        let tab_label = format!("{}{}", tab.title, dirty_mark);

        let close_btn = button(text("✕").size(10))
            .on_press(IdeMessage::CloseTab(idx))
            .padding([1, 4])
            .style(button::text);

        let tab_item = button(
            row![
                text(tab_label).size(12),
                close_btn,
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::SelectTab(idx))
        .padding([4, 8])
        .style(if is_active {
            button::primary
        } else {
            ide_secondary_button
        });

        tab_buttons = tab_buttons.push(tab_item);
    }
    let tab_bar = scrollable(tab_buttons).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new(),
    ));

    // ── 3. Code Editor & Line Number / Breakpoint Gutter ─────────────────────
    let (editor_element, status_text): (Element<'a, IdeMessage>, String) = if let Some(current) = tabs.get(active_tab) {
        let cursor = current.last_edit_cursor.unwrap_or_else(|| current.content.cursor());
        let total_lines = current.content.line_count().max(1);
        let bp_count = current.breakpoints.len();

        let status = if is_debugging {
            if let Some((sl, sc, _el, _ec)) = current.debug_paused_span {
                let expr_preview = current.debug_paused_expr.as_deref().unwrap_or("");
                format!(
                    "▶ PAUSED at Ln {sl}, Col {sc} {expr_preview}  |  F5: Continue  |  F10: Step Over  |  F11: Step Into  |  Shift+F11: Step Out  |  Shift+F5: Stop  |  {bp_count} breakpoint(s)"
                )
            } else if let Some(pline) = current.debug_paused_line {
                format!(
                    "▶ PAUSED at Line {}  |  F5: Continue  |  F10: Step Over  |  F11: Step Into  |  Shift+F11: Step Out  |  Shift+F5: Stop  |  {} breakpoint(s)",
                    pline, bp_count
                )
            } else {
                format!("🐞 DEBUGGING active  |  {} breakpoint(s) (F9: Toggle BP)", bp_count)
            }
        } else {
            let bps_info = if bp_count > 0 {
                let formatted: Vec<String> = current
                    .breakpoints
                    .iter()
                    .map(|bp| format!("{}:{}", bp.line, bp.col))
                    .collect();
                format!("Breakpoints: [{}]", formatted.join(", "))
            } else {
                "No breakpoints".to_string()
            };
            format!(
                "Ln {}, Col {}  |  {} lines  |  {}  |  OpenCAD LISP  |  F5: Run, F9: Breakpoint, Shift+Alt+F: Format",
                cursor.position.line + 1,
                cursor.position.column + 1,
                total_lines,
                bps_info
            )
        };

        let font_size = settings.font_size();
        let line_height = (font_size as f32 * 1.6).round().max(16.0);
        let char_w = settings.editor_char_width();
        const PADDING_Y: f32 = 6.0;
        const PADDING_X: f32 = 8.0;
        let exact_editor_height = (visible_lines as f32) * line_height + 2.0 * PADDING_Y;

        let tab_text = current.content.text();
        let all_lines: Vec<&str> = tab_text.lines().collect();

        let scroll_top = current.scroll_top_line;
        let visible_start = scroll_top + 1;
        let visible_end = (scroll_top + visible_lines).min(total_lines);

        let mut gutter_items: Vec<Element<'a, IdeMessage>> = Vec::new();
        let mut highlight_items: Vec<Element<'a, IdeMessage>> = Vec::new();

        let mut last_hl_y = 0.0;

        for line_num in visible_start..=visible_end {
            let line_bps = current.breakpoints_on_line(line_num);
            let is_bp = !line_bps.is_empty();
            let is_paused = current
                .debug_paused_span
                .is_some_and(|(sl, _, el, _)| line_num >= sl && line_num <= el)
                || current.debug_paused_line == Some(line_num);

            let line_str = all_lines.get(line_num.saturating_sub(1)).copied().unwrap_or("");

            let (marker_text, marker_color) = if is_paused {
                ("▶", iced::Color::from_rgb(0.2, 0.95, 0.2))
            } else if is_bp {
                ("●", iced::Color::from_rgb(0.95, 0.2, 0.2))
            } else {
                ("", iced::Color::TRANSPARENT)
            };

            let label_text = if !marker_text.is_empty() {
                marker_text.to_string()
            } else {
                format!("{line_num}")
            };
            let text_color = if !marker_text.is_empty() {
                marker_color
            } else {
                iced::Color::from_rgb(0.45, 0.45, 0.45)
            };

            let marker_btn = button(
                container(
                    text(label_text)
                        .size(10)
                        .font(settings.editor_font())
                        .style(move |_t| text::Style { color: Some(text_color) })
                )
                .center_x(Fill)
                .center_y(Fill)
            )
            .on_press(IdeMessage::ToggleBreakpoint(line_num))
            .height(Length::Fixed(line_height))
            .width(Length::Fixed(32.0))
            .padding(0)
            .style(move |t: &Theme, _status| {
                if is_paused {
                    button::Style {
                        background: Some(Background::Color(settings.execution_color)),
                        border: Border {
                            color: iced::Color { a: 1.0, ..settings.execution_color },
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        text_color: t.palette().background.base.text,
                        ..Default::default()
                    }
                } else if is_bp {
                    button::Style {
                        background: Some(Background::Color(settings.breakpoint_color)),
                        border: Border {
                            color: iced::Color { a: 1.0, ..settings.breakpoint_color },
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        text_color: t.palette().background.base.text,
                        ..Default::default()
                    }
                } else {
                    button::text(t, _status)
                }
            });

            gutter_items.push(marker_btn.into());

            // Highlight bar (S-expression span when paused, or parenthesis badges for breakpoints):
            if is_paused || is_bp {
                let rel_idx = line_num - visible_start;
                let target_y = PADDING_Y + (rel_idx as f32) * line_height;
                if target_y > last_hl_y {
                    highlight_items.push(Space::new().height(Length::Fixed(target_y - last_hl_y)).into());
                }

                if let Some((sl, sc, el, ec)) = current.debug_paused_span.filter(|&(sl, _, el, _)| line_num >= sl && line_num <= el) {
                    let bg_color = iced::Color {
                        a: (settings.execution_color.a * 0.75).clamp(0.22, 0.85),
                        ..settings.execution_color
                    };
                    let border_color = iced::Color {
                        a: 1.0,
                        ..settings.execution_color
                    };
                    let v_start = if line_num == sl {
                        crate::ide::highlight::visual_col_for_byte_col(line_str, sc)
                    } else {
                        0
                    };
                    let v_end = if line_num == el {
                        crate::ide::highlight::visual_col_for_byte_col(line_str, ec)
                    } else {
                        crate::ide::highlight::visual_col_for_byte_col(line_str, line_str.len() + 1)
                    };
                    let span_cols = if line_num == sl && line_num == el {
                        v_end.saturating_sub(v_start) + 1
                    } else if line_num == el {
                        v_end + 1
                    } else {
                        60
                    };
                    let offset_x = (PADDING_X + (v_start as f32) * char_w - 1.0).max(0.0);
                    let pill_w = ((span_cols.max(1)) as f32) * char_w + 2.0;

                    let paused_pill = container(Space::new())
                        .height(Length::Fixed(line_height))
                        .width(Length::Fixed(pill_w))
                        .style(move |_t: &Theme| container::Style {
                            background: Some(Background::Color(bg_color)),
                            border: Border {
                                color: border_color,
                                width: 1.5,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        });

                    let line_row = row![
                        Space::new().width(Length::Fixed(offset_x)),
                        paused_pill,
                    ]
                    .height(Length::Fixed(line_height))
                    .width(Fill);

                    highlight_items.push(line_row.into());
                } else if is_paused {
                    let bg_color = iced::Color {
                        a: (settings.execution_color.a * 0.7).clamp(0.15, 0.8),
                        ..settings.execution_color
                    };
                    let border_color = iced::Color {
                        a: 1.0,
                        ..settings.execution_color
                    };
                    let line_bg = container(Space::new())
                        .height(Length::Fixed(line_height))
                        .width(Fill)
                        .style(move |_t: &Theme| container::Style {
                            background: Some(Background::Color(bg_color)),
                            border: Border {
                                color: border_color,
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        });
                    highlight_items.push(line_bg.into());
                } else {
                    // Render distinct parenthesis breakpoint badges at each bp.col on this line
                    let bg_color = iced::Color {
                        a: (settings.breakpoint_color.a * 0.85).clamp(0.30, 0.90),
                        ..settings.breakpoint_color
                    };
                    let border_color = iced::Color {
                        a: 1.0,
                        ..settings.breakpoint_color
                    };
                    let mut row_items: Vec<Element<'a, IdeMessage>> = Vec::new();
                    let mut cur_x = 0.0_f32;
                    for bp in &line_bps {
                        let v_col = crate::ide::highlight::visual_col_for_byte_col(line_str, bp.col);
                        let glyph_x = PADDING_X + (v_col as f32) * char_w;
                        let bp_x = (glyph_x - 1.0).max(cur_x);
                        if bp_x > cur_x {
                            row_items.push(Space::new().width(Length::Fixed(bp_x - cur_x)).into());
                            cur_x = bp_x;
                        }
                        let right_edge = glyph_x + char_w + 1.0;
                        let badge_w = (right_edge - cur_x).max(char_w);
                        let bp_badge = container(Space::new())
                            .height(Length::Fixed(line_height))
                            .width(Length::Fixed(badge_w))
                            .style(move |_t: &Theme| container::Style {
                                background: Some(Background::Color(bg_color)),
                                border: Border {
                                    color: border_color,
                                    width: 1.0,
                                    radius: 3.0.into(),
                                },
                                ..Default::default()
                            });
                        row_items.push(bp_badge.into());
                        cur_x += badge_w;
                    }
                    let line_row = row(row_items)
                        .height(Length::Fixed(line_height))
                        .width(Fill);
                    highlight_items.push(line_row.into());
                }
                last_hl_y = target_y + line_height;
            }
        }

        let gutter_col = column(gutter_items)
            .width(Length::Fixed(32.0))
            .spacing(0);

        let gutter_container = container(
            column![
                Space::new().height(Length::Fixed(PADDING_Y)),
                gutter_col,
            ]
        )
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(exact_editor_height))
        .style(move |_t: &Theme| {
            container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                border: Border {
                    color: IDE_DARK_GRAY.scale_alpha(alpha),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        let highlights_col = column(highlight_items)
            .width(Fill)
            .height(Length::Fixed(exact_editor_height));

        let mut search_highlight_items: Vec<Element<'a, IdeMessage>> = Vec::new();
        let mut last_sh_y = 0.0;
        if !find_replace.matches.is_empty() {
            for line_num in visible_start..=visible_end {
                let line_idx = line_num.saturating_sub(1);
                let line_matches: Vec<(usize, &crate::ide::SearchMatch)> = find_replace
                    .matches
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.line == line_idx)
                    .collect();

                if !line_matches.is_empty() {
                    let rel_idx = line_num - visible_start;
                    let target_y = PADDING_Y + (rel_idx as f32) * line_height;
                    if target_y > last_sh_y {
                        search_highlight_items.push(Space::new().height(Length::Fixed(target_y - last_sh_y)).into());
                    }

                    let line_str = all_lines.get(line_idx).copied().unwrap_or("");
                    let mut row_items: Vec<Element<'a, IdeMessage>> = Vec::new();
                    let mut cur_x = 0.0_f32;

                    for (m_idx, m) in line_matches {
                        let v_start = crate::ide::highlight::visual_col_for_byte_col(line_str, m.col + 1);
                        let v_end = crate::ide::highlight::visual_col_for_byte_col(line_str, m.col + m.len + 1);
                        let glyph_x = PADDING_X + (v_start as f32) * char_w;
                        let pill_x = (glyph_x - 1.0).max(cur_x);
                        if pill_x > cur_x {
                            row_items.push(Space::new().width(Length::Fixed(pill_x - cur_x)).into());
                            cur_x = pill_x;
                        }
                        let pill_w = (((v_end.saturating_sub(v_start)).max(1) as f32) * char_w + 2.0).max(char_w);
                        let is_current = find_replace.current_match_idx == Some(m_idx);
                        let (bg_col, border_col) = if is_current {
                            (iced::Color::from_rgba(0.96, 0.60, 0.05, 0.75), iced::Color::from_rgb(1.0, 0.82, 0.2))
                        } else {
                            (iced::Color::from_rgba(0.95, 0.75, 0.15, 0.35), iced::Color::from_rgba(0.95, 0.75, 0.15, 0.6))
                        };
                        let pill = container(Space::new())
                            .height(Length::Fixed(line_height))
                            .width(Length::Fixed(pill_w))
                            .style(move |_t: &Theme| container::Style {
                                background: Some(Background::Color(bg_col)),
                                border: Border {
                                    color: border_col,
                                    width: 1.0,
                                    radius: 2.0.into(),
                                },
                                ..Default::default()
                            });
                        row_items.push(pill.into());
                        cur_x += pill_w;
                    }

                    let line_row = row(row_items)
                        .height(Length::Fixed(line_height))
                        .width(Fill);
                    search_highlight_items.push(line_row.into());
                    last_sh_y = target_y + line_height;
                }
            }
        }
        let search_highlights_col = column(search_highlight_items)
            .width(Fill)
            .height(Length::Fixed(exact_editor_height));

        let mut selection_highlight_items: Vec<Element<'a, IdeMessage>> = Vec::new();
        let mut last_sel_y = 0.0_f32;
        if current.is_scrolling_away {
            if let Some(edit_cursor) = current.last_edit_cursor {
                if let Some(sel) = edit_cursor.selection {
                    if edit_cursor.position != sel {
                        let (start_pos, end_pos) = if edit_cursor.position.line < sel.line
                            || (edit_cursor.position.line == sel.line
                                && edit_cursor.position.column <= sel.column)
                        {
                            (edit_cursor.position, sel)
                        } else {
                            (sel, edit_cursor.position)
                        };
                        let sel_bg = iced::Color::from_rgba(0.24, 0.52, 0.88, 0.35);

                        for line_num in visible_start..=visible_end {
                            let line_idx = line_num.saturating_sub(1);
                            if line_idx < start_pos.line || line_idx > end_pos.line {
                                continue;
                            }
                            let line_str = all_lines.get(line_idx).copied().unwrap_or("");
                            let byte_start = if line_idx == start_pos.line {
                                start_pos.column.min(line_str.len())
                            } else {
                                0
                            };
                            let byte_end = if line_idx == end_pos.line {
                                end_pos.column.min(line_str.len())
                            } else {
                                line_str.len()
                            };
                            if line_idx == end_pos.line && byte_end <= byte_start {
                                continue;
                            }

                            let v_start = crate::ide::highlight::visual_col_for_byte_col(line_str, byte_start + 1);
                            let v_end = crate::ide::highlight::visual_col_for_byte_col(line_str, byte_end + 1);
                            let cols = v_end.saturating_sub(v_start)
                                + if line_idx < end_pos.line { 1 } else { 0 };
                            if cols == 0 {
                                continue;
                            }

                            let rel_idx = line_num - visible_start;
                            let target_y = PADDING_Y + (rel_idx as f32) * line_height;
                            if target_y > last_sel_y {
                                selection_highlight_items.push(
                                    Space::new().height(Length::Fixed(target_y - last_sel_y)).into(),
                                );
                            }
                            let pill_x = PADDING_X + (v_start as f32) * char_w;
                            let pill_w = (cols as f32) * char_w;
                            let pill = container(Space::new())
                                .height(Length::Fixed(line_height))
                                .width(Length::Fixed(pill_w))
                                .style(move |_t: &Theme| container::Style {
                                    background: Some(Background::Color(sel_bg)),
                                    ..Default::default()
                                });
                            let mut row_items: Vec<Element<'a, IdeMessage>> = Vec::new();
                            if pill_x > 0.0 {
                                row_items.push(Space::new().width(Length::Fixed(pill_x)).into());
                            }
                            row_items.push(pill.into());
                            selection_highlight_items.push(
                                row(row_items)
                                    .height(Length::Fixed(line_height))
                                    .width(Fill)
                                    .into(),
                            );
                            last_sel_y = target_y + line_height;
                        }
                    }
                }
            }
        }
        let selection_highlights_col = column(selection_highlight_items)
            .width(Fill)
            .height(Length::Fixed(exact_editor_height));

        let is_lisp = current.path.as_ref().map_or_else(
            || current.title.ends_with(".lsp") || current.title.ends_with(".lisp"),
            |p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp") || e.eq_ignore_ascii_case("lisp")),
        );
        let is_scrolling_away = current.is_scrolling_away;
        let lisp_settings = crate::ide::highlight::LispHighlightSettings {
            colors: settings.syntax_colors.clone(),
            custom_functions: settings.custom_functions_set.clone(),
            is_lisp,
            breakpoints: std::sync::Arc::new(current.breakpoints.clone()),
        };
        let has_autocomplete = autocomplete_matches.is_some_and(|m| !m.is_empty());
        let find_is_open = find_replace.is_open;
        let ed = text_editor(&current.content)
            .id(EDITOR_ID)
            .on_action(IdeMessage::EditorAction)
            .font(settings.editor_font())
            .size(font_size as f32)
            .line_height(text::LineHeight::Absolute(iced::Pixels(line_height)))
            .padding([PADDING_Y, PADDING_X])
            .height(Length::Fixed(exact_editor_height))
            .wrapping(iced::widget::text::Wrapping::None)
            .highlight_with::<crate::ide::highlight::LispHighlighter>(
                lisp_settings,
                |format, _theme| *format,
            )
            .key_binding(move |key_press| {
                if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
                    return None;
                }
                if has_autocomplete {
                    match key_press.key.as_ref() {
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                            return Some(text_editor::Binding::Custom(IdeMessage::AutocompletePrev));
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                            return Some(text_editor::Binding::Custom(IdeMessage::AutocompleteNext));
                        }
                        keyboard::Key::Named(keyboard::key::Named::Tab) => {
                            return Some(text_editor::Binding::Custom(IdeMessage::AutocompleteAccept));
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            return Some(text_editor::Binding::Custom(IdeMessage::AutocompleteDismiss));
                        }
                        _ => {}
                    }
                }
                if find_is_open && matches!(key_press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Escape)) {
                    return Some(text_editor::Binding::Custom(IdeMessage::CloseFind));
                }
                match key_press.key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::F3) if key_press.modifiers.shift() => {
                        Some(text_editor::Binding::Custom(IdeMessage::FindPrev))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F3) => {
                        Some(text_editor::Binding::Custom(IdeMessage::FindNext))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F5) if key_press.modifiers.shift() => {
                        Some(text_editor::Binding::Custom(IdeMessage::DebugStop))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F5) => {
                        if is_debugging {
                            Some(text_editor::Binding::Custom(IdeMessage::DebugContinue))
                        } else {
                            Some(text_editor::Binding::Custom(IdeMessage::RunAll))
                        }
                    }
                    keyboard::Key::Named(keyboard::key::Named::F10) => {
                        Some(text_editor::Binding::Custom(IdeMessage::DebugStepOver))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F11) if key_press.modifiers.shift() => {
                        Some(text_editor::Binding::Custom(IdeMessage::DebugStepOut))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F11) => {
                        Some(text_editor::Binding::Custom(IdeMessage::DebugStepInto))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F9) if key_press.modifiers.shift() => {
                        Some(text_editor::Binding::Custom(IdeMessage::AddWatchAtCursor))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F9) => {
                        Some(text_editor::Binding::Custom(IdeMessage::ToggleBreakpointAtCursor))
                    }
                    keyboard::Key::Named(keyboard::key::Named::F8) => {
                        Some(text_editor::Binding::Custom(IdeMessage::RunSelection))
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) if key_press.modifiers.command() => {
                        Some(text_editor::Binding::Custom(IdeMessage::RunSelection))
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        Some(text_editor::Binding::Custom(IdeMessage::SmartEnter))
                    }
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        Some(text_editor::Binding::Custom(IdeMessage::SmartTab))
                    }
                    _ => {
                        let is_cmd = key_press.modifiers.command() || key_press.modifiers.control();
                        let is_f = match key_press.key.as_ref() {
                            keyboard::Key::Character(c) => c.eq_ignore_ascii_case("f"),
                            _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::KeyF)),
                        };
                        let is_bracket_right = match key_press.key.as_ref() {
                            keyboard::Key::Character(c) => c == "]" || c == "}",
                            _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketRight)),
                        };
                        let is_bracket_left = match key_press.key.as_ref() {
                            keyboard::Key::Character(c) => c == "[" || c == "{",
                            _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketLeft)),
                        };

                        if is_f && key_press.modifiers.shift() && key_press.modifiers.alt() {
                            return Some(text_editor::Binding::Custom(IdeMessage::FormatDocument));
                        }
                        if is_f && ((key_press.modifiers.shift() && is_cmd) || (key_press.modifiers.alt() && is_cmd)) {
                            return Some(text_editor::Binding::Custom(IdeMessage::FormatSelection));
                        }
                        if is_f && is_cmd {
                            return Some(text_editor::Binding::Custom(IdeMessage::OpenFind));
                        }
                        let is_h = match key_press.key.as_ref() {
                            keyboard::Key::Character(c) => c.eq_ignore_ascii_case("h"),
                            _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::KeyH)),
                        };
                        if is_h && is_cmd {
                            return Some(text_editor::Binding::Custom(IdeMessage::OpenReplace));
                        }
                        if is_bracket_right && is_cmd {
                            if key_press.modifiers.shift() {
                                return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: true }));
                            } else {
                                return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: true }));
                            }
                        }
                        if is_bracket_left && is_cmd {
                            if key_press.modifiers.shift() {
                                return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: false }));
                            } else {
                                return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: false }));
                            }
                        }

                        match key_press.key.as_ref() {
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") && is_cmd => {
                                if key_press.modifiers.shift() {
                                    Some(text_editor::Binding::Custom(IdeMessage::SaveTabAs))
                                } else {
                                    Some(text_editor::Binding::Custom(IdeMessage::SaveTab))
                                }
                            }
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("o") && is_cmd => {
                                Some(text_editor::Binding::Custom(IdeMessage::OpenFile))
                            }
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("n") && is_cmd => {
                                Some(text_editor::Binding::Custom(IdeMessage::NewTab))
                            }
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("w") && is_cmd => {
                                Some(text_editor::Binding::Custom(IdeMessage::CloseActiveTab))
                            }
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("z") && is_cmd => {
                                if key_press.modifiers.shift() {
                                    Some(text_editor::Binding::Custom(IdeMessage::Redo))
                                } else {
                                    Some(text_editor::Binding::Custom(IdeMessage::Undo))
                                }
                            }
                            keyboard::Key::Character(c) if c.eq_ignore_ascii_case("y") && is_cmd => {
                                Some(text_editor::Binding::Custom(IdeMessage::Redo))
                            }
                            _ => {
                                if is_scrolling_away {
                                    match key_press.key.as_ref() {
                                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::Left))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::Right))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::Up))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::Down))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::Home) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::Home))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::End) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::End))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::PageUp) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::PageUp))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::PageDown) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Move(text_editor::Motion::PageDown))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::SmartEnter))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Edit(text_editor::Edit::Backspace))))
                                        }
                                        keyboard::Key::Named(keyboard::key::Named::Delete) => {
                                            Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Edit(text_editor::Edit::Delete))))
                                        }
                                        keyboard::Key::Character(c) if !key_press.modifiers.command() && !key_press.modifiers.control() => {
                                            if let Some(ch) = c.chars().next() {
                                                Some(text_editor::Binding::Custom(IdeMessage::RestoreCursorAndAct(text_editor::Action::Edit(text_editor::Edit::Insert(ch)))))
                                            } else {
                                                None
                                            }
                                        }
                                        _ => None,
                                    }
                                } else {
                                    text_editor::Binding::from_key_press(key_press)
                                }
                            }
                        }
                    }
                }
            })
            .style(|_t: &Theme, _status| {
                text_editor::Style {
                    background: Background::Color(iced::Color::TRANSPARENT),
                    border: Border::default(),
                    placeholder: iced::Color::from_rgb(0.5, 0.5, 0.5),
                    value: settings.syntax_colors.default_text,
                    selection: iced::Color::from_rgba(0.2, 0.6, 1.0, 0.35),
                }
            });

        let autocomplete_overlay: Option<Element<'a, IdeMessage>> = if let Some(matches) = autocomplete_matches {
            if !matches.is_empty() {
                if cursor.position.line >= scroll_top && cursor.position.line < scroll_top + visible_lines {
                    let rel_line = cursor.position.line - scroll_top;
                    let cursor_y = PADDING_Y + (rel_line as f32) * line_height;
                    let cursor_x = PADDING_X + (cursor.position.column.min(60) as f32) * char_w;
                    let top_offset = cursor_y + line_height + 2.0;

                    Some(render_autocomplete_popup(
                        matches,
                        autocomplete_selected_index,
                        cursor_x,
                        top_offset,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut editor_stack = stack![
            highlights_col,
            selection_highlights_col,
            search_highlights_col,
            ed,
        ];
        if let Some(popup) = autocomplete_overlay {
            editor_stack = editor_stack.push(popup);
        }
        if find_replace.is_open {
            editor_stack = editor_stack.push(render_find_replace_overlay(find_replace, settings, alpha));
        }
        let editor_stack: Element<'a, IdeMessage> = mouse_area(
            editor_stack
                .width(Fill)
                .height(Length::Fixed(exact_editor_height)),
        )
        .on_right_release(IdeMessage::EditorRightClicked(Point::ORIGIN))
        .into();

        // Visual scrollbar for large files:
        let scrollbar_element: Element<'a, IdeMessage> = if total_lines > visible_lines {
            let thumb_ratio = (visible_lines as f32 / total_lines as f32).clamp(0.04, 0.9);
            let offset_ratio = (scroll_top as f32 / total_lines as f32).clamp(0.0, 1.0 - thumb_ratio);
            let bottom_ratio = (1.0 - offset_ratio - thumb_ratio).max(0.0);

            let top_portion = ((offset_ratio * 1000.0).round() as u16).max(1);
            let thumb_portion = ((thumb_ratio * 1000.0).round() as u16).max(25);
            let bottom_portion = ((bottom_ratio * 1000.0).round() as u16).max(1);

            let up_btn = button(
                container(text("▲").size(8))
                    .center_x(Fill)
                    .center_y(Fill)
            )
            .on_press(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: -10 }))
            .height(Length::Fixed(14.0))
            .width(Length::Fixed(12.0))
            .padding(0)
            .style(|t: &Theme, _s| button::secondary(t, _s));

            let down_btn = button(
                container(text("▼").size(8))
                    .center_x(Fill)
                    .center_y(Fill)
            )
            .on_press(IdeMessage::EditorAction(text_editor::Action::Scroll { lines: 10 }))
            .height(Length::Fixed(14.0))
            .width(Length::Fixed(12.0))
            .padding(0)
            .style(|t: &Theme, _s| button::secondary(t, _s));

            let thumb = container(Space::new())
                .width(Length::Fixed(8.0))
                .height(Length::FillPortion(thumb_portion))
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::from_rgba(0.5, 0.5, 0.5, 0.45))),
                    border: Border {
                        color: iced::Color::from_rgba(0.6, 0.6, 0.6, 0.3),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });

            let track = column![
                up_btn,
                Space::new().height(Length::FillPortion(top_portion)),
                thumb,
                Space::new().height(Length::FillPortion(bottom_portion)),
                down_btn,
            ]
            .width(Length::Fixed(12.0))
            .height(Fill)
            .align_x(iced::Alignment::Center);

            container(track)
                .width(Length::Fixed(14.0))
                .height(Fill)
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::from_rgba(0.08, 0.08, 0.08, 0.4))),
                    ..Default::default()
                })
                .into()
        } else {
            Space::new().width(Length::Fixed(0.0)).into()
        };

        let editor_content_row = row![
            gutter_container,
            editor_stack,
            scrollbar_element,
        ]
        .spacing(4)
        .width(Fill)
        .height(Length::Fixed(exact_editor_height));

        let editor_pane = container(editor_content_row)
            .height(Length::Fixed(exact_editor_height))
            .width(Fill)
            .style(move |_t: &Theme| {
                let editor_bg = settings.syntax_colors.background.scale_alpha(alpha);
                container::Style {
                    background: Some(Background::Color(editor_bg)),
                    border: Border {
                        color: IDE_DARK_GRAY.scale_alpha(alpha),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        (
            editor_pane.into(),
            status,
        )
    } else {
        (
            container(text("No open file").size(13).style(muted_style))
                .height(Fill)
                .width(Fill)
                .center_x(Fill)
                .center_y(Fill)
                .into(),
            "No active file".to_string(),
        )
    };

    // ── 4. Console Log & REPL ────────────────────────────────────────────────
    let console_lisp_settings = crate::ide::highlight::LispHighlightSettings {
        colors: settings.syntax_colors.clone(),
        custom_functions: settings.custom_functions_set.clone(),
        is_lisp: true,
        breakpoints: std::sync::Arc::new(std::collections::BTreeSet::new()),
    };
    let console_view = text_editor(console_log)
        .id(CONSOLE_EDITOR_ID)
        .on_action(IdeMessage::ConsoleAction)
        .font(settings.editor_font())
        .size(11)
        .padding(6)
        .height(Fill)
        .highlight_with::<crate::ide::highlight::LispHighlighter>(
            console_lisp_settings,
            |format, _theme| *format,
        )
        .key_binding(move |key_press| {
            if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
                return None;
            }
            let is_cmd = key_press.modifiers.command() || key_press.modifiers.control();
            let is_bracket_right = match key_press.key.as_ref() {
                keyboard::Key::Character(c) => c == "]" || c == "}",
                _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketRight)),
            };
            let is_bracket_left = match key_press.key.as_ref() {
                keyboard::Key::Character(c) => c == "[" || c == "{",
                _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketLeft)),
            };

            if is_bracket_right && is_cmd {
                if key_press.modifiers.shift() {
                    return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: true }));
                } else {
                    return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: true }));
                }
            }
            if is_bracket_left && is_cmd {
                if key_press.modifiers.shift() {
                    return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: false }));
                } else {
                    return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: false }));
                }
            }
            text_editor::Binding::from_key_press(key_press)
        })
        .style({
            let default_text = settings.syntax_colors.default_text;
            move |_t: &Theme, _status| {
                text_editor::Style {
                    background: Background::Color(iced::Color::TRANSPARENT),
                    border: Border::default(),
                    placeholder: iced::Color::from_rgb(0.5, 0.5, 0.5),
                    value: default_text,
                    selection: iced::Color::from_rgba(0.2, 0.6, 1.0, 0.35),
                }
            }
        });

    let is_repl_lisp = repl_mode == crate::ide::protocol::ReplMode::Lisp;
    let (prompt_str, placeholder_str, _syntax_hl) = if is_repl_lisp {
        ("LISP>", "OpenCAD LISP code / expression (e.g. (+ 1 2) or (getpoint) - Enter to run, Ctrl+Enter for newline)...", "lisp")
    } else {
        (">>>", "Python code / expression (Enter to run, Ctrl+Enter for newline)...", "py")
    };

    let repl_lisp_settings = crate::ide::highlight::LispHighlightSettings {
        colors: settings.syntax_colors.clone(),
        custom_functions: settings.custom_functions_set.clone(),
        is_lisp: is_repl_lisp,
        breakpoints: std::sync::Arc::new(std::collections::BTreeSet::new()),
    };
    let repl_editor = text_editor(repl_content)
        .id(REPL_EDITOR_ID)
        .on_action(IdeMessage::ReplAction)
        .font(settings.editor_font())
        .size(11)
        .padding(iced::Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
        .height(Length::Fixed(56.0))
        .highlight_with::<crate::ide::highlight::LispHighlighter>(
            repl_lisp_settings,
            |format, _theme| *format,
        )
        .placeholder(placeholder_str)
        .key_binding(move |key_press| {
            if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
                return None;
            }
            let is_cmd = key_press.modifiers.command() || key_press.modifiers.control();
            let is_f = match key_press.key.as_ref() {
                keyboard::Key::Character(c) => c.eq_ignore_ascii_case("f"),
                _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::KeyF)),
            };
            let is_bracket_right = match key_press.key.as_ref() {
                keyboard::Key::Character(c) => c == "]" || c == "}",
                _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketRight)),
            };
            let is_bracket_left = match key_press.key.as_ref() {
                keyboard::Key::Character(c) => c == "[" || c == "{",
                _ => matches!(key_press.physical_key, keyboard::key::Physical::Code(keyboard::key::Code::BracketLeft)),
            };

            if is_f && ((key_press.modifiers.shift() && is_cmd) || (key_press.modifiers.alt() && is_cmd)) {
                return Some(text_editor::Binding::Custom(IdeMessage::FormatSelection));
            }
            if is_bracket_right && is_cmd {
                if key_press.modifiers.shift() {
                    return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: true }));
                } else {
                    return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: true }));
                }
            }
            if is_bracket_left && is_cmd {
                if key_press.modifiers.shift() {
                    return Some(text_editor::Binding::Custom(IdeMessage::SelectMatchingParen { forward: false }));
                } else {
                    return Some(text_editor::Binding::Custom(IdeMessage::JumpToMatchingParen { forward: false }));
                }
            }

            match key_press.key.as_ref() {
                keyboard::Key::Named(keyboard::key::Named::Enter)
                    if is_cmd || key_press.modifiers.shift() =>
                {
                    Some(text_editor::Binding::Custom(IdeMessage::ReplInsertNewline))
                }
                keyboard::Key::Named(keyboard::key::Named::Enter) => {
                    Some(text_editor::Binding::Custom(IdeMessage::ReplSubmit))
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) if !key_press.modifiers.shift() => {
                    Some(text_editor::Binding::Custom(IdeMessage::ReplHistoryPrev))
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) if !key_press.modifiers.shift() => {
                    Some(text_editor::Binding::Custom(IdeMessage::ReplHistoryNext))
                }
                _ => text_editor::Binding::from_key_press(key_press),
            }
        })
        .style({
            let default_text = settings.syntax_colors.default_text;
            move |_t: &Theme, _status| {
                text_editor::Style {
                    background: Background::Color(iced::Color::TRANSPARENT),
                    border: Border::default(),
                    placeholder: iced::Color::from_rgb(0.5, 0.5, 0.5),
                    value: default_text,
                    selection: iced::Color::from_rgba(0.2, 0.6, 1.0, 0.35),
                }
            }
        });

    let repl_container = container(repl_editor)
        .width(Fill)
        .height(Length::Fixed(56.0))
        .style({
            let bg = settings.syntax_colors.background;
            move |_t: &Theme| {
                container::Style {
                    background: Some(Background::Color(bg.scale_alpha(alpha))),
                    border: Border {
                        color: IDE_DARK_GRAY.scale_alpha(alpha),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            }
        });

    let repl_run_btn = ide_tooltip(
        button(
            row![
                text("▶").size(9),
                text("Run").size(11),
            ]
            .spacing(3)
            .align_y(iced::Center),
        )
        .on_press(IdeMessage::ReplSubmit)
        .padding([2, 6])
        .style(button::primary),
        if is_repl_lisp {
            "Run OpenCAD LISP Code (Enter to run, Ctrl+Enter for newline)"
        } else {
            "Run Python Code (Enter to run, Ctrl+Enter for newline)"
        },
    );

    let repl_clear_btn = ide_tooltip(
        button(text("Clear").size(10))
            .on_press(IdeMessage::ReplClear)
            .padding([2, 6])
            .style(button::text),
        "Clear REPL Input",
    );

    let repl_actions = column![
        repl_run_btn,
        repl_clear_btn,
    ]
    .spacing(2)
    .align_x(iced::alignment::Horizontal::Center);

    let console_pane = column![
        row![
            text("Console / Output").size(11).style(primary_style),
            Space::new().width(Fill),
            text("Interactive OpenCAD LISP REPL (Enter to run, Ctrl+Enter for newline)").size(10).style(muted_style),
        ]
        .spacing(6)
        .align_y(iced::Center),
        container(console_view)
            .height(Fill)
            .width(Fill)
            .style({
                let bg = settings.syntax_colors.background;
                move |_t: &Theme| {
                    container::Style {
                        background: Some(Background::Color(bg.scale_alpha(alpha))),
                        border: Border {
                            color: IDE_DARK_GRAY.scale_alpha(alpha),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                }
            }),
        row![
            text(prompt_str).size(12).font(settings.editor_font()).style(primary_style),
            repl_container,
            repl_actions,
        ]
        .spacing(6)
        .align_y(iced::Center),
    ]
    .spacing(4)
    .height(Length::Fixed(console_height));

    // Editor column with status bar
    let editor_area = column![
        editor_element,
        row![
            text(status_text).size(11).style(muted_style),
        ]
        .padding([2, 4])
        .align_y(iced::Center),
    ]
    .spacing(2)
    .height(Length::FillPortion(3));

    // ── 5. Variables & Call Stack Inspector Drawer ───────────────────────────
    let variables_drawer: Option<Element<'a, IdeMessage>> = if variables_open {
        let mut watch_list = column![].spacing(3);
        let mut header_row = row![
            text("Expression").size(13).style(primary_style).width(Length::Fixed(95.0)),
            text("Value").size(13).style(primary_style).width(Fill),
        ]
        .spacing(4)
        .align_y(iced::Center);

        if !watch_vars.is_empty() {
            header_row = header_row.push(
                button(text("Clear").size(12))
                    .on_press(IdeMessage::ClearWatch)
                    .padding([1, 4])
                    .style(button::text),
            );
        }
        watch_list = watch_list.push(header_row);
        watch_list = watch_list.push(hsep());

        if watch_vars.is_empty() {
            watch_list = watch_list.push(
                text("No watch expressions (Right-click or Shift+F9 on variable)")
                    .size(12)
                    .style(muted_style),
            );
        } else {
            for (w_idx, var_name) in watch_vars.iter().enumerate() {
                let (val_str, type_str, in_scope) = if let Some((_, typ, val)) = debug_locals.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(var_name)) {
                    (val.clone(), typ.clone(), true)
                } else if is_debugging {
                    ("<not in scope>".to_string(), String::new(), false)
                } else {
                    ("<not debugging>".to_string(), String::new(), false)
                };

                let mut val_col = column![
                    text(val_str).size(13).font(iced::Font::MONOSPACE)
                        .style(move |_t| text::Style {
                            color: if in_scope { None } else { Some(iced::Color::from_rgb(0.55, 0.55, 0.55)) },
                        }),
                ]
                .width(Fill);

                if !type_str.is_empty() {
                    val_col = val_col.push(text(type_str).size(11).font(iced::Font::MONOSPACE).style(muted_style));
                }

                let row_item = container(
                    row![
                        text(var_name.clone()).size(13).font(iced::Font::MONOSPACE).width(Length::Fixed(95.0)),
                        val_col,
                        button(text("✕").size(12))
                            .on_press(IdeMessage::RemoveWatch(w_idx))
                            .padding([1, 4])
                            .style(button::text),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .padding([2, 4])
                .style(move |t: &Theme| {
                    let p = t.palette();
                    container::Style {
                        background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                        ..Default::default()
                    }
                });
                watch_list = watch_list.push(row_item);
            }
        }

        let mut vars_list = column![].spacing(3);
        vars_list = vars_list.push(
            row![
                text("Name").size(13).style(primary_style).width(Length::Fixed(95.0)),
                text("Type").size(13).style(primary_style).width(Length::Fixed(80.0)),
                text("Value").size(13).style(primary_style).width(Fill),
            ]
            .spacing(6)
            .align_y(iced::Center),
        );
        vars_list = vars_list.push(hsep());

        if debug_locals.is_empty() {
            vars_list = vars_list.push(
                text(if is_debugging { "No local variables in frame" } else { "Start debugging to inspect variables" })
                    .size(13)
                    .style(muted_style),
            );
        } else {
            for (name, type_name, repr_val) in debug_locals {
                let row_item = container(
                    row![
                        text(name).size(13).font(iced::Font::MONOSPACE).width(Length::Fixed(95.0)),
                        text(type_name).size(12).font(iced::Font::MONOSPACE).style(muted_style).width(Length::Fixed(80.0)),
                        text(repr_val).size(13).font(iced::Font::MONOSPACE).width(Fill),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .padding([2, 4])
                .style(move |t: &Theme| {
                    let p = t.palette();
                    container::Style {
                        background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                        ..Default::default()
                    }
                });
                vars_list = vars_list.push(row_item);
            }
        }

        let mut stack_list = column![].spacing(3);
        stack_list = stack_list.push(
            row![
                text("Function").size(13).style(primary_style).width(Length::Fixed(105.0)),
                text("Line").size(13).style(primary_style).width(Length::Fixed(48.0)),
                text("File").size(13).style(primary_style).width(Fill),
            ]
            .spacing(6)
            .align_y(iced::Center),
        );
        stack_list = stack_list.push(hsep());

        if debug_stack.is_empty() {
            stack_list = stack_list.push(
                text(if is_debugging { "Call stack empty" } else { "No active call stack" })
                    .size(13)
                    .style(muted_style),
            );
        } else {
            for (func_name, lineno, filename) in debug_stack {
                let short_fname = std::path::Path::new(filename)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(filename);
                let row_item = container(
                    row![
                        text(format!("{func_name}()")).size(13).font(iced::Font::MONOSPACE).width(Length::Fixed(105.0)),
                        text(format!(":{lineno}")).size(13).font(iced::Font::MONOSPACE).style(muted_style).width(Length::Fixed(48.0)),
                        text(short_fname).size(12).font(iced::Font::MONOSPACE).width(Fill),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .padding([2, 4])
                .style(move |t: &Theme| {
                    let p = t.palette();
                    container::Style {
                        background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                        ..Default::default()
                    }
                });
                stack_list = stack_list.push(row_item);
            }
        }

        let mut bps_list = column![].spacing(3);
        if let Some(tab) = tabs.get(active_tab) {
            if tab.breakpoints.is_empty() {
                bps_list = bps_list.push(
                    text("No breakpoints set (Press F9 on '(' or S-expr)")
                        .size(12)
                        .style(muted_style),
                );
            } else {
                let tab_text = tab.content.text();
                for &bp in &tab.breakpoints {
                    let is_paused = tab.debug_paused_span.is_some_and(|(sl, sc, _, _)| sl == bp.line && sc == bp.col)
                        || (tab.debug_paused_line == Some(bp.line) && tab.debug_paused_span.is_none());
                    let (marker, marker_color) = if is_paused {
                        ("▶", iced::Color { a: 1.0, ..settings.execution_color })
                    } else {
                        ("●", iced::Color { a: 1.0, ..settings.breakpoint_color })
                    };
                    let preview = crate::ide::paren::sexpr_preview_at_bp(&tab_text, bp)
                        .unwrap_or_else(|| "(".to_string());
                    let bp_row = container(
                        row![
                            text(marker).size(13).style(move |_t| text::Style { color: Some(marker_color) }),
                            text(format!("Ln {}:{} {preview}", bp.line, bp.col)).size(13).font(iced::Font::MONOSPACE).width(Fill),
                            button(text("✕").size(12))
                                .on_press(IdeMessage::ToggleParenBreakpoint(bp))
                                .padding([1, 4])
                                .style(button::text),
                        ]
                        .spacing(4)
                        .align_y(iced::Center),
                    )
                    .padding([2, 4])
                    .style(move |t: &Theme| {
                        let p = t.palette();
                        container::Style {
                            background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                            ..Default::default()
                        }
                    });
                    bps_list = bps_list.push(bp_row);
                }
            }
        }

        let panel = container(
            column![
                row![
                    text("Debug Inspector").size(15).style(primary_style),
                    Space::new().width(Fill),
                    button(text("✕").size(12))
                        .on_press(IdeMessage::ToggleVariables)
                        .padding([1, 4])
                        .style(button::text),
                ]
                .align_y(iced::Center),
                hsep(),
                text("Watch Expressions").size(14).style(primary_style),
                scrollable(watch_list).height(Length::FillPortion(2)),
                hsep(),
                text("Local Variables").size(14).style(primary_style),
                scrollable(vars_list).height(Length::FillPortion(2)),
                hsep(),
                text("Call Stack").size(14).style(primary_style),
                scrollable(stack_list).height(Length::FillPortion(1)),
                hsep(),
                text("Breakpoints").size(14).style(primary_style),
                scrollable(bps_list).height(Length::FillPortion(1)),
            ]
            .spacing(4)
            .padding(6),
        )
        .width(Length::Fixed(340.0))
        .height(Fill)
        .style(move |_t: &Theme| {
            container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                border: Border {
                    color: IDE_DARK_GRAY.scale_alpha(alpha),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        Some(panel.into())
    } else {
        None
    };

    // ── 6. OpenCAD API Reference Drawer (Collapsible) ────────────────────────
    let api_drawer: Option<Element<'a, IdeMessage>> = if api_drawer_open {
        let filter_lower = api_search.trim().to_lowercase();
        let mut api_list = column![].spacing(6);

        let active_enclosing_doc: Option<&'static ApiDoc> = if let Some(tab) = tabs.get(active_tab) {
            if tab.is_lisp() {
                let cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
                let text = tab.content.text();
                active_function_call_at_cursor(&text, cursor.position.line, cursor.position.column)
            } else {
                None
            }
        } else {
            None
        };

        let active_card: Element<'a, IdeMessage> = if let Some(enclosing) = active_enclosing_doc {
            let insert_btn = button(
                text("+ Insert")
                    .size(12)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(IdeMessage::InsertSnippet(enclosing.snippet.to_string()))
            .padding([2, 6])
            .style(ide_secondary_button);

            let mut card_col = column![
                row![
                    text(format!("λ Active: {}", enclosing.name))
                        .size(15)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(primary_style),
                    Space::new().width(Fill),
                    text(enclosing.category)
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(muted_style),
                    insert_btn,
                ]
                .spacing(8)
                .align_y(iced::Center),
                text(format!("💡 {}", enclosing.signature))
                    .size(14)
                    .font(iced::Font::MONOSPACE)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .style(muted_style),
                text(enclosing.description)
                    .size(14)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(4);

            if let Some(dxf) = enclosing.dxf_summary {
                let dxf_box = container(
                    text(dxf)
                        .size(13)
                        .font(iced::Font::MONOSPACE)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(|_t: &Theme| text::Style {
                            color: Some(iced::Color::from_rgb(0.78, 0.86, 0.96)),
                        })
                )
                .padding([4, 6])
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::from_rgba(0.06, 0.08, 0.12, 0.95))),
                    border: Border {
                        color: iced::Color::from_rgba(0.3, 0.5, 0.8, 0.6),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
                card_col = card_col.push(dxf_box);
            }

            container(card_col)
                .width(Fill)
                .padding(8)
                .style(move |t: &Theme| {
                    let p = t.palette();
                    container::Style {
                        background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                        border: Border {
                            color: iced::Color::from_rgba(0.3, 0.6, 1.0, 0.7),
                            width: 1.5,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .into()
        } else {
            container(
                text("Active Enclosure: (None at cursor)")
                    .size(13)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .style(muted_style),
            )
            .width(Fill)
            .padding([4, 6])
            .into()
        };

        let search_box = text_input("Search OpenCAD LISP & DXF API...", api_search)
            .on_input(IdeMessage::ApiSearch)
            .size(14)
            .padding([4, 6]);

        api_list = api_list
            .push(Space::new().width(Length::Fixed(376.0)).height(Length::Fixed(0.0)))
            .push(active_card)
            .push(hsep());

        for doc in API_DOCS {
            let matches_dxf = doc.dxf_summary.is_some_and(|d| d.to_lowercase().contains(&filter_lower));
            if !filter_lower.is_empty()
                && !doc.name.to_lowercase().contains(&filter_lower)
                && !doc.description.to_lowercase().contains(&filter_lower)
                && !doc.category.to_lowercase().contains(&filter_lower)
                && !matches_dxf
            {
                continue;
            }

            let insert_btn = button(
                text("+ Insert")
                    .size(12)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(IdeMessage::InsertSnippet(doc.snippet.to_string()))
            .padding([2, 6])
            .style(ide_secondary_button);

            let mut card_col = column![
                row![
                    text(doc.name)
                        .size(14)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(primary_style),
                    Space::new().width(Fill),
                    text(doc.category)
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(muted_style),
                    insert_btn,
                ]
                .spacing(8)
                .align_y(iced::Center),
                text(doc.signature)
                    .size(13)
                    .font(iced::Font::MONOSPACE)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .style(muted_style),
                text(doc.description)
                    .size(13)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(3);

            if let Some(dxf) = doc.dxf_summary {
                let dxf_box = container(
                    text(dxf)
                        .size(12)
                        .font(iced::Font::MONOSPACE)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(|_t: &Theme| text::Style {
                            color: Some(iced::Color::from_rgb(0.78, 0.86, 0.96)),
                        })
                )
                .padding([4, 6])
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(iced::Color::from_rgba(0.06, 0.08, 0.12, 0.9))),
                    border: Border {
                        color: iced::Color::from_rgba(0.3, 0.45, 0.7, 0.5),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
                card_col = card_col.push(dxf_box);
            }

            let card = container(card_col)
            .width(Fill)
            .padding(6)
            .style(move |t: &Theme| {
                let p = t.palette();
                container::Style {
                    background: Some(Background::Color(p.background.base.color.scale_alpha(alpha))),
                    border: Border {
                        color: IDE_DARK_GRAY.scale_alpha(alpha),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

            api_list = api_list.push(card);
        }

        let drawer_container = container(
            column![
                row![
                    text("λ OpenCAD LISP Function List & Reference")
                        .size(15)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(primary_style),
                    Space::new().width(Fill),
                    button(text("✕").size(14))
                        .on_press(IdeMessage::ToggleApiDrawer)
                        .padding([2, 6])
                        .style(button::text),
                ]
                .spacing(6)
                .align_y(iced::Center),
                hsep(),
                search_box,
                scrollable(api_list)
                    .direction(scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::default(),
                        horizontal: scrollable::Scrollbar::default(),
                    })
                    .width(Fill)
                    .height(Fill),
            ]
            .spacing(6)
            .padding(6),
        )
        .width(Length::Fixed(400.0))
        .height(Fill)
        .style(move |_t: &Theme| {
            container::Style {
                background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                border: Border {
                    color: IDE_DARK_GRAY.scale_alpha(alpha),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        Some(drawer_container.into())
    } else {
        None
    };

    // ── 7. Combined Center Workspace ─────────────────────────────────────────
    let mut center_row = row![
        column![
            tab_bar,
            editor_area,
        ]
        .spacing(4)
        .width(Fill)
        .height(Fill),
    ]
    .spacing(4)
    .height(Fill);

    if let Some(vars_panel) = variables_drawer {
        center_row = center_row.push(panel_vsep()).push(vars_panel);
    }
    if let Some(api_panel) = api_drawer {
        center_row = center_row.push(panel_vsep()).push(api_panel);
    }

    let center_workspace: Element<'a, IdeMessage> = center_row.into();

    let splitter = mouse_area(
        container(Space::new())
            .width(Fill)
            .height(Length::Fixed(2.0))
            .style(move |_t: &Theme| {
                container::Style {
                    background: Some(Background::Color(IDE_DARK_GRAY.scale_alpha(alpha))),
                    ..Default::default()
                }
            }),
    )
    .on_press(IdeMessage::SplitterGrab)
    .on_double_click(IdeMessage::SplitterReset)
    .interaction(iced::mouse::Interaction::ResizingVertically);

    // ── 8. Full Layout ───────────────────────────────────────────────────────
    let full_layout = container(
        column![
            toolbar_bar,
            center_workspace,
            splitter,
            console_pane,
        ]
        .spacing(2)
        .padding([2, 6])
        .width(layout_width)
        .height(layout_height),
    )
    .width(layout_width)
    .height(layout_height)
    .style(move |t: &Theme| container::Style {
        background: Some(Background::Color(t.palette().background.base.color.scale_alpha(alpha))),
        ..Default::default()
    });

    let base_element: Element<'a, IdeMessage> = if is_splitter_dragging {
        mouse_area(full_layout)
            .on_move(IdeMessage::SplitterMove)
            .on_release(IdeMessage::SplitterRelease)
            .interaction(iced::mouse::Interaction::ResizingVertically)
            .into()
    } else {
        full_layout.into()
    };

    let mut full_stack = stack![base_element];

    if help_menu_open {
        let dismiss_backdrop = mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill),
        )
        .on_press(IdeMessage::ToggleHelpMenu);

        let help_menu = container(
            column![
                button(
                    row![
                        text("Tutorials").size(13),
                    ]
                    .align_y(iced::Center),
                )
                .on_press(IdeMessage::OpenTutorials)
                .width(Fill)
                .padding([6, 12])
                .style(ide_secondary_button),
            ]
            .spacing(4)
            .width(Length::Fixed(170.0)),
        )
        .padding(6)
        .style(|_t: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgb(0.16, 0.17, 0.20))),
            border: Border {
                color: IDE_DARK_GRAY,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        let menu_overlay = container(help_menu)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                top: 42.0,
                right: 64.0,
                bottom: 0.0,
                left: 0.0,
            });

        full_stack = full_stack.push(dismiss_backdrop).push(menu_overlay);
    }

    if settings_open {
        let blocker = mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
        )
        .on_press(IdeMessage::Tick);

        let overlay = container(render_settings_dialog(
            settings,
            settings_tab,
            active_color_target,
            syntax_hex_input,
            font_size_input,
            transparency_input,
            selected_theme_name,
            new_theme_name,
            selected_custom_func,
            new_custom_func_input,
            window_height,
            theme,
        ))
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
            ..Default::default()
        });

        full_stack = full_stack.push(blocker).push(overlay);
    }

    if let Some(prompt) = save_prompt {
        let blocker = mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill),
        )
        .on_press(IdeMessage::Tick);

        let overlay = container(render_save_confirmation_dialog(prompt, theme))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
                ..Default::default()
            });

        full_stack = full_stack.push(blocker).push(overlay);
    }

    if let Some(cm) = context_menu {
        full_stack = full_stack.push(render_context_menu_overlay(cm, window_width, window_height));
    }

    full_stack.into()
}

fn render_save_confirmation_dialog<'a>(
    prompt: &'a crate::ide::SavePromptState,
    _theme: &'a Theme,
) -> Element<'a, IdeMessage> {
    let title_text = match &prompt.trigger {
        crate::ide::SavePromptTrigger::CloseWindow | crate::ide::SavePromptTrigger::HostClosing => {
            if prompt.dirty_files.len() == 1 {
                format!("Do you want to save changes to \"{}\"?", prompt.dirty_files[0].1)
            } else {
                format!("Do you want to save changes to {} files before closing?", prompt.dirty_files.len())
            }
        }
        crate::ide::SavePromptTrigger::CloseTab(_) => {
            format!("Do you want to save changes to \"{}\"?", prompt.dirty_files.first().map(|f| f.1.as_str()).unwrap_or("file"))
        }
    };

    let mut files_col: iced::widget::Column<'a, IdeMessage> = column![].spacing(3);
    for (_, title) in &prompt.dirty_files {
        files_col = files_col.push(
            row![
                text("• ").size(12).style(primary_style),
                text(title.clone()).size(12),
            ]
            .spacing(4)
            .align_y(iced::Center),
        );
    }

    let files_box = container(
        scrollable(files_col).height(Length::Fixed(
            (prompt.dirty_files.len() as f32 * 22.0 + 8.0).clamp(30.0, 120.0),
        )),
    )
    .padding(8)
    .width(Fill)
    .style(|_t: &Theme| container::Style {
        background: Some(Background::Color(iced::Color::from_rgba(0.08, 0.08, 0.10, 0.9))),
        border: Border {
            color: IDE_DARK_GRAY,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let btn_save = button(text("Save").size(12))
        .on_press(IdeMessage::ConfirmSaveAndProceed)
        .padding([6, 16])
        .style(button::primary);

    let btn_dont_save = button(text("Don't Save").size(12))
        .on_press(IdeMessage::DiscardAndProceed)
        .padding([6, 16])
        .style(button::danger);

    let btn_cancel = button(text("Cancel").size(12))
        .on_press(IdeMessage::CancelSavePrompt)
        .padding([6, 16])
        .style(ide_secondary_button);

    let buttons_row = row![
        Space::new().width(Fill),
        btn_save,
        btn_dont_save,
        btn_cancel,
    ]
    .spacing(8)
    .align_y(iced::Center);

    let middle_item: Element<'a, IdeMessage> = if prompt.dirty_files.len() > 1 {
        files_box.into()
    } else {
        Space::new().height(Length::Fixed(4.0)).into()
    };

    let dialog_card = container(
        column![
            row![
                text("💾 Unsaved Changes").size(15),
                Space::new().width(Fill),
                button(text("✕").size(12))
                    .on_press(IdeMessage::CancelSavePrompt)
                    .padding([2, 6])
                    .style(button::text),
            ]
            .align_y(iced::Center),
            container(Space::new())
                .width(Fill)
                .height(Length::Fixed(1.0))
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(IDE_DARK_GRAY)),
                    ..Default::default()
                }),
            text(title_text).size(13),
            middle_item,
            text("Your changes will be lost if you don't save them.").size(11).style(muted_style),
            Space::new().height(Length::Fixed(6.0)),
            buttons_row,
        ]
        .spacing(10)
        .padding(16),
    )
    .width(Length::Fixed(440.0))
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            color: iced::Color::from_rgba(0.4, 0.4, 0.45, 0.8),
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: iced::Vector::new(0.0, 8.0),
            blur_radius: 20.0,
        },
        ..Default::default()
    });

    dialog_card.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identifier_at_cursor() {
        let code = "b, h = float(base), float(height)\noc.add_line(0, 0, b, h)";
        // Line 0: "b, h = float(base), float(height)"
        // Cursor on 'b'
        assert_eq!(identifier_at_cursor(code, 0, 0), Some("b".to_string()));
        // Cursor on 'base'
        assert_eq!(identifier_at_cursor(code, 0, 13), Some("base".to_string()));
        assert_eq!(identifier_at_cursor(code, 0, 15), Some("base".to_string()));
        // Cursor on 'height'
        assert_eq!(identifier_at_cursor(code, 0, 28), Some("height".to_string()));
        // Cursor on space or punctuation
        assert_eq!(identifier_at_cursor(code, 0, 1), Some("b".to_string())); // adjacent right after 'b'
        // Line 1: "oc.add_line(0, 0, b, h)"
        assert_eq!(identifier_at_cursor(code, 1, 0), Some("oc".to_string()));
        assert_eq!(identifier_at_cursor(code, 1, 4), Some("add_line".to_string()));

        // OpenCAD LISP identifiers with hyphens, colons, and leading numbers
        let lisp_code = "(defun c:draw-box (/ pt1 pt2)\n  (vl-string-search \"A\" str)\n  (1+ count))";
        assert_eq!(identifier_at_cursor(lisp_code, 0, 8), Some("c:draw-box".to_string()));
        assert_eq!(identifier_at_cursor(lisp_code, 1, 5), Some("vl-string-search".to_string()));
        assert_eq!(identifier_at_cursor(lisp_code, 2, 4), Some("1+".to_string()));
    }

    #[test]
    fn test_python_ide_view_with_debug_and_watch() {
        let tabs = vec![IdeTab::new_empty("test.py".to_string())];
        let console_log = text_editor::Content::new();
        let repl_content = text_editor::Content::new();
        let theme = Theme::Dark;
        let find_replace = crate::ide::FindReplaceState::default();

        // Case 1: Debugging active, variables open, empty watch list
        let empty_watches: Vec<String> = vec![];
        let settings = IdeSettings::default();
        let _elem1 = view_standalone_window(
            &tabs,
            0,
            &console_log,
            &repl_content,
            crate::ide::protocol::ReplMode::Python,
            false,
            "",
            &theme,
            true,
            true,
            &[],
            &[],
            false,
            &empty_watches,
            220.0,
            false,
            40,
            &settings,
            false,
            false,
            SettingsTab::General,
            SyntaxColorTarget::Background,
            "#202225",
            "13",
            "50",
            "Base16 Mocha (Default)",
            "",
            None,
            "",
            None,
            0,
            None,
            None,
            &find_replace,
            1000.0,
            780.0,
        );

        // Case 2: Watch list populated with variable not in scope, multiline repl, splitter dragging active
        let watch_vars = vec!["x".to_string(), "y".to_string()];
        let locals = vec![("x".to_string(), "int".to_string(), "42".to_string())];
        let stack = vec![("main".to_string(), 10, "test.py".to_string())];
        let _elem2 = view_standalone_window(
            &tabs,
            0,
            &console_log,
            &repl_content,
            crate::ide::protocol::ReplMode::Lisp,
            true,
            "line",
            &theme,
            true,
            true,
            &locals,
            &stack,
            true, // 50% transparency active
            &watch_vars,
            260.0,
            true,
            40,
            &settings,
            true,
            false,
            SettingsTab::EditorSyntax,
            SyntaxColorTarget::CustomFunction,
            "#82AAFF",
            "13",
            "50",
            "Base16 Mocha (Default)",
            "",
            None,
            "",
            None,
            0,
            None,
            None,
            &find_replace,
            1000.0,
            780.0,
        );

        // Case 3: Settings dialog open on OpenCAD LISP Formatting tab
        let _elem3 = view_standalone_window(
            &tabs,
            0,
            &console_log,
            &repl_content,
            crate::ide::protocol::ReplMode::Lisp,
            true,
            "line",
            &theme,
            false,
            false,
            &[],
            &[],
            false,
            &empty_watches,
            220.0,
            false,
            40,
            &settings,
            true,
            false,
            SettingsTab::Formatting,
            SyntaxColorTarget::Background,
            "#202225",
            "13",
            "50",
            "Base16 Mocha (Default)",
            "",
            None,
            "",
            None,
            0,
            None,
            None,
            &find_replace,
            1000.0,
            780.0,
        );

        // Case 4: Save confirmation modal dialog open
        let save_prompt_state = crate::ide::SavePromptState {
            trigger: crate::ide::SavePromptTrigger::CloseWindow,
            dirty_files: vec![(0, "test.py".to_string())],
        };
        let _elem4 = view_standalone_window(
            &tabs,
            0,
            &console_log,
            &repl_content,
            crate::ide::protocol::ReplMode::Lisp,
            false,
            "",
            &theme,
            false,
            false,
            &[],
            &[],
            false,
            &empty_watches,
            220.0,
            false,
            40,
            &settings,
            false,
            false,
            SettingsTab::General,
            SyntaxColorTarget::Background,
            "#202225",
            "13",
            "50",
            "Base16 Mocha (Default)",
            "",
            None,
            "",
            None,
            0,
            None,
            Some(&save_prompt_state),
            &find_replace,
            1000.0,
            780.0,
        );
    }

    #[test]
    fn test_ide_initial_template_and_command_snippet() {
        let tab = IdeTab::new_empty("untitled.lsp".to_string());
        assert_eq!(tab.content.text(), ";; OpenCAD LISP script\n");

        let doc = API_DOCS.iter().find(|d| d.name == "entmake").expect("entmake doc");
        assert_eq!(doc.signature, "(entmake dxf-alist) -> dxf-alist | nil");
        assert!(doc.dxf_summary.is_some());
        assert!(doc.dxf_summary.unwrap().contains("CIRCLE:     (0 . \"CIRCLE\") (10 x y z) (40 . radius)"));
        assert!(doc.dxf_summary.unwrap().contains("LINE:       (0 . \"LINE\") (10 x y z) (11 x y z)"));

        let cmd_doc = API_DOCS.iter().find(|d| d.name == "command").expect("command doc");
        assert_eq!(cmd_doc.signature, "(command [cmd-token1 ...]) -> nil");
    }

    #[test]
    fn test_active_function_call_at_cursor() {
        // Case 1: Lisp prefix call right after open paren of (entmake
        let text1 = "(entmake ";
        let doc1 = active_function_call_at_cursor(text1, 0, text1.len());
        assert!(doc1.is_some());
        assert_eq!(doc1.unwrap().name, "entmake");
        assert!(doc1.unwrap().dxf_summary.is_some());

        // Case 2: Multi-line prefix call:
        // (entmake
        //   (list
        let text2 = "(entmake\n  (list ";
        let doc2 = active_function_call_at_cursor(text2, 1, 8);
        assert!(doc2.is_some());
        assert_eq!(doc2.unwrap().name, "list");

        // Case 3: Extended LISP function with hyphens
        let text3 = "(vl-string-search \"CAD\" ";
        let doc3 = active_function_call_at_cursor(text3, 0, text3.len());
        assert!(doc3.is_some());
        assert_eq!(doc3.unwrap().name, "vl-string-search");

        // Case 4: Math / symbol identifiers like 1+ or +
        let text4 = "(1+ i";
        let doc4 = active_function_call_at_cursor(text4, 0, text4.len());
        assert!(doc4.is_some());
        assert_eq!(doc4.unwrap().name, "1+");

        // Case 5: Balanced parens - cursor after closing paren should not show tip
        let text5 = "(command \"_.LINE\" '(0 0 0) '(10 10 0) \"\")";
        let doc5 = active_function_call_at_cursor(text5, 0, text5.len());
        assert!(doc5.is_none());

        // Case 6: Standalone symbol at cursor without parens
        let text6 = "getpoint";
        let doc6 = active_function_call_at_cursor(text6, 0, text6.len());
        assert!(doc6.is_some());
        assert_eq!(doc6.unwrap().name, "getpoint");
    }

    #[test]
    fn test_scroll_past_cursor_anchoring() {
        let mut tab = IdeTab::new_empty("test.py".to_string());
        // Populate with 200 lines
        let code = (1..=200).map(|i| format!("# line {i}")).collect::<Vec<_>>().join("\n");
        tab.content = text_editor::Content::with_text(&code);
        assert_eq!(tab.content.line_count(), 200);

        // Place cursor at line 15, column 5
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: 15,
                column: 5,
            },
            selection: None,
        });
        assert_eq!(tab.content.cursor().position.line, 15);
        assert_eq!(tab.content.cursor().position.column, 5);
        assert_eq!(tab.scroll_top_line, 0);
        assert!(tab.last_edit_cursor.is_none());
        assert!(!tab.is_scrolling_away);

        // Simulate scrolling away to line 80
        tab.last_edit_cursor = Some(tab.content.cursor());
        tab.is_scrolling_away = true;
        tab.scroll_top_line = 80;
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: 80,
                column: 0,
            },
            selection: None,
        });

        assert_eq!(tab.scroll_top_line, 80);
        assert!(tab.is_scrolling_away);
        // The original cursor is remembered!
        let remembered = tab.last_edit_cursor.unwrap();
        assert_eq!(remembered.position.line, 15);
        assert_eq!(remembered.position.column, 5);

        // Simulate scrolling back into range: line 15 is visible when scroll_top <= 15
        tab.scroll_top_line = 10;
        tab.content.move_to(remembered);
        tab.is_scrolling_away = false;

        assert_eq!(tab.content.cursor().position.line, 15);
        assert_eq!(tab.content.cursor().position.column, 5);
        assert!(!tab.is_scrolling_away);
    }

    #[test]
    fn test_status_bar_readout_stability_when_scrolled_away() {
        let mut tab = IdeTab::new_empty("test.py".to_string());
        let code = (1..=100).map(|i| format!("print('line {i}')")).collect::<Vec<_>>().join("\n");
        tab.content = text_editor::Content::with_text(&code);

        // User placed edit cursor at line 12, column 7
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: 12,
                column: 7,
            },
            selection: None,
        });

        tab.last_edit_cursor = Some(tab.content.cursor());
        tab.is_scrolling_away = true;

        // View moves to line 60 (scrolled away)
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: 60,
                column: 0,
            },
            selection: None,
        });

        // The status bar cursor calculation prioritizes last_edit_cursor
        let status_cursor = tab.last_edit_cursor.unwrap_or_else(|| tab.content.cursor());
        assert_eq!(status_cursor.position.line + 1, 13);
        assert_eq!(status_cursor.position.column + 1, 8);
    }

    #[test]
    fn test_scroll_bounds_and_bottom_clamping() {
        let total: usize = 60;
        let visible_lines: usize = 22;
        let max_scroll = total.saturating_sub(visible_lines);
        assert_eq!(max_scroll, 38);

        // Scrolling down past max_scroll clamps to max_scroll
        let next_overscroll = (35i32 + 10).clamp(0, max_scroll as i32) as usize;
        assert_eq!(next_overscroll, 38);

        // Scrolling up from max_scroll immediately decreases
        let next_upscroll = (38i32 - 4).clamp(0, max_scroll as i32) as usize;
        assert_eq!(next_upscroll, 34);

        // Scrolling up past top clamps to 0
        let next_top = (2i32 - 5).clamp(0, max_scroll as i32) as usize;
        assert_eq!(next_top, 0);
    }

    #[test]
    fn test_console_autoscroll_and_color_settings() {
        let (mut app, _) = crate::ide::app::IdeApp::new();
        let initial_lines = app.console_log.line_count();
        assert!(initial_lines > 0);

        // Append 50 lines of output to console
        for i in 1..=50 {
            app.append_to_console(&format!("Line {i}: (+ {i} 100)\n"));
        }

        let new_lines = app.console_log.line_count();
        assert_eq!(new_lines, initial_lines + 50);

        // The cursor should be moved to the end of the document (last line)
        let cursor = app.console_log.cursor();
        assert_eq!(cursor.position.line, new_lines.saturating_sub(1));

        // Test clear console
        app.set_console_text("=== Console Cleared ===\n");
        assert_eq!(app.console_log.line_count(), 2);
        let cursor = app.console_log.cursor();
        assert_eq!(cursor.position.line, 1);
    }

    #[test]
    fn test_function_tooltip_suppression_and_master_toggle() {
        let mut settings = IdeSettings::default();
        assert!(settings.show_function_tooltips);
        assert!(!settings.is_tooltip_ignored("entmake"));

        let text = "(entmake (list ";
        let col = text.len();
        let doc = active_function_call_at_cursor(text, 0, col);
        assert_eq!(doc.map(|d| d.name), Some("list"));

        // If list is suppressed:
        settings.add_ignored_tooltip_function("list".to_string());
        assert!(settings.is_tooltip_ignored("list"));
        assert!(settings.is_tooltip_ignored("LIST")); // case-insensitive check

        let filtered_doc = active_function_call_at_cursor(text, 0, col)
            .filter(|d| !settings.is_tooltip_ignored(d.name));
        assert!(filtered_doc.is_none());

        // Unsuppress list:
        assert!(settings.remove_ignored_tooltip_function("list"));
        assert!(!settings.is_tooltip_ignored("list"));
        let restored_doc = active_function_call_at_cursor(text, 0, col)
            .filter(|d| !settings.is_tooltip_ignored(d.name));
        assert!(restored_doc.is_some());

        // Master toggle off:
        settings.show_function_tooltips = false;
        let master_disabled_doc = if !settings.show_function_tooltips {
            None
        } else {
            active_function_call_at_cursor(text, 0, col)
        };
        assert!(master_disabled_doc.is_none());
    }

    #[test]
    fn test_app_tooltip_and_drawer_messages() {
        let (mut app, _) = crate::ide::app::IdeApp::new();
        app.settings.ignored_tooltip_functions.clear();
        app.settings.rebuild_ignored_tooltip_functions_set();

        // 1. ShowInApiDrawer opens the drawer and sets the search query
        assert!(app.api_drawer_open);
        let _ = app.update(IdeMessage::ToggleApiDrawer);
        assert!(!app.api_drawer_open);
        let _ = app.update(IdeMessage::ShowInApiDrawer("entmake".to_string()));
        assert!(app.api_drawer_open);
        assert_eq!(app.api_search, "entmake");

        // 2. Master toggle
        assert!(app.settings.show_function_tooltips);
        let _ = app.update(IdeMessage::ToggleShowTooltips(false));
        assert!(!app.settings.show_function_tooltips);
        let _ = app.update(IdeMessage::ToggleShowTooltips(true));
        assert!(app.settings.show_function_tooltips);

        // 3. AddIgnoredTooltipFunction and RemoveIgnoredTooltipFunction
        let _ = app.update(IdeMessage::AddIgnoredTooltipFunction("entmod".to_string()));
        assert!(app.settings.is_tooltip_ignored("entmod"));
        let _ = app.update(IdeMessage::RemoveIgnoredTooltipFunction("entmod".to_string()));
        assert!(!app.settings.is_tooltip_ignored("entmod"));

        // 4. UI add input, submit, select, remove, clear
        let _ = app.update(IdeMessage::SetNewIgnoredFunctionInput("vl-string-search".to_string()));
        assert_eq!(app.new_ignored_func_input, "vl-string-search");
        let _ = app.update(IdeMessage::AddIgnoredFunction);
        assert!(app.new_ignored_func_input.is_empty());
        assert!(app.settings.is_tooltip_ignored("vl-string-search"));

        let _ = app.update(IdeMessage::SelectIgnoredFunction(Some("vl-string-search".to_string())));
        assert_eq!(app.selected_ignored_func.as_deref(), Some("vl-string-search"));
        let _ = app.update(IdeMessage::RemoveIgnoredFunction);
        assert!(!app.settings.is_tooltip_ignored("vl-string-search"));
        assert!(app.selected_ignored_func.is_none());

        // 5. Clear all
        let _ = app.update(IdeMessage::AddIgnoredTooltipFunction("func-a".to_string()));
        let _ = app.update(IdeMessage::AddIgnoredTooltipFunction("func-b".to_string()));
        assert_eq!(app.settings.ignored_tooltip_functions.len(), 2);
        let _ = app.update(IdeMessage::ClearIgnoredFunctions);
        assert!(app.settings.ignored_tooltip_functions.is_empty());
    }

    #[test]
    fn test_primary_function_in_line_and_context_menu_resolution() {
        // 1. primary_function_in_line with various line formats
        assert_eq!(primary_function_in_line("(setq a 1)").map(|d| d.name), Some("setq"));
        assert_eq!(primary_function_in_line("  (defun c:test ()").map(|d| d.name), Some("defun"));
        assert_eq!(primary_function_in_line("    (entmake (list '(0 . \"LINE\")))").map(|d| d.name), Some("entmake"));
        assert_eq!(primary_function_in_line("(alert \"done\")").map(|d| d.name), Some("alert"));
        assert_eq!(primary_function_in_line("; (alert \"commented\")").map(|d| d.name), None);
        assert_eq!(primary_function_in_line("not a lisp form").map(|d| d.name), None);

        // 2. Verify target_func resolution hierarchy:
        // Priority 1: exact target_ident under caret or selection
        let mut settings = IdeSettings::default();
        let target_ident = Some("entmake".to_string());
        let active_doc = API_DOCS.iter().find(|d| d.name == "defun");
        let line = "  (entmake (list '(0 . \"LINE\")))";

        let target_func = target_ident
            .as_ref()
            .map(|id| id.trim().trim_start_matches('\'').to_lowercase())
            .filter(|name| {
                !name.is_empty() && (
                    API_DOCS.iter().any(|d| d.name.eq_ignore_ascii_case(name))
                        || settings.custom_functions_set.contains(name.as_str())
                        || settings.is_tooltip_ignored(name.as_str())
                )
            })
            .or_else(|| active_doc.map(|d| d.name.to_lowercase()))
            .or_else(|| {
                primary_function_in_line(line).map(|d| d.name.to_lowercase())
            });

        // Must resolve to "entmake", NOT the enclosing "defun"
        assert_eq!(target_func.as_deref(), Some("entmake"));

        // Priority 2: When caret is in arguments without a specific ident, falls back to active_doc
        let target_ident_none: Option<String> = None;
        let target_func2 = target_ident_none
            .as_ref()
            .map(|id| id.trim().trim_start_matches('\'').to_lowercase())
            .filter(|name| {
                !name.is_empty() && (
                    API_DOCS.iter().any(|d| d.name.eq_ignore_ascii_case(name))
                        || settings.custom_functions_set.contains(name.as_str())
                        || settings.is_tooltip_ignored(name.as_str())
                )
            })
            .or_else(|| active_doc.map(|d| d.name.to_lowercase()))
            .or_else(|| {
                primary_function_in_line(line).map(|d| d.name.to_lowercase())
            });
        assert_eq!(target_func2.as_deref(), Some("defun"));

        // Priority 3: When parens are balanced and active_doc is None, falls back to primary_function_in_line
        let active_doc_none: Option<&ApiDoc> = None;
        let target_func3 = target_ident_none
            .as_ref()
            .map(|id| id.trim().trim_start_matches('\'').to_lowercase())
            .filter(|name| {
                !name.is_empty() && (
                    API_DOCS.iter().any(|d| d.name.eq_ignore_ascii_case(name))
                        || settings.custom_functions_set.contains(name.as_str())
                        || settings.is_tooltip_ignored(name.as_str())
                )
            })
            .or_else(|| active_doc_none.map(|d| d.name.to_lowercase()))
            .or_else(|| {
                primary_function_in_line(line).map(|d| d.name.to_lowercase())
            });
        assert_eq!(target_func3.as_deref(), Some("entmake"));

        // 3. Settings suppression toggle
        assert!(!settings.is_tooltip_ignored("entmake"));
        assert!(settings.add_ignored_tooltip_function("entmake".to_string()));
        assert!(settings.is_tooltip_ignored("entmake"));
        assert!(settings.remove_ignored_tooltip_function("entmake"));
        assert!(!settings.is_tooltip_ignored("entmake"));
    }

    #[test]
    fn test_context_menu_app_integration_and_f2() {
        let (mut app, _) = crate::ide::app::IdeApp::new();
        app.settings = IdeSettings::default();

        // 1. Initial state: context menu is None, api drawer is open by default
        assert!(app.context_menu.is_none());
        assert!(app.api_drawer_open);

        // 2. F2 toggles api drawer
        let _ = app.update(IdeMessage::ToggleApiDrawer);
        assert!(!app.api_drawer_open);
        let _ = app.update(IdeMessage::ToggleApiDrawer);
        assert!(app.api_drawer_open);

        // 3. Set tab content with OpenCAD LISP code
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(defun c:mycmd ()\n  (entmake (list '(0 . \"LINE\")))\n)");
        }

        // 4. Right-click on line 2 (where entmake is)
        let click_pos = Point::new(120.0, 96.0);
        let _ = app.update(IdeMessage::EditorRightClicked(click_pos));
        assert!(app.context_menu.is_some());
        let cm = app.context_menu.as_ref().unwrap();
        assert_eq!(cm.target_func.as_deref(), Some("entmake"));
        assert_eq!(cm.cursor_line, 2);

        // 5. CloseContextMenu closes menu
        let _ = app.update(IdeMessage::CloseContextMenu);
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn test_autocomplete_prefix_detection_and_alphabetical_matches() {
        let (mut app, _) = crate::ide::app::IdeApp::new();

        // 1. Positive Case: Direct opening paren "(entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();

        let (start_col, end_col, matches) = app.current_autocomplete_matches().expect("matches for (entm");
        assert_eq!(start_col, 1);
        assert_eq!(end_col, 5);
        assert!(matches.contains(&"entmake".to_string()));
        assert!(matches.contains(&"entmod".to_string()));

        // Alphabetical sort check
        for w in matches.windows(2) {
            assert!(w[0].to_ascii_lowercase() <= w[1].to_ascii_lowercase());
        }

        // Verify cached matches on app
        assert_eq!(app.autocomplete_matches, matches);
        assert_eq!(app.autocomplete_selected_index, 0);

        // 2. Positive Case: Space between paren and function "(   entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(   entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(!app.autocomplete_matches.is_empty());
        assert_eq!(app.current_autocomplete_matches().unwrap().0, 4);

        // 3. Positive Case: Nested function call "(setq x (entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(setq x (entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 13 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(!app.autocomplete_matches.is_empty());
        assert_eq!(app.current_autocomplete_matches().unwrap().0, 9);

        // 4. Positive Case: Multiline function call "(\n  entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(\n  entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 1, column: 6 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(!app.autocomplete_matches.is_empty());
        assert_eq!(app.current_autocomplete_matches().unwrap().0, 2);

        // 5. Negative Case: Standalone without paren "entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 4 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 6. Negative Case: Inside argument position "(setq entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(setq entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 10 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 7. Negative Case: Argument to car "(car entm"
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(car entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 9 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());
    }

    #[test]
    fn test_autocomplete_tab_completion_with_trailing_space() {
        let (mut app, _) = crate::ide::app::IdeApp::new();
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(!app.autocomplete_matches.is_empty());

        // Select the first match ("entmake") via AutocompleteAccept (triggered by Tab)
        let _ = app.update(IdeMessage::AutocompleteAccept);

        let tab = app.tabs.get(app.active_tab).unwrap();
        assert_eq!(tab.content.text(), "(entmake ");
        assert!(app.autocomplete_dismissed);
        assert!(app.autocomplete_matches.is_empty());
    }

    #[test]
    fn test_autocomplete_arrow_navigation() {
        let (mut app, _) = crate::ide::app::IdeApp::new();
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(ent");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 4 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        let total = app.autocomplete_matches.len();
        assert!(total >= 2);
        assert_eq!(app.autocomplete_selected_index, 0);

        // Arrow down
        let _ = app.update(IdeMessage::AutocompleteNext);
        assert_eq!(app.autocomplete_selected_index, 1);

        // Arrow up back to 0
        let _ = app.update(IdeMessage::AutocompletePrev);
        assert_eq!(app.autocomplete_selected_index, 0);

        // Arrow up wraps to last item
        let _ = app.update(IdeMessage::AutocompletePrev);
        assert_eq!(app.autocomplete_selected_index, total - 1);

        // Arrow down wraps to 0
        let _ = app.update(IdeMessage::AutocompleteNext);
        assert_eq!(app.autocomplete_selected_index, 0);

        // Escape dismisses
        let _ = app.update(IdeMessage::AutocompleteDismiss);
        assert!(app.autocomplete_dismissed);
        assert!(app.autocomplete_matches.is_empty());
    }

    #[test]
    fn test_autocomplete_suppressed_when_caret_on_valid_function() {
        let (mut app, _) = crate::ide::app::IdeApp::new();

        // 1. Incomplete prefix "(entm" -> NOT a valid function -> shows matches
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(entm");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(!app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_some());

        // 2. Fully typed valid function "(entmake" (caret at end) -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(entmake");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 3. Caret inside valid function "(ent|make" -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(entmake");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 4 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 4. Caret on "defun" in existing code -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(defun c:draw () (princ))");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 3 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 5. Caret on "setq" in existing code -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(setq a 10)");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 5 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 6. Caret on "getpoint" in existing code -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(getpoint \"\\nPick: \")");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 6 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 7. Custom functions: incomplete prefix matches, full valid function suppresses
        app.settings.add_custom_function("my-custom-fn".to_string());
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(my-cust");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 8 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert_eq!(app.autocomplete_matches, vec!["my-custom-fn".to_string()]);

        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(my-custom-fn");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 13 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        // 8. Built-in operators: "(+ 1 2)" and "(1+ x)" -> suppressed
        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(+ 1 2)");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 2 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());

        if let Some(tab) = app.tabs.get_mut(app.active_tab) {
            tab.content = text_editor::Content::with_text("(1+ x)");
            tab.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 3 },
                selection: None,
            });
        }
        app.refresh_autocomplete_matches();
        assert!(app.autocomplete_matches.is_empty());
        assert!(app.current_autocomplete_matches().is_none());
    }

    #[test]
    fn test_sidebar_dynamic_enclosing_function_display() {
        let code = "(defun c:test ()\n  (entmake (list (cons 0 \"LINE\")))\n)";
        // Cursor at column 12 on line 1 is inside 'list'
        let doc1 = active_function_call_at_cursor(code, 1, 15);
        assert_eq!(doc1.map(|d| d.name), Some("list"));

        // Cursor at column 25 on line 1 is inside 'cons'
        let doc2 = active_function_call_at_cursor(code, 1, 26);
        assert_eq!(doc2.map(|d| d.name), Some("cons"));

        // Cursor at column 9 on line 1 is inside 'entmake'
        let doc3 = active_function_call_at_cursor(code, 1, 9);
        assert_eq!(doc3.map(|d| d.name), Some("entmake"));

        // Cursor at line 0 inside c:test defun
        let doc0 = active_function_call_at_cursor(code, 0, 15);
        assert_eq!(doc0.map(|d| d.name), Some("defun"));
    }
}

