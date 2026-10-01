//! One explicit coordinate destination; source refs never imply a drop target.
pub fn destination(parameters: &serde_json::Value) -> Result<(f64, f64), String> {
    let params = parameters
        .as_object()
        .ok_or("drag parameters must be an object")?;
    let canonical = params.contains_key("toX") || params.contains_key("toY");
    let legacy = params.contains_key("x1") || params.contains_key("y1");
    if canonical && legacy {
        return Err("drag destination aliases cannot be mixed".into());
    }
    let (x_key, y_key) = if canonical {
        ("toX", "toY")
    } else {
        ("x1", "y1")
    };
    let x = params.get(x_key).and_then(serde_json::Value::as_f64);
    let y = params.get(y_key).and_then(serde_json::Value::as_f64);
    let (Some(x), Some(y)) = (x, y) else {
        return Err("drag requires a complete numeric destination pair".into());
    };
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return Err("invalid drag destination".into());
    }
    Ok((x, y))
}
