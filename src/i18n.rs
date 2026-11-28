//! Localization runtime for OpenCAD LISP & IDE (Project Fluent `.ftl` catalogs).
//!
//! Uses the same Fluent + `i18n-embed` stack as OpenCAD Studio:
//! - Source catalog: `locales/en-US/opencad_lisp.ftl` (embedded in the DLL/EXE).
//! - Host language: inherited from OpenCAD Studio via `OCS_LANGUAGE`
//!   (`ocs_plugin_api::host_language()`), falling back to the OS locale and then `en-US`.
//! - OCIDE override: optional explicit BCP 47 tag in `ocide_settings.json`.
//! - External `.ftl` overrides: optional `locales/<tag>/opencad_lisp.ftl` on disk
//!   (via `OCS_LOCALE_DIR`, next to the plugin DLL / IDE executable, or in the
//!   OpenCAD Studio plugin folder) so translators can test edits without compiling Rust.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(not(test))]
use i18n_embed::DesktopLanguageRequester;
use fluent::{FluentArgs, FluentValue};
use i18n_embed::fluent::{fluent_language_loader, FluentLanguageLoader};
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{I18nAssets, I18nEmbedError, LanguageLoader};
use rust_embed::RustEmbed;

pub const DOMAIN: &str = "opencad_lisp";
const CATALOG_FILE: &str = "opencad_lisp.ftl";

#[derive(RustEmbed)]
#[folder = "locales/"]
struct EmbeddedLocalizations;

/// Asset source that checks external `locales/` directories on disk before
/// falling back to the `.ftl` files embedded at compile time.
pub struct DiskOverlayAssets {
    extra_dirs: Vec<PathBuf>,
}

impl DiskOverlayAssets {
    /// Create an overlay asset loader with explicit disk directories (used in tests).
    pub fn with_dirs(extra_dirs: Vec<PathBuf>) -> Self {
        Self { extra_dirs }
    }

    /// Standard search order for external translator `.ftl` files:
    /// 1. `OCS_LOCALE_DIR` environment variable (if set)
    /// 2. `<OCS_PLUGIN_CDYLIB_PATH parent>/locales`
    /// 3. `<current_exe parent>/locales`
    /// 4. `<config_dir>/OpenCADStudio/plugins/opencad.lisp_and_ide/locales`
    pub fn default_paths() -> Self {
        let mut dirs = Vec::new();
        let mut push_unique = |p: PathBuf| {
            if p.as_os_str().is_empty() {
                return;
            }
            let exists = dirs.iter().any(|existing: &PathBuf| {
                if cfg!(windows) {
                    existing
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&p.to_string_lossy())
                } else {
                    existing == &p
                }
            });
            if !exists {
                dirs.push(p);
            }
        };

        if let Ok(custom) = std::env::var("OCS_LOCALE_DIR") {
            let trimmed = custom.trim();
            if !trimmed.is_empty() {
                push_unique(PathBuf::from(trimmed));
            }
        }

        if let Ok(cdylib) = std::env::var("OCS_PLUGIN_CDYLIB_PATH") {
            if let Some(parent) = Path::new(&cdylib).parent() {
                push_unique(parent.join("locales"));
            }
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                push_unique(parent.join("locales"));
            }
        }

        if let Some(config_base) = config_base_dir() {
            push_unique(
                config_base
                    .join("OpenCADStudio")
                    .join("plugins")
                    .join("opencad.lisp_and_ide")
                    .join("locales"),
            );
        }

        Self { extra_dirs: dirs }
    }
}

fn config_base_dir() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        Some(PathBuf::from(appdata))
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        Some(PathBuf::from(xdg))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
    }
}

fn is_safe_relative_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains("..")
        && !path.contains(':')
}

impl I18nAssets for DiskOverlayAssets {
    fn get_files(&self, file_path: &str) -> Vec<Cow<'_, [u8]>> {
        let mut files = Vec::new();
        if is_safe_relative_asset_path(file_path) {
            for dir in &self.extra_dirs {
                let candidate = dir.join(file_path);
                if candidate.is_file() {
                    if let Ok(bytes) = std::fs::read(&candidate) {
                        files.push(Cow::Owned(bytes));
                    }
                }
            }
        }

        if let Some(embedded) = EmbeddedLocalizations::get(file_path) {
            files.push(embedded.data);
        }

        files
    }

    fn filenames_iter(&self) -> Box<dyn Iterator<Item = String> + '_> {
        let mut names = BTreeSet::new();

        for embedded in EmbeddedLocalizations::iter() {
            names.insert(embedded.into_owned());
        }

        for dir in &self.extra_dirs {
            let Ok(lang_entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for lang_entry in lang_entries.flatten() {
                let lang_path = lang_entry.path();
                if !lang_path.is_dir() {
                    continue;
                }
                let Some(lang_name) = lang_entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let Ok(file_entries) = std::fs::read_dir(&lang_path) else {
                    continue;
                };
                for file_entry in file_entries.flatten() {
                    let file_path = file_entry.path();
                    if !file_path.is_file() {
                        continue;
                    }
                    let Some(file_name) = file_entry.file_name().to_str().map(str::to_string) else {
                        continue;
                    };
                    if file_name.ends_with(".ftl") {
                        names.insert(format!("{lang_name}/{file_name}"));
                    }
                }
            }
        }

        Box::new(names.into_iter())
    }
}

fn parse_lang_tag(raw: &str) -> Option<LanguageIdentifier> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<LanguageIdentifier>()
        .ok()
        .or_else(|| trimmed.replace('_', "-").parse::<LanguageIdentifier>().ok())
}

fn requested_languages(override_tag: Option<&str>) -> Vec<LanguageIdentifier> {
    if let Some(tag) = override_tag.and_then(parse_lang_tag) {
        return vec![tag];
    }

    // Keep unit tests deterministic in English regardless of developer OS locale
    // or OCS_LANGUAGE in the surrounding environment.
    #[cfg(test)]
    {
        vec!["en-US".parse().expect("en-US is a valid language tag")]
    }

    #[cfg(not(test))]
    {
        let mut requested = Vec::new();
        if let Some(host_lang) = ocs_plugin_api::host_language().and_then(|s| parse_lang_tag(&s)) {
            requested.push(host_lang);
        }
        for sys_lang in DesktopLanguageRequester::requested_languages() {
            if !requested.contains(&sys_lang) {
                requested.push(sys_lang);
            }
        }
        if requested.is_empty() {
            requested.push("en-US".parse().expect("en-US is a valid language tag"));
        }
        requested
    }
}

fn load_into(
    loader: &FluentLanguageLoader,
    assets: &dyn I18nAssets,
    override_tag: Option<&str>,
) -> Result<Vec<LanguageIdentifier>, I18nEmbedError> {
    let mut requested = requested_languages(override_tag);
    if requested.is_empty() {
        requested.push(loader.fallback_language().clone());
    }
    i18n_embed::select(loader, assets, &requested)
}

/// Process-wide Fluent language loader.
pub fn loader() -> &'static FluentLanguageLoader {
    static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();
    LOADER.get_or_init(|| {
        let loader = fluent_language_loader!();
        let assets = DiskOverlayAssets::default_paths();
        if let Err(err) = load_into(&loader, &assets, None) {
            eprintln!("[i18n] Unable to load preferred language: {err}");
            let _ = loader.load_languages(&assets, &[loader.fallback_language().clone()]);
        }
        loader
    })
}

/// Initialize or refresh the process-wide language from OpenCAD Studio (`OCS_LANGUAGE`)
/// or the operating system locale.
pub fn init_from_host_or_system() {
    let _ = set_language(None);
}

/// Initialize the OCIDE GUI language using the optional setting override,
/// falling back to `OCS_LANGUAGE`, OS locale, and `en-US`.
pub fn init_for_ide(override_tag: Option<&str>) {
    let _ = set_language(override_tag);
}

/// Switch the active UI language at runtime (`None` = follow OpenCAD Studio / System).
pub fn set_language(
    override_tag: Option<&str>,
) -> Result<Vec<LanguageIdentifier>, I18nEmbedError> {
    let assets = DiskOverlayAssets::default_paths();
    let loader = loader();
    let result = load_into(loader, &assets, override_tag);
    if result.is_err() {
        let _ = loader.load_languages(&assets, &[loader.fallback_language().clone()]);
    }
    result
}

/// Currently active BCP 47 language tag (e.g. `"en-US"`, `"es-ES"`).
pub fn active_language_tag() -> String {
    loader()
        .current_languages()
        .into_iter()
        .next()
        .unwrap_or_else(|| loader().fallback_language().clone())
        .to_string()
}

/// All available locale tags discovered across both embedded catalogs and disk overlays.
pub fn available_languages() -> Vec<String> {
    available_languages_in(&DiskOverlayAssets::default_paths())
}

fn available_languages_in(assets: &dyn I18nAssets) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for filename in assets.filenames_iter() {
        let normalized = filename.replace('\\', "/");
        if let Some((tag, file)) = normalized.split_once('/') {
            if file == CATALOG_FILE && parse_lang_tag(tag).is_some() {
                tags.insert(tag.to_string());
            }
        }
    }
    let mut ordered = Vec::with_capacity(tags.len());
    if tags.remove("en-US") {
        ordered.push("en-US".to_string());
    }
    ordered.extend(tags);
    ordered
}

/// Human-friendly native label for a BCP 47 language tag in the Settings dropdown.
pub fn language_display_name(tag: &str) -> String {
    match tag {
        "en-US" => "English (en-US)".to_string(),
        "es-ES" => "Español (es-ES)".to_string(),
        "fr-FR" => "Français (fr-FR)".to_string(),
        "de-DE" => "Deutsch (de-DE)".to_string(),
        "pt-BR" => "Português (pt-BR)".to_string(),
        "it-IT" => "Italiano (it-IT)".to_string(),
        "nl-NL" => "Nederlands (nl-NL)".to_string(),
        "pl-PL" => "Polski (pl-PL)".to_string(),
        "ru-RU" => "Русский (ru-RU)".to_string(),
        "tr-TR" => "Türkçe (tr-TR)".to_string(),
        "zh-CN" => "简体中文 (zh-CN)".to_string(),
        "zh-TW" => "繁體中文 (zh-TW)".to_string(),
        "ja-JP" => "日本語 (ja-JP)".to_string(),
        "ko-KR" => "한국어 (ko-KR)".to_string(),
        "ar-SA" => "العربية (ar-SA)".to_string(),
        "hi-IN" => "हिन्दी (hi-IN)".to_string(),
        "cs-CZ" => "Čeština (cs-CZ)".to_string(),
        "bg-BG" => "Български (bg-BG)".to_string(),
        "el-GR" => "Ελληνικά (el-GR)".to_string(),
        "fi-FI" => "Suomi (fi-FI)".to_string(),
        "hu-HU" => "Magyar (hu-HU)".to_string(),
        "vi-VN" => "Tiếng Việt (vi-VN)".to_string(),
        "lo-LA" => "ລາວ (lo-LA)".to_string(),
        "uz-UZ" => "Oʻzbek (uz-UZ)".to_string(),
        other => other.to_string(),
    }
}

/// Strip Fluent's Unicode bidi isolation marks (`\u{2068}` / `\u{2069}`) around
/// interpolated values so command-line output, dialogs, and Iced text stay clean.
pub fn plain(text: String) -> String {
    if text.contains(['\u{2068}', '\u{2069}']) {
        text.replace(['\u{2068}', '\u{2069}'], "")
    } else {
        text
    }
}

/// Argument value passed to a parameterized Fluent message (`tr_args`).
#[derive(Debug, Clone)]
pub enum Arg<'a> {
    Str(&'a str),
    Owned(String),
    Num(i64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(v: &'a str) -> Self {
        Self::Str(v)
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(v: &'a String) -> Self {
        Self::Str(v.as_str())
    }
}

impl From<String> for Arg<'_> {
    fn from(v: String) -> Self {
        Self::Owned(v)
    }
}

impl From<usize> for Arg<'_> {
    fn from(v: usize) -> Self {
        Self::Num(v as i64)
    }
}

impl From<i64> for Arg<'_> {
    fn from(v: i64) -> Self {
        Self::Num(v)
    }
}

impl From<u32> for Arg<'_> {
    fn from(v: u32) -> Self {
        Self::Num(v as i64)
    }
}

fn format_args_with(
    loader: &FluentLanguageLoader,
    id: &str,
    args: &[(&str, Arg<'_>)],
) -> String {
    let mut fluent_args = FluentArgs::with_capacity(args.len());
    for (key, value) in args {
        let fv = match value {
            Arg::Str(s) => FluentValue::from(*s),
            Arg::Owned(s) => FluentValue::from(s.as_str()),
            Arg::Num(n) => FluentValue::from(*n),
        };
        fluent_args.set(*key, fv);
    }
    plain(loader.get_args_fluent(id, Some(&fluent_args)))
}

/// Translate a message by semantic Fluent ID.
pub fn tr(id: &str) -> String {
    plain(loader().get(id))
}

/// Translate a parameterized Fluent message by semantic ID.
pub fn tr_args(id: &str, args: &[(&str, Arg<'_>)]) -> String {
    format_args_with(loader(), id, args)
}

/// Translate a message once and leak it as `&'static str` for static ribbon definitions.
pub fn tr_static(id: &str) -> &'static str {
    Box::leak(tr(id).into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};
    use std::collections::{BTreeMap, BTreeSet};

    fn collect_placeholders_from_pattern(pattern: &Pattern<&str>, out: &mut BTreeSet<String>) {
        for elem in &pattern.elements {
            if let PatternElement::Placeable { expression } = elem {
                collect_placeholders_from_expr(expression, out);
            }
        }
    }

    fn collect_placeholders_from_expr(expr: &Expression<&str>, out: &mut BTreeSet<String>) {
        match expr {
            Expression::Select { selector, variants } => {
                collect_placeholders_from_inline(selector, out);
                for variant in variants {
                    collect_placeholders_from_pattern(&variant.value, out);
                }
            }
            Expression::Inline(inline) => collect_placeholders_from_inline(inline, out),
        }
    }

    fn collect_placeholders_from_inline(inline: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
        match inline {
            InlineExpression::VariableReference { id } => {
                out.insert(id.name.to_string());
            }
            InlineExpression::Placeable { expression } => {
                collect_placeholders_from_expr(expression, out);
            }
            InlineExpression::FunctionReference { arguments, .. } => {
                for arg in &arguments.positional {
                    collect_placeholders_from_inline(arg, out);
                }
                for named in &arguments.named {
                    collect_placeholders_from_inline(&named.value, out);
                }
            }
            _ => {}
        }
    }

    fn parse_catalog_messages(tag: &str, source: &str) -> BTreeMap<String, BTreeSet<String>> {
        let resource = fluent_syntax::parser::parse(source).unwrap_or_else(|(_, errs)| {
            panic!("Fluent syntax errors in {tag}/{CATALOG_FILE}: {errs:?}");
        });
        let mut messages = BTreeMap::new();
        for entry in resource.body {
            if let Entry::Message(msg) = entry {
                let id = msg.id.name.to_string();
                let mut vars = BTreeSet::new();
                if let Some(ref pattern) = msg.value {
                    collect_placeholders_from_pattern(pattern, &mut vars);
                }
                for attr in &msg.attributes {
                    collect_placeholders_from_pattern(&attr.value, &mut vars);
                }
                assert!(
                    messages.insert(id.clone(), vars).is_none(),
                    "Duplicate message id `{id}` in {tag}/{CATALOG_FILE}"
                );
            }
        }
        messages
    }

    #[test]
    fn every_catalog_covers_and_formats_the_source_catalog() {
        let en_file = EmbeddedLocalizations::get(&format!("en-US/{CATALOG_FILE}"))
            .expect("en-US source catalog must be embedded");
        let en_text = std::str::from_utf8(&en_file.data).expect("en-US catalog must be UTF-8");
        let en_messages = parse_catalog_messages("en-US", en_text);
        assert!(
            en_messages.len() >= 150,
            "Expected full source catalog, found only {} keys",
            en_messages.len()
        );

        let embedded_langs = available_languages_in(&DiskOverlayAssets::with_dirs(Vec::new()));
        assert!(embedded_langs.contains(&"en-US".to_string()));
        assert!(embedded_langs.contains(&"es-ES".to_string()));

        for tag in embedded_langs {
            let file = EmbeddedLocalizations::get(&format!("{tag}/{CATALOG_FILE}"))
                .unwrap_or_else(|| panic!("Missing embedded catalog for {tag}"));
            let text = std::str::from_utf8(&file.data)
                .unwrap_or_else(|e| panic!("Invalid UTF-8 in {tag}/{CATALOG_FILE}: {e}"));
            let msgs = parse_catalog_messages(&tag, text);

            let missing: Vec<&String> = en_messages
                .keys()
                .filter(|k| !msgs.contains_key(*k))
                .collect();
            let extra: Vec<&String> = msgs
                .keys()
                .filter(|k| !en_messages.contains_key(*k))
                .collect();
            assert!(
                missing.is_empty(),
                "{tag}/{CATALOG_FILE} is missing keys: {missing:?}"
            );
            assert!(
                extra.is_empty(),
                "{tag}/{CATALOG_FILE} has unknown extra keys: {extra:?}"
            );

            for (key, expected_vars) in &en_messages {
                let actual_vars = &msgs[key];
                assert_eq!(
                    actual_vars, expected_vars,
                    "Placeholder mismatch for `{key}` in {tag}/{CATALOG_FILE}"
                );
            }
        }
    }

    #[test]
    fn spanish_catalog_formats_plurals_and_placeholders() {
        let assets = DiskOverlayAssets::with_dirs(Vec::new());
        let es_loader = FluentLanguageLoader::new(DOMAIN, "en-US".parse().unwrap());
        load_into(&es_loader, &assets, Some("es-ES")).expect("load es-ES");

        assert_eq!(plain(es_loader.get("ribbon-tool-reload")), "Recargar");
        assert_eq!(
            plain(es_loader.get("appload-startup-title")),
            "Scripts de inicio"
        );
        assert_eq!(
            format_args_with(&es_loader, "appload-info-updated", &[("count", 1usize.into())]),
            "APPLOAD: Scripts de inicio actualizados (1 archivo guardado)."
        );
        assert_eq!(
            format_args_with(&es_loader, "appload-info-updated", &[("count", 3usize.into())]),
            "APPLOAD: Scripts de inicio actualizados (3 archivos guardados)."
        );

        // Unknown locale cleanly falls back to en-US
        let fallback_loader = FluentLanguageLoader::new(DOMAIN, "en-US".parse().unwrap());
        load_into(&fallback_loader, &assets, Some("zz-ZZ")).expect("fallback to en-US");
        assert_eq!(plain(fallback_loader.get("ribbon-tool-reload")), "Reload");
    }

    #[test]
    fn disk_overlay_overrides_embedded_and_discovers_new_locale() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ocs_lisp_i18n_overlay_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        // 1. Override an embedded locale (es-ES) on disk
        let es_dir = temp_dir.join("es-ES");
        std::fs::create_dir_all(&es_dir).unwrap();
        std::fs::write(
            es_dir.join(CATALOG_FILE),
            "ribbon-tool-reload = Recargar (Disco)\n",
        )
        .unwrap();

        // 2. Add a brand-new locale (fr-FR) solely on disk without compiling
        let fr_dir = temp_dir.join("fr-FR");
        std::fs::create_dir_all(&fr_dir).unwrap();
        std::fs::write(
            fr_dir.join(CATALOG_FILE),
            "ribbon-tool-reload = Recharger\nappload-btn-cancel = Annuler\n",
        )
        .unwrap();

        let assets = DiskOverlayAssets::with_dirs(vec![temp_dir.clone()]);
        let langs = available_languages_in(&assets);
        assert!(langs.contains(&"en-US".to_string()));
        assert!(langs.contains(&"es-ES".to_string()));
        assert!(langs.contains(&"fr-FR".to_string()));

        // Verify partial disk override beats embedded es-ES for overridden keys,
        // while untouched keys still resolve from the embedded es-ES bundle (not en-US).
        let es_loader = FluentLanguageLoader::new(DOMAIN, "en-US".parse().unwrap());
        load_into(&es_loader, &assets, Some("es-ES")).expect("load overridden es-ES");
        assert_eq!(
            plain(es_loader.get("ribbon-tool-reload")),
            "Recargar (Disco)"
        );
        assert_eq!(
            plain(es_loader.get("appload-startup-title")),
            "Scripts de inicio"
        );

        // Verify disk-only fr-FR loads and falls back to en-US for omitted keys
        let fr_loader = FluentLanguageLoader::new(DOMAIN, "en-US".parse().unwrap());
        load_into(&fr_loader, &assets, Some("fr-FR")).expect("load disk fr-FR");
        assert_eq!(plain(fr_loader.get("ribbon-tool-reload")), "Recharger");
        assert_eq!(plain(fr_loader.get("appload-btn-cancel")), "Annuler");
        assert_eq!(plain(fr_loader.get("appload-btn-ok")), "OK");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
