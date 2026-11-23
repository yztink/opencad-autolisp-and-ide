use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use iced::{Color, Font};
use serde::{Deserialize, Serialize};

pub const DEFAULT_FONT_NAME: &str = "Consolas (Bold)";

/// User-customizable syntax highlighting colors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyntaxColors {
    #[serde(with = "color_serde")]
    pub background: Color,
    #[serde(with = "color_serde")]
    pub default_text: Color,
    #[serde(with = "color_serde")]
    pub comment: Color,
    #[serde(with = "color_serde")]
    pub string: Color,
    #[serde(with = "color_serde")]
    pub keyword: Color,
    #[serde(with = "color_serde")]
    pub builtin_function: Color,
    #[serde(default, with = "opt_color_serde")]
    pub custom_function: Option<Color>,
    #[serde(with = "color_serde")]
    pub number: Color,
    #[serde(with = "color_serde")]
    pub paren: Color,
}

impl Default for SyntaxColors {
    fn default() -> Self {
        Self::base16_mocha()
    }
}

impl SyntaxColors {
    /// Default OCIDE theme matching Base16 Mocha.
    pub fn base16_mocha() -> Self {
        Self {
            background: Color::from_rgb8(0x20, 0x22, 0x25),
            default_text: Color::from_rgb8(0xD0, 0xC8, 0xC6),
            comment: Color::from_rgb8(0x7E, 0x70, 0x5A),
            string: Color::from_rgb8(0xBE, 0xB5, 0x5B),
            keyword: Color::from_rgb8(0xA8, 0x9B, 0xB9),
            builtin_function: Color::from_rgb8(0x8A, 0xB3, 0xB5),
            custom_function: None, // None = match builtin_function
            number: Color::from_rgb8(0xD2, 0x8B, 0x71),
            paren: Color::from_rgb8(0xD0, 0xC8, 0xC6),
        }
    }

    /// Classic Light Theme.
    pub fn classic_light() -> Self {
        Self {
            background: Color::from_rgb8(0xFF, 0xFF, 0xFF),
            default_text: Color::from_rgb8(0x00, 0x00, 0x00),
            comment: Color::from_rgb8(0x80, 0x00, 0x80), // Classic purple comments
            string: Color::from_rgb8(0xA0, 0x52, 0x2D),  // Sienna brown strings
            keyword: Color::from_rgb8(0x00, 0x00, 0xFF), // Blue keywords
            builtin_function: Color::from_rgb8(0x00, 0x00, 0x80), // Navy built-ins
            custom_function: None,
            number: Color::from_rgb8(0x00, 0x80, 0x80),  // Teal numbers
            paren: Color::from_rgb8(0xFF, 0x00, 0x00),   // Red parentheses
        }
    }

    /// Monokai Dark Theme.
    pub fn monokai() -> Self {
        Self {
            background: Color::from_rgb8(0x27, 0x28, 0x22),
            default_text: Color::from_rgb8(0xF8, 0xF8, 0xF2),
            comment: Color::from_rgb8(0x75, 0x71, 0x5E),
            string: Color::from_rgb8(0xE6, 0xDB, 0x74),
            keyword: Color::from_rgb8(0xF9, 0x26, 0x72),
            builtin_function: Color::from_rgb8(0x66, 0xD9, 0xEF),
            custom_function: None,
            number: Color::from_rgb8(0xAE, 0x81, 0xFF),
            paren: Color::from_rgb8(0xF8, 0xF8, 0xF2),
        }
    }

    /// Solarized Dark Theme.
    pub fn solarized_dark() -> Self {
        Self {
            background: Color::from_rgb8(0x00, 0x2B, 0x36),
            default_text: Color::from_rgb8(0x83, 0x94, 0x96),
            comment: Color::from_rgb8(0x58, 0x6E, 0x75),
            string: Color::from_rgb8(0x2A, 0xA1, 0x98),
            keyword: Color::from_rgb8(0x85, 0x99, 0x00),
            builtin_function: Color::from_rgb8(0x26, 0x8B, 0xD2),
            custom_function: None,
            number: Color::from_rgb8(0xD3, 0x36, 0x82),
            paren: Color::from_rgb8(0x93, 0xA1, 0xA1),
        }
    }

    /// Returns the effective color for user custom functions.
    pub fn effective_custom_func_color(&self) -> Color {
        self.custom_function.unwrap_or(self.builtin_function)
    }
}

/// A user-saved custom syntax color theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserTheme {
    pub name: String,
    pub colors: SyntaxColors,
}

/// Convert an `iced::Color` to a 6-digit hex string `#RRGGBB`.
pub fn color_to_hex(c: Color) -> String {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Parse a hex string (`#RRGGBB` or `#RGB`) into an `iced::Color`.
pub fn hex_to_color(hex: &str) -> Option<Color> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some(Color::from_rgb8(r, g, b))
    } else if clean.len() == 3 {
        let r = u8::from_str_radix(&clean[0..1], 16).ok()?;
        let g = u8::from_str_radix(&clean[1..2], 16).ok()?;
        let b = u8::from_str_radix(&clean[2..3], 16).ok()?;
        Some(Color::from_rgb8(r * 17, g * 17, b * 17))
    } else {
        None
    }
}

/// Helper module for serializing/deserializing iced::Color as [r, g, b, a].
pub mod color_serde {
    use iced::Color;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(color: &Color, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        [color.r, color.g, color.b, color.a].serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Color, D::Error>
    where
        D: Deserializer<'de>,
    {
        let [r, g, b, a] = <[f32; 4]>::deserialize(deserializer)?;
        Ok(Color { r, g, b, a })
    }
}

/// Helper module for Option<Color>.
pub mod opt_color_serde {
    use iced::Color;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(opt: &Option<Color>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        opt.map(|c| [c.r, c.g, c.b, c.a]).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Color>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = <Option<[f32; 4]>>::deserialize(deserializer)?;
        Ok(opt.map(|[r, g, b, a]| Color { r, g, b, a }))
    }
}

/// Supported font weight variants for selection in OCIDE.
pub const FONT_WEIGHT_VARIANTS: &[(&str, iced::font::Weight)] = &[
    ("", iced::font::Weight::Normal),
    (" (Bold)", iced::font::Weight::Bold),
    (" (Extra Bold)", iced::font::Weight::ExtraBold),
    (" (SemiBold)", iced::font::Weight::Semibold),
    (" (Medium)", iced::font::Weight::Medium),
    (" (Light)", iced::font::Weight::Light),
];

pub fn weight_num_to_suffix(w: u16) -> Option<(&'static str, iced::font::Weight)> {
    match w {
        100..=150 => Some((" (Thin)", iced::font::Weight::Thin)),
        151..=250 => Some((" (Extra Light)", iced::font::Weight::ExtraLight)),
        251..=350 => Some((" (Light)", iced::font::Weight::Light)),
        351..=450 => Some(("", iced::font::Weight::Normal)),
        451..=550 => Some((" (Medium)", iced::font::Weight::Medium)),
        551..=650 => Some((" (SemiBold)", iced::font::Weight::Semibold)),
        651..=750 => Some((" (Bold)", iced::font::Weight::Bold)),
        751..=850 => Some((" (Extra Bold)", iced::font::Weight::ExtraBold)),
        851..=999 => Some((" (Black)", iced::font::Weight::Black)),
        _ => None,
    }
}

/// Parses a font selection string like `"Consolas (Bold)"` into base family name and weight.
pub fn parse_font_spec(spec: &str) -> (&str, iced::font::Weight) {
    let trimmed = spec.trim();
    if let Some(base) = trimmed.strip_suffix(" (Extra Bold)").or_else(|| trimmed.strip_suffix(" (ExtraBold)")) {
        (base.trim(), iced::font::Weight::ExtraBold)
    } else if let Some(base) = trimmed.strip_suffix(" (Bold)") {
        (base.trim(), iced::font::Weight::Bold)
    } else if let Some(base) = trimmed.strip_suffix(" (SemiBold)").or_else(|| trimmed.strip_suffix(" (Semi Bold)")) {
        (base.trim(), iced::font::Weight::Semibold)
    } else if let Some(base) = trimmed.strip_suffix(" (Medium)") {
        (base.trim(), iced::font::Weight::Medium)
    } else if let Some(base) = trimmed.strip_suffix(" (Light)") {
        (base.trim(), iced::font::Weight::Light)
    } else if let Some(base) = trimmed.strip_suffix(" (Thin)") {
        (base.trim(), iced::font::Weight::Thin)
    } else if let Some(base) = trimmed.strip_suffix(" (Black)") {
        (base.trim(), iced::font::Weight::Black)
    } else {
        (trimmed, iced::font::Weight::Normal)
    }
}

/// Enumerate base monospace font family names on the user's system.
pub fn available_base_monospace_families() -> &'static [&'static str] {
    static FAMILIES: OnceLock<Vec<&'static str>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let popular_mono = [
            "Consolas",
            "Cascadia Code",
            "Cascadia Mono",
            "Courier New",
            "Lucida Console",
            "Fira Code",
            "JetBrains Mono",
            "Source Code Pro",
        ];

        let mut seen = BTreeSet::new();
        for face in db.faces() {
            let is_mono = face.monospaced
                || popular_mono.iter().any(|&m| face.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(m)));
            if is_mono {
                for (family, _) in &face.families {
                    if !family.starts_with('@') && !family.is_empty() {
                        seen.insert(family.clone());
                    }
                }
            }
        }

        for fam in popular_mono {
            if db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            }).is_some() {
                seen.insert(fam.to_string());
            }
        }

        let mut sorted: Vec<String> = seen.into_iter().collect();
        sorted.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

        let mut list = Vec::new();
        for s in sorted {
            list.push(Box::leak(s.into_boxed_str()) as &'static str);
        }
        list
    })
}

/// Enumerate all installed monospace font entries, only creating an entry if the font family actually supports the weight/style.
pub fn available_monospace_fonts() -> &'static [&'static str] {
    static FONTS: OnceLock<Vec<&'static str>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let popular_mono = [
            "Consolas",
            "Cascadia Code",
            "Cascadia Mono",
            "Courier New",
            "Lucida Console",
            "Fira Code",
            "JetBrains Mono",
            "Source Code Pro",
        ];

        let mut family_weights: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
        for face in db.faces() {
            let is_mono = face.monospaced
                || popular_mono.iter().any(|&m| face.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(m)));
            if is_mono {
                for (family, _) in &face.families {
                    if !family.starts_with('@') && !family.is_empty() {
                        family_weights.entry(family.clone()).or_default().insert(face.weight.0);
                    }
                }
            }
        }

        for fam in popular_mono {
            if db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            }).is_some() {
                family_weights.entry(fam.to_string()).or_default().insert(400);
            }
        }

        // Put DEFAULT_FONT_NAME ("Consolas (Bold)") at the top as the default choice
        let mut list = vec![DEFAULT_FONT_NAME];

        for (fam, weights) in family_weights {
            let mut variants: Vec<(&'static str, iced::font::Weight)> = Vec::new();
            for &w in &weights {
                if let Some(v) = weight_num_to_suffix(w) {
                    if !variants.iter().any(|&(s, _)| s == v.0) {
                        variants.push(v);
                    }
                }
            }
            // Sort variants: Regular ("") first, then Thin to Black
            variants.sort_by_key(|&(s, w)| {
                if s.is_empty() {
                    0
                } else {
                    match w {
                        iced::font::Weight::Thin => 1,
                        iced::font::Weight::ExtraLight => 2,
                        iced::font::Weight::Light => 3,
                        iced::font::Weight::Normal => 4,
                        iced::font::Weight::Medium => 5,
                        iced::font::Weight::Semibold => 6,
                        iced::font::Weight::Bold => 7,
                        iced::font::Weight::ExtraBold => 8,
                        iced::font::Weight::Black => 9,
                    }
                }
            });

            for (suffix, _) in variants {
                let entry = format!("{}{}", fam, suffix);
                if entry != DEFAULT_FONT_NAME {
                    list.push(Box::leak(entry.into_boxed_str()) as &'static str);
                }
            }
        }

        list
    })
}

/// Map a chosen font name string (e.g. `"Consolas (Bold)"`) to an `iced::Font`.
pub fn resolve_monospace_font(name: Option<&str>) -> Font {
    let raw = match name {
        None => DEFAULT_FONT_NAME,
        Some(s) if s.trim().is_empty() => DEFAULT_FONT_NAME,
        Some(s) => s.trim(),
    };

    let (base_family, weight) = parse_font_spec(raw);

    if base_family.contains("Default") {
        Font::new("Consolas").weight(weight)
    } else {
        Font::new(Box::leak(base_family.to_string().into_boxed_str())).weight(weight)
    }
}

/// Extract `advanceWidth / unitsPerEm` from raw TrueType/OpenType font bytes for a monospace font.
fn extract_ttf_monospace_advance_ratio(data: &[u8], face_index: u32) -> Option<f32> {
    if data.len() < 12 {
        return None;
    }
    let table_dir_offset = if &data[0..4] == b"ttcf" {
        if data.len() < 16 {
            return None;
        }
        let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        if face_index >= num_fonts {
            return None;
        }
        let off_pos = 12 + (face_index as usize) * 4;
        if off_pos + 4 > data.len() {
            return None;
        }
        u32::from_be_bytes([
            data[off_pos],
            data[off_pos + 1],
            data[off_pos + 2],
            data[off_pos + 3],
        ]) as usize
    } else {
        0usize
    };

    if table_dir_offset + 12 > data.len() {
        return None;
    }
    let num_tables = u16::from_be_bytes([
        data[table_dir_offset + 4],
        data[table_dir_offset + 5],
    ]) as usize;

    let mut head_off = None;
    let mut hhea_off = None;
    let mut hmtx_off = None;

    for i in 0..num_tables {
        let rec = table_dir_offset + 12 + i * 16;
        if rec + 16 > data.len() {
            break;
        }
        let tag = &data[rec..rec + 4];
        let offset = u32::from_be_bytes([
            data[rec + 8],
            data[rec + 9],
            data[rec + 10],
            data[rec + 11],
        ]) as usize;
        match tag {
            b"head" => head_off = Some(offset),
            b"hhea" => hhea_off = Some(offset),
            b"hmtx" => hmtx_off = Some(offset),
            _ => {}
        }
    }

    let head = head_off?;
    let hhea = hhea_off?;
    let hmtx = hmtx_off?;
    if head + 20 > data.len() || hhea + 36 > data.len() {
        return None;
    }

    let units_per_em = u16::from_be_bytes([data[head + 18], data[head + 19]]) as f32;
    if !(16.0..=16384.0).contains(&units_per_em) {
        return None;
    }

    let num_h_metrics = u16::from_be_bytes([data[hhea + 34], data[hhea + 35]]) as usize;
    if num_h_metrics == 0 {
        return None;
    }

    // Sample printable glyph entries in hmtx (indices 3..32) to get the monospace cell advance width
    let check_end = num_h_metrics.min(32);
    let start_idx = if check_end > 4 { 4 } else { 0 };
    for idx in start_idx..check_end {
        let pos = hmtx + idx * 4;
        if pos + 2 > data.len() {
            break;
        }
        let adv = u16::from_be_bytes([data[pos], data[pos + 1]]) as f32;
        if adv > 0.0 {
            let ratio = adv / units_per_em;
            if (0.35..=0.85).contains(&ratio) {
                return Some(ratio);
            }
        }
    }

    None
}

/// Return the exact horizontal character advance ratio (`char_width / font_size`) for the given monospace font.
pub fn monospace_advance_ratio(name: Option<&str>) -> f32 {
    static RATIOS: OnceLock<std::sync::Mutex<BTreeMap<String, f32>>> = OnceLock::new();
    let raw = match name {
        None => DEFAULT_FONT_NAME,
        Some(s) if s.trim().is_empty() => DEFAULT_FONT_NAME,
        Some(s) => s.trim(),
    };
    let (base_family, _) = parse_font_spec(raw);
    let fam_name = if base_family.contains("Default") || base_family.is_empty() {
        "Consolas"
    } else {
        base_family
    };
    let key = fam_name.to_ascii_lowercase();

    let cache = RATIOS.get_or_init(|| std::sync::Mutex::new(BTreeMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(&ratio) = guard.get(&key) {
            return ratio;
        }
    }

    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let queried = db
        .query(&fontdb::Query {
            families: &[fontdb::Family::Name(fam_name)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        })
        .and_then(|id| db.with_face_data(id, extract_ttf_monospace_advance_ratio).flatten());

    let ratio = queried.unwrap_or_else(|| {
        if key.contains("consolas") {
            1126.0 / 2048.0 // 0.5498047
        } else if key.contains("cascadia") {
            1200.0 / 2048.0 // 0.5859375
        } else if key.contains("courier") {
            1229.0 / 2048.0 // 0.60009766
        } else if key.contains("lucida") {
            1234.0 / 2048.0 // 0.60253906
        } else {
            1126.0 / 2048.0
        }
    });

    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, ratio);
    }
    ratio
}

/// Settings passed to `LispHighlighter`.
#[derive(Debug, Clone, PartialEq)]
pub struct LispHighlightSettings {
    pub colors: SyntaxColors,
    pub custom_functions: Arc<BTreeSet<String>>,
    pub is_lisp: bool,
    pub breakpoints: Arc<BTreeSet<crate::ide::protocol::ParenBreakpoint>>,
}

/// Highlighting token types for Lisp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LispTokenType {
    DefaultText,
    Comment,
    String,
    Keyword,
    BuiltinFunction,
    CustomFunction,
    Number,
    Paren,
}

/// Custom OpenCAD LISP syntax highlighter implementing `iced::advanced::text::highlighter::Highlighter`.
pub struct LispHighlighter {
    settings: LispHighlightSettings,
    current_line: usize,
    /// Tracks whether a line ends inside a multiline block comment (`;| ... |;`)
    in_block_comment_after_line: Vec<bool>,
}

impl iced::advanced::text::highlighter::Highlighter for LispHighlighter {
    type Settings = LispHighlightSettings;
    type Highlight = iced::advanced::text::highlighter::Format<Font>;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Self::Highlight)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            settings: settings.clone(),
            current_line: 0,
            in_block_comment_after_line: Vec::new(),
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.settings = new_settings.clone();
    }

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
        self.in_block_comment_after_line.truncate(line);
    }

    fn current_line(&self) -> usize {
        self.current_line
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let is_in_block = if self.current_line > 0 && self.current_line <= self.in_block_comment_after_line.len() {
            self.in_block_comment_after_line[self.current_line - 1]
        } else {
            false
        };

        let (tokens, ends_in_block) = tokenize_lisp_line(
            line,
            is_in_block,
            &self.settings.custom_functions,
            self.settings.is_lisp,
        );

        if self.current_line < self.in_block_comment_after_line.len() {
            self.in_block_comment_after_line[self.current_line] = ends_in_block;
        } else {
            self.in_block_comment_after_line.push(ends_in_block);
        }
        self.current_line += 1;
        let line_1based = self.current_line;

        let colors = &self.settings.colors;
        let bps = &self.settings.breakpoints;
        let formats: Vec<(Range<usize>, iced::advanced::text::highlighter::Format<Font>)> = tokens
            .into_iter()
            .map(|(range, tok)| {
                let is_bp_paren = tok == LispTokenType::Paren
                    && !bps.is_empty()
                    && line.as_bytes().get(range.start) == Some(&b'(')
                    && bps.contains(&crate::ide::protocol::ParenBreakpoint::new(
                        line_1based,
                        range.start + 1,
                    ));
                let color = if is_bp_paren {
                    Color::from_rgb(1.0, 0.22, 0.22)
                } else {
                    match tok {
                        LispTokenType::DefaultText => colors.default_text,
                        LispTokenType::Comment => colors.comment,
                        LispTokenType::String => colors.string,
                        LispTokenType::Keyword => colors.keyword,
                        LispTokenType::BuiltinFunction => colors.builtin_function,
                        LispTokenType::CustomFunction => colors.effective_custom_func_color(),
                        LispTokenType::Number => colors.number,
                        LispTokenType::Paren => colors.paren,
                    }
                };
                (
                    range,
                    iced::advanced::text::highlighter::Format {
                        color: Some(color),
                        font: None,
                    },
                )
            })
            .collect();

        formats.into_iter()
    }
}

/// Check if an identifier is a standard OpenCAD LISP keyword or special form.
pub fn is_lisp_keyword(ident: &str) -> bool {
    let lower = ident.to_ascii_lowercase();
    matches!(
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
            | "t"
            | "nil"
            | "pi"
            | "pause"
    )
}

/// Check if an identifier is a native built-in OpenCAD LISP, VL, VLAX, or CAD function.
pub fn is_lisp_builtin(ident: &str) -> bool {
    let lower = ident.to_ascii_lowercase();
    if lower.starts_with("vl-")
        || lower.starts_with("vlax-")
        || lower.starts_with("vla-")
        || lower.starts_with("cad:")
        || lower.starts_with("oc-")
    {
        return true;
    }

    matches!(
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
            | "redraw" | "vports" | "ver" | "vl-version" | "get_clipboard" | "set_clipboard"
            | "msgbox"
    )
}

/// Tokenize a single line of OpenCAD LISP code into highlighting spans.
pub fn tokenize_lisp_line(
    line: &str,
    mut in_block_comment: bool,
    custom_functions: &BTreeSet<String>,
    is_lisp: bool,
) -> (Vec<(Range<usize>, LispTokenType)>, bool) {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Fallback for non-Lisp (Python scripts)
    if !is_lisp {
        while i < len {
            if bytes[i] == b'#' {
                tokens.push((i..len, LispTokenType::Comment));
                break;
            } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                let start = i;
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < len {
                    i += 1;
                }
                tokens.push((start..i, LispTokenType::String));
            } else {
                let start = i;
                while i < len && bytes[i] != b'#' && bytes[i] != b'"' && bytes[i] != b'\'' {
                    i += 1;
                }
                tokens.push((start..i, LispTokenType::DefaultText));
            }
        }
        return (tokens, false);
    }

    while i < len {
        // 1. Inside a multiline block comment: ;| ... |;
        if in_block_comment {
            let start = i;
            let mut closed = false;
            while i + 1 < len {
                if bytes[i] == b'|' && bytes[i + 1] == b';' {
                    i += 2;
                    closed = true;
                    in_block_comment = false;
                    break;
                }
                i += 1;
            }
            if !closed {
                i = len;
            }
            tokens.push((start..i, LispTokenType::Comment));
            continue;
        }

        // 2. Whitespace
        if bytes[i].is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            tokens.push((start..i, LispTokenType::DefaultText));
            continue;
        }

        // 3. Comments: block comment start ;| or line comment ;
        if bytes[i] == b';' {
            if i + 1 < len && bytes[i + 1] == b'|' {
                let start = i;
                i += 2;
                in_block_comment = true;
                let mut closed = false;
                while i + 1 < len {
                    if bytes[i] == b'|' && bytes[i + 1] == b';' {
                        i += 2;
                        closed = true;
                        in_block_comment = false;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    i = len;
                }
                tokens.push((start..i, LispTokenType::Comment));
                continue;
            } else {
                // Line comment continues to end of line
                tokens.push((i..len, LispTokenType::Comment));
                break;
            }
        }

        // 4. String literal: "..."
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1;
            }
            tokens.push((start..i, LispTokenType::String));
            continue;
        }

        // 5. Parentheses and quote prefixes: ' ( ) ` ,
        if bytes[i] == b'(' || bytes[i] == b')' {
            tokens.push((i..i + 1, LispTokenType::Paren));
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' || bytes[i] == b'`' || bytes[i] == b',' {
            tokens.push((i..i + 1, LispTokenType::Paren));
            i += 1;
            continue;
        }

        // 6. Number or Symbol / Identifier
        let start = i;
        while i < len
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'('
            && bytes[i] != b')'
            && bytes[i] != b'"'
            && bytes[i] != b';'
            && bytes[i] != b'\''
            && bytes[i] != b'`'
        {
            i += 1;
        }

        let word = &line[start..i];
        let tok_type = if is_number_literal(word) {
            LispTokenType::Number
        } else if is_lisp_keyword(word) {
            LispTokenType::Keyword
        } else if custom_functions.contains(&word.to_ascii_lowercase()) {
            LispTokenType::CustomFunction
        } else if is_lisp_builtin(word) {
            LispTokenType::BuiltinFunction
        } else {
            LispTokenType::DefaultText
        };

        tokens.push((start..i, tok_type));
    }

    (tokens, in_block_comment)
}

/// Helper to test if a string slice is a numeric literal.
fn is_number_literal(s: &str) -> bool {
    if s.is_empty() || s == "+" || s == "-" {
        return false;
    }
    let trimmed = s.strip_prefix('+').or_else(|| s.strip_prefix('-')).unwrap_or(s);
    if trimmed.is_empty() {
        return false;
    }
    let mut has_digit = false;
    let mut has_dot = false;
    let mut chars = trimmed.chars();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            has_digit = true;
        } else if c == '.' && !has_dot {
            has_dot = true;
        } else if (c == 'e' || c == 'E') && has_digit {
            // Scientific notation: e+10, e-5, e2
            let rest: String = chars.collect();
            let exp_trimmed = rest.strip_prefix('+').or_else(|| rest.strip_prefix('-')).unwrap_or(&rest);
            return !exp_trimmed.is_empty() && exp_trimmed.chars().all(|ch| ch.is_ascii_digit());
        } else {
            return false;
        }
    }
    has_digit
}

/// Calculates the 0-based visual display column for a 1-based character/byte column,
/// correctly expanding `\t` tab stops every 8 columns (matching `cosmic-text` and standard text editors).
pub fn visual_col_for_byte_col(line: &str, byte_col_1based: usize) -> usize {
    if byte_col_1based <= 1 {
        return 0;
    }
    let target_offset = byte_col_1based - 1;
    let mut visual_col = 0;
    for (idx, ch) in line.char_indices() {
        if idx >= target_offset {
            break;
        }
        if ch == '\t' {
            visual_col = (visual_col / 8 + 1) * 8;
        } else {
            visual_col += 1;
        }
    }
    visual_col
}

/// Calculates the 0-based byte/char offset in `line` corresponding to a 0-based visual display column,
/// correctly accounting for `\t` tab stops every 8 columns.
pub fn byte_col_for_visual_col(line: &str, visual_col: usize) -> usize {
    let mut cur_vis = 0;
    for (idx, ch) in line.char_indices() {
        if cur_vis >= visual_col {
            return idx;
        }
        if ch == '\t' {
            let next_stop = (cur_vis / 8 + 1) * 8;
            if next_stop > visual_col {
                return idx;
            }
            cur_vis = next_stop;
        } else {
            cur_vis += 1;
        }
    }
    line.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_number_literal() {
        assert!(is_number_literal("42"));
        assert!(is_number_literal("-10"));
        assert!(is_number_literal("+3.14"));
        assert!(is_number_literal(".5"));
        assert!(is_number_literal("1e6"));
        assert!(is_number_literal("-2.5E-3"));
        assert!(!is_number_literal("+"));
        assert!(!is_number_literal("-"));
        assert!(!is_number_literal("1+"));
        assert!(!is_number_literal("1-"));
        assert!(!is_number_literal("my-var"));
    }

    #[test]
    fn test_tokenize_lisp_line() {
        let mut custom = BTreeSet::new();
        custom.insert("my-helper".to_string());

        let line = "(defun c:test () (my-helper \"hello\") ; test comment\n)";
        let (tokens, ends_in_block) = tokenize_lisp_line(line, false, &custom, true);
        assert!(!ends_in_block);

        let types: Vec<LispTokenType> = tokens.into_iter().map(|(_, t)| t).collect();
        assert!(types.contains(&LispTokenType::Keyword)); // defun
        assert!(types.contains(&LispTokenType::CustomFunction)); // my-helper
        assert!(types.contains(&LispTokenType::String)); // "hello"
        assert!(types.contains(&LispTokenType::Comment)); // ; test comment
        assert!(types.contains(&LispTokenType::Paren)); // ( )
    }

    #[test]
    fn test_available_monospace_fonts_has_default() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let popular_mono = [
            "Consolas",
            "Cascadia Code",
            "Cascadia Mono",
            "Courier New",
            "Lucida Console",
            "Fira Code",
            "JetBrains Mono",
            "Source Code Pro",
        ];
        let mut weights_by_fam: std::collections::BTreeMap<String, std::collections::BTreeSet<u16>> = std::collections::BTreeMap::new();
        for face in db.faces() {
            let is_mono = face.monospaced || popular_mono.iter().any(|&m| face.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(m)));
            if is_mono {
                for (family, _) in &face.families {
                    if !family.starts_with('@') && !family.is_empty() {
                        weights_by_fam.entry(family.clone()).or_default().insert(face.weight.0);
                    }
                }
            }
        }
        let fonts = available_monospace_fonts();
        assert_eq!(fonts[0], DEFAULT_FONT_NAME);
        assert_eq!(DEFAULT_FONT_NAME, "Consolas (Bold)");
        assert!(fonts.contains(&"Consolas (Bold)"));
        // Lucida Console only supports Regular (400), so Lucida Console (Bold) should not exist
        assert!(!fonts.contains(&"Lucida Console (Bold)"));
    }

    #[test]
    fn test_hex_and_color_roundtrip() {
        let col = Color::from_rgb8(0x20, 0x22, 0x25);
        let hex = color_to_hex(col);
        assert_eq!(hex, "#202225");

        let parsed = hex_to_color(&hex).expect("parsed color");
        assert_eq!(color_to_hex(parsed), "#202225");

        // 3-digit hex
        let parsed_3 = hex_to_color("#FFF").expect("parsed 3-digit");
        assert_eq!(color_to_hex(parsed_3), "#FFFFFF");
    }

    #[test]
    fn test_ide_settings_serialization_and_custom_func_modes() {
        let mut settings = crate::ide::IdeSettings::default();
        assert!(settings.custom_function_use_native_color);
        assert_eq!(settings.syntax_colors.custom_function, None);
        assert_eq!(settings.syntax_colors.effective_custom_func_color(), settings.syntax_colors.builtin_function);

        // Switch to dedicated custom color
        settings.set_custom_function_color_mode(false);
        assert!(!settings.custom_function_use_native_color);
        assert!(settings.syntax_colors.custom_function.is_some());
        let custom_c = settings.syntax_colors.custom_function.unwrap();
        assert_eq!(settings.syntax_colors.effective_custom_func_color(), custom_c);

        // Switch back to matching native color
        settings.set_custom_function_color_mode(true);
        assert!(settings.custom_function_use_native_color);
        assert_eq!(settings.syntax_colors.custom_function, None);
        assert_eq!(settings.syntax_colors.effective_custom_func_color(), settings.syntax_colors.builtin_function);

        // Serialize to JSON and deserialize back
        let json = serde_json::to_string_pretty(&settings).expect("serialized json");
        let deserialized: crate::ide::IdeSettings = serde_json::from_str(&json).expect("deserialized json");
        assert_eq!(deserialized.custom_function_use_native_color, settings.custom_function_use_native_color);
        assert_eq!(deserialized.syntax_colors.background, settings.syntax_colors.background);
    }

    #[test]
    fn test_presets_colors() {
        let mocha = SyntaxColors::base16_mocha();
        assert_eq!(color_to_hex(mocha.background), "#202225");

        let classic = SyntaxColors::classic_light();
        assert_eq!(color_to_hex(classic.background), "#FFFFFF");
        assert_eq!(color_to_hex(classic.keyword), "#0000FF"); // Blue

        let monokai = SyntaxColors::monokai();
        assert_eq!(color_to_hex(monokai.background), "#272822");

        let solarized = SyntaxColors::solarized_dark();
        assert_eq!(color_to_hex(solarized.background), "#002B36");
    }

    #[test]
    fn test_user_themes_saving_and_deletion() {
        let mut settings = crate::ide::IdeSettings::default();
        assert!(settings.user_themes.is_empty());

        let mut custom_colors = SyntaxColors::monokai();
        custom_colors.background = Color::from_rgb8(10, 20, 30);

        let saved_name = settings.save_user_theme("My Dark Theme".to_string(), custom_colors.clone());
        assert_eq!(saved_name, "My Dark Theme");
        assert_eq!(settings.user_themes.len(), 1);
        assert_eq!(settings.user_themes[0].name, "My Dark Theme");
        assert_eq!(settings.user_themes[0].colors.background, Color::from_rgb8(10, 20, 30));

        // Overwrite existing user theme
        let mut updated_colors = custom_colors.clone();
        updated_colors.background = Color::from_rgb8(40, 50, 60);
        let saved_name2 = settings.save_user_theme("My Dark Theme".to_string(), updated_colors);
        assert_eq!(saved_name2, "My Dark Theme");
        assert_eq!(settings.user_themes.len(), 1);
        assert_eq!(settings.user_themes[0].colors.background, Color::from_rgb8(40, 50, 60));

        // Delete user theme
        settings.delete_user_theme("My Dark Theme");
        assert!(settings.user_themes.is_empty());
    }

    #[test]
    fn test_custom_functions_list_add_remove_and_legacy_deserialization() {
        let mut settings = crate::ide::IdeSettings::default();
        assert_eq!(settings.custom_functions, vec!["my-func".to_string(), "draw-plate".to_string(), "c:test".to_string()]);

        // Add duplicate or empty
        assert!(!settings.add_custom_function("".to_string()));
        assert!(!settings.add_custom_function("draw-plate".to_string()));

        // Add new
        assert!(settings.add_custom_function("c:my-cmd".to_string()));
        assert!(settings.custom_functions.contains(&"c:my-cmd".to_string()));
        assert!(settings.is_custom_function("c:my-cmd"));

        // Remove
        settings.remove_custom_function("draw-plate");
        assert!(!settings.custom_functions.contains(&"draw-plate".to_string()));
        assert!(!settings.is_custom_function("draw-plate"));

        // Test legacy comma-separated string JSON deserialization
        let legacy_json = r#"{
            "custom_functions": "func-one, func-two, func-three"
        }"#;
        let mut deserialized: crate::ide::IdeSettings = serde_json::from_str(legacy_json).expect("deserialized legacy json");
        deserialized.rebuild_custom_functions_set();
        assert_eq!(deserialized.custom_functions, vec!["func-one".to_string(), "func-two".to_string(), "func-three".to_string()]);
        assert!(deserialized.is_custom_function("func-one"));
        assert!(deserialized.is_custom_function("func-two"));
        assert!(deserialized.is_custom_function("func-three"));
    }

    #[test]
    fn test_visual_col_for_byte_col() {
        assert_eq!(visual_col_for_byte_col("(+ 1 2)", 1), 0);
        assert_eq!(visual_col_for_byte_col("(+ 1 2)", 2), 1);
        // Tab at start -> next char at byte 1 is visual column 8
        assert_eq!(visual_col_for_byte_col("\t(setq", 2), 8);
        // Tab + 3 spaces -> next char at byte 4 is visual column 11
        assert_eq!(visual_col_for_byte_col("\t   (setq", 5), 11);

        // Reverse conversion: visual column to byte offset
        assert_eq!(byte_col_for_visual_col("\t   (setq", 0), 0);
        assert_eq!(byte_col_for_visual_col("\t   (setq", 7), 0);
        assert_eq!(byte_col_for_visual_col("\t   (setq", 8), 1);
        assert_eq!(byte_col_for_visual_col("\t   (setq", 11), 4);
    }

    #[test]
    fn test_monospace_advance_ratio_and_editor_char_width() {
        let ratio = monospace_advance_ratio(Some("Consolas (Bold)"));
        let expected = 1126.0 / 2048.0;
        assert!(
            (ratio - expected).abs() < 0.002,
            "Consolas advance ratio should match 1126/2048 ({expected}), got {ratio}"
        );

        let settings = crate::ide::IdeSettings::default();
        let char_w = settings.editor_char_width();
        let expected_w = 13.0 * expected;
        assert!(
            (char_w - expected_w).abs() < 0.05,
            "13px Consolas char width should be ~{expected_w}px, got {char_w}px"
        );
    }
}

