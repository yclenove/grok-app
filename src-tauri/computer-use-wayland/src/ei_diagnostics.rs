//! Owned native acceptance only; never routing, permission or input history.
//! Bounded snapshots run on the existing EI owner, with no leaked native refs.
use super::{ffi, CStr, Native};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct Diagnostics {
    last_generation: Option<u64>,
    emitted: usize,
}
fn mapping(id: Option<&[u8]>) -> Value {
    match id {
        Some(bytes) => json!({
            "bytes": bytes.iter().take(512).copied().collect::<Vec<_>>(),
            "truncated": bytes.len() > 512,
        }),
        None => Value::Null,
    }
}
impl Diagnostics {
    pub(super) fn observe(&mut self, native: &Native) {
        if self.emitted >= 128
            || self.last_generation == Some(native.generation)
            || std::env::var("GROK_CU_EI_DIAGNOSTICS").as_deref() != Ok("1")
        {
            return;
        }
        self.last_generation = Some(native.generation);
        self.emitted += 1;
        let devices = native.devices.iter().take(64).enumerate().map(|(index, d)| {
            let mut regions = Vec::new();
            for i in 0..=128 {
                let r = unsafe { ffi::ei_device_get_region(d.raw, i) };
                if r.is_null() { break; }
                if i == 128 {
                    regions.push(json!({"overflow": true}));
                    break;
                }
                let id = unsafe { ffi::ei_region_get_mapping_id(r) };
                let id = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_bytes());
                regions.push(unsafe { json!({
                    "mapping": mapping(id), "x": ffi::ei_region_get_x(r),
                    "y": ffi::ei_region_get_y(r), "width": ffi::ei_region_get_width(r),
                    "height": ffi::ei_region_get_height(r),
                    "physicalScale": ffi::ei_region_get_physical_scale(r)
                }) });
            }
            json!({"index": index, "type": unsafe { ffi::ei_device_get_type(d.raw) },
                "resumed": d.resumed,
                "capabilities": ([ffi::POINTER, ffi::ABSOLUTE, ffi::KEYBOARD, ffi::BUTTON, ffi::SCROLL]
                    .into_iter().filter(|cap| unsafe { ffi::ei_device_has_capability(d.raw, *cap) }).collect::<Vec<_>>()),
                "regions": regions})
        }).collect::<Vec<_>>();
        eprintln!(
            "{}",
            json!({"event": "OWNED_EI_DIAGNOSTIC", "generation": native.generation,
            "connected": native.connected, "streamMapping": mapping(native.mapping.as_deref().map(str::as_bytes)),
            "matchedRegions": native.regions().len(), "devices": devices,
            "authorityChangedByDiagnostic": false})
        );
    }
}
