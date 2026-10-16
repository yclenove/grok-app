//! Window capture via XComposite and fallback root window cropping.

use crate::x11::{XError, XServer, XWindowGeometry};
use gcu_core::{
    driver::{Capture, DriverError},
    model::{Geometry, Rect},
};
use std::collections::HashMap;

/// Window visibility assessment relative to screen boundaries and stacking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowVisibility {
    /// Whether the window is completely within screen bounds.
    pub is_fully_on_screen: bool,
    /// Whether any mapped window above this window overlaps its area.
    pub is_occluded: bool,
}

/// Evaluates visibility and occlusion of a window.
pub fn check_window_visibility(
    target_geom: &XWindowGeometry,
    screen_w: u32,
    screen_h: u32,
    stacking_order: &[u32],
    target_id: u32,
    window_geometries: &HashMap<u32, (XWindowGeometry, bool)>,
) -> WindowVisibility {
    let is_fully_on_screen = target_geom.x >= 0
        && target_geom.y >= 0
        && (target_geom.x + target_geom.width as i32) <= screen_w as i32
        && (target_geom.y + target_geom.height as i32) <= screen_h as i32;

    let target_idx = stacking_order.iter().position(|&id| id == target_id);
    let mut is_occluded = false;

    if let Some(idx) = target_idx {
        let t_x0 = target_geom.x;
        let t_y0 = target_geom.y;
        let t_x1 = target_geom.x + target_geom.width as i32;
        let t_y1 = target_geom.y + target_geom.height as i32;

        for &above_id in &stacking_order[idx + 1..] {
            if let Some((above_geom, is_mapped)) = window_geometries.get(&above_id) {
                if !is_mapped {
                    continue;
                }
                let a_x0 = above_geom.x;
                let a_y0 = above_geom.y;
                let a_x1 = above_geom.x + above_geom.width as i32;
                let a_y1 = above_geom.y + above_geom.height as i32;

                if t_x0 < a_x1 && t_x1 > a_x0 && t_y0 < a_y1 && t_y1 > a_y0 {
                    is_occluded = true;
                    break;
                }
            }
        }
    }

    WindowVisibility {
        is_fully_on_screen,
        is_occluded,
    }
}

/// Strategy for window capture based on extension availability and visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStrategy {
    /// Capture via XComposite named window pixmap (independent of occlusion).
    Composite,
    /// Window is fully visible and top-most; safe to crop from root window.
    RootCrop,
    /// No composite and window is occluded or partially off-screen; unsafe to deliver root contents.
    OccludedUnsafe,
}

/// Determines the safe capture strategy according to architecture §4.4.
pub fn decide_capture_strategy(
    composite_available: bool,
    visibility: WindowVisibility,
) -> CaptureStrategy {
    if composite_available {
        CaptureStrategy::Composite
    } else if visibility.is_fully_on_screen && !visibility.is_occluded {
        CaptureStrategy::RootCrop
    } else {
        CaptureStrategy::OccludedUnsafe
    }
}

/// Captures window pixels and encodes into a PNG `Capture`.
pub fn capture_window(x: &dyn XServer, window_id: u32) -> Result<Capture, DriverError> {
    let exclusion = x.overlay_capture_scope(window_id).map_err(|e| match e {
        XError::WindowNotFound(_) => DriverError::WindowGone,
        _ => DriverError::Native {
            message: format!("Overlay capture barrier failed: {e}"),
            maybe_executed: false,
        },
    })?;
    let geom = x.window_geometry(window_id).map_err(|e| match e {
        XError::WindowNotFound(_) => DriverError::WindowGone,
        _ => DriverError::Native {
            message: format!("Failed to read window geometry: {e}"),
            maybe_executed: false,
        },
    })?;

    if geom.width == 0 || geom.height == 0 {
        return Err(DriverError::Native {
            message: "Window has zero dimensions".into(),
            maybe_executed: false,
        });
    }

    let root = x.root_geometry().map_err(|e| DriverError::Native {
        message: format!("Failed to read root geometry: {e}"),
        maybe_executed: false,
    })?;

    let composite_avail = x.is_composite_available().unwrap_or(false);

    let raw_image = if composite_avail {
        match x.composite_name_window_pixmap(window_id) {
            Ok(pixmap) => {
                let img_res = x.get_image(pixmap, 0, 0, geom.width as u16, geom.height as u16);
                let _ = x.free_pixmap(pixmap);
                img_res.map_err(|e| DriverError::Native {
                    message: format!("Composite GetImage failed: {e}"),
                    maybe_executed: false,
                })?
            }
            Err(e) => {
                if !exclusion.allows_root_capture() {
                    return Err(DriverError::CapabilityUnavailable);
                }
                tracing::warn!(
                    error = %e,
                    window = window_id,
                    "composite_name_window_pixmap failed; evaluating root visibility fallback"
                );
                let stacking = x.query_tree(root.root_id).unwrap_or_default();
                let mut geoms = HashMap::new();
                for &w in &stacking {
                    if let Ok(g) = x.window_geometry(w) {
                        let mapped = x.is_window_mapped(w).unwrap_or(false);
                        geoms.insert(w, (g, mapped));
                    }
                }
                let vis = check_window_visibility(
                    &geom,
                    root.width,
                    root.height,
                    &stacking,
                    window_id,
                    &geoms,
                );
                if vis.is_fully_on_screen && !vis.is_occluded {
                    x.get_image(
                        root.root_id,
                        geom.x as i16,
                        geom.y as i16,
                        geom.width as u16,
                        geom.height as u16,
                    )
                    .map_err(|e| DriverError::Native {
                        message: format!("Root GetImage fallback failed: {e}"),
                        maybe_executed: false,
                    })?
                } else {
                    return Err(DriverError::CapabilityUnavailable);
                }
            }
        }
    } else {
        if !exclusion.allows_root_capture() {
            return Err(DriverError::CapabilityUnavailable);
        }
        let stacking = x.query_tree(root.root_id).unwrap_or_default();
        let mut geoms = HashMap::new();
        for &w in &stacking {
            if let Ok(g) = x.window_geometry(w) {
                let mapped = x.is_window_mapped(w).unwrap_or(false);
                geoms.insert(w, (g, mapped));
            }
        }
        let vis =
            check_window_visibility(&geom, root.width, root.height, &stacking, window_id, &geoms);
        let strategy = decide_capture_strategy(false, vis);
        match strategy {
            CaptureStrategy::RootCrop => x
                .get_image(
                    root.root_id,
                    geom.x as i16,
                    geom.y as i16,
                    geom.width as u16,
                    geom.height as u16,
                )
                .map_err(|e| DriverError::Native {
                    message: format!("Root GetImage failed: {e}"),
                    maybe_executed: false,
                })?,
            _ => return Err(DriverError::CapabilityUnavailable),
        }
    };

    // Release before PNG encoding; only the pixel read belongs to this scope.
    drop(exclusion);
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, geom.width, geom.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| DriverError::Native {
            message: format!("PNG header error: {e}"),
            maybe_executed: false,
        })?;
        writer
            .write_image_data(&raw_image.data)
            .map_err(|e| DriverError::Native {
                message: format!("PNG write image data error: {e}"),
                maybe_executed: false,
            })?;
        writer.finish().map_err(|e| DriverError::Native {
            message: format!("PNG finish error: {e}"),
            maybe_executed: false,
        })?;
    }

    Ok(Capture {
        bytes: png_bytes,
        width: geom.width,
        height: geom.height,
        mime: "image/png".into(),
        geometry: Geometry {
            frame: Rect {
                x: geom.x as f64,
                y: geom.y as f64,
                w: geom.width as f64,
                h: geom.height as f64,
            },
            scale: root.scale,
        },
    })
}
