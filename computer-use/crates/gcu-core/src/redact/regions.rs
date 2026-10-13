use super::{RedactionError, REGION_PADDING};
use crate::model::Rect;

pub(super) fn bounds(
    rect: Rect,
    width: u32,
    height: u32,
    padding: f64,
) -> Result<(u32, u32, u32, u32), RedactionError> {
    if ![
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        rect.x + rect.w,
        rect.y + rect.h,
    ]
    .iter()
    .all(|v| v.is_finite())
        || rect.w <= 0.
        || rect.h <= 0.
        || rect.x >= f64::from(width)
        || rect.y >= f64::from(height)
        || rect.x + rect.w <= 0.
        || rect.y + rect.h <= 0.
    {
        return Err(RedactionError::OutOfBounds);
    }
    Ok((
        (rect.x - padding).floor().max(0.) as u32,
        (rect.y - padding).floor().max(0.) as u32,
        (rect.x + rect.w + padding).ceil().min(f64::from(width)) as u32,
        (rect.y + rect.h + padding).ceil().min(f64::from(height)) as u32,
    ))
}
pub(super) fn sensitive(
    rect: Option<Rect>,
    width: u32,
    height: u32,
) -> Result<(u32, u32, u32, u32), RedactionError> {
    rect.ok_or(RedactionError::NeedsHandoff).and_then(|rect| {
        bounds(rect, width, height, REGION_PADDING).map_err(|_| RedactionError::NeedsHandoff)
    })
}
