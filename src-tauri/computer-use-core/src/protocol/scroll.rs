//! One wire convention on every surface: positive delta moves DOWN; negative
//! moves UP. Native positive-up APIs must invert at their boundary, not callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollDelta(i32);

impl ScrollDelta {
    pub fn parse(parameters: &serde_json::Value) -> Result<Self, String> {
        let params = parameters
            .as_object()
            .ok_or("scroll parameters must be an object")?;
        if params.len() != 1 {
            return Err("scroll requires only delta".into());
        }
        let delta = params
            .get("delta")
            .and_then(serde_json::Value::as_i64)
            .filter(|n| (-2400..=2400).contains(n))
            .ok_or("invalid integer scroll delta")?;
        Ok(Self(delta as i32))
    }

    /// Signed wire delta; positive is down. Native wheel steps and semantic
    /// pages are not promised to be pixel-exact distances across applications.
    pub fn value(self) -> i32 {
        self.0
    }

    /// Quartz wheel1 / Win32 wheel high word: native positive is up.
    pub fn native_positive_up(self) -> i32 {
        -self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_down_is_native_negative_and_wire_up_native_positive() {
        for value in [-2400, -360, -120, -1, 0, 1, 120, 360, 2400] {
            let delta = ScrollDelta::parse(&json!({"delta":value})).unwrap();
            assert_eq!(delta.value(), value);
            assert_eq!(delta.native_positive_up(), -value);
        }
    }

    #[test]
    fn parser_never_defaults_clamps_truncates_or_ignores_parameters() {
        for params in [
            json!(null),
            json!([]),
            json!({}),
            json!({"delta":"120"}),
            json!({"delta":true}),
            json!({"delta":1.5}),
            json!({"delta":2401}),
            json!({"delta":-2401}),
            json!({"delta":i64::MIN}),
            json!({"delta":u64::MAX}),
            json!({"delta":120,"extra":true}),
        ] {
            assert!(ScrollDelta::parse(&params).is_err(), "{params}");
        }
    }
}
