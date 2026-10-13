//! Bound even TAR metadata/padding and finish gzip checksum validation.
//! The normal TAR iterator hides extended headers, so validate raw entries first.

use crate::playwright_materialize::MaterializeError;
use crate::runtime_limits::{error, TreeBudget, TreeLimits};
use flate2::bufread::GzDecoder;
use std::io::{Cursor, Read, Take};

type Reader<'a> = Take<GzDecoder<Cursor<&'a [u8]>>>;

pub(crate) fn decoder(bytes: &[u8], limits: TreeLimits) -> Result<Reader<'_>, MaterializeError> {
    let ceiling = limits
        .max_total_bytes
        .checked_add(u64::from(limits.max_entries) * 1024)
        .and_then(|value| value.checked_add(10240 + 1))
        .ok_or_else(|| error("oversize", "TAR byte ceiling overflow"))?;
    Ok(GzDecoder::new(Cursor::new(bytes)).take(ceiling))
}

pub(crate) fn validate(bytes: &[u8], limits: TreeLimits) -> Result<(), MaterializeError> {
    let mut archive = tar::Archive::new(decoder(bytes, limits)?);
    let mut budget = TreeBudget::default();
    for entry in archive
        .entries()
        .map_err(|e| error("truncated", e.to_string()))?
        .raw(true)
    {
        let mut entry = entry.map_err(|e| error("truncated", e.to_string()))?;
        let kind = entry.header().entry_type();
        let size = entry.size();
        let metadata = kind.is_gnu_longname()
            || kind.is_gnu_longlink()
            || kind.is_pax_global_extensions()
            || kind.is_pax_local_extensions();
        if !(kind.is_file()
            || kind.is_dir()
            || kind.is_symlink()
            || kind.is_hard_link()
            || metadata)
        {
            return Err(error("symlink", "unsupported raw TAR entry type"));
        }
        if metadata && size > 4096 {
            return Err(error("oversize", "TAR extended header exceeds bound"));
        }
        if (kind.is_dir() || kind.is_symlink() || kind.is_hard_link()) && size != 0 {
            return Err(error("path", "TAR directory/link contains a payload"));
        }
        budget.add(
            limits,
            if kind.is_dir() || metadata {
                None
            } else {
                Some(size)
            },
        )?;
        crate::runtime_tree::copy_exact(&mut entry, &mut std::io::sink(), size)?;
    }
    finish(archive.into_inner())
}

fn finish(mut reader: Reader<'_>) -> Result<(), MaterializeError> {
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| error("truncated", e.to_string()))?;
        if count == 0 {
            break;
        }
        if buffer[..count].iter().any(|byte| *byte != 0) {
            return Err(error("path", "non-padding data after TAR end"));
        }
    }
    if reader.limit() == 0 {
        return Err(error("oversize", "expanded TAR exceeds byte ceiling"));
    }
    let compressed = reader.get_ref().get_ref();
    if compressed.position() != compressed.get_ref().len() as u64 {
        return Err(error("path", "trailing data or additional gzip member"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut writer = GzEncoder::new(Vec::new(), Compression::default());
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap()
    }

    fn raw_file(size: usize, kind: tar::EntryType) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(size as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/data", vec![b'a'; size].as_slice())
            .unwrap();
        builder.into_inner().unwrap()
    }

    #[test]
    fn complete_gzip_checksum_is_required_after_tar_end_markers() {
        let mut bytes = gzip(&raw_file(1, tar::EntryType::Regular));
        validate(&bytes, crate::runtime_limits::playwright()).unwrap();
        let position = bytes.len() - 8;
        bytes[position] ^= 1;
        assert_eq!(
            validate(&bytes, crate::runtime_limits::playwright())
                .unwrap_err()
                .code,
            "truncated"
        );
    }

    #[test]
    fn nonzero_tar_tail_and_additional_gzip_members_are_rejected() {
        let mut raw = raw_file(1, tar::EntryType::Regular);
        raw.extend_from_slice(b"hidden content after end markers");
        assert_eq!(
            validate(&gzip(&raw), crate::runtime_limits::playwright())
                .unwrap_err()
                .code,
            "path"
        );
        let mut bytes = gzip(&raw_file(1, tar::EntryType::Regular));
        bytes.extend_from_slice(&gzip(b"second member"));
        assert_eq!(
            validate(&bytes, crate::runtime_limits::playwright())
                .unwrap_err()
                .code,
            "path"
        );
    }

    #[test]
    fn hidden_extended_headers_and_directory_payloads_have_limits() {
        let bytes = gzip(&raw_file(4097, tar::EntryType::GNULongName));
        assert_eq!(
            validate(&bytes, crate::runtime_limits::playwright())
                .unwrap_err()
                .code,
            "oversize"
        );
        let bytes = gzip(&raw_file(1, tar::EntryType::Directory));
        assert_eq!(
            validate(&bytes, crate::runtime_limits::playwright())
                .unwrap_err()
                .code,
            "path"
        );
    }

    #[test]
    fn zero_padding_bombs_are_bounded_even_without_materialized_files() {
        let mut limits = crate::runtime_limits::playwright();
        limits.max_total_bytes = 16;
        limits.max_entries = 1;
        let bytes = gzip(&vec![0; 16 + 1024 + 10240 + 1]);
        assert_eq!(validate(&bytes, limits).unwrap_err().code, "oversize");
    }
}
