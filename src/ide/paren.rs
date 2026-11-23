use serde::{Deserialize, Serialize};

/// Configuration options for OpenCAD LISP code formatting according to classic LISP standards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LispFormatOptions {
    pub indent_size: usize,
    pub downcase_builtins: bool,
    pub wide_style_calls: bool,
    pub align_setq_pairs: bool,
    pub structural_reflow: bool,
}

impl Default for LispFormatOptions {
    fn default() -> Self {
        Self {
            indent_size: 2,
            downcase_builtins: true,
            wide_style_calls: true,
            align_setq_pairs: true,
            structural_reflow: true,
        }
    }
}

/// Returns true if `head` is a standard OpenCAD LISP special form or control-flow construct
/// that uses Narrow Style formatting (open_col + indent_size).
pub fn is_lisp_special_form(head: &str) -> bool {
    let lower = head.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "defun"
            | "defun-q"
            | "lambda"
            | "progn"
            | "if"
            | "while"
            | "repeat"
            | "foreach"
            | "cond"
            | "vl-catch-all-apply"
            | "vl-catch-all-error-p"
    )
}

/// Represents a parenthesis token in OpenCAD LISP source code (outside comments and strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParenToken {
    pub offset: usize,
    pub is_open: bool, // true for '(', false for ')'
}

/// Converts a 0-based (line, byte_col) position into a 0-based byte offset in `text`.
pub fn pos_to_offset(text: &str, target_line: usize, target_col: usize) -> usize {
    let mut current_line = 0;
    let mut line_start = 0;
    for (i, b) in text.bytes().enumerate() {
        if current_line == target_line {
            return (line_start + target_col).min(text.len());
        }
        if b == b'\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    (line_start + target_col).min(text.len())
}

/// Converts a 0-based byte offset into a 0-based (line, byte_col) position.
pub fn offset_to_pos(text: &str, target_offset: usize) -> (usize, usize) {
    let target_offset = target_offset.min(text.len());
    let mut line = 0;
    let mut col = 0;
    for (i, b) in text.bytes().enumerate() {
        if i == target_offset {
            return (line, col);
        }
        if b == b'\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Scans OpenCAD LISP source text and collects all active parentheses outside of comments and strings.
pub fn scan_parens(text: &str) -> Vec<ParenToken> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut parens = Vec::new();
    let mut i = 0;

    let mut in_string = false;
    let mut in_string_escape = false;
    let mut in_line_comment = false;
    let mut block_comment_depth: usize = 0;

    while i < len {
        let b = bytes[i];

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if block_comment_depth > 0 {
            if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
                block_comment_depth += 1;
                i += 2;
                continue;
            } else if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b';' {
                block_comment_depth -= 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_string {
            if in_string_escape {
                in_string_escape = false;
            } else if b == b'\\' {
                in_string_escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        // In normal code:
        if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
            block_comment_depth = 1;
            i += 2;
            continue;
        } else if b == b';' {
            in_line_comment = true;
            i += 1;
            continue;
        } else if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        } else if b == b'(' {
            parens.push(ParenToken {
                offset: i,
                is_open: true,
            });
        } else if b == b')' {
            parens.push(ParenToken {
                offset: i,
                is_open: false,
            });
        }

        i += 1;
    }

    parens
}

/// Given text and a cursor position (line, col) that is at or adjacent to a parenthesis,
/// returns `Some(((start_line, start_col), (end_line, end_col)))` covering the full S-expression
/// from opening `(` to closing `)` (where end position is immediately after `)`).
pub fn find_matching_paren_range(
    text: &str,
    target_line: usize,
    target_col: usize,
) -> Option<((usize, usize), (usize, usize))> {
    let offset = pos_to_offset(text, target_line, target_col);
    let parens = scan_parens(text);

    // Check if `offset` or `offset - 1` is in `parens`.
    let paren_idx = parens.iter().position(|p| p.offset == offset).or_else(|| {
        if offset > 0 {
            if let Some(idx) = parens.iter().position(|p| p.offset == offset - 1) {
                let p = parens[idx];
                if !p.is_open {
                    Some(idx)
                } else {
                    let curr_is_word = text.as_bytes().get(offset).map_or(false, |b| {
                        b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-' || *b == b':'
                    });
                    if !curr_is_word {
                        Some(idx)
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    })?;

    let target = parens[paren_idx];
    if target.is_open {
        // Search forward for matching ')'
        let mut depth = 1usize;
        for p in &parens[paren_idx + 1..] {
            if p.is_open {
                depth += 1;
            } else {
                depth -= 1;
                if depth == 0 {
                    let start = offset_to_pos(text, target.offset);
                    let end = offset_to_pos(text, p.offset + 1);
                    return Some((start, end));
                }
            }
        }
    } else {
        // Search backward for matching '('
        let mut depth = 1usize;
        for p in parens[..paren_idx].iter().rev() {
            if !p.is_open {
                depth += 1;
            } else {
                depth -= 1;
                if depth == 0 {
                    let start = offset_to_pos(text, p.offset);
                    let end = offset_to_pos(text, target.offset + 1);
                    return Some((start, end));
                }
            }
        }
    }

    None
}

/// Finds the innermost enclosing S-expression containing the cursor.
/// If cursor is already on a parenthesis, it delegates to `find_matching_paren_range`.
pub fn find_enclosing_paren_range(
    text: &str,
    target_line: usize,
    target_col: usize,
) -> Option<((usize, usize), (usize, usize))> {
    // If the cursor is directly on a paren, use that first
    if let Some(range) = find_matching_paren_range(text, target_line, target_col) {
        return Some(range);
    }

    let offset = pos_to_offset(text, target_line, target_col);
    let parens = scan_parens(text);

    // Look backward from `offset` for an unclosed opening paren
    let mut depth: isize = 0;
    let mut open_idx = None;

    for (idx, p) in parens.iter().enumerate().rev() {
        if p.offset >= offset {
            continue;
        }
        if !p.is_open {
            depth += 1;
        } else {
            if depth == 0 {
                // Found unclosed opening paren enclosing `offset`!
                open_idx = Some(idx);
                break;
            } else {
                depth -= 1;
            }
        }
    }

    let open_idx = open_idx?;
    let open_token = parens[open_idx];

    // Search forward from `open_idx + 1` for matching closing paren
    let mut f_depth = 1usize;
    for p in &parens[open_idx + 1..] {
        if p.is_open {
            f_depth += 1;
        } else {
            f_depth -= 1;
            if f_depth == 0 {
                let start = offset_to_pos(text, open_token.offset);
                let end = offset_to_pos(text, p.offset + 1);
                return Some((start, end));
            }
        }
    }

    None
}

/// Returns all opening `(` positions (1-based line, 1-based col) on `line_1based` outside strings/comments.
pub fn all_open_parens_on_line(
    text: &str,
    line_1based: usize,
) -> Vec<crate::ide::protocol::ParenBreakpoint> {
    if line_1based == 0 {
        return Vec::new();
    }
    let target_line_0 = line_1based - 1;
    let parens = scan_parens(text);
    let mut result = Vec::new();
    for p in parens {
        if p.is_open {
            let (l0, c0) = offset_to_pos(text, p.offset);
            if l0 == target_line_0 {
                result.push(crate::ide::protocol::ParenBreakpoint::new(
                    line_1based,
                    c0 + 1,
                ));
            }
        }
    }
    result
}

/// Returns the first opening `(` on `line_1based` (1-based line and column), if any.
pub fn first_open_paren_on_line(
    text: &str,
    line_1based: usize,
) -> Option<crate::ide::protocol::ParenBreakpoint> {
    all_open_parens_on_line(text, line_1based).into_iter().next()
}

/// Resolves the target opening `(` breakpoint for a 0-based cursor `(line_0based, col_0based)`:
/// 1. If cursor is directly on or immediately after an opening `(`, returns that exact `(`.
/// 2. If cursor is in leading whitespace before the first `(` on `line_0based`, returns the first `(` on `line_0based`.
/// 3. If cursor is on `)` or inside an S-expression `(func arg1 ...)`, returns the opening `(` of that S-expression.
/// 4. Fallback: returns the first `(` on `line_0based` if any.
pub fn resolve_breakpoint_paren_at_cursor(
    text: &str,
    line_0based: usize,
    col_0based: usize,
) -> Option<crate::ide::protocol::ParenBreakpoint> {
    let offset = pos_to_offset(text, line_0based, col_0based);
    let parens = scan_parens(text);

    // 1. Direct hit on '(' at cursor or immediately before cursor
    if let Some(p) = parens.iter().find(|p| p.is_open && p.offset == offset) {
        let (l0, c0) = offset_to_pos(text, p.offset);
        return Some(crate::ide::protocol::ParenBreakpoint::new(l0 + 1, c0 + 1));
    }
    if offset > 0 {
        if let Some(p) = parens.iter().find(|p| p.is_open && p.offset == offset - 1) {
            let (l0, c0) = offset_to_pos(text, p.offset);
            return Some(crate::ide::protocol::ParenBreakpoint::new(l0 + 1, c0 + 1));
        }
    }

    // 2. If cursor is in leading indentation before the first '(' on the same line, target that first '('
    let line_start = pos_to_offset(text, line_0based, 0);
    if offset >= line_start && text.as_bytes()[line_start..offset.min(text.len())].iter().all(|b| b.is_ascii_whitespace()) {
        if let Some(first_on_line) = first_open_paren_on_line(text, line_0based + 1) {
            return Some(first_on_line);
        }
    }

    // 3. Enclosing or matched S-expression (handles cursor on ')' or inside `(func arg1 ...)`):
    if let Some(((start_l0, start_c0), _)) = find_enclosing_paren_range(text, line_0based, col_0based) {
        return Some(crate::ide::protocol::ParenBreakpoint::new(
            start_l0 + 1,
            start_c0 + 1,
        ));
    }

    // 4. Fallback to first '(' on the cursor's line
    first_open_paren_on_line(text, line_0based + 1)
}

/// Returns the 1-based `(end_line, end_col)` of the closing `)` matching `bp`, if balanced.
pub fn matching_close_paren_for_bp(
    text: &str,
    bp: crate::ide::protocol::ParenBreakpoint,
) -> Option<(usize, usize)> {
    if bp.line == 0 || bp.col == 0 {
        return None;
    }
    let ((_, _), (end_l0, end_c0_after)) =
        find_matching_paren_range(text, bp.line - 1, bp.col - 1)?;
    Some((end_l0 + 1, end_c0_after.max(1)))
}

/// Returns a compact preview string (e.g. `(polar center 0.0 ...)`) for the S-expression starting at `bp`.
pub fn sexpr_preview_at_bp(
    text: &str,
    bp: crate::ide::protocol::ParenBreakpoint,
) -> Option<String> {
    if bp.line == 0 || bp.col == 0 {
        return None;
    }
    let start_off = pos_to_offset(text, bp.line - 1, bp.col - 1);
    if start_off >= text.len() || text.as_bytes()[start_off] != b'(' {
        return None;
    }
    let slice = if let Some((_, (end_l0, end_c0_after))) =
        find_matching_paren_range(text, bp.line - 1, bp.col - 1)
    {
        let end_off = pos_to_offset(text, end_l0, end_c0_after).min(text.len());
        &text[start_off..end_off]
    } else {
        &text[start_off..]
    };
    let first_line = slice.lines().next().unwrap_or("").trim();
    if first_line.len() > 36 {
        Some(format!("{}...)", &first_line[..33]))
    } else if slice.contains('\n') && !first_line.ends_with(')') {
        Some(format!("{first_line} ...)"))
    } else {
        Some(first_line.to_string())
    }
}

/// Returns the target position (line, col) to jump the cursor to without selection.
/// - If `forward` is false (Ctrl+[): jumps to the LEFT ONLY (target offset < current offset).
///   When the cursor is immediately after a closing paren ')' (e.g. `(foo)|`), it matches that ')'
///   and jumps to its matching '(' (`|(foo)`).
/// - If `forward` is true (Ctrl+]): jumps to the RIGHT ONLY (target offset > current offset).
///   When the cursor is immediately before an opening paren '(' (e.g. `|(foo)`), it matches that '('
///   and jumps to its matching ')' (`(foo)|`).
pub fn find_target_paren(
    text: &str,
    target_line: usize,
    target_col: usize,
    forward: bool,
) -> Option<(usize, usize)> {
    let offset = pos_to_offset(text, target_line, target_col);
    let parens = scan_parens(text);

    if !forward {
        // MATCH TO THE LEFT ONLY (Ctrl+[)
        // 1. If cursor is immediately after a closing paren ')' (cursor is next to it, outside of it):
        //    Match that ')' and jump left to its matching '('
        if offset > 0 {
            if let Some(close_idx) = parens.iter().position(|p| p.offset == offset - 1 && !p.is_open) {
                let mut depth = 1usize;
                for target in parens[..close_idx].iter().rev() {
                    if !target.is_open {
                        depth += 1;
                    } else {
                        depth -= 1;
                        if depth == 0 {
                            return Some(offset_to_pos(text, target.offset));
                        }
                    }
                }
            }
        }

        // 2. Otherwise, find the innermost unclosed '(' strictly before `offset` and jump to it
        let mut stack = Vec::new();
        for p in &parens {
            if p.offset >= offset {
                break;
            }
            if p.is_open {
                stack.push(p.offset);
            } else {
                stack.pop();
            }
        }

        if let Some(open_offset) = stack.pop() {
            if open_offset < offset {
                return Some(offset_to_pos(text, open_offset));
            }
        }

        None
    } else {
        // MATCH TO THE RIGHT ONLY (Ctrl+])
        // 1. If cursor is immediately before an opening paren '(' (cursor is next to it, outside of it):
        //    Match that '(' and jump right to its matching ')'
        if let Some(open_idx) = parens.iter().position(|p| p.offset == offset && p.is_open) {
            let mut depth = 1usize;
            for target in &parens[open_idx + 1..] {
                if target.is_open {
                    depth += 1;
                } else {
                    depth -= 1;
                    if depth == 0 {
                        return Some(offset_to_pos(text, target.offset + 1));
                    }
                }
            }
        }

        // 2. Otherwise, find the innermost unclosed '(' strictly before `offset` and jump to its matching ')'
        let mut stack = Vec::new();
        for (idx, p) in parens.iter().enumerate() {
            if p.offset >= offset {
                break;
            }
            if p.is_open {
                stack.push(idx);
            } else {
                stack.pop();
            }
        }

        if let Some(open_idx) = stack.pop() {
            let mut depth = 1usize;
            for target in &parens[open_idx + 1..] {
                if target.is_open {
                    depth += 1;
                } else {
                    depth -= 1;
                    if depth == 0 {
                        if target.offset + 1 > offset {
                            return Some(offset_to_pos(text, target.offset + 1));
                        }
                    }
                }
            }
        }

        None
    }
}

/// Returns `true` if the end of `prefix` is inside an unclosed string literal `"..."`
/// or an unclosed block comment `;| ... |;`.
pub fn is_in_multiline_string_or_block_comment(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    let mut in_string = false;
    let mut in_string_escape = false;
    let mut in_line_comment = false;
    let mut block_comment_depth: usize = 0;

    while i < len {
        let b = bytes[i];

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if block_comment_depth > 0 {
            if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
                block_comment_depth += 1;
                i += 2;
                continue;
            } else if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b';' {
                block_comment_depth -= 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_string {
            if in_string_escape {
                in_string_escape = false;
            } else if b == b'\\' {
                in_string_escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
            block_comment_depth = 1;
            i += 2;
            continue;
        } else if b == b';' {
            in_line_comment = true;
            i += 1;
            continue;
        } else if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }

        i += 1;
    }

    in_string || block_comment_depth > 0
}

/// Returns a stack of byte offsets of all unclosed opening `(` in `prefix`
/// from outermost to innermost.
pub fn find_unclosed_parens_stack(prefix: &str) -> Vec<usize> {
    let parens = scan_parens(prefix);
    let mut stack = Vec::new();
    for p in parens {
        if p.is_open {
            stack.push(p.offset);
        } else {
            stack.pop();
        }
    }
    stack
}

/// Inspects the line containing an opening `(` at `open_offset` in `text`.
/// Returns `(head_symbol, first_element_col, first_argument_col)`.
fn extract_head_and_arg1_col(
    text: &str,
    open_offset: usize,
) -> (Option<&str>, Option<usize>, Option<usize>) {
    let (_, open_col) = offset_to_pos(text, open_offset);
    let after_open = &text[open_offset + 1..];
    let line_end = after_open.find(['\r', '\n']).unwrap_or(after_open.len());
    let line_slice = &after_open[..line_end];

    let ws1 = line_slice
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    let rest1 = &line_slice[ws1..];
    if rest1.is_empty() || rest1.starts_with(';') {
        return (None, None, None);
    }

    let first_token_col = open_col + 1 + ws1;
    if rest1.starts_with('(') || rest1.starts_with("'(") {
        return (None, Some(first_token_col), None);
    }

    let head_len = rest1
        .bytes()
        .take_while(|&b| {
            !b.is_ascii_whitespace() && b != b'(' && b != b')' && b != b';' && b != b'"'
        })
        .count();
    if head_len == 0 {
        return (None, Some(first_token_col), None);
    }

    let head = &rest1[..head_len];
    let after_head = &rest1[head_len..];
    let ws2 = after_head
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    let rest2 = &after_head[ws2..];
    let arg1_col = if !rest2.is_empty() && !rest2.starts_with(';') && !rest2.starts_with(')') {
        Some(first_token_col + head_len + ws2)
    } else {
        None
    };

    (Some(head), Some(first_token_col), arg1_col)
}

/// Computes the target space indentation for a line given:
/// - `prefix`: all text preceding the line (up to the `\n` before the line)
/// - `trimmed_line_rest`: the text of the line with leading spaces/tabs stripped
///
/// Returns `None` if `prefix` ends inside an unclosed multi-line string or block comment
/// (so whole-file formatting preserves interior lines of multi-line strings/comments).
/// Computes the target space indentation for a line given formatting options.
pub fn compute_lisp_line_indent_with_options(
    prefix: &str,
    trimmed_line_rest: &str,
    options: &LispFormatOptions,
) -> Option<usize> {
    if is_in_multiline_string_or_block_comment(prefix) {
        return None;
    }

    // 1. Triple-semicolon section header comments (;;;) always format flush-left at column 0
    if trimmed_line_rest.starts_with(";;;") {
        return Some(0);
    }

    let stack = find_unclosed_parens_stack(prefix);

    // 2. If the line starts with a closing ')', align it with its matching opening '('
    if trimmed_line_rest.starts_with(')') {
        let col = stack
            .last()
            .map(|&off| offset_to_pos(prefix, off).1)
            .unwrap_or(0);
        return Some(col);
    }

    // 3. If there is no unclosed '(' before this line, indent to column 0
    let Some(&open_offset) = stack.last() else {
        return Some(0);
    };

    let (_, open_col) = offset_to_pos(prefix, open_offset);
    let is_comment_line = trimmed_line_rest.starts_with(';');

    // 4. Check if the unclosed '(' is a quoted list `'(`
    if open_offset > 0 && prefix.as_bytes()[open_offset - 1] == b'\'' {
        let (_, first_elem_col, _) = extract_head_and_arg1_col(prefix, open_offset);
        return Some(first_elem_col.unwrap_or(open_col + 1));
    }

    // 5. Check if the parent unclosed '(' is `(cond ...)` — if so, `open_offset` is a `cond` clause
    if stack.len() >= 2 {
        let parent_offset = stack[stack.len() - 2];
        let (parent_head, _, _) = extract_head_and_arg1_col(prefix, parent_offset);
        if parent_head.is_some_and(|h| h.eq_ignore_ascii_case("cond")) {
            let (_, first_elem_col, _) = extract_head_and_arg1_col(prefix, open_offset);
            return Some(first_elem_col.unwrap_or(open_col + 1));
        }
    }

    // 6. Inspect the head token and first argument on `open_line`
    let (head_opt, first_elem_col, arg1_col) = extract_head_and_arg1_col(prefix, open_offset);
    match head_opt {
        None => {
            if let Some(c) = first_elem_col {
                Some(c)
            } else {
                Some(open_col + options.indent_size)
            }
        }
        Some(head) => {
            if head.eq_ignore_ascii_case("setq") || head.eq_ignore_ascii_case("set") {
                if options.align_setq_pairs && !is_comment_line {
                    Some(arg1_col.unwrap_or(open_col + options.indent_size))
                } else {
                    Some(open_col + options.indent_size)
                }
            } else if is_lisp_special_form(head) {
                // Special forms always use Narrow Style (2-space indent from open_col)
                Some(open_col + options.indent_size)
            } else if options.wide_style_calls && !is_comment_line {
                // Regular function call (command, strcat, list, mapcar, etc.)
                // Wide style: align multi-line arguments under the first argument on line 1
                Some(arg1_col.unwrap_or(open_col + options.indent_size))
            } else {
                Some(open_col + options.indent_size)
            }
        }
    }
}

pub fn compute_lisp_line_indent(prefix: &str, trimmed_line_rest: &str) -> Option<usize> {
    compute_lisp_line_indent_with_options(prefix, trimmed_line_rest, &LispFormatOptions::default())
}

/// Computes the target space indentation when the user presses `Enter` at `cursor_offset` in `text`.
pub fn compute_lisp_indent_at_offset(text: &str, cursor_offset: usize) -> usize {
    let cursor_offset = cursor_offset.min(text.len());
    let prefix = &text[..cursor_offset];
    let after = &text[cursor_offset..];
    let line_end = after.find(['\r', '\n']).unwrap_or(after.len());
    let rest_of_line = after[..line_end].trim_start_matches([' ', '\t']);
    compute_lisp_line_indent(prefix, rest_of_line).unwrap_or(0)
}

/// Normalizes intra-line whitespace in a single line of OpenCAD LISP code (assuming leading
/// indentation has already been stripped or is handled separately), while tracking
/// multi-line string (`in_string`) and block comment (`block_comment_depth`) state.
///
/// Rules:
/// - Strings `"..."` and comments `;...` / `;|...|;` are preserved verbatim.
/// - No space immediately after `(`, `'`, `` ` ``, `,`, or `,@`.
/// - No space immediately before `)`.
/// - Single space between adjacent parameters/tokens and before `(` or `'` when preceded by a token or `)`.
/// Normalizes intra-line whitespace in a single line of OpenCAD LISP code (assuming leading
/// indentation has already been stripped or is handled separately), while tracking
/// multi-line string (`in_string`) and block comment (`block_comment_depth`) state
/// and formatting options (such as symbol casing normalization).
pub fn normalize_lisp_line_whitespace_with_options(
    trimmed_line: &str,
    in_string: &mut bool,
    block_comment_depth: &mut usize,
    options: &LispFormatOptions,
) -> String {
    let bytes = trimmed_line.trim_end_matches([' ', '\t', '\r']).as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    let mut in_string_escape = false;
    let mut pending_space = false;

    while i < len {
        let b = bytes[i];

        if *block_comment_depth > 0 {
            if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
                *block_comment_depth += 1;
                out.push_str(";|");
                i += 2;
                continue;
            } else if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b';' {
                *block_comment_depth -= 1;
                out.push_str("|;");
                pending_space = true;
                i += 2;
                continue;
            }
            let ch = trimmed_line[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }

        if *in_string {
            let ch = trimmed_line[i..].chars().next().unwrap();
            out.push(ch);
            if in_string_escape {
                in_string_escape = false;
            } else if b == b'\\' {
                in_string_escape = true;
            } else if b == b'"' {
                *in_string = false;
                pending_space = true;
            }
            i += ch.len_utf8();
            continue;
        }

        // Normal code state:
        // 1. Block comment start `;|`
        if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
            if !out.is_empty() && !out.ends_with(['(', '\'', '`', ' ']) {
                out.push(' ');
            }
            pending_space = false;
            *block_comment_depth = 1;
            out.push_str(";|");
            i += 2;
            continue;
        }

        // 2. Line comment `;` — preserve rest of line verbatim
        if b == b';' {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&trimmed_line[i..].trim_end_matches([' ', '\t', '\r']));
            break;
        }

        // 3. Whitespace
        if b == b' ' || b == b'\t' {
            pending_space = true;
            i += 1;
            continue;
        }

        // 4. Closing paren `)` — never preceded by space
        if b == b')' {
            pending_space = false;
            out.push(')');
            i += 1;
            continue;
        }

        // 5. Opening paren `(` — preceded by space if after an atom, string, or `)`, never after `(`, `'`, `` ` ``, `,`
        if b == b'(' {
            if !out.is_empty() && !out.ends_with(['(', '\'', '`', ',', '@', ' ']) {
                out.push(' ');
            }
            pending_space = false;
            out.push('(');
            i += 1;
            continue;
        }

        // 6. Quote / Backquote / Comma prefix `'`, `` ` ``, `,`
        if b == b'\'' || b == b'`' || b == b',' {
            if !out.is_empty() && !out.ends_with(['(', '\'', '`', ',', '#', ' ']) {
                out.push(' ');
            }
            pending_space = false;
            out.push(b as char);
            if b == b',' && i + 1 < len && bytes[i + 1] == b'@' {
                out.push('@');
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // 7. String literal start `"`
        if b == b'"' {
            if !out.is_empty() && !out.ends_with(['(', '\'', '`', ',', ' ']) {
                out.push(' ');
            }
            pending_space = false;
            *in_string = true;
            in_string_escape = false;
            out.push('"');
            i += 1;
            continue;
        }

        // 8. Normal token character (symbol, number, dot, operator)
        if (pending_space || out.ends_with([')', '"']))
            && !out.is_empty()
            && !out.ends_with(['(', '\'', '`', ',', ' '])
            && !(out.ends_with(",@"))
        {
            out.push(' ');
        }
        pending_space = false;

        let start_tok = i;
        while i < len {
            let tb = bytes[i];
            if tb.is_ascii_whitespace()
                || tb == b'('
                || tb == b')'
                || tb == b'"'
                || tb == b';'
                || tb == b'\''
                || tb == b'`'
                || tb == b','
            {
                break;
            }
            i += 1;
        }
        let token = &trimmed_line[start_tok..i];
        if options.downcase_builtins {
            if token.eq_ignore_ascii_case("t") {
                out.push_str("T");
            } else if token.eq_ignore_ascii_case("nil") {
                out.push_str("nil");
            } else if crate::ide::highlight::is_lisp_keyword(token)
                || crate::ide::highlight::is_lisp_builtin(token)
            {
                out.push_str(&token.to_ascii_lowercase());
            } else {
                out.push_str(token);
            }
        } else {
            out.push_str(token);
        }
    }

    out
}

pub fn normalize_lisp_line_whitespace(
    trimmed_line: &str,
    in_string: &mut bool,
    block_comment_depth: &mut usize,
) -> String {
    normalize_lisp_line_whitespace_with_options(
        trimmed_line,
        in_string,
        block_comment_depth,
        &LispFormatOptions::default(),
    )
}

/// Convenience helper to normalize a single standalone line slice with format options.
pub fn normalize_lisp_line_slice_with_options(
    trimmed_line: &str,
    options: &LispFormatOptions,
) -> String {
    let mut in_string = false;
    let mut block_comment_depth = 0usize;
    normalize_lisp_line_whitespace_with_options(
        trimmed_line,
        &mut in_string,
        &mut block_comment_depth,
        options,
    )
}

/// Convenience helper to normalize a single standalone line slice (starting outside string/block-comment).
pub fn normalize_lisp_line_slice(trimmed_line: &str) -> String {
    normalize_lisp_line_slice_with_options(trimmed_line, &LispFormatOptions::default())
}

/// Returns true if all parentheses in `text` outside comments and strings are fully balanced.
pub fn is_parens_balanced(text: &str) -> bool {
    let parens = scan_parens(text);
    if parens.is_empty() {
        return true;
    }
    let mut depth: isize = 0;
    for p in parens {
        if p.is_open {
            depth += 1;
        } else {
            depth -= 1;
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0
}

/// Token kind scanned for structural re-flow.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    OpenParen,
    CloseParen,
    Prefix(String),
    Atom(String),
    StringLiteral(String),
    LineComment(String),
    BlockComment(String),
}

/// Token scanned for structural re-flow with line-break metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct LispToken {
    pub kind: TokenKind,
    pub newlines_before: usize,
}

/// Tokenizes OpenCAD LISP text while recording newlines before each token.
pub fn tokenize_lisp_for_reflow(text: &str) -> Vec<LispToken> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut newlines = 0usize;

    while i < len {
        let b = bytes[i];

        if b == b'\n' {
            newlines += 1;
            i += 1;
            continue;
        }

        if b == b' ' || b == b'\t' || b == b'\r' {
            i += 1;
            continue;
        }

        let nl_before = newlines;
        newlines = 0;

        // Block comment ;| ... |;
        if i + 1 < len && b == b';' && bytes[i + 1] == b'|' {
            let start = i;
            i += 2;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if i + 1 < len && bytes[i] == b';' && bytes[i + 1] == b'|' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b';' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            tokens.push(LispToken {
                kind: TokenKind::BlockComment(text[start..i].to_string()),
                newlines_before: nl_before,
            });
            continue;
        }

        // Line comment ; ... \n
        if b == b';' {
            let start = i;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(LispToken {
                kind: TokenKind::LineComment(text[start..i].trim_end_matches(['\r', '\n']).to_string()),
                newlines_before: nl_before,
            });
            continue;
        }

        // String literal " ... "
        if b == b'"' {
            let start = i;
            i += 1;
            let mut in_escape = false;
            while i < len {
                if in_escape {
                    in_escape = false;
                } else if bytes[i] == b'\\' {
                    in_escape = true;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(LispToken {
                kind: TokenKind::StringLiteral(text[start..i].to_string()),
                newlines_before: nl_before,
            });
            continue;
        }

        // Parentheses
        if b == b'(' {
            tokens.push(LispToken {
                kind: TokenKind::OpenParen,
                newlines_before: nl_before,
            });
            i += 1;
            continue;
        }
        if b == b')' {
            tokens.push(LispToken {
                kind: TokenKind::CloseParen,
                newlines_before: nl_before,
            });
            i += 1;
            continue;
        }

        // Prefixes
        if b == b'\'' || b == b'`' {
            tokens.push(LispToken {
                kind: TokenKind::Prefix((b as char).to_string()),
                newlines_before: nl_before,
            });
            i += 1;
            continue;
        }
        if b == b',' {
            if i + 1 < len && bytes[i + 1] == b'@' {
                tokens.push(LispToken {
                    kind: TokenKind::Prefix(",@".to_string()),
                    newlines_before: nl_before,
                });
                i += 2;
            } else {
                tokens.push(LispToken {
                    kind: TokenKind::Prefix(",".to_string()),
                    newlines_before: nl_before,
                });
                i += 1;
            }
            continue;
        }
        if b == b'#' && i + 1 < len && bytes[i + 1] == b'\'' {
            tokens.push(LispToken {
                kind: TokenKind::Prefix("#'".to_string()),
                newlines_before: nl_before,
            });
            i += 2;
            continue;
        }

        // Atom
        let start = i;
        while i < len {
            let tb = bytes[i];
            if tb.is_ascii_whitespace()
                || tb == b'('
                || tb == b')'
                || tb == b'"'
                || tb == b';'
                || tb == b'\''
                || tb == b'`'
                || tb == b','
            {
                break;
            }
            i += 1;
        }
        tokens.push(LispToken {
            kind: TokenKind::Atom(text[start..i].to_string()),
            newlines_before: nl_before,
        });
    }

    tokens
}

/// S-Expression AST form representation for structural re-flow.
#[derive(Debug, Clone, PartialEq)]
pub enum LispForm {
    Atom {
        text: String,
        newlines_before: usize,
    },
    StringLiteral {
        text: String,
        newlines_before: usize,
    },
    LineComment {
        text: String,
        newlines_before: usize,
    },
    BlockComment {
        text: String,
        newlines_before: usize,
    },
    List {
        prefix: Option<String>,
        elements: Vec<LispForm>,
        close_on_newline: bool,
        has_internal_newlines: bool,
        newlines_before: usize,
    },
}

impl LispForm {
    pub fn newlines_before(&self) -> usize {
        match self {
            LispForm::Atom { newlines_before, .. } => *newlines_before,
            LispForm::StringLiteral { newlines_before, .. } => *newlines_before,
            LispForm::LineComment { newlines_before, .. } => *newlines_before,
            LispForm::BlockComment { newlines_before, .. } => *newlines_before,
            LispForm::List { newlines_before, .. } => *newlines_before,
        }
    }

    pub fn is_comment(&self) -> bool {
        matches!(self, LispForm::LineComment { .. } | LispForm::BlockComment { .. })
    }

    pub fn to_single_line(&self) -> String {
        match self {
            LispForm::Atom { text, .. } => text.clone(),
            LispForm::StringLiteral { text, .. } => text.clone(),
            LispForm::LineComment { text, .. } => text.clone(),
            LispForm::BlockComment { text, .. } => text.clone(),
            LispForm::List { prefix, elements, .. } => {
                let mut out = String::new();
                if let Some(p) = prefix {
                    out.push_str(p);
                }
                out.push('(');
                let non_comments: Vec<_> = elements.iter().filter(|e| !e.is_comment()).collect();
                for (idx, elem) in non_comments.iter().enumerate() {
                    if idx > 0 {
                        out.push(' ');
                    }
                    out.push_str(&elem.to_single_line());
                }
                out.push(')');
                out
            }
        }
    }
}

pub fn parse_lisp_forms(tokens: &[LispToken]) -> Vec<LispForm> {
    let mut forms = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(form) = parse_single_form(tokens, &mut i, None) {
            forms.push(form);
        }
    }
    forms
}

fn parse_single_form(tokens: &[LispToken], i: &mut usize, prefix: Option<String>) -> Option<LispForm> {
    if *i >= tokens.len() {
        return None;
    }

    let tok = &tokens[*i];
    let newlines_before = tok.newlines_before;

    match &tok.kind {
        TokenKind::LineComment(s) => {
            let s_clone = s.clone();
            *i += 1;
            Some(LispForm::LineComment {
                text: s_clone,
                newlines_before,
            })
        }
        TokenKind::BlockComment(s) => {
            let s_clone = s.clone();
            *i += 1;
            Some(LispForm::BlockComment {
                text: s_clone,
                newlines_before,
            })
        }
        TokenKind::StringLiteral(s) => {
            let s_clone = s.clone();
            *i += 1;
            let val = match prefix {
                Some(p) => format!("{p}{s_clone}"),
                None => s_clone,
            };
            Some(LispForm::StringLiteral {
                text: val,
                newlines_before,
            })
        }
        TokenKind::Atom(s) => {
            let s_clone = s.clone();
            *i += 1;
            let val = match prefix {
                Some(p) => format!("{p}{s_clone}"),
                None => s_clone,
            };
            Some(LispForm::Atom {
                text: val,
                newlines_before,
            })
        }
        TokenKind::Prefix(p) => {
            let p_clone = p.clone();
            *i += 1;
            parse_single_form(tokens, i, Some(p_clone))
        }
        TokenKind::OpenParen => {
            *i += 1;
            let mut elements = Vec::new();
            let mut close_on_newline = false;
            let mut has_internal_newlines = false;
            while *i < tokens.len() {
                if let TokenKind::CloseParen = &tokens[*i].kind {
                    close_on_newline = tokens[*i].newlines_before > 0;
                    *i += 1;
                    break;
                }
                if let Some(child) = parse_single_form(tokens, i, None) {
                    if child.newlines_before() > 0 {
                        has_internal_newlines = true;
                    }
                    elements.push(child);
                }
            }
            Some(LispForm::List {
                prefix,
                elements,
                close_on_newline,
                has_internal_newlines,
                newlines_before,
            })
        }
        TokenKind::CloseParen => {
            *i += 1;
            Some(LispForm::Atom {
                text: ")".to_string(),
                newlines_before,
            })
        }
    }
}

fn render_defun(
    prefix: Option<&str>,
    elements: &[LispForm],
    out: &mut String,
    options: &LispFormatOptions,
) {
    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push('(');

    let mut non_comment_indices = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        if !elem.is_comment() {
            non_comment_indices.push(i);
        }
    }

    if non_comment_indices.is_empty() {
        out.push_str("defun)");
        return;
    }

    let defun_elem = &elements[non_comment_indices[0]];
    out.push_str(&defun_elem.to_single_line());

    if non_comment_indices.len() > 1 {
        out.push(' ');
        let name_elem = &elements[non_comment_indices[1]];
        out.push_str(&name_elem.to_single_line());
    }

    if non_comment_indices.len() > 2 {
        out.push(' ');
        let params_elem = &elements[non_comment_indices[2]];
        out.push_str(&params_elem.to_single_line());
    }

    let after_params = if non_comment_indices.len() > 2 {
        non_comment_indices[2] + 1
    } else {
        elements.len()
    };

    let has_body = after_params < elements.len();
    if has_body {
        for elem in &elements[after_params..] {
            if let LispForm::LineComment { text, newlines_before: 0 } = elem {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(text);
                out.push('\n');
            } else {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                render_form_reflowed(elem, out, options);
            }
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push(')');
    } else {
        out.push(')');
    }
}

fn render_progn(
    prefix: Option<&str>,
    elements: &[LispForm],
    out: &mut String,
    options: &LispFormatOptions,
) {
    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push_str("(progn");
    if elements.len() <= 1 {
        out.push(')');
        return;
    }

    for elem in &elements[1..] {
        if let LispForm::LineComment { text, newlines_before: 0 } = elem {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(text);
            out.push('\n');
        } else {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            render_form_reflowed(elem, out, options);
        }
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push(')');
}

fn render_loop(
    prefix: Option<&str>,
    loop_kw: &str,
    elements: &[LispForm],
    out: &mut String,
    options: &LispFormatOptions,
) {
    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push('(');
    out.push_str(loop_kw);

    let mut nc = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        if !elem.is_comment() {
            nc.push(i);
        }
    }

    let header_arg_count = if loop_kw.eq_ignore_ascii_case("foreach") { 2 } else { 1 };
    let mut last_header_idx = 0;
    for &idx in nc.iter().skip(1).take(header_arg_count) {
        out.push(' ');
        out.push_str(&elements[idx].to_single_line());
        last_header_idx = idx;
    }

    let body_start = if last_header_idx > 0 { last_header_idx + 1 } else { elements.len() };
    for elem in &elements[body_start..] {
        if let LispForm::LineComment { text, newlines_before: 0 } = elem {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(text);
            out.push('\n');
        } else {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            render_form_reflowed(elem, out, options);
        }
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push(')');
}

fn render_cond(
    prefix: Option<&str>,
    elements: &[LispForm],
    out: &mut String,
    options: &LispFormatOptions,
) {
    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push_str("(cond");
    if elements.len() <= 1 {
        out.push(')');
        return;
    }

    for elem in &elements[1..] {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if let LispForm::List { prefix: c_pfx, elements: c_elems, close_on_newline, has_internal_newlines, .. } = elem {
            if let Some(p) = c_pfx {
                out.push_str(p);
            }
            out.push('(');
            let c_nc: Vec<_> = c_elems.iter().filter(|e| !e.is_comment()).collect();
            if !*close_on_newline && !*has_internal_newlines && c_nc.len() <= 2 && elem.to_single_line().len() <= 50 {
                for (idx, ce) in c_elems.iter().enumerate() {
                    if idx > 0 {
                        out.push(' ');
                    }
                    render_form_reflowed(ce, out, options);
                }
                out.push(')');
            } else {
                if let Some(first) = c_elems.first() {
                    render_form_reflowed(first, out, options);
                }
                for ce in c_elems.iter().skip(1) {
                    if let LispForm::LineComment { text, newlines_before: 0 } = ce {
                        if !out.ends_with(' ') {
                            out.push(' ');
                        }
                        out.push_str(text);
                        out.push('\n');
                    } else {
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        render_form_reflowed(ce, out, options);
                    }
                }
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push(')');
            }
        } else {
            render_form_reflowed(elem, out, options);
        }
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push(')');
}

fn render_setq(
    prefix: Option<&str>,
    elements: &[LispForm],
    close_on_newline: bool,
    out: &mut String,
    options: &LispFormatOptions,
) -> bool {
    let non_comments: Vec<_> = elements.iter().filter(|e| !e.is_comment()).collect();
    if non_comments.len() <= 1 {
        return false;
    }

    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push_str("(setq ");

    let mut nc_idx = 1;
    let mut first_pair = true;

    for elem in elements.iter().skip(1) {
        if let LispForm::LineComment { text, newlines_before } = elem {
            if *newlines_before == 0 {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(text);
                out.push('\n');
            } else {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(text);
                out.push('\n');
            }
            continue;
        }

        if first_pair {
            if nc_idx == 1 {
                out.push_str(&elem.to_single_line());
                nc_idx += 1;
            } else if nc_idx == 2 {
                out.push(' ');
                render_form_reflowed(elem, out, options);
                first_pair = false;
                nc_idx += 1;
            }
        } else {
            if nc_idx % 2 == 1 {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&elem.to_single_line());
                nc_idx += 1;
            } else {
                out.push(' ');
                render_form_reflowed(elem, out, options);
                nc_idx += 1;
            }
        }
    }

    if close_on_newline && non_comments.len() > 3 {
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push(')');
    true
}

fn render_if(
    prefix: Option<&str>,
    elements: &[LispForm],
    out: &mut String,
    options: &LispFormatOptions,
) -> bool {
    let non_comments: Vec<_> = elements.iter().filter(|e| !e.is_comment()).collect();
    if non_comments.len() < 3 {
        return false;
    }

    let single_line = LispForm::List {
        prefix: prefix.map(|s| s.to_string()),
        elements: elements.to_vec(),
        close_on_newline: false,
        has_internal_newlines: false,
        newlines_before: 0,
    }.to_single_line();

    let then_is_compound = matches!(non_comments[2], LispForm::List { .. });
    let else_is_compound = non_comments.get(3).map_or(false, |e| matches!(e, LispForm::List { .. }));

    if !then_is_compound && !else_is_compound && single_line.len() <= 50 {
        return false;
    }

    if let Some(p) = prefix {
        out.push_str(p);
    }
    out.push_str("(if ");
    out.push_str(&non_comments[1].to_single_line());
    out.push('\n');

    render_form_reflowed(non_comments[2], out, options);

    if non_comments.len() > 3 {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        render_form_reflowed(non_comments[3], out, options);
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push(')');
    true
}

fn render_form_reflowed(form: &LispForm, out: &mut String, options: &LispFormatOptions) {
    match form {
        LispForm::Atom { text, .. } => {
            out.push_str(text);
        }
        LispForm::StringLiteral { text, .. } => {
            out.push_str(text);
        }
        LispForm::LineComment { text, newlines_before } => {
            if *newlines_before == 0 {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            } else {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push_str(text);
            out.push('\n');
        }
        LispForm::BlockComment { text, .. } => {
            out.push_str(text);
        }
        LispForm::List { prefix, elements, close_on_newline, has_internal_newlines, .. } => {
            if elements.is_empty() {
                if let Some(p) = prefix {
                    out.push_str(p);
                }
                out.push_str("()");
                return;
            }

            let has_line_comment = elements.iter().any(|e| matches!(e, LispForm::LineComment { .. }));
            let single_line = form.to_single_line();

            let head_idx = elements.iter().position(|e| !e.is_comment());
            let head = head_idx.and_then(|idx| {
                if let LispForm::Atom { text, .. } = &elements[idx] {
                    Some(text.as_str())
                } else {
                    None
                }
            });

            if let Some(h) = head {
                if h.eq_ignore_ascii_case("defun") || h.eq_ignore_ascii_case("defun-q") {
                    render_defun(prefix.as_deref(), elements, out, options);
                    return;
                }
                if h.eq_ignore_ascii_case("progn") {
                    render_progn(prefix.as_deref(), elements, out, options);
                    return;
                }
                if h.eq_ignore_ascii_case("while")
                    || h.eq_ignore_ascii_case("repeat")
                    || h.eq_ignore_ascii_case("foreach")
                {
                    render_loop(prefix.as_deref(), h, elements, out, options);
                    return;
                }
                if h.eq_ignore_ascii_case("cond") {
                    render_cond(prefix.as_deref(), elements, out, options);
                    return;
                }
                if h.eq_ignore_ascii_case("setq") || h.eq_ignore_ascii_case("set") {
                    if render_setq(prefix.as_deref(), elements, *close_on_newline, out, options) {
                        return;
                    }
                }
                if h.eq_ignore_ascii_case("if") {
                    if render_if(prefix.as_deref(), elements, out, options) {
                        return;
                    }
                }
            }

            let is_compact_head = head.map_or(false, |h| {
                matches!(
                    h,
                    "+" | "-" | "*" | "/" | "=" | "/=" | "<" | ">" | "<=" | ">="
                        | "1+" | "1-" | "not" | "null" | "car" | "cdr" | "cadr" | "caddr" | "cons" | "list"
                )
            });
            let all_simple = elements.iter().all(|e| matches!(e, LispForm::Atom { .. } | LispForm::StringLiteral { .. }));

            // Compact operator expressions (e.g. (+ 1 2)) are joined into plane style
            if is_compact_head && all_simple && !has_line_comment && single_line.len() <= 60 {
                if let Some(p) = prefix {
                    out.push_str(p);
                }
                out.push_str(&single_line);
                return;
            }

            // Wide-style calls: if options.wide_style_calls is true and the list originally had internal newlines:
            if options.wide_style_calls && (*has_internal_newlines || *close_on_newline) && !is_compact_head {
                if let Some(p) = prefix {
                    out.push_str(p);
                }
                out.push('(');
                for (idx, elem) in elements.iter().enumerate() {
                    if idx == 0 {
                        render_form_reflowed(elem, out, options);
                    } else if idx == 1 && elem.newlines_before() == 0 {
                        out.push(' ');
                        render_form_reflowed(elem, out, options);
                    } else {
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        render_form_reflowed(elem, out, options);
                    }
                }
                if *close_on_newline && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push(')');
                return;
            }

            // Short atomic list without comments -> Plane style
            if !has_line_comment && !*has_internal_newlines && !*close_on_newline && single_line.len() <= 60 {
                out.push_str(&single_line);
                return;
            }

            // Also line-join fragmented atoms like (\n + \n 1 \n 2 \n) if all elements are simple atoms!
            if !has_line_comment && all_simple && single_line.len() <= 50 {
                out.push_str(&single_line);
                return;
            }

            // General list fallback
            if let Some(p) = prefix {
                out.push_str(p);
            }
            out.push('(');
            for (idx, elem) in elements.iter().enumerate() {
                if idx > 0 && !out.ends_with(['(', ' ']) {
                    out.push(' ');
                }
                render_form_reflowed(elem, out, options);
            }
            if *close_on_newline && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push(')');
        }
    }
}

/// Performs structural line re-flow (line breaking and line joining) on OpenCAD LISP code.
pub fn reflow_lisp_text(text: &str, options: &LispFormatOptions) -> String {
    let tokens = tokenize_lisp_for_reflow(text);
    if tokens.is_empty() {
        return text.to_string();
    }
    let forms = parse_lisp_forms(&tokens);
    let mut out = String::with_capacity(text.len() + 64);
    for (idx, form) in forms.iter().enumerate() {
        if idx > 0 {
            if form.newlines_before() >= 2 {
                if !out.ends_with("\n\n") {
                    if out.ends_with('\n') {
                        out.push('\n');
                    } else {
                        out.push_str("\n\n");
                    }
                }
            } else {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        render_form_reflowed(form, &mut out, options);
    }
    if text.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Indents and normalizes whitespace strictly line-by-line without re-flowing forms across lines.
pub fn format_lisp_line_range_indent_only(
    text: &str,
    start_line: usize,
    end_line: usize,
    options: &LispFormatOptions,
) -> String {
    let has_trailing_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len() + 64);
    let mut consecutive_blank_lines = 0usize;

    for (idx, raw_line) in lines.iter().enumerate() {
        if idx < start_line || idx > end_line {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(raw_line);
            consecutive_blank_lines = 0;
            continue;
        }

        let trimmed_start = raw_line.trim_start_matches([' ', '\t']);
        if trimmed_start.is_empty() {
            consecutive_blank_lines += 1;
            if consecutive_blank_lines > 2 {
                continue;
            }
            if idx > 0 {
                out.push('\n');
            }
            continue;
        }
        consecutive_blank_lines = 0;

        if idx > 0 {
            out.push('\n');
        }

        match compute_lisp_line_indent_with_options(&out, trimmed_start, options) {
            Some(indent) => {
                for _ in 0..indent {
                    out.push(' ');
                }
                let normalized = normalize_lisp_line_slice_with_options(trimmed_start, options);
                out.push_str(&normalized);
            }
            None => {
                out.push_str(raw_line);
            }
        }
    }

    if has_trailing_newline {
        out.push('\n');
    }
    out
}

/// Re-indents, normalizes whitespace, and optionally performs structural line re-flow
/// on lines `start_line..=end_line` (0-indexed, inclusive) according to OpenCAD LISP rules.
pub fn format_lisp_line_range_with_options(
    text: &str,
    start_line: usize,
    end_line: usize,
    options: &LispFormatOptions,
) -> String {
    if options.structural_reflow {
        if start_line == 0 && end_line == usize::MAX {
            let reflowed = reflow_lisp_text(text, options);
            return format_lisp_line_range_indent_only(&reflowed, 0, usize::MAX, options);
        } else {
            let lines: Vec<&str> = text.lines().collect();
            if !lines.is_empty() {
                let s_line = start_line.min(lines.len() - 1);
                let e_line = end_line.min(lines.len() - 1);
                if s_line <= e_line {
                    let selected_slice = lines[s_line..=e_line].join("\n");
                    if is_parens_balanced(&selected_slice) {
                        let reflowed_slice = reflow_lisp_text(&selected_slice, options);
                        let mut rebuilt = String::with_capacity(text.len() + 64);
                        for l in &lines[..s_line] {
                            rebuilt.push_str(l);
                            rebuilt.push('\n');
                        }
                        rebuilt.push_str(&reflowed_slice);
                        if !rebuilt.ends_with('\n') && e_line + 1 < lines.len() {
                            rebuilt.push('\n');
                        }
                        for (idx, l) in lines[e_line + 1..].iter().enumerate() {
                            if idx > 0 {
                                rebuilt.push('\n');
                            }
                            rebuilt.push_str(l);
                        }
                        let reflowed_line_count = reflowed_slice.lines().count().max(1);
                        let new_end_line = s_line + reflowed_line_count - 1;
                        return format_lisp_line_range_indent_only(&rebuilt, s_line, new_end_line, options);
                    }
                }
            }
        }
    }

    format_lisp_line_range_indent_only(text, start_line, end_line, options)
}

pub fn format_lisp_line_range(text: &str, start_line: usize, end_line: usize) -> String {
    format_lisp_line_range_with_options(text, start_line, end_line, &LispFormatOptions::default())
}

pub fn format_lisp_code_with_options(text: &str, options: &LispFormatOptions) -> String {
    format_lisp_line_range_with_options(text, 0, usize::MAX, options)
}

/// Formats the indentation and intra-line whitespace of an entire OpenCAD LISP source buffer.
pub fn format_lisp_code(text: &str) -> String {
    format_lisp_code_with_options(text, &LispFormatOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pos_to_offset_and_back() {
        let code = "(setq a 10)\n(setq b 20)\n";
        assert_eq!(pos_to_offset(code, 0, 0), 0);
        assert_eq!(pos_to_offset(code, 0, 1), 1);
        assert_eq!(pos_to_offset(code, 1, 0), 12);
        assert_eq!(offset_to_pos(code, 0), (0, 0));
        assert_eq!(offset_to_pos(code, 1), (0, 1));
        assert_eq!(offset_to_pos(code, 12), (1, 0));
    }

    #[test]
    fn test_scan_parens_ignores_strings_and_comments() {
        let code = r#"
            ; (commented-out paren)
            ;| (block commented (nested)) |;
            (setq msg "string (with parens) \" and \\\" (fake)")
            (princ msg)
        "#;
        let parens = scan_parens(code);
        // Only the actual OpenCAD LISP code parens should be captured:
        // (setq msg "...")
        // (princ msg)
        assert_eq!(parens.len(), 4);
        assert!(parens[0].is_open);
        assert!(!parens[1].is_open);
        assert!(parens[2].is_open);
        assert!(!parens[3].is_open);
    }

    #[test]
    fn test_find_matching_paren_single_line() {
        let code = "(setq a (+ 1 2))";
        // Double-click on outermost '(' at col 0
        let range = find_matching_paren_range(code, 0, 0);
        assert_eq!(range, Some(((0, 0), (0, 16))));

        // Double-click on outermost ')' at col 15
        let range = find_matching_paren_range(code, 0, 15);
        assert_eq!(range, Some(((0, 0), (0, 16))));

        // Double-click on inner '(' at col 8
        let range = find_matching_paren_range(code, 0, 8);
        assert_eq!(range, Some(((0, 8), (0, 15))));

        // Double-click on inner ')' at col 14
        let range = find_matching_paren_range(code, 0, 14);
        assert_eq!(range, Some(((0, 8), (0, 15))));
    }

    #[test]
    fn test_find_matching_paren_multiline() {
        let code = "(defun c:test ()\n  (command \"circle\" \"0,0\" 10)\n  (princ)\n)";
        // Outermost '(' is at (0, 0), closing ')' is at (3, 0)
        let range = find_matching_paren_range(code, 0, 0);
        assert_eq!(range, Some(((0, 0), (3, 1))));

        // Outermost ')' at (3, 0)
        let range = find_matching_paren_range(code, 3, 0);
        assert_eq!(range, Some(((0, 0), (3, 1))));

        // Middle (command ...) at line 1, col 2
        let range = find_matching_paren_range(code, 1, 2);
        assert_eq!(range, Some(((1, 2), (1, 29))));
    }

    #[test]
    fn test_find_enclosing_paren_range() {
        let code = "(defun c:test () (setq x 10) (princ))";
        // Cursor inside `setq` at col 20
        let range = find_enclosing_paren_range(code, 0, 20);
        assert_eq!(range, Some(((0, 17), (0, 28))));
    }

    #[test]
    fn test_find_target_paren_jump() {
        let code = "(setq a (+ 1 2))";
        // At col 0, jump forward to matching ')' (lands outside, after ')')
        let target = find_target_paren(code, 0, 0, true);
        assert_eq!(target, Some((0, 16)));

        // At col 16 (after outer ')'), jump backward to matching '(' (lands outside, before '(')
        let target = find_target_paren(code, 0, 16, false);
        assert_eq!(target, Some((0, 0)));

        // At col 15 (after inner ')'), jump backward to matching inner '(' (lands outside, before inner '(')
        let target = find_target_paren(code, 0, 15, false);
        assert_eq!(target, Some((0, 8)));

        // Inside code at col 3 ('t' in setq), jump forward to closing ')' (lands outside)
        let target = find_target_paren(code, 0, 3, true);
        assert_eq!(target, Some((0, 16)));

        // Inside code at col 3, jump backward to opening '(' (lands outside)
        let target = find_target_paren(code, 0, 3, false);
        assert_eq!(target, Some((0, 0)));
    }

    #[test]
    fn test_smart_enter_and_lisp_formatting() {
        // 1. Top-level closed expression -> 0 spaces
        let top = "(setq a 10)";
        assert_eq!(compute_lisp_indent_at_offset(top, top.len()), 0);

        // 2. Inside defun -> 2 spaces
        let defun_open = "(defun c:test (/ pt)";
        assert_eq!(compute_lisp_indent_at_offset(defun_open, defun_open.len()), 2);

        // 3. Inside setq with first variable on same line -> aligns b under a (col 8 when setq is at col 2)
        let setq_open = "(defun c:test ()\n  (setq pt1 (getpoint \"\\nPick: \")";
        assert_eq!(compute_lisp_indent_at_offset(setq_open, setq_open.len()), 8);

        // 4. Dropping closing ')' onto its own line -> aligns with matching '('
        let before_close = "(defun c:test ()\n  (if (> x 0)\n    (progn\n      (princ))";
        // Cursor right before the last ')' which closes `(progn` at col 4
        assert_eq!(
            compute_lisp_indent_at_offset(before_close, before_close.len() - 1),
            4
        );

        // 5. Intra-line whitespace normalization between parameters and parentheses
        assert_eq!(
            normalize_lisp_line_slice("(  defun   c:test   (  /   a   b  )  )"),
            "(defun c:test (/ a b))"
        );
        assert_eq!(
            normalize_lisp_line_slice("(  setq   filter   '  (  (  0   .   \"LINE\"  )   (  8   .   \"WALLS\"  )  )  )"),
            "(setq filter '((0 . \"LINE\") (8 . \"WALLS\")))"
        );
        assert_eq!(
            normalize_lisp_line_slice("(if(> a 0)(princ \"keep   inner   spaces\"))  ;   comment   spaces"),
            "(if (> a 0) (princ \"keep   inner   spaces\")) ;   comment   spaces"
        );

        // 6. Full OpenCAD LISP format check (including collapsing extra spaces inside parens & between args)
        let unformatted = "\
(  defun   c:MyCommand   ( /   pt1   pt2   dist )
(  setq   pt1    (  getpoint   \"\\nSpecify first point: \"  )
pt2    (  getpoint   pt1   \"\\nSpecify second point: \"  )
dist   (  distance   pt1   pt2  )
)
(  if   (  >   dist   10.0  )
(  progn
(  command   \"_.CIRCLE\"   pt1   dist  )
(  princ   \"\\nLarge circle drawn.\"  )
)
(  princ   \"\\nDistance too small.\"  )
)
(  cond
(  (  <   dist   5.0  )
(  princ   \"\\nVery small.\"  )
)
(  T
(  princ   \"\\nMedium.\"  )
)
)
(  princ  )
)";
        let expected = "\
(defun c:MyCommand (/ pt1 pt2 dist)
  (setq pt1 (getpoint \"\\nSpecify first point: \")
        pt2 (getpoint pt1 \"\\nSpecify second point: \")
        dist (distance pt1 pt2)
  )
  (if (> dist 10.0)
    (progn
      (command \"_.CIRCLE\" pt1 dist)
      (princ \"\\nLarge circle drawn.\")
    )
    (princ \"\\nDistance too small.\")
  )
  (cond
    ((< dist 5.0)
     (princ \"\\nVery small.\")
    )
    (T
     (princ \"\\nMedium.\")
    )
  )
  (princ)
)";
        assert_eq!(format_lisp_code(unformatted), expected);
    }

    #[test]
    fn test_find_target_paren_directional() {
        let code = "(setq a (+ 1 2))";
        // Positions:
        // 0: '('
        // 8: '(+ 1 2)' inner '('
        // 14: '(+ 1 2)' inner ')'
        // 15: outer ')'
        // 16: after outer ')'

        // Test 1: Cursor at offset 16 (after outer ')'): `(setq a (+ 1 2))|`
        // Ctrl+[ (forward=false) should match outer ')' and jump left to `|(setq a (+ 1 2))` (offset 0)
        assert_eq!(find_target_paren(code, 0, 16, false), Some((0, 0)));
        // Ctrl+] (forward=true) at offset 16 must NOT jump backwards: returns None
        assert_eq!(find_target_paren(code, 0, 16, true), None);

        // Test 2: Cursor at offset 0 (before outer '('): `|(setq a (+ 1 2))`
        // Ctrl+] (forward=true) should match outer '(' and jump right to `(setq a (+ 1 2))|` (offset 16)
        assert_eq!(find_target_paren(code, 0, 0, true), Some((0, 16)));
        // Ctrl+[ (forward=false) at offset 0 must NOT jump forward: returns None
        assert_eq!(find_target_paren(code, 0, 0, false), None);

        // Test 3: Cursor between parens at offset 15: `(setq a (+ 1 2)|)`
        // Next to inner ')' at offset 14, but outside of (+ 1 2)
        // Ctrl+[ matches inner ')' and jumps left to `(setq a |(+ 1 2))` (offset 8)
        assert_eq!(find_target_paren(code, 0, 15, false), Some((0, 8)));
        // Ctrl+] inside outer expr jumps right to outer ')' outside: `(setq a (+ 1 2))|` (offset 16)
        assert_eq!(find_target_paren(code, 0, 15, true), Some((0, 16)));

        // Test 4: Cursor at offset 8 (before inner '('): `(setq a |(+ 1 2))`
        // Ctrl+] matches inner '(' and jumps right to `(setq a (+ 1 2)|)` (offset 15)
        assert_eq!(find_target_paren(code, 0, 8, true), Some((0, 15)));
        // Ctrl+[ inside outer expr jumps left to outer '(': `|(setq a (+ 1 2))` (offset 0)
        assert_eq!(find_target_paren(code, 0, 8, false), Some((0, 0)));

        // Test 5: Cursor inside inner expr at offset 11: `(setq a (+ 1| 2))`
        // Ctrl+[ jumps left to inner '(' at offset 8
        assert_eq!(find_target_paren(code, 0, 11, false), Some((0, 8)));
        // Ctrl+] jumps right to inner ')' at offset 15
        assert_eq!(find_target_paren(code, 0, 11, true), Some((0, 15)));
    }

    #[test]
    fn test_wide_style_calls() {
        let code_standalone_close = "(command \"_.LINE\"\npt1\npt2\n\"\"\n)";
        let formatted = format_lisp_code(code_standalone_close);
        let expected = "(command \"_.LINE\"\n         pt1\n         pt2\n         \"\"\n)";
        assert_eq!(formatted, expected);

        let code_inline_close = "(command \"_.LINE\"\npt1\npt2\n\"\")";
        let formatted2 = format_lisp_code(code_inline_close);
        let expected2 = "(command \"_.LINE\"\n         pt1\n         pt2\n         \"\")";
        assert_eq!(formatted2, expected2);
    }

    #[test]
    fn test_comment_conventions() {
        let code = "\
;;; ==============================
;;; Program Header (Flush Left)
;;; ==============================
(defun c:test ()
;; Block comment indents to current block level
(setq a 1) ; inline comment
(princ)
)";
        let formatted = format_lisp_code(code);
        let expected = "\
;;; ==============================
;;; Program Header (Flush Left)
;;; ==============================
(defun c:test ()
  ;; Block comment indents to current block level
  (setq a 1) ; inline comment
  (princ)
)";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_symbol_casing_normalization() {
        let code = "\
(DEFUN C:MYCOMMAND ( / P1 P2 )
  (SETQ P1 (GETPOINT \"\\nPick: \")
        P2 (GETCORNER P1 \"\\nCorner: \")
  )
  (COND
    ((NULL P1)
     (PRINC \"\\nCancelled.\")
    )
    (T
     (COMMAND \"_.RECTANGLE\" P1 P2)
    )
  )
  (PRINC)
)";
        let formatted = format_lisp_code(code);
        let expected = "\
(defun C:MYCOMMAND (/ P1 P2)
  (setq P1 (getpoint \"\\nPick: \")
        P2 (getcorner P1 \"\\nCorner: \")
  )
  (cond
    ((null P1)
     (princ \"\\nCancelled.\")
    )
    (T
     (command \"_.RECTANGLE\" P1 P2)
    )
  )
  (princ)
)";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_format_options_custom() {
        let mut opts = LispFormatOptions::default();
        opts.downcase_builtins = false;
        opts.wide_style_calls = false;
        opts.indent_size = 4;

        let code = "(DEFUN TEST ()\n(COMMAND \"LINE\" A B)\n)";
        let formatted = format_lisp_code_with_options(code, &opts);
        let expected = "(DEFUN TEST ()\n    (COMMAND \"LINE\" A B)\n)";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_structural_line_breaking() {
        // 1. defun body on single line expands to multiple lines
        let single_line_defun = "(defun c:test () (setq a 1) (princ))";
        let formatted_defun = format_lisp_code(single_line_defun);
        let expected_defun = "(defun c:test ()\n  (setq a 1)\n  (princ)\n)";
        assert_eq!(formatted_defun, expected_defun);

        // 2. progn body on single line expands to multiple lines
        let single_line_progn = "(progn (princ \"hello\") (setq x 10))";
        let formatted_progn = format_lisp_code(single_line_progn);
        let expected_progn = "(progn\n  (princ \"hello\")\n  (setq x 10)\n)";
        assert_eq!(formatted_progn, expected_progn);

        // 3. multi-pair setq on single line expands each pair to a new line
        let single_line_setq = "(setq a 1 b 2 c 3)";
        let formatted_setq = format_lisp_code(single_line_setq);
        let expected_setq = "(setq a 1\n      b 2\n      c 3)";
        assert_eq!(formatted_setq, expected_setq);

        // Multi-pair setq with closing paren on its own line preserves standalone close
        let sc_setq = "(setq a 1 b 2 c 3\n)";
        let formatted_sc = format_lisp_code(sc_setq);
        let expected_sc = "(setq a 1\n      b 2\n      c 3\n)";
        assert_eq!(formatted_sc, expected_sc);

        // 4. while loop on single line expands body to separate lines
        let single_line_while = "(while (< i 10) (princ i) (setq i (1+ i)))";
        let formatted_while = format_lisp_code(single_line_while);
        let expected_while = "(while (< i 10)\n  (princ i)\n  (setq i (1+ i))\n)";
        assert_eq!(formatted_while, expected_while);
    }

    #[test]
    fn test_structural_line_joining() {
        // 1. Fragmented defun header and atomic addition join onto single lines
        let fragmented = "(\n defun \n c:test \n ( / a b ) \n ( \n + 1 \n 2 \n ) \n)";
        let formatted = format_lisp_code(fragmented);
        let expected = "(defun c:test (/ a b)\n  (+ 1 2)\n)";
        assert_eq!(formatted, expected);

        // 2. Fragmented single-pair setq joins
        let fragmented_setq = "(\n setq \n x \n 100 \n)";
        let formatted_setq = format_lisp_code(fragmented_setq);
        assert_eq!(formatted_setq, "(setq x 100)");
    }

    #[test]
    fn test_structural_reflow_disabled() {
        let mut opts = LispFormatOptions::default();
        opts.structural_reflow = false;

        // With reflow disabled, single-line defun is kept on one line
        let code = "(defun c:test () (setq a 1) (princ))";
        let formatted = format_lisp_code_with_options(code, &opts);
        assert_eq!(formatted, "(defun c:test () (setq a 1) (princ))");
    }

    #[test]
    fn test_breakpoint_paren_resolution_and_preview() {
        use crate::ide::protocol::ParenBreakpoint;
        let code = "(defun c:demo (/ a b)\n  (setq a (+ 1 2) b (* a 3))\n  (princ b)\n)";

        // Line 2 has 3 opening parens: (setq ... at col 3, (+ 1 2) at col 11, (* a 3) at col 21
        let parens_line2 = all_open_parens_on_line(code, 2);
        assert_eq!(
            parens_line2,
            vec![
                ParenBreakpoint::new(2, 3),
                ParenBreakpoint::new(2, 11),
                ParenBreakpoint::new(2, 21),
            ]
        );
        assert_eq!(first_open_paren_on_line(code, 2), Some(ParenBreakpoint::new(2, 3)));

        // Cursor at `+` on line 2 (0-based line 1, col 11) resolves to enclosing `(+ 1 2)` at (2, 11)
        let bp_inner = resolve_breakpoint_paren_at_cursor(code, 1, 11).unwrap();
        assert_eq!(bp_inner, ParenBreakpoint::new(2, 11));
        assert_eq!(sexpr_preview_at_bp(code, bp_inner).as_deref(), Some("(+ 1 2)"));

        // Cursor at `)` of `(+ 1 2)` (0-based line 1, col 16) resolves to its matching `(` at (2, 11)
        let bp_from_close = resolve_breakpoint_paren_at_cursor(code, 1, 16).unwrap();
        assert_eq!(bp_from_close, ParenBreakpoint::new(2, 11));

        // Cursor at `b` on line 2 (0-based line 1, col 18) resolves to enclosing `(setq ...)` at (2, 3)
        let bp_outer = resolve_breakpoint_paren_at_cursor(code, 1, 18).unwrap();
        assert_eq!(bp_outer, ParenBreakpoint::new(2, 3));
        assert_eq!(
            sexpr_preview_at_bp(code, bp_outer).as_deref(),
            Some("(setq a (+ 1 2) b (* a 3))")
        );
    }
}

