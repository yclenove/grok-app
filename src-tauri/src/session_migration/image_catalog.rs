//! Stream one bounded descriptor, skipping original refs/source payloads.
use super::image_reader::ImageState;
use super::types::AssetState;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::io::{BufReader, Read};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImageDescriptor {
    pub asset_id: String,
    pub digest: Option<String>,
    pub mime: Option<String>,
    pub size: u64,
    pub state: AssetState,
}

struct Selected<'a>(&'a str);
impl<'de> Visitor<'de> for Selected<'_> {
    type Value = ImageDescriptor;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an asset catalog")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut selected = None;
        while let Some(asset) = seq.next_element::<ImageDescriptor>()? {
            if asset.asset_id == self.0 {
                if selected.is_some() {
                    return Err(de::Error::custom("duplicate asset identity"));
                }
                selected = Some(asset);
            }
        }
        selected.ok_or_else(|| de::Error::custom("unknown asset identity"))
    }
}

pub(super) fn read(reader: impl Read, id: &str) -> Result<ImageDescriptor, ImageState> {
    // serde_json consumes Read one byte at a time. Catalogs retain embedded
    // source references, so even skipped fields need buffered filesystem I/O.
    let mut decoder = serde_json::Deserializer::from_reader(BufReader::new(reader));
    let asset = decoder
        .deserialize_seq(Selected(id))
        .map_err(|_| ImageState::Invalid)?;
    decoder.end().map_err(|_| ImageState::Invalid)?;
    Ok(asset)
}

#[cfg(test)]
#[path = "image_catalog_tests.rs"]
mod tests;
