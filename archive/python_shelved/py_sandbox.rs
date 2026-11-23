// ============================================================================
// DO NOT EDIT — PYTHON SUPPORT IS SHELVED / FROZEN.
// Do NOT modify this file or add features to Python to mirror OpenCAD LISP.
// All active OpenCAD LISP development belongs strictly in:
//   - ../opencad-lisp-engine (crates/lisp-core, crates/lisp-cad)
//   - src/rust_lisp.rs
// ============================================================================

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SecurityConfig {
    pub sandbox_enabled: bool,
    pub allow_network: bool,
    pub allow_subprocesses: bool,
    pub allow_native_binaries: bool,
    pub max_entities_per_run: usize,
    pub trusted_directories: Vec<PathBuf>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            sandbox_enabled: true,
            allow_network: false,
            allow_subprocesses: false,
            allow_native_binaries: false,
            max_entities_per_run: 100_000,
            trusted_directories: Vec::new(),
        }
    }
}

static CONFIG_CACHE: LazyLock<RwLock<Option<SecurityConfig>>> =
    LazyLock::new(|| RwLock::new(None));

static ENTITY_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Return the canonicalized config directory for OpenCADStudio across OSes.
pub fn get_config_dir() -> PathBuf {
    let base = if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata)
    } else if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(d)
    } else if let Some(home) = std::env::var_os("HOME") {
        #[cfg(target_os = "macos")]
        {
            let mac_dir = PathBuf::from(&home).join("Library").join("Application Support");
            if mac_dir.exists() {
                mac_dir
            } else {
                PathBuf::from(&home).join(".config")
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            PathBuf::from(home).join(".config")
        }
    } else {
        PathBuf::from(".")
    };
    base.join("OpenCADStudio")
}

/// Path to pyrunner_security.json.
pub fn get_security_config_path() -> PathBuf {
    get_config_dir().join("pyrunner_security.json")
}

/// Helper to normalize Windows verbatim paths (`\\?\C:\...` -> `C:\...`).
pub fn normalize_path(path: &Path) -> PathBuf {
    let p_str = path.to_string_lossy();
    if let Some(stripped) = p_str.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// Load configuration from disk, falling back to defaults if missing or corrupted.
pub fn load_config() -> SecurityConfig {
    if let Ok(guard) = CONFIG_CACHE.read() {
        if let Some(cfg) = &*guard {
            return cfg.clone();
        }
    }

    let path = get_security_config_path();
    let config = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str::<SecurityConfig>(&content).unwrap_or_default()
    } else {
        SecurityConfig::default()
    };

    if let Ok(mut guard) = CONFIG_CACHE.write() {
        *guard = Some(config.clone());
    }
    config
}

/// Save configuration to disk and update cache.
pub fn save_config(config: &SecurityConfig) -> Result<(), String> {
    let dir = get_config_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let path = get_security_config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Serialization error: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    if let Ok(mut guard) = CONFIG_CACHE.write() {
        *guard = Some(config.clone());
    }
    Ok(())
}

/// Reset in-memory cache so next read reloads from disk.
pub fn reload_config() -> SecurityConfig {
    if let Ok(mut guard) = CONFIG_CACHE.write() {
        *guard = None;
    }
    load_config()
}

/// Check if a directory or script path is trusted under current security policy.
pub fn is_path_trusted(path: &Path) -> bool {
    let config = load_config();
    if !config.sandbox_enabled {
        return true;
    }

    // Resolve canonical path
    let canonical = match path.canonicalize() {
        Ok(p) => normalize_path(&p),
        Err(_) => {
            // If file doesn't exist yet, try canonicalizing parent
            if let Some(parent) = path.parent() {
                if let Ok(can_parent) = parent.canonicalize() {
                    let norm_parent = normalize_path(&can_parent);
                    if let Some(file_name) = path.file_name() {
                        norm_parent.join(file_name)
                    } else {
                        norm_parent
                    }
                } else {
                    normalize_path(path)
                }
            } else {
                normalize_path(path)
            }
        }
    };

    // 1. Check against OpenCADStudio default scripts directory
    let default_scripts_dir = crate::py_runtime::scripts_dir();
    let norm_default = match default_scripts_dir.canonicalize() {
        Ok(p) => normalize_path(&p),
        Err(_) => normalize_path(&default_scripts_dir),
    };

    if path_is_or_inside(&canonical, &norm_default) {
        return true;
    }

    // 2. Check against explicit trusted_directories
    for trusted in &config.trusted_directories {
        let norm_trusted = match trusted.canonicalize() {
            Ok(p) => normalize_path(&p),
            Err(_) => normalize_path(trusted),
        };
        if path_is_or_inside(&canonical, &norm_trusted) {
            return true;
        }
    }

    false
}

fn path_is_or_inside(target: &Path, base: &Path) -> bool {
    #[cfg(windows)]
    {
        let t = target.to_string_lossy().to_lowercase();
        let b = base.to_string_lossy().to_lowercase();
        if t == b {
            return true;
        }
        let b_slash = if b.ends_with('\\') || b.ends_with('/') {
            b
        } else {
            format!("{}\\", b)
        };
        t.starts_with(&b_slash)
    }

    #[cfg(not(windows))]
    {
        target == base || target.starts_with(base)
    }
}

/// Add a directory to the trusted paths whitelist.
pub fn add_trusted_directory(dir: &Path) -> Result<PathBuf, String> {
    let canonical = match dir.canonicalize() {
        Ok(p) => normalize_path(&p),
        Err(_) => {
            if dir.is_dir() {
                normalize_path(dir)
            } else {
                return Err(format!("Directory '{}' does not exist or cannot be accessed", dir.display()));
            }
        }
    };

    let mut config = load_config();
    for existing in &config.trusted_directories {
        let norm = match existing.canonicalize() {
            Ok(p) => normalize_path(&p),
            Err(_) => normalize_path(existing),
        };
        if norm == canonical {
            return Ok(canonical); // Already trusted
        }
    }

    config.trusted_directories.push(canonical.clone());
    save_config(&config)?;
    Ok(canonical)
}

/// Remove a directory from the trusted paths whitelist.
pub fn remove_trusted_directory(dir: &Path) -> Result<bool, String> {
    let norm_target = normalize_path(dir);
    let mut config = load_config();
    let initial_len = config.trusted_directories.len();
    config.trusted_directories.retain(|p| {
        let norm = normalize_path(p);
        norm != norm_target
    });
    if config.trusted_directories.len() != initial_len {
        save_config(&config)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Reset entity counter before a new script run.
pub fn reset_entity_counter() {
    ENTITY_COUNTER.store(0, Ordering::SeqCst);
}

/// Check and increment the entity counter. Returns Err if limit exceeded.
pub fn check_and_increment_entity(count: usize) -> Result<(), String> {
    let config = load_config();
    if !config.sandbox_enabled || config.max_entities_per_run == 0 {
        return Ok(());
    }

    let prev = ENTITY_COUNTER.fetch_add(count, Ordering::SeqCst);
    let total = prev + count;
    if total > config.max_entities_per_run {
        return Err(format!(
            "Security limit exceeded: Script created {} entities, exceeding the ceiling of {}. \
             Execution aborted to prevent CAD drawing freezing.",
            total, config.max_entities_per_run
        ));
    }
    Ok(())
}
