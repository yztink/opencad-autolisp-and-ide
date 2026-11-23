// ============================================================================
// DO NOT EDIT — PYTHON SUPPORT IS SHELVED / FROZEN.
// Do NOT modify this file or add features to Python to mirror OpenCAD LISP.
// All active OpenCAD LISP development belongs strictly in:
//   - ../opencad-lisp-engine (crates/lisp-core, crates/lisp-cad)
//   - src/rust_lisp.rs
// ============================================================================

use std::sync::Mutex;
use std::collections::BTreeMap;
use pyo3::prelude::*;
use pyo3::exceptions::PyRuntimeError;
use ocs_plugin_api::host::acadrust::entities::{
    Arc as CadArc, Circle, Ellipse, Insert, Line, LwPolyline, LwVertex, MText, Point, Ray, Solid, Text, XLine,
};
use ocs_plugin_api::host::acadrust::types::{Color, Vector2, Vector3};
use ocs_plugin_api::host::{
    EntityType, Handle, HostApi, HostSettingValue, SolidBoolean, SolidOperation, SolidPrimitive,
    TableOperation, TextStyleConfig,
};
use crate::context::with_host;
use crate::interactive::RubberBandSpec;

static REGISTERED_COMMANDS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

pub fn get_registered_commands() -> BTreeMap<String, String> {
    REGISTERED_COMMANDS.lock().unwrap().clone()
}

pub fn sync_registered_aliases(_host: &mut dyn HostApi) {
    // Commands registered via @oc.command are dispatched in-memory through
    // REGISTERED_COMMANDS (see dispatcher.rs dispatch_command `_ =>` arm).
    // The host's `register_alias` API is not available in the vendored
    // ocs_plugin_api crate, so autocomplete is provided via `command_prefixes`
    // in plugin.toml and the `PYLIST` command instead.
}

pub(crate) fn ensure_layer_exists(h: &mut dyn HostApi, layer_name: &str) {
    let name = layer_name.trim();
    if name.is_empty() || name == "0" {
        return;
    }
    if !h.document().layers.contains(name) {
        let config = ocs_plugin_api::host::LayerConfig {
            name: name.to_string(),
            ..Default::default()
        };
        h.add_layer(config);
    }
}

#[pyfunction]
fn info(msg: &str) -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        println!("[INFO] {}", msg);
        return Ok(());
    }
    if with_host(|h| h.push_info(msg)).is_err() {
        println!("[INFO] {}", msg);
    }
    Ok(())
}

#[pyfunction]
fn output(msg: &str) -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        println!("[OUTPUT] {}", msg);
        return Ok(());
    }
    if with_host(|h| h.push_output(msg)).is_err() {
        println!("[OUTPUT] {}", msg);
    }
    Ok(())
}

#[pyfunction]
fn error(msg: &str) -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        eprintln!("[ERROR] {}", msg);
        return Ok(());
    }
    if with_host(|h| h.push_error(msg)).is_err() {
        eprintln!("[ERROR] {}", msg);
    }
    Ok(())
}

#[pyfunction]
fn undo(label: &str) -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        return Ok(());
    }
    let _ = with_host(|h| h.push_undo(label));
    Ok(())
}

#[pyfunction]
fn set_dirty() -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        return Ok(());
    }
    let _ = with_host(|h| h.set_dirty());
    Ok(())
}

#[pyfunction]
fn bump_geometry() -> PyResult<()> {
    if crate::context::active_interactive().is_some() {
        return Ok(());
    }
    let _ = with_host(|h| h.bump_geometry());
    Ok(())
}


fn parse_color(c: &Bound<'_, PyAny>) -> Result<Color, String> {
    if let Ok(idx) = c.extract::<i16>() {
        Ok(Color::from_index(idx))
    } else if let Ok((r, g, b)) = c.extract::<(u8, u8, u8)>() {
        Ok(Color::from_rgb(r, g, b))
    } else if let Ok(seq) = c.extract::<Vec<u8>>() {
        if seq.len() == 3 {
            Ok(Color::from_rgb(seq[0], seq[1], seq[2]))
        } else {
            Err(format!("Color list must have 3 RGB components, got {}", seq.len()))
        }
    } else if let Ok(seq) = c.extract::<Vec<i64>>() {
        if seq.len() == 3 {
            let r = seq[0].clamp(0, 255) as u8;
            let g = seq[1].clamp(0, 255) as u8;
            let b = seq[2].clamp(0, 255) as u8;
            Ok(Color::from_rgb(r, g, b))
        } else {
            Err(format!("Color list must have 3 RGB components, got {}", seq.len()))
        }
    } else if let Ok(s) = c.extract::<String>() {
        match s.trim().to_ascii_lowercase().as_str() {
            "red" => Ok(Color::from_index(1)),
            "yellow" => Ok(Color::from_index(2)),
            "green" => Ok(Color::from_index(3)),
            "cyan" => Ok(Color::from_index(4)),
            "blue" => Ok(Color::from_index(5)),
            "magenta" => Ok(Color::from_index(6)),
            "white" => Ok(Color::from_index(7)),
            "orange" => Ok(Color::from_rgb(255, 165, 0)),
            "gray" | "grey" => Ok(Color::from_index(8)),
            "bylayer" => Ok(Color::ByLayer),
            "byblock" => Ok(Color::ByBlock),
            other => {
                if other.starts_with('#') && other.len() == 7 {
                    let r = u8::from_str_radix(&other[1..3], 16).map_err(|e| e.to_string())?;
                    let g = u8::from_str_radix(&other[3..5], 16).map_err(|e| e.to_string())?;
                    let b = u8::from_str_radix(&other[5..7], 16).map_err(|e| e.to_string())?;
                    Ok(Color::from_rgb(r, g, b))
                } else if let Ok(idx) = other.parse::<i16>() {
                    Ok(Color::from_index(idx))
                } else {
                    Err(format!("Unknown color string '{}'", other))
                }
            }
        }
    } else {
        Err("color must be an integer ACI (1-255), color name ('red', 'green', ...), or (r, g, b) tuple".to_string())
    }
}

fn parse_point(obj: &Bound<'_, PyAny>) -> Result<[f64; 3], String> {
    if let Ok((x, y, z)) = obj.extract::<(f64, f64, f64)>() {
        Ok([x, y, z])
    } else if let Ok((x, y)) = obj.extract::<(f64, f64)>() {
        Ok([x, y, 0.0])
    } else if let Ok(seq) = obj.extract::<Vec<f64>>() {
        match seq.len() {
            2 => Ok([seq[0], seq[1], 0.0]),
            3 => Ok([seq[0], seq[1], seq[2]]),
            n => Err(format!("Point sequence must have 2 or 3 coordinates, got {}", n)),
        }
    } else {
        Err(format!(
            "Point must be a 2D (x, y) or 3D (x, y, z) tuple or list, got '{}'",
            obj
        ))
    }
}

fn parse_preview_color(c: &Bound<'_, PyAny>) -> Result<[f32; 4], String> {
    if let Ok((r, g, b, a)) = c.extract::<(f32, f32, f32, f32)>() {
        let (r, g, b, a) = if r > 1.0 || g > 1.0 || b > 1.0 || a > 1.0 {
            (r / 255.0, g / 255.0, b / 255.0, a / 255.0)
        } else {
            (r, g, b, a)
        };
        Ok([r, g, b, a])
    } else if let Ok((r, g, b)) = c.extract::<(f32, f32, f32)>() {
        let (r, g, b) = if r > 1.0 || g > 1.0 || b > 1.0 {
            (r / 255.0, g / 255.0, b / 255.0)
        } else {
            (r, g, b)
        };
        Ok([r, g, b, 1.0])
    } else if let Ok(seq) = c.extract::<Vec<f32>>() {
        match seq.len() {
            3 => {
                let (r, g, b) = if seq[0] > 1.0 || seq[1] > 1.0 || seq[2] > 1.0 {
                    (seq[0] / 255.0, seq[1] / 255.0, seq[2] / 255.0)
                } else {
                    (seq[0], seq[1], seq[2])
                };
                Ok([r, g, b, 1.0])
            }
            4 => {
                let (r, g, b, a) = if seq[0] > 1.0 || seq[1] > 1.0 || seq[2] > 1.0 || seq[3] > 1.0 {
                    (seq[0] / 255.0, seq[1] / 255.0, seq[2] / 255.0, seq[3] / 255.0)
                } else {
                    (seq[0], seq[1], seq[2], seq[3])
                };
                Ok([r, g, b, a])
            }
            n => Err(format!("Color sequence must have 3 or 4 values, got {}", n)),
        }
    } else if let Ok(s) = c.extract::<String>() {
        let trimmed = s.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "red" => Ok([1.0, 0.0, 0.0, 1.0]),
            "yellow" => Ok([1.0, 1.0, 0.0, 1.0]),
            "green" => Ok([0.0, 1.0, 0.0, 1.0]),
            "cyan" => Ok([0.0, 1.0, 1.0, 1.0]),
            "blue" => Ok([0.0, 0.0, 1.0, 1.0]),
            "magenta" => Ok([1.0, 0.0, 1.0, 1.0]),
            "white" => Ok([1.0, 1.0, 1.0, 1.0]),
            "orange" => Ok([1.0, 0.65, 0.0, 1.0]),
            other => {
                if other.starts_with('#') && (other.len() == 7 || other.len() == 9) {
                    let r = u8::from_str_radix(&other[1..3], 16).map_err(|e| e.to_string())? as f32 / 255.0;
                    let g = u8::from_str_radix(&other[3..5], 16).map_err(|e| e.to_string())? as f32 / 255.0;
                    let b = u8::from_str_radix(&other[5..7], 16).map_err(|e| e.to_string())? as f32 / 255.0;
                    let a = if other.len() == 9 {
                        u8::from_str_radix(&other[7..9], 16).map_err(|e| e.to_string())? as f32 / 255.0
                    } else {
                        1.0
                    };
                    Ok([r, g, b, a])
                } else if let Ok(idx) = other.parse::<i16>() {
                    match idx {
                        1 => Ok([1.0, 0.0, 0.0, 1.0]),
                        2 => Ok([1.0, 1.0, 0.0, 1.0]),
                        3 => Ok([0.0, 1.0, 0.0, 1.0]),
                        4 => Ok([0.0, 1.0, 1.0, 1.0]),
                        5 => Ok([0.0, 0.0, 1.0, 1.0]),
                        6 => Ok([1.0, 0.0, 1.0, 1.0]),
                        7 => Ok([1.0, 1.0, 1.0, 1.0]),
                        _ => Err(format!("Unsupported color index: {}", idx)),
                    }
                } else {
                    Err(format!("Unknown color string '{}'", other))
                }
            }
        }
    } else if let Ok(idx) = c.extract::<i16>() {
        match idx {
            1 => Ok([1.0, 0.0, 0.0, 1.0]),
            2 => Ok([1.0, 1.0, 0.0, 1.0]),
            3 => Ok([0.0, 1.0, 0.0, 1.0]),
            4 => Ok([0.0, 1.0, 1.0, 1.0]),
            5 => Ok([0.0, 0.0, 1.0, 1.0]),
            6 => Ok([1.0, 0.0, 1.0, 1.0]),
            7 => Ok([1.0, 1.0, 1.0, 1.0]),
            _ => Err(format!("Unsupported color index: {}", idx)),
        }
    } else {
        Err("color must be a name string ('red', 'cyan'), hex ('#ff0000'), ACI index (1-7), or (r, g, b) / (r, g, b, a) tuple".to_string())
    }
}

fn parse_rubber_band(
    spec_obj: Option<&Bound<'_, PyAny>>,
    base_point_obj: Option<&Bound<'_, PyAny>>,
    color_obj: Option<&Bound<'_, PyAny>>,
) -> Result<RubberBandSpec, String> {
    let default_color = match color_obj {
        Some(c) => {
            if c.is_none() {
                None
            } else {
                Some(parse_preview_color(c)?)
            }
        }
        None => None,
    };

    if let Some(spec) = spec_obj {
        if spec.is_none() {
            if let Some(bp) = base_point_obj {
                if !bp.is_none() {
                    let from = parse_point(bp)?;
                    return Ok(RubberBandSpec::Line { from, color: default_color });
                }
            }
            return Ok(RubberBandSpec::None);
        }

        if let Ok(pt) = parse_point(spec) {
            return Ok(RubberBandSpec::Line { from: pt, color: default_color });
        }

        if let Ok(s) = spec.extract::<String>() {
            if s.trim().eq_ignore_ascii_case("none") {
                return Ok(RubberBandSpec::None);
            }
        }

        if let Ok(seq) = spec.extract::<Vec<Bound<'_, PyAny>>>() {
            if seq.is_empty() {
                return Ok(RubberBandSpec::None);
            }
            let kind = seq[0].extract::<String>().map_err(|_| {
                "First element of rubber_band tuple must be a mode string, e.g. 'line', 'circle', 'arc_3p', 'arc_center_start', 'tangent_arc'".to_string()
            })?.to_ascii_lowercase();

            match kind.as_str() {
                "none" => Ok(RubberBandSpec::None),
                "line" => {
                    if seq.len() < 2 {
                        return Err("rubber_band ('line', from_point) requires from_point".to_string());
                    }
                    let from = parse_point(&seq[1])?;
                    let color = if seq.len() >= 3 && !seq[2].is_none() {
                        Some(parse_preview_color(&seq[2])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::Line { from, color })
                }
                "rect" | "rectangle" | "box" => {
                    if seq.len() < 2 {
                        return Err("rubber_band ('rectangle', corner) requires corner point".to_string());
                    }
                    let corner = parse_point(&seq[1])?;
                    let color = if seq.len() >= 3 && !seq[2].is_none() {
                        Some(parse_preview_color(&seq[2])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::Rectangle { corner, color })
                }
                "circle" => {
                    if seq.len() < 2 {
                        return Err("rubber_band ('circle', center) requires center point".to_string());
                    }
                    let center = parse_point(&seq[1])?;
                    let color = if seq.len() >= 3 && !seq[2].is_none() {
                        Some(parse_preview_color(&seq[2])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::Circle { center, color })
                }
                "arc_3p" | "arc_3point" => {
                    if seq.len() < 3 {
                        return Err("rubber_band ('arc_3p', p1, p2) requires p1 and p2".to_string());
                    }
                    let p1 = parse_point(&seq[1])?;
                    let p2 = parse_point(&seq[2])?;
                    let color = if seq.len() >= 4 && !seq[3].is_none() {
                        Some(parse_preview_color(&seq[3])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::Arc3Point { p1, p2, color })
                }
                "arc_center_start" | "arc_cs" => {
                    if seq.len() < 3 {
                        return Err("rubber_band ('arc_center_start', center, start) requires center and start".to_string());
                    }
                    let center = parse_point(&seq[1])?;
                    let start = parse_point(&seq[2])?;
                    let color = if seq.len() >= 4 && !seq[3].is_none() {
                        Some(parse_preview_color(&seq[3])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::ArcCenterStart { center, start, color })
                }
                "tangent_arc" | "tarc" => {
                    if seq.len() < 3 {
                        return Err("rubber_band ('tangent_arc', start, tangent) requires start and tangent vector".to_string());
                    }
                    let start = parse_point(&seq[1])?;
                    let tangent = parse_point(&seq[2])?;
                    let color = if seq.len() >= 4 && !seq[3].is_none() {
                        Some(parse_preview_color(&seq[3])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::TangentArc { start, tangent, color })
                }
                "custom" => {
                    if seq.len() < 2 {
                        return Err("rubber_band ('custom', wires) requires list of wires (points)".to_string());
                    }
                    let wire_list = seq[1].extract::<Vec<Vec<Bound<'_, PyAny>>>>().map_err(|e| e.to_string())?;
                    let mut wires = Vec::new();
                    for w in wire_list {
                        let mut pts = Vec::new();
                        for pt in w {
                            pts.push(parse_point(&pt)?);
                        }
                        wires.push(pts);
                    }
                    let color = if seq.len() >= 3 && !seq[2].is_none() {
                        Some(parse_preview_color(&seq[2])?)
                    } else {
                        default_color
                    };
                    Ok(RubberBandSpec::Custom { wires, color })
                }
                other => Err(format!("Unknown rubber_band mode '{}'", other)),
            }
        } else {
            Err("rubber_band must be a tuple/list: e.g. ('line', pt), ('circle', center), ('tangent_arc', start, tangent)".to_string())
        }
    } else if let Some(bp) = base_point_obj {
        if bp.is_none() {
            Ok(RubberBandSpec::None)
        } else {
            let from = parse_point(bp)?;
            Ok(RubberBandSpec::Line { from, color: default_color })
        }
    } else {
        Ok(RubberBandSpec::None)
    }
}

fn build_axis_transform(center: [f64; 3], axis_endpoint: [f64; 3]) -> Result<([f64; 16], f64), String> {
    let vx = axis_endpoint[0] - center[0];
    let vy = axis_endpoint[1] - center[1];
    let vz = axis_endpoint[2] - center[2];
    let len = (vx * vx + vy * vy + vz * vz).sqrt();
    if len < 1e-9 {
        return Err("axis_endpoint cannot coincide with center".to_string());
    }
    let ux = vx / len;
    let uy = vy / len;
    let uz = vz / len;

    let c = uz;
    let (r00, r01, r02, r10, r11, r12, r20, r21, r22) = if (c - 1.0).abs() < 1e-9 {
        (1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
    } else if (c + 1.0).abs() < 1e-9 {
        (1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0)
    } else {
        let factor = 1.0 / (1.0 + c);
        (
            1.0 - ux * ux * factor,
            -ux * uy * factor,
            ux,
            -ux * uy * factor,
            1.0 - uy * uy * factor,
            uy,
            -ux,
            -uy,
            c,
        )
    };

    let matrix = [
        r00, r10, r20, 0.0,
        r01, r11, r21, 0.0,
        r02, r12, r22, 0.0,
        center[0], center[1], center[2], 1.0,
    ];
    Ok((matrix, len))
}

fn build_rotation_z_transform(corner: [f64; 3], local_offset: [f64; 3], angle_rad: f64) -> [f64; 16] {
    let cos = angle_rad.cos();
    let sin = angle_rad.sin();
    let rx = cos * local_offset[0] - sin * local_offset[1];
    let ry = sin * local_offset[0] + cos * local_offset[1];
    let rz = local_offset[2];
    let tx = corner[0] - rx;
    let ty = corner[1] - ry;
    let tz = corner[2] - rz;

    [
        cos, sin, 0.0, 0.0,
        -sin, cos, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        tx, ty, tz, 1.0,
    ]
}

fn record_entity_created() -> PyResult<()> {
    crate::py_sandbox::check_and_increment_entity(1).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn is_sandboxed() -> PyResult<bool> {
    Ok(crate::py_sandbox::load_config().sandbox_enabled)
}

#[pyfunction]
fn get_security_status(py: Python<'_>) -> PyResult<Bound<'_, pyo3::types::PyDict>> {
    let cfg = crate::py_sandbox::load_config();
    let d = pyo3::types::PyDict::new(py);
    d.set_item("sandbox_enabled", cfg.sandbox_enabled)?;
    d.set_item("allow_network", cfg.allow_network)?;
    d.set_item("allow_subprocesses", cfg.allow_subprocesses)?;
    d.set_item("allow_native_binaries", cfg.allow_native_binaries)?;
    d.set_item("max_entities_per_run", cfg.max_entities_per_run)?;
    let trusted = pyo3::types::PyList::empty(py);
    for p in cfg.trusted_directories {
        trusted.append(p.to_string_lossy().to_string())?;
    }
    d.set_item("trusted_directories", trusted)?;
    Ok(d)
}

#[pyfunction]
fn trust_directory(dir: &str) -> PyResult<String> {
    match crate::py_sandbox::add_trusted_directory(std::path::Path::new(dir)) {
        Ok(p) => Ok(p.to_string_lossy().to_string()),
        Err(e) => Err(PyRuntimeError::new_err(e)),
    }
}

#[pyfunction]
#[pyo3(signature = (pt, layer="0", color=None))]
fn add_point(
    pt: &Bound<'_, PyAny>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x, y, z] = parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    if let Some(bridge) = crate::context::active_interactive() {
        let mut pt_ent = Point::from_coords(x, y, z);
        pt_ent.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            pt_ent.common.color = c;
        }
        let handle = bridge.add_pending_entity(EntityType::Point(pt_ent));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let mut pt_ent = Point::from_coords(x, y, z);
        pt_ent.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            pt_ent.common.color = c;
        }
        let handle = h.add_entity(EntityType::Point(pt_ent));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (p1, p2, layer="0", color=None))]
fn add_line(
    p1: &Bound<'_, PyAny>,
    p2: &Bound<'_, PyAny>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x1, y1, z1] = parse_point(p1).map_err(|e| PyRuntimeError::new_err(e))?;
    let [x2, y2, z2] = parse_point(p2).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    crate::interactive::dbg_trace(&format!("add_line: active_interactive={}", crate::context::active_interactive().is_some()));
    if let Some(bridge) = crate::context::active_interactive() {
        let mut line = Line::from_coords(x1, y1, z1, x2, y2, z2);
        line.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            line.common.color = c;
        }
        let handle = bridge.add_pending_entity(EntityType::Line(line));
        crate::interactive::dbg_trace(&format!("add_line: added pending line handle={}", handle));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let mut line = Line::from_coords(x1, y1, z1, x2, y2, z2);
        line.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            line.common.color = c;
        }
        let handle = h.add_entity(EntityType::Line(line));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, radius, layer="0", color=None))]
fn add_circle(
    center: &Bound<'_, PyAny>,
    radius: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    crate::interactive::dbg_trace(&format!("add_circle: active_interactive={}", crate::context::active_interactive().is_some()));
    if let Some(bridge) = crate::context::active_interactive() {
        let mut circle = Circle::from_coords(cx, cy, cz, radius);
        circle.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            circle.common.color = c;
        }
        let handle = bridge.add_pending_entity(EntityType::Circle(circle));
        crate::interactive::dbg_trace(&format!("add_circle: added pending circle handle={}", handle));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let mut circle = Circle::from_coords(cx, cy, cz, radius);
        circle.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            circle.common.color = c;
        }
        let handle = h.add_entity(EntityType::Circle(circle));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (text, pt, height=2.5, rotation=0.0, style="STANDARD", layer="0", color=None))]
fn add_text(
    text: &str,
    pt: &Bound<'_, PyAny>,
    height: f64,
    rotation: f64,
    style: &str,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x, y, z] = parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    if let Some(bridge) = crate::context::active_interactive() {
        let mut t = Text::new();
        t.value = text.to_string();
        t.insertion_point = Vector3::new(x, y, z);
        t.height = height;
        t.rotation = rotation;
        t.style = style.to_string();
        t.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            t.common.color = c;
        }
        let handle = bridge.add_pending_entity(EntityType::Text(t));
        return Ok(handle);
    }

    with_host(|h| {
        ensure_layer_exists(h, layer);
        let mut t = Text::new();
        t.value = text.to_string();
        t.insertion_point = Vector3::new(x, y, z);
        t.height = height;
        t.rotation = rotation;
        t.style = style.to_string();
        t.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            t.common.color = c;
        }
        let handle = h.add_entity(EntityType::Text(t));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, radius, start_angle, end_angle, layer="0", color=None))]
fn add_arc(
    center: &Bound<'_, PyAny>,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    if let Some(bridge) = crate::context::active_interactive() {
        let mut arc = CadArc::from_center_radius_angles(
            Vector3::new(cx, cy, cz),
            radius,
            start_angle,
            end_angle,
        );
        arc.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            arc.common.color = c;
        }
        let handle = bridge.add_pending_entity(EntityType::Arc(arc));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let mut arc = CadArc::from_center_radius_angles(
            Vector3::new(cx, cy, cz),
            radius,
            start_angle,
            end_angle,
        );
        arc.common.layer = layer.to_string();
        if let Some(c) = parsed_color {
            arc.common.color = c;
        }
        let handle = h.add_entity(EntityType::Arc(arc));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, major_axis, minor_axis=None, ratio=None, start_param=0.0, end_param=std::f64::consts::TAU, layer="0", color=None))]
fn add_ellipse(
    center: &Bound<'_, PyAny>,
    major_axis: &Bound<'_, PyAny>,
    minor_axis: Option<&Bound<'_, PyAny>>,
    ratio: Option<f64>,
    start_param: f64,
    end_param: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    let [mx, my, mz] = parse_point(major_axis).map_err(|e| PyRuntimeError::new_err(e))?;
    let major_len = (mx * mx + my * my + mz * mz).sqrt();
    if major_len < 1e-9 {
        return Err(PyRuntimeError::new_err("major_axis length must be positive"));
    }

    let calculated_ratio = if let Some(r) = ratio {
        r
    } else if let Some(min_obj) = minor_axis {
        if let Ok(f) = min_obj.extract::<f64>() {
            f / major_len
        } else if let Ok(min_pt) = parse_point(min_obj) {
            let min_len = (min_pt[0] * min_pt[0] + min_pt[1] * min_pt[1] + min_pt[2] * min_pt[2]).sqrt();
            min_len / major_len
        } else {
            return Err(PyRuntimeError::new_err("minor_axis must be a float or coordinate tuple"));
        }
    } else {
        0.5
    };

    if calculated_ratio <= 0.0 || calculated_ratio > 1.0 {
        return Err(PyRuntimeError::new_err(format!(
            "minor_axis_ratio must be in range (0, 1], got {}",
            calculated_ratio
        )));
    }

    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut ell = Ellipse::from_center_axes(
        Vector3::new(cx, cy, cz),
        Vector3::new(mx, my, mz),
        calculated_ratio,
    );
    ell.start_parameter = start_param;
    ell.end_parameter = end_param;
    ell.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        ell.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::Ellipse(ell));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::Ellipse(ell));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (points, is_closed=false, layer="0", color=None))]
fn add_polyline(
    points: &Bound<'_, PyAny>,
    is_closed: bool,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let mut vertices = Vec::new();
    let iter = points.try_iter().map_err(|_| {
        PyRuntimeError::new_err("points must be a list of (x, y) or (x, y, bulge) tuples")
    })?;

    for item in iter {
        let pt_any = item.map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        if let Ok((x, y, bulge)) = pt_any.extract::<(f64, f64, f64)>() {
            vertices.push(LwVertex::with_bulge(Vector2::new(x, y), bulge));
        } else if let Ok((x, y)) = pt_any.extract::<(f64, f64)>() {
            vertices.push(LwVertex::new(Vector2::new(x, y)));
        } else if let Ok(seq) = pt_any.extract::<Vec<f64>>() {
            match seq.len() {
                2 => vertices.push(LwVertex::new(Vector2::new(seq[0], seq[1]))),
                3 => vertices.push(LwVertex::with_bulge(Vector2::new(seq[0], seq[1]), seq[2])),
                _ => return Err(PyRuntimeError::new_err("each vertex must have 2 or 3 elements")),
            }
        } else {
            return Err(PyRuntimeError::new_err("vertex must be (x, y) or (x, y, bulge)"));
        }
    }

    if vertices.len() < 2 {
        return Err(PyRuntimeError::new_err("add_polyline requires at least 2 vertices"));
    }

    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut pl = LwPolyline::new();
    pl.vertices = vertices;
    pl.is_closed = is_closed;
    pl.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        pl.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::LwPolyline(pl));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::LwPolyline(pl));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (insertion_point, text, height=2.5, width=0.0, rotation=0.0, style="STANDARD", layer="0", color=None))]
fn add_mtext(
    insertion_point: &Bound<'_, PyAny>,
    text: &str,
    height: f64,
    width: f64,
    rotation: f64,
    style: &str,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x, y, z] = parse_point(insertion_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut m = MText::new();
    m.value = text.to_string();
    m.insertion_point = Vector3::new(x, y, z);
    m.height = height;
    m.rectangle_width = width;
    m.rotation = rotation;
    m.style = style.to_string();
    m.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        m.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::MText(m));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::MText(m));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (base_point, direction_point, layer="0", color=None))]
fn add_ray(
    base_point: &Bound<'_, PyAny>,
    direction_point: &Bound<'_, PyAny>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [bx, by, bz] = parse_point(base_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let [dx, dy, dz] = parse_point(direction_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let vx = dx - bx;
    let vy = dy - by;
    let vz = dz - bz;
    let len = (vx * vx + vy * vy + vz * vz).sqrt();
    if len < 1e-9 {
        return Err(PyRuntimeError::new_err("direction_point cannot coincide with base_point"));
    }
    let dir = Vector3::new(vx / len, vy / len, vz / len);
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut ray = Ray::new(Vector3::new(bx, by, bz), dir);
    ray.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        ray.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::Ray(ray));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::Ray(ray));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (base_point, direction_point, layer="0", color=None))]
fn add_xline(
    base_point: &Bound<'_, PyAny>,
    direction_point: &Bound<'_, PyAny>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [bx, by, bz] = parse_point(base_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let [dx, dy, dz] = parse_point(direction_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let vx = dx - bx;
    let vy = dy - by;
    let vz = dz - bz;
    let len = (vx * vx + vy * vy + vz * vz).sqrt();
    if len < 1e-9 {
        return Err(PyRuntimeError::new_err("direction_point cannot coincide with base_point"));
    }
    let dir = Vector3::new(vx / len, vy / len, vz / len);
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut xline = XLine::new(Vector3::new(bx, by, bz), dir);
    xline.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        xline.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::XLine(xline));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::XLine(xline));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (p1, p2, p3, p4=None, layer="0", color=None))]
fn add_solid(
    p1: &Bound<'_, PyAny>,
    p2: &Bound<'_, PyAny>,
    p3: &Bound<'_, PyAny>,
    p4: Option<&Bound<'_, PyAny>>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x1, y1, z1] = parse_point(p1).map_err(|e| PyRuntimeError::new_err(e))?;
    let [x2, y2, z2] = parse_point(p2).map_err(|e| PyRuntimeError::new_err(e))?;
    let [x3, y3, z3] = parse_point(p3).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut sol = if let Some(p4_obj) = p4 {
        let [x4, y4, z4] = parse_point(p4_obj).map_err(|e| PyRuntimeError::new_err(e))?;
        Solid::new(
            Vector3::new(x1, y1, z1),
            Vector3::new(x2, y2, z2),
            Vector3::new(x3, y3, z3),
            Vector3::new(x4, y4, z4),
        )
    } else {
        Solid::triangle(
            Vector3::new(x1, y1, z1),
            Vector3::new(x2, y2, z2),
            Vector3::new(x3, y3, z3),
        )
    };
    sol.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        sol.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::Solid(sol));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::Solid(sol));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (corner, length, width, height, rotation=0.0, layer="0", color=None))]
fn add_box(
    corner: &Bound<'_, PyAny>,
    length: f64,
    width: f64,
    height: f64,
    rotation: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = color;
    let [cx, cy, cz] = parse_point(corner).map_err(|e| PyRuntimeError::new_err(e))?;
    if length <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Err(PyRuntimeError::new_err("Box dimensions must be positive"));
    }

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        if rotation.abs() < 1e-9 {
            let center = [cx + length / 2.0, cy + width / 2.0, cz + height / 2.0];
            let primitive = SolidPrimitive::Box {
                center,
                size: [length, width, height],
            };
            h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })
            .map(|handle| handle.value())
        } else {
            let primitive = SolidPrimitive::Box {
                center: [0.0, 0.0, 0.0],
                size: [length, width, height],
            };
            let handle = h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })?;
            let matrix = build_rotation_z_transform(
                [cx, cy, cz],
                [-length / 2.0, -width / 2.0, -height / 2.0],
                rotation,
            );
            h.solid_operation(SolidOperation::Transform { handle, matrix })
                .map(|h| h.value())
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (corner, length, width, height, rotation=0.0, layer="0", color=None))]
fn add_wedge(
    corner: &Bound<'_, PyAny>,
    length: f64,
    width: f64,
    height: f64,
    rotation: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = color;
    let [cx, cy, cz] = parse_point(corner).map_err(|e| PyRuntimeError::new_err(e))?;
    if length <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Err(PyRuntimeError::new_err("Wedge dimensions must be positive"));
    }

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        if rotation.abs() < 1e-9 {
            let primitive = SolidPrimitive::Wedge {
                origin: [cx, cy, cz],
                size: [length, width, height],
            };
            h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })
            .map(|handle| handle.value())
        } else {
            let primitive = SolidPrimitive::Wedge {
                origin: [0.0, 0.0, 0.0],
                size: [length, width, height],
            };
            let handle = h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })?;
            let matrix = build_rotation_z_transform(
                [cx, cy, cz],
                [0.0, 0.0, 0.0],
                rotation,
            );
            h.solid_operation(SolidOperation::Transform { handle, matrix })
                .map(|h| h.value())
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, radius, height=None, axis_endpoint=None, layer="0", color=None))]
fn add_cylinder(
    center: &Bound<'_, PyAny>,
    radius: f64,
    height: Option<f64>,
    axis_endpoint: Option<&Bound<'_, PyAny>>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = color;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    if radius <= 0.0 {
        return Err(PyRuntimeError::new_err("radius must be positive"));
    }

    let parsed_axis = match axis_endpoint {
        Some(pt) => Some(parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        if let Some(axis_pt) = parsed_axis {
            let (matrix, len) = build_axis_transform([cx, cy, cz], axis_pt)?;
            let primitive = SolidPrimitive::Cylinder {
                center: [0.0, 0.0, 0.0],
                radius,
                height: len,
            };
            let handle = h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })?;
            h.solid_operation(SolidOperation::Transform { handle, matrix })
                .map(|h| h.value())
        } else if let Some(h_val) = height {
            if h_val <= 0.0 {
                return Err("height must be positive".to_string());
            }
            let primitive = SolidPrimitive::Cylinder {
                center: [cx, cy, cz],
                radius,
                height: h_val,
            };
            h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })
            .map(|handle| handle.value())
        } else {
            Err("Either height or axis_endpoint must be provided".to_string())
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, radius, layer="0", color=None))]
fn add_sphere(
    center: &Bound<'_, PyAny>,
    radius: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = color;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    if radius <= 0.0 {
        return Err(PyRuntimeError::new_err("radius must be positive"));
    }

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        let primitive = SolidPrimitive::Sphere {
            center: [cx, cy, cz],
            radius,
        };
        h.solid_operation(SolidOperation::Create {
            primitive,
            layer: Some(layer.to_string()),
        })
        .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, major_radius, minor_radius, axis_endpoint=None, layer="0", color=None))]
fn add_torus(
    center: &Bound<'_, PyAny>,
    major_radius: f64,
    minor_radius: f64,
    axis_endpoint: Option<&Bound<'_, PyAny>>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = color;
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    if major_radius <= 0.0 || minor_radius <= 0.0 {
        return Err(PyRuntimeError::new_err("Radii must be positive"));
    }

    let parsed_axis = match axis_endpoint {
        Some(pt) => Some(parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        if let Some(axis_pt) = parsed_axis {
            let (matrix, _) = build_axis_transform([cx, cy, cz], axis_pt)?;
            let primitive = SolidPrimitive::Torus {
                center: [0.0, 0.0, 0.0],
                major: major_radius,
                minor: minor_radius,
            };
            let handle = h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })?;
            h.solid_operation(SolidOperation::Transform { handle, matrix })
                .map(|h| h.value())
        } else {
            let primitive = SolidPrimitive::Torus {
                center: [cx, cy, cz],
                major: major_radius,
                minor: minor_radius,
            };
            h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })
            .map(|handle| handle.value())
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (center, base_radius, height=None, sides=4, top_radius=0.0, axis_endpoint=None, layer="0", color=None))]
fn add_pyramid(
    center: &Bound<'_, PyAny>,
    base_radius: f64,
    height: Option<f64>,
    sides: u32,
    top_radius: f64,
    axis_endpoint: Option<&Bound<'_, PyAny>>,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let _ = (top_radius, color);
    let [cx, cy, cz] = parse_point(center).map_err(|e| PyRuntimeError::new_err(e))?;
    if base_radius <= 0.0 {
        return Err(PyRuntimeError::new_err("base_radius must be positive"));
    }
    if sides < 3 {
        return Err(PyRuntimeError::new_err("Pyramid must have at least 3 sides"));
    }

    let parsed_axis = match axis_endpoint {
        Some(pt) => Some(parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let res = with_host(|h| {
        ensure_layer_exists(h, layer);
        if let Some(axis_pt) = parsed_axis {
            let (matrix, len) = build_axis_transform([cx, cy, cz], axis_pt)?;
            let primitive = SolidPrimitive::Pyramid {
                center: [0.0, 0.0, 0.0],
                radius: base_radius,
                height: len,
                sides,
            };
            let handle = h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })?;
            h.solid_operation(SolidOperation::Transform { handle, matrix })
                .map(|h| h.value())
        } else if let Some(h_val) = height {
            if h_val <= 0.0 {
                return Err("height must be positive".to_string());
            }
            let primitive = SolidPrimitive::Pyramid {
                center: [cx, cy, cz],
                radius: base_radius,
                height: h_val,
                sides,
            };
            h.solid_operation(SolidOperation::Create {
                primitive,
                layer: Some(layer.to_string()),
            })
            .map(|handle| handle.value())
        } else {
            Err("Either height or axis_endpoint must be provided".to_string())
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (first, second, keep_operands=false))]
fn solid_union(first: u64, second: u64, keep_operands: bool) -> PyResult<u64> {
    let res = with_host(|h| {
        h.solid_operation(SolidOperation::Boolean {
            first: Handle::new(first),
            second: Handle::new(second),
            operation: SolidBoolean::Union,
            layer: None,
            keep_operands,
        })
        .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (first, second, keep_operands=false))]
fn solid_subtract(first: u64, second: u64, keep_operands: bool) -> PyResult<u64> {
    let res = with_host(|h| {
        h.solid_operation(SolidOperation::Boolean {
            first: Handle::new(first),
            second: Handle::new(second),
            operation: SolidBoolean::Subtract,
            layer: None,
            keep_operands,
        })
        .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (first, second, keep_operands=false))]
fn solid_intersect(first: u64, second: u64, keep_operands: bool) -> PyResult<u64> {
    let res = with_host(|h| {
        h.solid_operation(SolidOperation::Boolean {
            first: Handle::new(first),
            second: Handle::new(second),
            operation: SolidBoolean::Intersect,
            layer: None,
            keep_operands,
        })
        .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn remove_entity(handle: u64) -> PyResult<bool> {
    with_host(|h| {
        let removed = h.remove_entity(Handle::new(handle));
        if removed {
            h.set_dirty();
            h.bump_geometry();
        }
        removed
    }).map_err(|e| PyRuntimeError::new_err(e))
}

fn parse_lineweight(lw: &Bound<'_, PyAny>) -> Result<ocs_plugin_api::host::acadrust::types::LineWeight, String> {
    use ocs_plugin_api::host::acadrust::types::LineWeight;
    if lw.is_instance_of::<pyo3::types::PyInt>() {
        let i = lw.extract::<i64>().map_err(|e| e.to_string())?;
        if i < 0 {
            match i {
                -1 => Ok(LineWeight::ByLayer),
                -2 => Ok(LineWeight::ByBlock),
                -3 => Ok(LineWeight::Default),
                _ => Ok(LineWeight::Default),
            }
        } else if (0..=211).contains(&i) {
            Ok(LineWeight::from_value(i as i16))
        } else {
            Err(format!("lineweight integer must be between 0 and 211 (1/100 mm), got {}", i))
        }
    } else if lw.is_instance_of::<pyo3::types::PyFloat>() {
        let f = lw.extract::<f64>().map_err(|e| e.to_string())?;
        let val = if f <= 2.5 {
            (f * 100.0).round() as i16
        } else {
            f.round() as i16
        };
        if (0..=211).contains(&val) {
            Ok(LineWeight::from_value(val))
        } else {
            Err(format!("lineweight must be between 0 and 211 (1/100 mm), got {}", val))
        }
    } else if let Ok(s) = lw.extract::<String>() {
        match s.trim().to_ascii_lowercase().as_str() {
            "bylayer" => Ok(LineWeight::ByLayer),
            "byblock" => Ok(LineWeight::ByBlock),
            "default" => Ok(LineWeight::Default),
            other => {
                let cleaned = other.trim_end_matches("mm").trim();
                if cleaned.contains('.') {
                    if let Ok(f) = cleaned.parse::<f64>() {
                        let val = if f <= 2.5 { (f * 100.0).round() as i16 } else { f.round() as i16 };
                        if (0..=211).contains(&val) {
                            return Ok(LineWeight::from_value(val));
                        }
                    }
                } else if let Ok(i) = cleaned.parse::<i16>() {
                    if (0..=211).contains(&i) {
                        return Ok(LineWeight::from_value(i));
                    }
                }
                Err(format!("Unknown lineweight '{}'", other))
            }
        }
    } else {
        Err("lineweight must be float (e.g. 0.25 mm), integer (e.g. 25), or string ('default', 'bylayer', 'byblock')".to_string())
    }
}

fn parse_transparency(tr: &Bound<'_, PyAny>) -> Result<ocs_plugin_api::host::acadrust::types::Transparency, String> {
    use ocs_plugin_api::host::acadrust::types::Transparency;
    if let Ok(f) = tr.extract::<f64>() {
        let pct = if f > 1.0 { f / 100.0 } else { f }.clamp(0.0, 0.9);
        Ok(Transparency::from_percent(pct))
    } else if let Ok(i) = tr.extract::<i64>() {
        let pct = ((i as f64).clamp(0.0, 90.0)) / 100.0;
        Ok(Transparency::from_percent(pct))
    } else if let Ok(s) = tr.extract::<String>() {
        match s.trim().to_ascii_lowercase().as_str() {
            "bylayer" | "-1" => Ok(Transparency::ByLayer),
            "byblock" | "-2" => Ok(Transparency::ByBlock),
            "opaque" | "0" => Ok(Transparency::OPAQUE),
            other => {
                let cleaned = other.trim_end_matches('%').trim();
                if let Ok(val) = cleaned.parse::<f64>() {
                    let pct = if val > 1.0 { val / 100.0 } else { val }.clamp(0.0, 0.9);
                    Ok(Transparency::from_percent(pct))
                } else {
                    Err(format!("Unknown transparency '{}'", other))
                }
            }
        }
    } else {
        Err("transparency must be a percentage number (0-90) or string ('ByLayer', 'ByBlock', 'Opaque')".to_string())
    }
}
fn build_layer_config(
    name: &str,
    color: Option<&Bound<'_, PyAny>>,
    linetype: Option<&str>,
    lineweight: Option<&Bound<'_, PyAny>>,
    off: Option<bool>,
    visible: Option<bool>,
    on: Option<bool>,
    frozen: Option<bool>,
    locked: Option<bool>,
    plottable: Option<bool>,
    plot: Option<bool>,
    transparency: Option<&Bound<'_, PyAny>>,
    description: Option<&str>,
) -> PyResult<(String, ocs_plugin_api::host::LayerConfig)> {
    let layer_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    let parsed_lineweight = match lineweight {
        Some(lw) => Some(parse_lineweight(lw).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };
    let parsed_transparency = match transparency {
        Some(tr) => Some(parse_transparency(tr).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    // Calculate effective visibility: `off` takes priority, then `visible`, then `on`
    let effective_off = if let Some(off_val) = off {
        Some(off_val)
    } else if let Some(vis_val) = visible {
        Some(!vis_val)
    } else if let Some(on_val) = on {
        Some(!on_val)
    } else {
        None
    };

    let effective_plottable = if let Some(p) = plottable {
        Some(p)
    } else {
        plot
    };

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(PyRuntimeError::new_err("Layer name cannot be empty"));
    }

    let config = ocs_plugin_api::host::LayerConfig {
        name: trimmed.to_string(),
        color: layer_color,
        linetype: linetype.map(|s| s.to_string()),
        lineweight: parsed_lineweight,
        off: effective_off,
        frozen,
        locked,
        plottable: effective_plottable,
        transparency: parsed_transparency,
        description: description.map(|s| s.to_string()),
    };

    Ok((trimmed.to_string(), config))
}

#[pyfunction]
#[pyo3(signature = (
    name,
    color=None,
    linetype=None,
    lineweight=None,
    off=None,
    visible=None,
    on=None,
    frozen=None,
    locked=None,
    plottable=None,
    plot=None,
    transparency=None,
    description=None
))]
fn add_layer(
    name: &str,
    color: Option<&Bound<'_, PyAny>>,
    linetype: Option<&str>,
    lineweight: Option<&Bound<'_, PyAny>>,
    off: Option<bool>,
    visible: Option<bool>,
    on: Option<bool>,
    frozen: Option<bool>,
    locked: Option<bool>,
    plottable: Option<bool>,
    plot: Option<bool>,
    transparency: Option<&Bound<'_, PyAny>>,
    description: Option<&str>,
) -> PyResult<u64> {
    let (layer_name, config) = build_layer_config(
        name, color, linetype, lineweight, off, visible, on, frozen, locked, plottable, plot, transparency, description,
    )?;

    let res = with_host(|h| {
        match h.table_operation(TableOperation::LayerCreate { config }) {
            Ok(handle) => Ok(handle.value()),
            Err(e) => {
                if h.document().layers.contains(&layer_name) {
                    Err(format!("Layer '{}' already exists; use modify_layer() to update properties", layer_name))
                } else {
                    Err(e)
                }
            }
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (
    name,
    color=None,
    linetype=None,
    lineweight=None,
    off=None,
    visible=None,
    on=None,
    frozen=None,
    locked=None,
    plottable=None,
    plot=None,
    transparency=None,
    description=None
))]
fn modify_layer(
    name: &str,
    color: Option<&Bound<'_, PyAny>>,
    linetype: Option<&str>,
    lineweight: Option<&Bound<'_, PyAny>>,
    off: Option<bool>,
    visible: Option<bool>,
    on: Option<bool>,
    frozen: Option<bool>,
    locked: Option<bool>,
    plottable: Option<bool>,
    plot: Option<bool>,
    transparency: Option<&Bound<'_, PyAny>>,
    description: Option<&str>,
) -> PyResult<bool> {
    let (layer_name, config) = build_layer_config(
        name, color, linetype, lineweight, off, visible, on, frozen, locked, plottable, plot, transparency, description,
    )?;

    let res = with_host(|h| {
        match h.table_operation(TableOperation::LayerModify { config }) {
            Ok(_) => Ok(true),
            Err(e) => {
                if !h.document().layers.contains(&layer_name) {
                    Err(format!("Layer '{}' does not exist; use add_layer() to create it", layer_name))
                } else {
                    Err(e)
                }
            }
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn set_layer_color(name: &str, color: &Bound<'_, PyAny>) -> PyResult<bool> {
    let c = parse_color(color).map_err(|e| PyRuntimeError::new_err(e))?;
    let trimmed = name.trim();
    let config = ocs_plugin_api::host::LayerConfig {
        name: trimmed.to_string(),
        color: Some(c),
        ..Default::default()
    };
    let res = with_host(|h| {
        h.table_operation(TableOperation::LayerModify { config })
            .map(|_| true)
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn rename_layer(old_name: &str, new_name: &str) -> PyResult<bool> {
    let res = with_host(|h| {
        h.table_operation(TableOperation::LayerRename {
            from: old_name.to_string(),
            to: new_name.to_string(),
        })
        .map(|_| true)
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (name, erase_objects=false))]
fn delete_layer(name: &str, erase_objects: bool) -> PyResult<bool> {
    let res = with_host(|h| {
        h.table_operation(TableOperation::LayerDelete {
            name: name.to_string(),
            erase_objects,
        })
        .map(|_| true)
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (name, font_family=None, height=None, width_factor=None, oblique_angle=None))]
fn add_text_style(
    name: &str,
    font_family: Option<&str>,
    height: Option<f64>,
    width_factor: Option<f64>,
    oblique_angle: Option<f64>,
) -> PyResult<u64> {
    let config = TextStyleConfig {
        name: name.to_string(),
        height,
        width_factor,
        oblique_angle,
        true_type_font: font_family.map(|s| s.to_string()),
        ..Default::default()
    };
    let res = with_host(|h| {
        h.table_operation(TableOperation::TextStyleCreate { config })
            .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (name, base_point, entity_handles, erase_originals=false, description=None))]
fn define_block(
    name: &str,
    base_point: &Bound<'_, PyAny>,
    entity_handles: Vec<u64>,
    erase_originals: bool,
    description: Option<&str>,
) -> PyResult<u64> {
    let [x, y, z] = parse_point(base_point).map_err(|e| PyRuntimeError::new_err(e))?;
    let handles: Vec<Handle> = entity_handles.into_iter().map(Handle::new).collect();
    let res = with_host(|h| {
        h.table_operation(TableOperation::BlockCreate {
            name: name.to_string(),
            entities: handles,
            base_point: [x, y, z],
            erase_originals,
            description: description.map(|s| s.to_string()),
        })
        .map(|handle| handle.value())
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;
    res.map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
#[pyo3(signature = (name, pt, scale=None, rotation=0.0, layer="0", color=None))]
fn insert_block(
    name: &str,
    pt: &Bound<'_, PyAny>,
    scale: Option<&Bound<'_, PyAny>>,
    rotation: f64,
    layer: &str,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<u64> {
    record_entity_created()?;
    let [x, y, z] = parse_point(pt).map_err(|e| PyRuntimeError::new_err(e))?;
    let parsed_color = match color {
        Some(c) => Some(parse_color(c).map_err(|e| PyRuntimeError::new_err(e))?),
        None => None,
    };

    let mut ins = Insert::new(name, Vector3::new(x, y, z));
    if let Some(s) = scale {
        if let Ok(f) = s.extract::<f64>() {
            ins.set_x_scale(f);
            ins.set_y_scale(f);
            ins.set_z_scale(f);
        } else if let Ok((sx, sy, sz)) = s.extract::<(f64, f64, f64)>() {
            ins.set_x_scale(sx);
            ins.set_y_scale(sy);
            ins.set_z_scale(sz);
        } else if let Ok((sx, sy)) = s.extract::<(f64, f64)>() {
            ins.set_x_scale(sx);
            ins.set_y_scale(sy);
            ins.set_z_scale(1.0);
        } else if let Ok(seq) = s.extract::<Vec<f64>>() {
            match seq.len() {
                1 => {
                    ins.set_x_scale(seq[0]);
                    ins.set_y_scale(seq[0]);
                    ins.set_z_scale(seq[0]);
                }
                2 => {
                    ins.set_x_scale(seq[0]);
                    ins.set_y_scale(seq[1]);
                    ins.set_z_scale(1.0);
                }
                3 => {
                    ins.set_x_scale(seq[0]);
                    ins.set_y_scale(seq[1]);
                    ins.set_z_scale(seq[2]);
                }
                _ => return Err(PyRuntimeError::new_err("scale must have 1, 2, or 3 elements")),
            }
        } else {
            return Err(PyRuntimeError::new_err("scale must be a number or tuple of numbers"));
        }
    }
    ins.rotation = rotation;
    ins.common.layer = layer.to_string();
    if let Some(c) = parsed_color {
        ins.common.color = c;
    }

    if let Some(bridge) = crate::context::active_interactive() {
        let handle = bridge.add_pending_entity(EntityType::Insert(ins));
        return Ok(handle);
    }
    with_host(|h| {
        ensure_layer_exists(h, layer);
        let handle = h.add_entity(EntityType::Insert(ins));
        h.set_dirty();
        h.bump_geometry();
        handle.value()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn cmd(cmd_str: &str) -> PyResult<bool> {
    if let Some(bridge) = crate::context::active_interactive() {
        bridge.finish();
        crate::context::set_active_interactive(None);
    }

    let has_newline = cmd_str.ends_with('\n') || cmd_str.ends_with('\r');
    let trimmed = cmd_str.trim();

    if trimmed.is_empty() {
        return with_host(|h| {
            let to_send = if has_newline { "\n" } else { "" };
            h.execute_command(to_send)
        })
        .map_err(|e| PyRuntimeError::new_err(e));
    }

    let upper = trimmed.to_uppercase();
    if upper.starts_with("PY:")
        || upper.starts_with("PYRUN:")
        || upper.starts_with("PY ")
        || upper == "PY"
        || upper == "OCIDE"
        || upper == "PYLIST"
    {
        if let Ok(handled) = with_host(|h| {
            crate::dispatcher::dispatch_command(h, cmd_str)
        }) {
            return Ok(handled);
        }
    }

    let to_send = if has_newline {
        format!("{trimmed}\n")
    } else {
        trimmed.to_string()
    };

    with_host(|h| {
        h.execute_command(&to_send)
    })
    .map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn execute_command(cmd_str: &str) -> PyResult<bool> {
    cmd(cmd_str)
}

#[pyfunction]
fn enter() -> PyResult<bool> {
    cmd("ENTER")
}

#[pyfunction]
#[pyo3(signature = (cmd_name, description=None))]
fn _register_host_alias(cmd_name: &str, description: Option<&str>) -> PyResult<bool> {
    let upper = cmd_name.trim().to_uppercase();
    REGISTERED_COMMANDS.lock().unwrap().insert(upper, description.unwrap_or("").to_string());
    Ok(true)
}

struct LayerData {
    name: String,
    handle: u64,
    off: bool,
    frozen: bool,
    locked: bool,
    plottable: bool,
    color: String,
    color_index: i16,
    linetype: String,
    lineweight: f64,
    transparency: f64,
}

impl LayerData {
    fn from_layer(l: &ocs_plugin_api::host::acadrust::tables::Layer) -> Self {
        use ocs_plugin_api::host::acadrust::types::LineWeight;
        let (color_str, color_idx) = match l.color {
            Color::Index(i) => (format!("{i}"), i as i16),
            Color::Rgb { r, g, b } => (format!("#{:02X}{:02X}{:02X}", r, g, b), 0),
            Color::ByLayer => ("ByLayer".to_string(), 256),
            Color::ByBlock => ("ByBlock".to_string(), 0),
            _ => ("ByLayer".to_string(), 256),
        };
        let lw = match l.line_weight {
            LineWeight::Value(v) => (v as f64) / 100.0,
            _ => -1.0,
        };
        let lt = if l.line_type.is_empty() {
            "Continuous".to_string()
        } else {
            l.line_type.clone()
        };
        Self {
            name: l.name.clone(),
            handle: l.handle.value(),
            off: l.flags.off,
            frozen: l.flags.frozen,
            locked: l.flags.locked,
            plottable: l.is_plottable,
            color: color_str,
            color_index: color_idx,
            linetype: lt,
            lineweight: lw,
            transparency: l.transparency.as_percent() * 100.0,
        }
    }

    fn to_py_dict<'py>(&self, py: Python<'py>) -> Bound<'py, pyo3::types::PyDict> {
        let dict = pyo3::types::PyDict::new(py);
        let _ = dict.set_item("name", &self.name);
        let _ = dict.set_item("handle", self.handle);
        let _ = dict.set_item("off", self.off);
        let _ = dict.set_item("visible", !self.off);
        let _ = dict.set_item("frozen", self.frozen);
        let _ = dict.set_item("locked", self.locked);
        let _ = dict.set_item("plottable", self.plottable);
        let _ = dict.set_item("color", &self.color);
        let _ = dict.set_item("color_index", self.color_index);
        let _ = dict.set_item("linetype", &self.linetype);
        let _ = dict.set_item("lineweight", self.lineweight);
        let _ = dict.set_item("transparency", self.transparency);
        dict
    }
}

#[pyfunction]
fn get_layers(py: Python<'_>) -> PyResult<PyObject> {
    let layers: Vec<LayerData> = with_host(|h| {
        let doc = h.document();
        doc.layers.iter().map(LayerData::from_layer).collect()
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;

    let list = pyo3::types::PyList::empty(py);
    for l in layers {
        let _ = list.append(l.to_py_dict(py));
    }
    Ok(list.into())
}

#[pyfunction]
fn get_layer(py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
    let trimmed = name.trim();
    let data: Option<LayerData> = with_host(|h| {
        let doc = h.document();
        doc.layers.get(trimmed).map(LayerData::from_layer)
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;

    match data {
        Some(d) => Ok(Some(d.to_py_dict(py).into())),
        None => Ok(None),
    }
}

#[pyfunction]
fn get_current_layer() -> PyResult<String> {
    with_host(|h| {
        let cur = &h.document().header.current_layer_name;
        if cur.is_empty() {
            "0".to_string()
        } else {
            cur.clone()
        }
    })
    .map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn set_current_layer(name: &str) -> PyResult<bool> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    with_host(|h| {
        ensure_layer_exists(h, trimmed);
        h.document_mut().header.current_layer_name = trimmed.to_string();
        h.set_dirty();
        let _ = h.execute_command(&format!("CLAYER {}\n", trimmed));
        true
    })
    .map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn entity_count() -> PyResult<usize> {
    with_host(|h| {
        let reader = h.document_reader();
        reader.entity_count()
    }).map_err(|e| PyRuntimeError::new_err(e))
}

struct EntityData {
    handle: u64,
    kind: &'static str,
    layer: String,
}

#[pyfunction]
fn get_entities(py: Python<'_>) -> PyResult<PyObject> {
    let entities: Vec<EntityData> = with_host(|h| {
        let reader = h.document_reader();
        let mut out = Vec::new();
        reader.for_each_entity(&mut |e| {
            let kind_str = match e.kind {
                ocs_plugin_api::host::ReaderEntityKind::Point => "Point",
                ocs_plugin_api::host::ReaderEntityKind::Line => "Line",
                ocs_plugin_api::host::ReaderEntityKind::Circle => "Circle",
                ocs_plugin_api::host::ReaderEntityKind::Arc => "Arc",
                ocs_plugin_api::host::ReaderEntityKind::Polyline => "Polyline",
                ocs_plugin_api::host::ReaderEntityKind::Text => "Text",
                _ => "Other",
            };
            out.push(EntityData {
                handle: e.handle.value(),
                kind: kind_str,
                layer: e.layer_name.to_string(),
            });
        });
        out
    }).map_err(|e| PyRuntimeError::new_err(e))?;

    let list = pyo3::types::PyList::empty(py);
    for e in entities {
        let dict = pyo3::types::PyDict::new(py);
        let _ = dict.set_item("handle", e.handle);
        let _ = dict.set_item("kind", e.kind);
        let _ = dict.set_item("layer", e.layer);
        let _ = list.append(dict);
    }
    Ok(list.into())
}

pub enum RawEntityDetails {
    Line { start: [f64; 3], end: [f64; 3] },
    Arc { center: [f64; 3], radius: f64, start_angle: f64, end_angle: f64 },
    Circle { center: [f64; 3], radius: f64 },
    Point { location: [f64; 3] },
    Other { kind: String },
}

pub struct RawEntity {
    pub handle: u64,
    pub layer: String,
    pub details: RawEntityDetails,
}

pub fn raw_entity_from_entity(ent: &EntityType) -> RawEntity {
    let handle = ent.common().handle.value();
    let layer = ent.common().layer.clone();
    let details = match ent {
        EntityType::Line(l) => RawEntityDetails::Line {
            start: [l.start.x, l.start.y, l.start.z],
            end: [l.end.x, l.end.y, l.end.z],
        },
        EntityType::Arc(a) => RawEntityDetails::Arc {
            center: [a.center.x, a.center.y, a.center.z],
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        },
        EntityType::Circle(c) => RawEntityDetails::Circle {
            center: [c.center.x, c.center.y, c.center.z],
            radius: c.radius,
        },
        EntityType::Point(p) => RawEntityDetails::Point {
            location: [p.location.x, p.location.y, p.location.z],
        },
        _ => RawEntityDetails::Other {
            kind: "OTHER".to_string(),
        },
    };
    RawEntity { handle, layer, details }
}

pub fn raw_entity_to_py(py: Python<'_>, ent: RawEntity) -> PyResult<Option<PyObject>> {
    let d = pyo3::types::PyDict::new(py);
    d.set_item("handle", ent.handle)?;
    d.set_item("layer", ent.layer)?;
    match ent.details {
        RawEntityDetails::Line { start, end } => {
            d.set_item("type", "LINE")?;
            d.set_item("start", (start[0], start[1], start[2]))?;
            d.set_item("end", (end[0], end[1], end[2]))?;
        }
        RawEntityDetails::Arc { center, radius, start_angle, end_angle } => {
            d.set_item("type", "ARC")?;
            d.set_item("center", (center[0], center[1], center[2]))?;
            d.set_item("radius", radius)?;
            d.set_item("start_angle", start_angle)?;
            d.set_item("end_angle", end_angle)?;
        }
        RawEntityDetails::Circle { center, radius } => {
            d.set_item("type", "CIRCLE")?;
            d.set_item("center", (center[0], center[1], center[2]))?;
            d.set_item("radius", radius)?;
        }
        RawEntityDetails::Point { location } => {
            d.set_item("type", "POINT")?;
            d.set_item("location", (location[0], location[1], location[2]))?;
        }
        RawEntityDetails::Other { kind } => {
            d.set_item("type", kind)?;
        }
    }
    Ok(Some(d.into()))
}

#[pyfunction]
fn get_entity_data(py: Python<'_>, handle: u64) -> PyResult<Option<PyObject>> {
    if let Some(bridge) = crate::context::active_interactive() {
        if let Some(raw) = bridge.query_entity(handle, |e| raw_entity_from_entity(e)) {
            return raw_entity_to_py(py, raw);
        }
        return Ok(None);
    }

    let ent_opt: Option<RawEntity> = with_host(|h| {
        let doc = h.document();
        let ent = match doc.get_entity(Handle::new(handle)) {
            Some(e) => e,
            None => return None,
        };
        Some(raw_entity_from_entity(ent))
    }).map_err(|e| PyRuntimeError::new_err(e))?;

    match ent_opt {
        Some(ent) => raw_entity_to_py(py, ent),
        None => Ok(None),
    }
}

#[pyfunction]
fn register_command(alias: &str, target: &str) -> PyResult<bool> {
    let trimmed_alias = alias.trim().to_uppercase();
    let trimmed_target = if target.starts_with("PY ") || target.starts_with("py ") {
        target.trim().to_string()
    } else {
        format!("PY {}", target.trim())
    };
    REGISTERED_COMMANDS.lock().unwrap().insert(trimmed_alias, trimmed_target);
    Ok(true)
}

pub(crate) fn register_command_alias(alias: &str, target: &str) {
    let trimmed_alias = alias.trim().to_uppercase();
    REGISTERED_COMMANDS.lock().unwrap().insert(trimmed_alias, target.to_string());
}

#[pyfunction]
#[pyo3(signature = (script_name, args=None))]
fn run_script(script_name: &str, args: Option<Vec<String>>) -> PyResult<bool> {
    let arg_vec = args.unwrap_or_default();
    with_host(|h| {
        crate::py_runtime::execute_script(script_name, &arg_vec, h)
            .map(|_| true)
            .map_err(|e| PyRuntimeError::new_err(e))
    })
    .map_err(|e| PyRuntimeError::new_err(e))?
}

#[pyfunction]
#[pyo3(signature = (spec=None, color=None))]
fn set_rubber_band(spec: Option<&Bound<'_, PyAny>>, color: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    let rb = parse_rubber_band(spec, None, color).map_err(|e| PyRuntimeError::new_err(e))?;
    if let Some(bridge) = crate::context::active_interactive() {
        bridge.set_rubber_band(rb);
    }
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (prompt_or_base=None, base_point=None, rubber_band=None, color=None))]
fn get_point(
    py: Python<'_>,
    prompt_or_base: Option<&Bound<'_, PyAny>>,
    base_point: Option<&Bound<'_, PyAny>>,
    rubber_band: Option<&Bound<'_, PyAny>>,
    color: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<(f64, f64, f64)>> {
    let (prompt_str, bp_arg) = match prompt_or_base {
        Some(obj) if !obj.is_none() => {
            if let Ok(s) = obj.extract::<String>() {
                (Some(s), base_point)
            } else if parse_point(obj).is_ok() {
                // First arg was a base point coordinate
                (None, Some(obj))
            } else {
                (None, base_point)
            }
        }
        _ => (None, base_point),
    };

    let rb_spec = parse_rubber_band(rubber_band, bp_arg, color)
        .map_err(|e| PyRuntimeError::new_err(e))?;

    let p = prompt_str.as_deref().unwrap_or("Specify point: ");
    let bridge = crate::context::ensure_interactive(p, false)
        .map_err(|e| PyRuntimeError::new_err(e))?;
    let res = py.allow_threads(|| bridge.get_point_with_rubber_band(prompt_str.as_deref(), Some(rb_spec)));
    if res.is_none() {
        bridge.finish();
        crate::context::set_active_interactive(None);
    }
    Ok(res.map(|[x, y, z]| (x, y, z)))
}

#[pyfunction]
#[pyo3(signature = (prompt_or_base=None, base_point_or_prompt=None))]
fn get_dist(
    py: Python<'_>,
    prompt_or_base: Option<&Bound<'_, PyAny>>,
    base_point_or_prompt: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<f64>> {
    let mut prompt_str: Option<String> = None;
    let mut base_pt: Option<[f64; 3]> = None;

    if let Some(arg1) = prompt_or_base {
        if !arg1.is_none() {
            if let Ok(s) = arg1.extract::<String>() {
                prompt_str = Some(s);
            } else if let Ok(pt) = parse_point(arg1) {
                base_pt = Some(pt);
            }
        }
    }

    if let Some(arg2) = base_point_or_prompt {
        if !arg2.is_none() {
            if let Ok(s) = arg2.extract::<String>() {
                if prompt_str.is_none() {
                    prompt_str = Some(s);
                }
            } else if let Ok(pt) = parse_point(arg2) {
                if base_pt.is_none() {
                    base_pt = Some(pt);
                }
            }
        }
    }

    let p1 = if let Some(pt) = base_pt {
        pt
    } else {
        let p_prompt = prompt_str.as_deref().unwrap_or("Specify first point: ");
        let bridge = crate::context::ensure_interactive(p_prompt, false)
            .map_err(|e| PyRuntimeError::new_err(e))?;
        let res = py.allow_threads(|| bridge.get_point_with_rubber_band(Some(p_prompt), None));
        match res {
            Some(pt) => pt,
            None => {
                bridge.finish();
                crate::context::set_active_interactive(None);
                return Ok(None);
            }
        }
    };

    let p2_prompt = prompt_str.as_deref().unwrap_or("Specify second point: ");
    crate::interactive::dbg_trace(&format!("get_dist: calling ensure_interactive with p2_prompt={:?}, base p1={:?}", p2_prompt, p1));
    let bridge = crate::context::ensure_interactive(p2_prompt, false)
        .map_err(|e| PyRuntimeError::new_err(e))?;
    let rb = RubberBandSpec::Line { from: p1, color: None };
    let res2 = py.allow_threads(|| bridge.get_point_with_rubber_band(Some(p2_prompt), Some(rb)));
    crate::interactive::dbg_trace(&format!("get_dist: res2={:?}", res2));
    match res2 {
        Some(p2) => {
            let dx = p2[0] - p1[0];
            let dy = p2[1] - p1[1];
            let dz = p2[2] - p1[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            crate::interactive::dbg_trace(&format!("get_dist: calculated dist={}", dist));
            Ok(Some(dist))
        }
        None => {
            crate::interactive::dbg_trace("get_dist: res2 was None, finishing bridge");
            bridge.finish();
            crate::context::set_active_interactive(None);
            Ok(None)
        }
    }
}

#[pyfunction]
#[pyo3(signature = (prompt=None))]
fn get_entity(py: Python<'_>, prompt: Option<&str>) -> PyResult<Option<(u64, (f64, f64, f64))>> {
    let p = prompt.unwrap_or("Select object: ");
    let bridge = crate::context::ensure_interactive(p, true)
        .map_err(|e| PyRuntimeError::new_err(e))?;
    let res = py.allow_threads(|| bridge.get_entity(prompt));
    if res.is_none() {
        bridge.finish();
        crate::context::set_active_interactive(None);
    }
    Ok(res.map(|(handle, [x, y, z])| (handle, (x, y, z))))
}

#[pyfunction]
#[pyo3(signature = (prompt=None))]
fn get_selection(py: Python<'_>, prompt: Option<&str>) -> PyResult<Option<Vec<u64>>> {
    // If prompt is None and objects are already pre-selected, return them immediately
    if prompt.is_none() {
        let pre = get_preselection()?;
        if !pre.is_empty() {
            return Ok(Some(pre));
        }
        if let Ok(sel) = with_host(|h| h.selection()) {
            if !sel.is_empty() {
                return Ok(Some(sel.into_iter().map(|h| h.value()).collect()));
            }
        }
    }

    let p = prompt.unwrap_or("Select objects: ");
    let bridge = crate::context::ensure_interactive(p, true)
        .map_err(|e| PyRuntimeError::new_err(e))?;
    let res = py.allow_threads(|| bridge.get_selection(Some(p)));
    if res.is_none() {
        bridge.finish();
        crate::context::set_active_interactive(None);
    }
    Ok(res)
}

#[pyfunction]
fn set_selection(handles: Vec<u64>) -> PyResult<()> {
    let h_vec: Vec<Handle> = handles.into_iter().map(Handle::new).collect();
    with_host(|h| {
        h.set_selection(&h_vec)
    })
    .map_err(|e| PyRuntimeError::new_err(e))?
    .map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn get_var(py: Python<'_>, name: &str) -> PyResult<Option<PyObject>> {
    use lisp_cad::bridge::DxfValue;
    use pyo3::IntoPyObjectExt;

    let key = name.trim().to_ascii_uppercase();
    let dxf_opt = with_host(|h| {
        if let Some(sv) = h.system_variable(&key) {
            return Some(match sv {
                HostSettingValue::Text(s) => DxfValue::Str(s),
                HostSettingValue::Number(n) => DxfValue::Real(n),
            });
        }
        if let Some(v) = crate::rust_lisp::read_header_var(h.document(), &key) {
            return Some(v);
        }
        None
    })
    .map_err(|e| PyRuntimeError::new_err(e))?;

    match dxf_opt {
        Some(DxfValue::Str(s)) => Ok(Some(s.into_py_any(py)?)),
        Some(DxfValue::Int(i)) => Ok(Some(i.into_py_any(py)?)),
        Some(DxfValue::Real(r)) => Ok(Some(r.into_py_any(py)?)),
        Some(DxfValue::Point(pt)) => Ok(Some((pt[0], pt[1], pt[2]).into_py_any(py)?)),
        Some(DxfValue::Handle(h)) => Ok(Some(format!("{:X}", h).into_py_any(py)?)),
        None => Ok(None),
    }
}

#[pyfunction]
fn set_var(name: &str, value: &Bound<'_, PyAny>) -> PyResult<bool> {
    use lisp_cad::bridge::DxfValue;

    let key = name.trim().to_ascii_uppercase();
    let dxf_val = if let Ok(b) = value.extract::<bool>() {
        DxfValue::Int(i64::from(b))
    } else if let Ok(i) = value.extract::<i64>() {
        DxfValue::Int(i)
    } else if let Ok(f) = value.extract::<f64>() {
        DxfValue::Real(f)
    } else if let Ok(s) = value.extract::<String>() {
        DxfValue::Str(s)
    } else {
        return Err(PyRuntimeError::new_err(
            "Variable value must be a number, boolean, or string",
        ));
    };

    with_host(|h| crate::rust_lisp::apply_host_sysvar(h, &key, &dxf_val).map(|_| true))
        .map_err(|e| PyRuntimeError::new_err(e))?
        .map_err(|e| PyRuntimeError::new_err(e))
}

#[pyfunction]
fn get_preselection() -> PyResult<Vec<u64>> {
    Ok(crate::interactive::get_cached_selection())
}


#[pyfunction]
fn _show_dialog_raw(dialog_json: &str) -> PyResult<String> {
    let def: ocs_plugin_api::dialog::DialogDef = serde_json::from_str(dialog_json)
        .map_err(|e| PyRuntimeError::new_err(format!("Invalid dialog definition: {e}")))?;

    let bridge = if let Some(b) = crate::context::active_interactive() {
        b
    } else {
        crate::context::ensure_interactive_dialog(&def.title, def.clone())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to start dialog session: {e}")))?
    };

    let response = bridge.show_dialog(def);
    serde_json::to_string(&response)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize dialog response: {e}")))
}

#[pyfunction]
fn _register_panel_raw(panel_json: &str) -> PyResult<bool> {
    let def: ocs_plugin_api::dialog::PanelDef = serde_json::from_str(panel_json)
        .map_err(|e| PyRuntimeError::new_err(format!("Invalid panel definition: {e}")))?;
    match with_host(|h| h.register_panel(def)) {
        Ok(b) => Ok(b),
        Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
    }
}

#[pyfunction]
#[pyo3(signature = (panel_id, values_json))]
fn _update_panel_raw(panel_id: &str, values_json: &str) -> PyResult<bool> {
    let values: std::collections::HashMap<String, ocs_plugin_api::dialog::DialogValue> =
        serde_json::from_str(values_json)
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid panel values: {e}")))?;

    if let Some(bridge) = crate::context::active_interactive() {
        bridge.add_pending_panel_update(panel_id.to_string(), values);
        return Ok(true);
    }

    match with_host(|h| h.update_panel(panel_id, values)) {
        Ok(b) => Ok(b),
        Err(e) => {
            eprintln!("[plugin] [DEBUG] update_panel bypassed: {e}");
            Ok(false)
        }
    }
}

#[pyfunction]
#[pyo3(signature = (panel_id, show = None))]
fn _toggle_panel_raw(panel_id: &str, show: Option<bool>) -> PyResult<bool> {
    match with_host(|h| h.toggle_panel(panel_id, show)) {
        Ok(b) => Ok(b),
        Err(e) => {
            eprintln!("[plugin] [DEBUG] toggle_panel bypassed: {e}");
            Ok(false)
        }
    }
}

pub fn handle_panel_action(
    panel_id: String,
    control_id: String,
    action: String,
    value: Option<ocs_plugin_api::dialog::DialogValue>,
) {
    let val_json = value.and_then(|v| serde_json::to_string(&v).ok());
    Python::with_gil(|py| {
        if let Ok(m) = py.import("opencad") {
            if let Ok(dispatch_fn) = m.getattr("_dispatch_panel_action") {
                let _ = dispatch_fn.call1((panel_id, control_id, action, val_json));
            }
        }
    });
}

#[pyfunction]
fn lisp_eval(py: Python<'_>, code: &str) -> PyResult<PyObject> {
    let opencad_lisp_mod = py.import("opencad_lisp")?;
    let eval_fn = opencad_lisp_mod.getattr("eval_lisp")?;
    let res = eval_fn.call1((code,))?;
    Ok(res.into())
}

#[pyfunction]
fn lisp_load(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    let opencad_lisp_mod = py.import("opencad_lisp")?;
    let load_fn = opencad_lisp_mod.getattr("load_lisp_file")?;
    let res = load_fn.call1((file_path,))?;
    Ok(res.into())
}

pub fn populate_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(lisp_eval, m)?)?;
    m.add_function(wrap_pyfunction!(lisp_load, m)?)?;
    m.add_function(wrap_pyfunction!(_show_dialog_raw, m)?)?;
    m.add_function(wrap_pyfunction!(_register_panel_raw, m)?)?;
    m.add_function(wrap_pyfunction!(_update_panel_raw, m)?)?;
    m.add_function(wrap_pyfunction!(_toggle_panel_raw, m)?)?;
    m.add_function(wrap_pyfunction!(info, m)?)?;
    m.add_function(wrap_pyfunction!(output, m)?)?;
    m.add_function(wrap_pyfunction!(error, m)?)?;
    m.add_function(wrap_pyfunction!(undo, m)?)?;
    m.add_function(wrap_pyfunction!(set_dirty, m)?)?;
    m.add_function(wrap_pyfunction!(bump_geometry, m)?)?;
    m.add_function(wrap_pyfunction!(add_point, m)?)?;
    m.add_function(wrap_pyfunction!(add_line, m)?)?;
    m.add_function(wrap_pyfunction!(add_circle, m)?)?;
    m.add_function(wrap_pyfunction!(add_arc, m)?)?;
    m.add_function(wrap_pyfunction!(add_ellipse, m)?)?;
    m.add_function(wrap_pyfunction!(add_polyline, m)?)?;
    m.add_function(wrap_pyfunction!(add_text, m)?)?;
    m.add_function(wrap_pyfunction!(add_mtext, m)?)?;
    m.add_function(wrap_pyfunction!(add_ray, m)?)?;
    m.add_function(wrap_pyfunction!(add_xline, m)?)?;
    m.add_function(wrap_pyfunction!(add_solid, m)?)?;
    m.add_function(wrap_pyfunction!(add_box, m)?)?;
    m.add_function(wrap_pyfunction!(add_wedge, m)?)?;
    m.add_function(wrap_pyfunction!(add_cylinder, m)?)?;
    m.add_function(wrap_pyfunction!(add_sphere, m)?)?;
    m.add_function(wrap_pyfunction!(add_torus, m)?)?;
    m.add_function(wrap_pyfunction!(add_pyramid, m)?)?;
    m.add_function(wrap_pyfunction!(solid_union, m)?)?;
    m.add_function(wrap_pyfunction!(solid_subtract, m)?)?;
    m.add_function(wrap_pyfunction!(solid_intersect, m)?)?;
    m.add_function(wrap_pyfunction!(remove_entity, m)?)?;
    m.add_function(wrap_pyfunction!(add_layer, m)?)?;
    m.add_function(wrap_pyfunction!(rename_layer, m)?)?;
    m.add_function(wrap_pyfunction!(delete_layer, m)?)?;
    m.add_function(wrap_pyfunction!(add_text_style, m)?)?;
    m.add_function(wrap_pyfunction!(define_block, m)?)?;
    m.add_function(wrap_pyfunction!(insert_block, m)?)?;
    m.add_function(wrap_pyfunction!(set_layer_color, m)?)?;
    m.add_function(wrap_pyfunction!(cmd, m)?)?;
    m.add_function(wrap_pyfunction!(execute_command, m)?)?;
    m.add_function(wrap_pyfunction!(enter, m)?)?;
    m.add_function(wrap_pyfunction!(_register_host_alias, m)?)?;
    m.add_function(wrap_pyfunction!(register_command, m)?)?;
    m.add("add_alias", wrap_pyfunction!(register_command, m)?)?;
    m.add_function(wrap_pyfunction!(run_script, m)?)?;
    m.add_function(wrap_pyfunction!(get_layers, m)?)?;
    m.add_function(wrap_pyfunction!(get_layer, m)?)?;
    m.add_function(wrap_pyfunction!(modify_layer, m)?)?;
    m.add("set_layer", wrap_pyfunction!(modify_layer, m)?)?;
    m.add_function(wrap_pyfunction!(get_current_layer, m)?)?;
    m.add_function(wrap_pyfunction!(set_current_layer, m)?)?;
    m.add_function(wrap_pyfunction!(entity_count, m)?)?;
    m.add_function(wrap_pyfunction!(get_entities, m)?)?;
    m.add_function(wrap_pyfunction!(get_entity_data, m)?)?;
    m.add("entget", wrap_pyfunction!(get_entity_data, m)?)?;
    m.add_function(wrap_pyfunction!(get_point, m)?)?;
    m.add_function(wrap_pyfunction!(get_dist, m)?)?;
    m.add("getdist", wrap_pyfunction!(get_dist, m)?)?;
    m.add_function(wrap_pyfunction!(set_rubber_band, m)?)?;
    m.add_function(wrap_pyfunction!(get_entity, m)?)?;
    m.add_function(wrap_pyfunction!(get_selection, m)?)?;
    m.add_function(wrap_pyfunction!(set_selection, m)?)?;
    m.add_function(wrap_pyfunction!(get_preselection, m)?)?;
    m.add_function(wrap_pyfunction!(get_var, m)?)?;
    m.add_function(wrap_pyfunction!(set_var, m)?)?;
    m.add_function(wrap_pyfunction!(is_sandboxed, m)?)?;
    m.add_function(wrap_pyfunction!(get_security_status, m)?)?;
    m.add_function(wrap_pyfunction!(trust_directory, m)?)?;

    // Inject Python-level decorator and dispatcher machinery
    let py = m.py();
    let py_code = r#"
import inspect

_COMMAND_REGISTRY = {}

class CommandDef:
    def __init__(self, name, func, description=""):
        self.name = name.upper()
        self.func = func
        self.description = description or func.__doc__ or ""

def _register_command(func, name=None, description=None):
    raw_name = name or func.__name__
    cmd_name = raw_name.upper()
    if cmd_name.startswith("C_") and len(cmd_name) > 2:
        cmd_name = cmd_name[2:]
    elif cmd_name.startswith("C-") and len(cmd_name) > 2:
        cmd_name = cmd_name[2:]
    
    desc = description or func.__doc__ or ""
    _COMMAND_REGISTRY[cmd_name] = CommandDef(cmd_name, func, desc)
    _register_host_alias(cmd_name, desc)
    import sys
    if "__main__" in sys.modules:
        main_dict = sys.modules["__main__"].__dict__
        main_dict[func.__name__] = func
        main_dict[cmd_name] = func
    return func

def command(fn_or_name=None, name=None, description=None):
    if callable(fn_or_name):
        return _register_command(fn_or_name, name=name, description=description)
    
    cmd_name = name if name is not None else fn_or_name
    if isinstance(cmd_name, str) and (" " in cmd_name or "\t" in cmd_name):
        return cmd(cmd_name)

    def decorator(func):
        return _register_command(func, name=cmd_name, description=description)
    return decorator

def _invoke_command(cmd_name, str_args):
    cmd_def = _COMMAND_REGISTRY.get(cmd_name.upper())
    if not cmd_def:
        raise RuntimeError(f"Unknown Python command '{cmd_name}'. Use PYLIST to see available commands.")
    
    fn = cmd_def.func
    sig = inspect.signature(fn)
    params = list(sig.parameters.values())

    if len(params) == 1 and (params[0].name == 'args' or params[0].kind == inspect.Parameter.VAR_POSITIONAL):
        return fn(str_args)
    
    if len(params) == 0:
        return fn()
    
    converted_args = []
    for i, p in enumerate(params):
        if p.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD):
            if i < len(str_args):
                raw_val = str_args[i]
                if p.annotation is not inspect.Parameter.empty:
                    try:
                        if p.annotation is bool:
                            val = raw_val.lower() in ("true", "1", "yes", "on")
                        else:
                            val = p.annotation(raw_val)
                    except Exception:
                        val = raw_val
                else:
                    try:
                        if "." in raw_val:
                            val = float(raw_val)
                        else:
                            val = int(raw_val)
                    except ValueError:
                        val = raw_val
                converted_args.append(val)
            elif p.default is not inspect.Parameter.empty:
                converted_args.append(p.default)
            else:
                usage_parts = []
                for param in params:
                    if param.default is inspect.Parameter.empty:
                        usage_parts.append(f"<{param.name}>")
                    else:
                        usage_parts.append(f"[{param.name}={param.default}]")
                usage = " ".join(usage_parts)
                raise ValueError(f"Missing required argument '{p.name}' for {cmd_name}.\nUsage: {cmd_name} {usage}")
        elif p.kind == inspect.Parameter.VAR_POSITIONAL:
            for raw_val in str_args[i:]:
                converted_args.append(raw_val)
            break
            
    return fn(*converted_args)

import json, re

class AttrDict(dict):
    """Dictionary supporting both attribute (d.foo) and dict (d['foo']) access."""
    def __getattr__(self, name):
        try:
            return self[name]
        except KeyError:
            raise AttributeError(f"'AttrDict' object has no attribute '{name}'")
    def __setattr__(self, name, value):
        self[name] = value
    def __delattr__(self, name):
        try:
            del self[name]
        except KeyError:
            raise AttributeError(f"'AttrDict' object has no attribute '{name}'")
    def __repr__(self):
        return f"AttrDict({super().__repr__()})"

class _Row:
    def __init__(self, controls, raw_items=None):
        self.controls = controls
        self.raw_items = raw_items or []

class _Field:
    pass

class _TextField(_Field):
    def __init__(self, default="", label=None):
        self.default = str(default)
        self.label = label

class _CheckboxField(_Field):
    def __init__(self, default=False, label=None):
        self.default = bool(default)
        self.label = label

class _NumberField(_Field):
    def __init__(self, default=0.0, min=None, max=None, step=None, label=None):
        self.default = float(default)
        self.min = float(min) if min is not None else None
        self.max = float(max) if max is not None else None
        self.step = float(step) if step is not None else None
        self.label = label

class _ChoiceField(_Field):
    def __init__(self, options, default=None, label=None):
        self.options = [str(o) for o in options]
        if default is not None:
            if isinstance(default, int):
                self.default_index = default
            elif str(default) in self.options:
                self.default_index = self.options.index(str(default))
            else:
                self.default_index = 0
        else:
            self.default_index = 0
        self.label = label
        self.default = self.options[self.default_index] if self.options else ""

    def __str__(self):
        return self.default

    @property
    def value(self):
        return self.default

class _FileField(_Field):
    def __init__(self, label=None, extensions=None, save=False, default=""):
        self.label = label
        self.extensions = list(extensions) if extensions else []
        self.save = bool(save)
        self.default = str(default)

class _ColorField(_Field):
    def __init__(self, default="red", label=None):
        self.label = label
        if isinstance(default, (list, tuple)) and len(default) == 3:
            self.default = [int(default[0]), int(default[1]), int(default[2])]
        elif isinstance(default, str):
            _COLORS = {
                "red": [255, 0, 0],
                "green": [0, 255, 0],
                "blue": [0, 0, 255],
                "yellow": [255, 255, 0],
                "cyan": [0, 255, 255],
                "magenta": [255, 0, 255],
                "white": [255, 255, 255],
                "black": [0, 0, 0],
                "gray": [128, 128, 128],
                "grey": [128, 128, 128],
                "orange": [255, 165, 0],
            }
            self.default = _COLORS.get(default.lower(), [255, 0, 0])
        else:
            self.default = [255, 0, 0]

class _InfoField(_Field):
    def __init__(self, text):
        self.text = str(text)

class _FieldsField(_Field):
    def __init__(self, title=None, controls=None, id=None, collapsed=False, raw_items=None):
        self.title = str(title) if title is not None else None
        self.controls = controls or []
        self.id = id
        self.collapsed = bool(collapsed)
        self.raw_items = raw_items or []

class _TableField(_Field):
    def __init__(self, headers, rows=None, id=None, label=None, allow_add=True, allow_delete=True):
        self.headers = [str(h) for h in headers]
        self.rows = [[str(c) for c in r] for r in (rows or [])]
        self.id = str(id) if id else None
        self.label = str(label) if label else None
        self.allow_add = bool(allow_add)
        self.allow_delete = bool(allow_delete)

class _ButtonField(_Field):
    def __init__(self, label, on_click=None, id=None):
        self.label = str(label)
        self.on_click = on_click
        self.id = str(id) if id else re.sub(r'[^a-zA-Z0-9_]+', '_', self.label.lower()).strip('_')

def button(label, on_click=None, id=None):
    """Action button control for panels and forms."""
    return _ButtonField(label, on_click=on_click, id=id)

def text(default="", label=None):
    """Field helper for a text input field."""
    return _TextField(default, label)

def checkbox(default=False, label=None):
    """Field helper for a boolean checkbox toggle."""
    return _CheckboxField(default, label)

def number(default=0.0, min=None, max=None, step=None, label=None):
    """Field helper for a numeric input with optional min, max, step bounds."""
    return _NumberField(default, min, max, step, label)

def choice(options, default=None, label=None):
    """Field helper for a dropdown selection pick list."""
    return _ChoiceField(options, default, label)

def file(label=None, extensions=None, save=False, default=""):
    """Field helper for an OS file picker input."""
    return _FileField(label, extensions, save, default)

def color(default="red", label=None):
    """Field helper for a color picker input."""
    return _ColorField(default, label)

def info_text(text):
    """Informational static text line in a dialog."""
    return _InfoField(text)

def fields(title=None, *args, collapsed=False, **kwargs):
    """
    Grouped property fields section with aligned 2-column key-value layout.
    Supports collapsible toggle.
    Usage:
        oc.fields("Dimensions", length=100.0, width=50.0)
        oc.fields("Parameters", {"thickness": 2.5, "material": "Steel"}, collapsed=True)
        oc.fields(title="Geometry", radius=oc.number(15.0, min=0.0))
    """
    ctrls = []
    raw_items = []
    sec_title = None
    dict_data = {}

    if isinstance(title, str):
        sec_title = title
    elif isinstance(title, dict):
        dict_data.update(title)
    elif title is not None:
        args = (title,) + args

    for a in args:
        if isinstance(a, dict):
            dict_data.update(a)
        elif isinstance(a, tuple) and len(a) == 2:
            ctrls.append(_build_control(a[0], a[1]))
            raw_items.append((a[0], a[1]))
        elif isinstance(a, _Field):
            name = (getattr(a, "label", None) or getattr(a, "id", None) or f"field_{len(ctrls)+1}").lower().replace(" ", "_")
            ctrls.append(_build_control(name, a))
            raw_items.append((name, a))

    for k, v in dict_data.items():
        ctrls.append(_build_control(k, v))
        raw_items.append((k, v))
    for k, v in kwargs.items():
        ctrls.append(_build_control(k, v))
        raw_items.append((k, v))

    if sec_title:
        id_name = re.sub(r'[^a-zA-Z0-9_]+', '_', sec_title.lower()).strip('_')
    else:
        id_name = "fields"
    return _FieldsField(sec_title, ctrls, id=id_name, collapsed=collapsed, raw_items=raw_items)

def table(headers, rows=None, id=None, label=None, allow_add=True, allow_delete=True):
    """
    Interactive editable tabular grid control with column headers and editable cells.
    Returns a 2D list of strings containing updated rows upon dialog submission.
    Usage:
        oc.table(["Item", "Qty", "Material"], [["Bolt", "4", "Steel"], ["Nut", "4", "Steel"]])
    """
    return _TableField(headers, rows, id, label, allow_add, allow_delete)

def row(*args, **kwargs):
    """Place multiple controls side-by-side in a horizontal row."""
    controls = []
    raw_items = []
    for a in args:
        if isinstance(a, _Row):
            controls.extend(a.controls)
            raw_items.extend(getattr(a, "raw_items", []))
        elif isinstance(a, _Field):
            name = (getattr(a, "id", None) or getattr(a, "label", None) or f"field_{len(controls)+1}").lower().replace(" ", "_")
            controls.append(a)
            raw_items.append((name, a))
        elif isinstance(a, tuple) and len(a) == 2:
            controls.append(a[1])
            raw_items.append((a[0], a[1]))
        elif isinstance(a, dict):
            controls.append(a)
    for k, v in kwargs.items():
        if isinstance(v, _Field):
            if not getattr(v, "id", None):
                v.id = k
            controls.append(v)
        else:
            controls.append(v)
        raw_items.append((k, v))
    return _Row(controls, raw_items)

def _build_control(name, val):
    label_default = name.replace("_", " ").title()
    if isinstance(val, dict):
        if any(k in val for k in ("Text", "Number", "Checkbox", "Choice", "Color", "FilePicker", "Fields", "Table", "Button", "InfoText", "Row")):
            return val
    if isinstance(val, _Row):
        row_ctrls = []
        for i, c in enumerate(val.controls):
            if isinstance(c, dict):
                row_ctrls.append(c)
            elif isinstance(c, _Field):
                c_name = (getattr(c, "id", None) or getattr(c, "label", None) or f"ctrl_{i+1}").lower().replace(" ", "_")
                row_ctrls.append(_build_control(c_name, c))
            else:
                row_ctrls.append(_build_control(f"ctrl_{i+1}", c))
        return {
            "Row": row_ctrls
        }
    elif isinstance(val, _ButtonField):
        action_val = None
        if isinstance(val.on_click, str):
            action_val = val.on_click
        elif hasattr(val, "_panel_cmd"):
            action_val = val._panel_cmd
        return {
            "Button": {
                "id": val.id or name,
                "label": val.label,
                "action": action_val,
            }
        }
    elif isinstance(val, _InfoField):
        return {"InfoText": {"text": val.text}}
    elif isinstance(val, _TextField):
        return {
            "Text": {
                "id": name,
                "label": val.label or label_default,
                "default": val.default,
            }
        }
    elif isinstance(val, _CheckboxField):
        return {
            "Checkbox": {
                "id": name,
                "label": val.label or label_default,
                "default": val.default,
            }
        }
    elif isinstance(val, _NumberField):
        return {
            "Number": {
                "id": name,
                "label": val.label or label_default,
                "default": val.default,
                "min": val.min,
                "max": val.max,
                "step": val.step,
            }
        }
    elif isinstance(val, _ChoiceField):
        return {
            "Choice": {
                "id": name,
                "label": val.label or label_default,
                "options": val.options,
                "default_index": val.default_index,
            }
        }
    elif isinstance(val, _FieldsField):
        return {
            "Fields": {
                "id": val.id or name,
                "title": val.title,
                "controls": val.controls,
                "collapsed": val.collapsed,
            }
        }
    elif isinstance(val, _TableField):
        return {
            "Table": {
                "id": val.id or name,
                "label": val.label,
                "headers": val.headers,
                "rows": val.rows,
                "allow_add": val.allow_add,
                "allow_delete": val.allow_delete,
            }
        }
    elif isinstance(val, _FileField):
        return {
            "FilePicker": {
                "id": name,
                "label": val.label or label_default,
                "default_path": val.default,
                "filter_name": "Files",
                "extensions": val.extensions,
                "save": val.save,
            }
        }
    elif isinstance(val, _ColorField):
        return {
            "Color": {
                "id": name,
                "label": val.label or label_default,
                "default": val.default,
            }
        }
    elif isinstance(val, bool):
        return {
            "Checkbox": {
                "id": name,
                "label": label_default,
                "default": val,
            }
        }
    elif isinstance(val, (int, float)):
        return {
            "Number": {
                "id": name,
                "label": label_default,
                "default": float(val),
                "min": None,
                "max": None,
                "step": None,
            }
        }
    elif isinstance(val, (list, tuple)):
        opts = [str(x) for x in val]
        return {
            "Choice": {
                "id": name,
                "label": label_default,
                "options": opts,
                "default_index": 0,
            }
        }
    else:
        return {
            "Text": {
                "id": name,
                "label": label_default,
                "default": str(val) if val is not None else "",
            }
        }

def dialog(title="Input", *args, submit_label="OK", cancel_label="Cancel", width=None, **kwargs):
    """
    Display a native declarative modal dialog and return an AttrDict of entered values,
    or None if the user cancelled.
    """
    rows = []
    for a in args:
        if isinstance(a, _Row):
            row_ctrls = []
            for j, x in enumerate(a.controls):
                if isinstance(x, dict):
                    row_ctrls.append(x)
                elif isinstance(x, _Field):
                    cid = getattr(x, "id", None) or getattr(x, "label", None) or f"ctrl_{j+1}"
                    row_ctrls.append(_build_control(cid, x))
                else:
                    row_ctrls.append(_build_control(f"ctrl_{j+1}", x))
            rows.append({"controls": row_ctrls})
        elif isinstance(a, _FieldsField):
            ctrl = _build_control(a.id or "fields", a)
            rows.append({"controls": [ctrl]})
        elif isinstance(a, _TableField):
            ctrl = _build_control(a.id or "table", a)
            rows.append({"controls": [ctrl]})
        elif isinstance(a, _Field):
            name = (getattr(a, "label", None) or f"field_{len(rows)+1}").lower().replace(" ", "_")
            rows.append({"controls": [_build_control(name, a)]})
    for k, v in kwargs.items():
        if isinstance(v, _Row):
            row_ctrls = []
            for j, x in enumerate(v.controls):
                if isinstance(x, dict):
                    row_ctrls.append(x)
                elif isinstance(x, _Field):
                    cid = getattr(x, "id", None) or getattr(x, "label", None) or f"ctrl_{j+1}"
                    row_ctrls.append(_build_control(cid, x))
                else:
                    row_ctrls.append(_build_control(f"ctrl_{j+1}", x))
            rows.append({"controls": row_ctrls})
        else:
            rows.append({"controls": [_build_control(k, v)]})

    def_obj = {
        "title": str(title),
        "rows": rows,
        "submit_label": str(submit_label),
        "cancel_label": str(cancel_label),
    }
    if width is not None:
        def_obj["width"] = float(width)

    raw_json = json.dumps(def_obj)
    resp_json = _show_dialog_raw(raw_json)
    resp = json.loads(resp_json)

    if "Submitted" in resp:
        values = {}
        for k, v in resp["Submitted"].items():
            if isinstance(v, dict):
                if "Text" in v:
                    values[k] = v["Text"]
                elif "Number" in v:
                    values[k] = v["Number"]
                elif "Bool" in v:
                    values[k] = v["Bool"]
                elif "Choice" in v:
                    values[k] = v["Choice"]["text"]
                    values[f"{k}_index"] = v["Choice"]["index"]
                elif "Color" in v:
                    values[k] = tuple(v["Color"]) if isinstance(v["Color"], (list, tuple)) else v["Color"]
                elif "FilePath" in v:
                    values[k] = v["FilePath"]
                elif "Table" in v:
                    values[k] = v["Table"]
                elif "Fields" in v:
                    field_dict = {}
                    for fk, fv in v["Fields"].items():
                        if isinstance(fv, dict):
                            if "Text" in fv:
                                field_dict[fk] = fv["Text"]
                            elif "Number" in fv:
                                field_dict[fk] = fv["Number"]
                            elif "Bool" in fv:
                                field_dict[fk] = fv["Bool"]
                            elif "Choice" in fv:
                                field_dict[fk] = fv["Choice"]["text"]
                                field_dict[f"{fk}_index"] = fv["Choice"]["index"]
                            elif "Color" in fv:
                                field_dict[fk] = tuple(fv["Color"]) if isinstance(fv["Color"], (list, tuple)) else fv["Color"]
                            elif "FilePath" in fv:
                                field_dict[fk] = fv["FilePath"]
                            elif "Table" in fv:
                                field_dict[fk] = fv["Table"]
                            else:
                                field_dict[fk] = fv
                        else:
                            field_dict[fk] = fv
                    values[k] = AttrDict(field_dict)
                    clean_k = re.sub(r'[^a-zA-Z0-9_]+', '_', k.lower()).strip('_')
                    if clean_k and clean_k not in values:
                        values[clean_k] = values[k]
                    for fk, fval in field_dict.items():
                        if fk not in values:
                            values[fk] = fval
                else:
                    values[k] = v
            else:
                values[k] = v
        return AttrDict(values)
    else:
        return None

def alert(message, title="Notice"):
    """Display an informational modal notice with an OK button."""
    return dialog(title, info=info_text(message), submit_label="OK", cancel_label="")

def confirm(question, title="Confirm"):
    """Display a confirmation modal dialog with Yes/No buttons, returning True or False."""
    res = dialog(title, question=info_text(question), submit_label="Yes", cancel_label="No")
    return res is not None

class TableProxy:
    def __init__(self, panel, table_id, headers, initial_rows=None):
        self._panel = panel
        self._table_id = table_id
        self._headers = list(headers)
        self._rows = [[str(c) for c in r] for r in (initial_rows or [])]

    @property
    def headers(self):
        return self._headers

    @property
    def rows(self):
        return self._rows

    def __len__(self):
        return len(self._rows)

    def __iter__(self):
        return iter(self._rows)

    def __getitem__(self, idx):
        return self._rows[idx]

    def __setitem__(self, idx, row):
        self._rows[idx] = [str(c) for c in row]
        self._sync()

    def add_row(self, row):
        """Append a new row to the table and sync to host in real time."""
        self._rows.append([str(c) for c in row])
        self._sync()

    def remove_row(self, idx):
        """Remove row at index and sync to host in real time."""
        if 0 <= idx < len(self._rows):
            del self._rows[idx]
            self._sync()

    def clear(self):
        """Clear all rows and sync to host in real time."""
        self._rows.clear()
        self._sync()

    def _sync(self):
        self._panel._sync_table(self._table_id, self._rows)

    def __repr__(self):
        return f"TableProxy({self._table_id}, rows={self._rows})"

class PanelProxy:
    def __init__(self, panel_id, title=None, width=None):
        self._panel_id = panel_id
        self._title = title or panel_id.replace("_", " ").title()
        self._width = width
        self._controls = []
        self._values = {}
        self._tables = {}
        self._callbacks = {}
        self._choices = {}

    def add(self, *args, **kwargs):
        for a in args:
            if isinstance(a, _Row):
                self._controls.append(a)
                for sub in getattr(a, "raw_items", []):
                    self._register_ctrl(sub[0], sub[1])
            elif isinstance(a, _FieldsField):
                self._controls.append(a)
                self._register_ctrl(a.id or "fields", a)
            elif isinstance(a, dict):
                for k, v in a.items():
                    self._register_ctrl(k, v)
                    self._controls.append(v if isinstance(v, _Field) else _build_control(k, v))
            elif isinstance(a, _Field):
                cid = getattr(a, "id", None) or getattr(a, "label", None) or f"ctrl_{len(self._controls)+1}"
                self._register_ctrl(cid, a)
                self._controls.append(a)
            elif isinstance(a, tuple) and len(a) == 2:
                self._register_ctrl(a[0], a[1])
                self._controls.append(_build_control(a[0], a[1]))
            else:
                self._controls.append(a)

        for k, v in kwargs.items():
            self._register_ctrl(k, v)
            if isinstance(v, _Field):
                if not getattr(v, "id", None):
                    v.id = k
                self._controls.append(v)
            else:
                self._controls.append(_build_control(k, v))
        return self

    def _register_ctrl(self, key, item):
        clean_k = re.sub(r'[^a-zA-Z0-9_]+', '_', (key or "").lower()).strip('_')
        if isinstance(item, _ButtonField):
            btn_id = item.id or clean_k
            if callable(item.on_click):
                self._callbacks[btn_id] = item.on_click
                if clean_k and clean_k != btn_id:
                    self._callbacks[clean_k] = item.on_click
        elif isinstance(item, _TableField):
            tid = item.id or clean_k
            tp = TableProxy(self, tid, item.headers, item.rows)
            self._tables[tid] = tp
            self._values[tid] = tp
            if clean_k and clean_k != tid:
                self._tables[clean_k] = tp
                self._values[clean_k] = tp
        elif isinstance(item, _ChoiceField):
            self._choices[key] = list(item.options)
            self._choices[clean_k] = list(item.options)
            val = item.options[item.default_index] if (0 <= item.default_index < len(item.options)) else (item.options[0] if item.options else "")
            self._values[key] = val
            if clean_k and clean_k != key:
                self._values[clean_k] = val
        elif isinstance(item, _FieldsField):
            fid = item.id or clean_k
            fdict = AttrDict()
            for sub_k, sub_v in getattr(item, "raw_items", []):
                sub_clean = re.sub(r'[^a-zA-Z0-9_]+', '_', (sub_k or "").lower()).strip('_')
                if isinstance(sub_v, _ChoiceField):
                    self._choices[sub_k] = list(sub_v.options)
                    self._choices[sub_clean] = list(sub_v.options)
                    val = sub_v.options[sub_v.default_index] if (0 <= sub_v.default_index < len(sub_v.options)) else (sub_v.options[0] if sub_v.options else "")
                else:
                    val = getattr(sub_v, "default", sub_v)
                fdict[sub_clean] = val
                if sub_clean not in self._values:
                    self._values[sub_clean] = val
            self._values[fid] = fdict
            if clean_k and clean_k != fid:
                self._values[clean_k] = fdict
        elif isinstance(item, _Field):
            val = getattr(item, "default", "")
            if clean_k:
                self._values[clean_k] = val
        else:
            if clean_k:
                self._values[clean_k] = item

    def _sync_table(self, table_id, rows):
        _update_panel_raw(self._panel_id, json.dumps({ table_id: { "Table": rows } }))

    def _sync_value(self, key, value):
        payload = {}
        if isinstance(value, bool):
            payload[key] = {"Bool": value}
        elif isinstance(value, (int, float)):
            payload[key] = {"Number": float(value)}
        elif isinstance(value, str):
            payload[key] = {"Text": value}
        elif isinstance(value, (list, tuple)):
            if len(value) == 3 and all(isinstance(x, int) for x in value):
                payload[key] = {"Color": list(value)}
            else:
                payload[key] = {"Table": [[str(c) for c in r] if isinstance(r, (list, tuple)) else [str(r)] for r in value]}
        else:
            payload[key] = {"Text": str(value)}
        _update_panel_raw(self._panel_id, json.dumps(payload))

    def __getattr__(self, name):
        if name in self._tables:
            return self._tables[name]
        clean_name = re.sub(r'[^a-zA-Z0-9_]+', '_', name.lower()).strip('_')
        if clean_name in self._tables:
            return self._tables[clean_name]
        if name in self._values:
            return self._values[name]
        if clean_name in self._values:
            return self._values[clean_name]
        raise AttributeError(f"'PanelProxy' object has no attribute '{name}'")

    def __setattr__(self, name, value):
        if name.startswith("_"):
            super().__setattr__(name, value)
            return
        if name in self._tables:
            if isinstance(value, (list, tuple)):
                self._tables[name].clear()
                for r in value:
                    self._tables[name].add_row(r)
            return
        clean_name = re.sub(r'[^a-zA-Z0-9_]+', '_', name.lower()).strip('_')
        if clean_name in self._tables:
            if isinstance(value, (list, tuple)):
                self._tables[clean_name].clear()
                for r in value:
                    self._tables[clean_name].add_row(r)
            return

        self._values[name] = value
        self._values[clean_name] = value
        self._sync_value(name, value)

    def close(self):
        """Hide or close the panel in the dock."""
        _toggle_panel_raw(self._panel_id, False)

    def open(self):
        """Open or show the panel in the dock."""
        _toggle_panel_raw(self._panel_id, True)

    def toggle(self):
        """Toggle visibility of the panel in the dock."""
        _toggle_panel_raw(self._panel_id, None)

_PANEL_REGISTRY = {}

def panel(title_or_fn=None, command=None, width=None):
    """
    Decorator declaring a dockable side panel / palette.
    Approach 2: The entire panel is encapsulated within a function.
    Usage:
        @oc.panel("Sample Tool", command="SAMPLETOOL", width=320)
        def sample_tool(panel):
            panel.add(
                oc.button("Pick Points", on_click=on_pick),
                results=oc.table(["Pt", "N", "E"]),
            )
    """
    def decorator(fn):
        panel_id = fn.__name__
        title = title_or_fn if isinstance(title_or_fn, str) else panel_id.replace("_", " ").title()

        p = PanelProxy(panel_id, title=title, width=width)
        _PANEL_REGISTRY[panel_id] = p

        res = fn(p)
        if isinstance(res, (list, tuple)):
            p.add(*res)
        elif isinstance(res, dict):
            p.add(**res)

        def _register_button_cmd(b):
            if isinstance(b, _ButtonField) and callable(b.on_click):
                btn_cmd = f"_PANEL_ACT_{panel_id}_{b.id}".upper()
                cb = b.on_click
                def _btn_cmd_wrapper(*args):
                    import inspect
                    sig = inspect.signature(cb)
                    if len(sig.parameters) == 0:
                        return cb()
                    else:
                        return cb(p)
                _register_command(_btn_cmd_wrapper, name=btn_cmd, description=f"Action for {title} -> {b.label}")
                b._panel_cmd = btn_cmd

        ctrls = []
        for i, c in enumerate(p._controls):
            if isinstance(c, _Row):
                row_ctrls = []
                for j, x in enumerate(c.controls):
                    if isinstance(x, dict):
                        row_ctrls.append(x)
                    elif isinstance(x, _Field):
                        _register_button_cmd(x)
                        cid = getattr(x, "id", None) or getattr(x, "label", None) or f"ctrl_{j+1}"
                        row_ctrls.append(_build_control(cid, x))
                    else:
                        row_ctrls.append(_build_control(f"ctrl_{j+1}", x))
                ctrls.append({"Row": row_ctrls})
            elif isinstance(c, _Field):
                _register_button_cmd(c)
                cid = getattr(c, "id", None) or getattr(c, "label", None) or f"ctrl_{i+1}"
                ctrls.append(_build_control(cid, c))
            elif isinstance(c, dict):
                ctrls.append(c)
            else:
                ctrls.append(_build_control(f"ctrl_{i+1}", c))

        cmd_name = (command or panel_id).upper()
        def_obj = {
            "id": panel_id,
            "title": title,
            "controls": ctrls,
            "command": cmd_name,
        }
        if width is not None:
            def_obj["width"] = float(width)

        _register_panel_raw(json.dumps(def_obj))

        def toggle_cmd(*args):
            _toggle_panel_raw(panel_id, None)
            info(f"{title} palette toggled.")
        _register_command(toggle_cmd, name=cmd_name, description=f"Toggle {title} panel")

        return fn

    if callable(title_or_fn):
        fn = title_or_fn
        title_or_fn = None
        return decorator(fn)
    return decorator

import threading

def _dispatch_panel_action(panel_id, control_id, action, val_json=None):
    p = _PANEL_REGISTRY.get(panel_id)
    if not p:
        return

    clean_cid = re.sub(r'[^a-zA-Z0-9_]+', '_', (control_id or "").lower()).strip('_')

    # 1. Handle Table interactions first so TableProxy is never overwritten by scalar values
    if control_id in p._tables or clean_cid in p._tables:
        tp = p._tables.get(control_id) or p._tables.get(clean_cid)
        if val_json:
            try:
                val_obj = json.loads(val_json)
                if isinstance(val_obj, dict):
                    if "Table" in val_obj:
                        tp._rows = [list(r) for r in val_obj["Table"]]
                    elif "Number" in val_obj and action == "remove_row":
                        idx = int(val_obj["Number"])
                        if 0 <= idx < len(tp._rows):
                            del tp._rows[idx]
            except Exception:
                pass
        if action == "add_row":
            tp._rows.append(["" for _ in range(len(tp.headers))])
        elif action == "clear":
            tp._rows.clear()
        p._values[control_id] = tp
        p._values[clean_cid] = tp
        return

    # 2. General control value updates
    if val_json:
        try:
            val_obj = json.loads(val_json)
            parsed_val = None
            if isinstance(val_obj, dict):
                if "Text" in val_obj:
                    parsed_val = val_obj["Text"]
                elif "Number" in val_obj:
                    parsed_val = val_obj["Number"]
                elif "Bool" in val_obj:
                    parsed_val = val_obj["Bool"]
                elif "Choice" in val_obj:
                    c_data = val_obj["Choice"]
                    c_idx = c_data.get("index", 0) if isinstance(c_data, dict) else 0
                    c_txt = c_data.get("text", "") if isinstance(c_data, dict) else ""
                    if not c_txt:
                        opts = p._choices.get(control_id) or p._choices.get(clean_cid) or []
                        if 0 <= c_idx < len(opts):
                            c_txt = opts[c_idx]
                    parsed_val = c_txt
                    p._values[f"{control_id}_index"] = c_idx
                    p._values[f"{clean_cid}_index"] = c_idx
                elif "Color" in val_obj:
                    parsed_val = tuple(val_obj["Color"])
                elif "FilePath" in val_obj:
                    parsed_val = val_obj["FilePath"]
            if parsed_val is not None:
                p._values[control_id] = parsed_val
                p._values[clean_cid] = parsed_val
        except Exception:
            pass

    if action == "click":
        cb = p._callbacks.get(control_id)
        if not cb:
            clean_cid = re.sub(r'[^a-zA-Z0-9_]+', '_', control_id.lower()).strip('_')
            cb = p._callbacks.get(clean_cid)
        if not cb:
            for k, v in p._callbacks.items():
                if k.lower() == control_id.lower() or k.replace("_", "") == control_id.replace("_", ""):
                    cb = v
                    break
        if cb:
            def _runner():
                try:
                    import inspect
                    sig = inspect.signature(cb)
                    if len(sig.parameters) == 0:
                        cb()
                    else:
                        cb(p)
                except Exception as e:
                    import traceback
                    error(f"Panel Callback Error: {e}\n{traceback.format_exc()}")
            threading.Thread(target=_runner, daemon=True).start()
"#;
    py.run(&std::ffi::CString::new(py_code).unwrap(), Some(&m.dict()), Some(&m.dict()))?;

    // Add uppercase aliases for CAD compatibility (e.g. COMMAND, ADD_CIRCLE, etc.)
    let cmd_fn = m.getattr("command")?;
    m.add("COMMAND", &cmd_fn)?;
    let dlg_fn = m.getattr("dialog")?;
    m.add("DIALOG", &dlg_fn)?;
    let alert_fn = m.getattr("alert")?;
    m.add("ALERT", &alert_fn)?;
    let confirm_fn = m.getattr("confirm")?;
    m.add("CONFIRM", &confirm_fn)?;
    let row_fn = m.getattr("row")?;
    m.add("ROW", &row_fn)?;
    let cb_fn = m.getattr("checkbox")?;
    m.add("CHECKBOX", &cb_fn)?;
    let fields_fn = m.getattr("fields")?;
    m.add("FIELDS", &fields_fn)?;
    let table_fn = m.getattr("table")?;
    m.add("TABLE", &table_fn)?;
    let btn_fn = m.getattr("button")?;
    m.add("BUTTON", &btn_fn)?;
    let pnl_fn = m.getattr("panel")?;
    m.add("PANEL", &pnl_fn)?;
    m.add("CMD", wrap_pyfunction!(cmd, m)?)?;
    m.add("EXECUTE_COMMAND", wrap_pyfunction!(execute_command, m)?)?;
    m.add("ENTER", wrap_pyfunction!(enter, m)?)?;
    m.add("ADD_POINT", wrap_pyfunction!(add_point, m)?)?;
    m.add("ADD_LINE", wrap_pyfunction!(add_line, m)?)?;
    m.add("ADD_CIRCLE", wrap_pyfunction!(add_circle, m)?)?;
    m.add("ADD_ARC", wrap_pyfunction!(add_arc, m)?)?;
    m.add("ADD_ELLIPSE", wrap_pyfunction!(add_ellipse, m)?)?;
    m.add("ADD_POLYLINE", wrap_pyfunction!(add_polyline, m)?)?;
    m.add("ADD_TEXT", wrap_pyfunction!(add_text, m)?)?;
    m.add("ADD_MTEXT", wrap_pyfunction!(add_mtext, m)?)?;
    m.add("ADD_RAY", wrap_pyfunction!(add_ray, m)?)?;
    m.add("ADD_XLINE", wrap_pyfunction!(add_xline, m)?)?;
    m.add("ADD_SOLID", wrap_pyfunction!(add_solid, m)?)?;
    m.add("ADD_BOX", wrap_pyfunction!(add_box, m)?)?;
    m.add("ADD_WEDGE", wrap_pyfunction!(add_wedge, m)?)?;
    m.add("ADD_CYLINDER", wrap_pyfunction!(add_cylinder, m)?)?;
    m.add("ADD_SPHERE", wrap_pyfunction!(add_sphere, m)?)?;
    m.add("ADD_TORUS", wrap_pyfunction!(add_torus, m)?)?;
    m.add("ADD_PYRAMID", wrap_pyfunction!(add_pyramid, m)?)?;
    m.add("SOLID_UNION", wrap_pyfunction!(solid_union, m)?)?;
    m.add("SOLID_SUBTRACT", wrap_pyfunction!(solid_subtract, m)?)?;
    m.add("SOLID_INTERSECT", wrap_pyfunction!(solid_intersect, m)?)?;
    m.add("REMOVE_ENTITY", wrap_pyfunction!(remove_entity, m)?)?;
    m.add("ADD_LAYER", wrap_pyfunction!(add_layer, m)?)?;
    m.add("RENAME_LAYER", wrap_pyfunction!(rename_layer, m)?)?;
    m.add("DELETE_LAYER", wrap_pyfunction!(delete_layer, m)?)?;
    m.add("ADD_TEXT_STYLE", wrap_pyfunction!(add_text_style, m)?)?;
    m.add("DEFINE_BLOCK", wrap_pyfunction!(define_block, m)?)?;
    m.add("INSERT_BLOCK", wrap_pyfunction!(insert_block, m)?)?;
    m.add("SET_LAYER_COLOR", wrap_pyfunction!(set_layer_color, m)?)?;
    m.add("REGISTER_COMMAND", wrap_pyfunction!(register_command, m)?)?;
    m.add("ADD_ALIAS", wrap_pyfunction!(register_command, m)?)?;
    m.add("RUN_SCRIPT", wrap_pyfunction!(run_script, m)?)?;
    m.add("GET_LAYERS", wrap_pyfunction!(get_layers, m)?)?;
    m.add("GET_LAYER", wrap_pyfunction!(get_layer, m)?)?;
    m.add("MODIFY_LAYER", wrap_pyfunction!(modify_layer, m)?)?;
    m.add("SET_LAYER", wrap_pyfunction!(modify_layer, m)?)?;
    m.add("GET_CURRENT_LAYER", wrap_pyfunction!(get_current_layer, m)?)?;
    m.add("SET_CURRENT_LAYER", wrap_pyfunction!(set_current_layer, m)?)?;
    m.add("ENTITY_COUNT", wrap_pyfunction!(entity_count, m)?)?;
    m.add("GET_ENTITIES", wrap_pyfunction!(get_entities, m)?)?;
    m.add("GET_ENTITY_DATA", wrap_pyfunction!(get_entity_data, m)?)?;
    m.add("ENTGET", wrap_pyfunction!(get_entity_data, m)?)?;
    m.add("GET_POINT", wrap_pyfunction!(get_point, m)?)?;
    m.add("SET_RUBBER_BAND", wrap_pyfunction!(set_rubber_band, m)?)?;
    m.add("GET_ENTITY", wrap_pyfunction!(get_entity, m)?)?;
    m.add("GET_SELECTION", wrap_pyfunction!(get_selection, m)?)?;
    m.add("SET_SELECTION", wrap_pyfunction!(set_selection, m)?)?;
    m.add("GET_PRESELECTION", wrap_pyfunction!(get_preselection, m)?)?;
    m.add("GET_VAR", wrap_pyfunction!(get_var, m)?)?;
    m.add("SET_VAR", wrap_pyfunction!(set_var, m)?)?;
    m.add("IS_SANDBOXED", wrap_pyfunction!(is_sandboxed, m)?)?;
    m.add("GET_SECURITY_STATUS", wrap_pyfunction!(get_security_status, m)?)?;
    m.add("TRUST_DIRECTORY", wrap_pyfunction!(trust_directory, m)?)?;
    m.add("_debug_check_line", wrap_pyfunction!(crate::py_debug::_debug_check_line, m)?)?;
    m.add("_debug_check_exception", wrap_pyfunction!(crate::py_debug::_debug_check_exception, m)?)?;
    Ok(())
}
