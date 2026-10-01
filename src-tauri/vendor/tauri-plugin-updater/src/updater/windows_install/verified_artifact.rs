//! Signed retained-artifact verification without an App handle or callback.
//! The trusted public key is supplied by the running binary, never by journal.
use super::super::windows_journal::{self as journal, Kind, Record};
use super::*;

pub(super) fn verify_retained(
    journal: &Journal,
    record: &Record,
    trusted_key: &str,
) -> Result<(File, WindowsUpdaterType)> {
    verify_in_store(&journal.witness_store()?, record, trusted_key)
}

pub(super) fn verify_in_store(
    store: &journal::witness::Store,
    record: &Record,
    trusted_key: &str,
) -> Result<(File, WindowsUpdaterType)> {
    if trusted_key.is_empty()
        || journal::digest(trusted_key.as_bytes()) != record.binding.key_digest
    {
        return Err(journal::pending(
            "Retained candidate is not bound to the trusted key",
        ));
    }
    if let Some(inventory) = &record.inventory {
        inventory.verify(trusted_key, record)?;
    }
    let mut payload = journal::protected_read(&store.artifact_path(&record.candidate_id, None)?)
        .map_err(journal::pending)?;
    let mut bytes = Vec::new();
    payload.read_to_end(&mut bytes).map_err(journal::pending)?;
    if journal::digest(&bytes) != record.payload_digest {
        return Err(journal::pending(
            "Original signed update payload has changed",
        ));
    }
    verify_signature(&bytes, &record.signature, trusted_key).map_err(journal::pending)?;
    let (kind, entry, expected) = installer_bytes(&bytes)?;
    if kind != record.kind
        || entry != record.archive_entry
        || journal::digest(&expected) != record.installer_digest
    {
        return Err(journal::pending(
            "Retained installer is not the original signed archive member",
        ));
    }
    let path = store.artifact_path(&record.candidate_id, Some(record.kind))?;
    let mut lease = journal::protected_read(&path).map_err(journal::pending)?;
    let mut actual = Vec::new();
    lease.read_to_end(&mut actual).map_err(journal::pending)?;
    lease.seek(SeekFrom::Start(0)).map_err(journal::pending)?;
    if actual != expected {
        return Err(journal::pending("Original prepared installer has changed"));
    }
    Ok((lease, windows_installer(record.kind, path)))
}

pub(super) fn windows_installer(kind: Kind, path: PathBuf) -> WindowsUpdaterType {
    match kind {
        Kind::Nsis => WindowsUpdaterType::nsis(path, None),
        Kind::Msi => WindowsUpdaterType::msi(path, None),
    }
}

// Do not extract unrelated archive entries or trust an archive path. A signed
// Tauri Windows bundle has one root-level installer. Ambiguity is an error.
pub(super) fn installer_bytes(bytes: &[u8]) -> Result<(Kind, Option<String>, Vec<u8>)> {
    #[cfg(feature = "zip")]
    if infer::archive::is_zip(bytes) {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        verify_unique_directory(&mut archive, bytes)?;
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index)?;
            let name = entry.name();
            if !name.contains(['/', '\\', ':'])
                && (name.ends_with(".exe") || name.ends_with(".msi"))
            {
                names.push((index, name.to_owned()));
            }
        }
        if names.len() != 1 {
            return Err(Error::InvalidUpdaterFormat);
        }
        let (index, name) = &names[0];
        let mut value = Vec::new();
        archive.by_index(*index)?.read_to_end(&mut value)?;
        let kind = installer_kind(&value)?;
        if (kind == Kind::Nsis) != name.ends_with(".exe") {
            return Err(Error::InvalidUpdaterFormat);
        }
        return Ok((kind, Some(name.clone()), value));
    }
    Ok((installer_kind(bytes)?, None, bytes.to_vec()))
}

/// ZipArchive's name map (including by_index/len) collapses duplicate names.
/// Require its indexed central headers to cover the physical directory without
/// holes. This checks metadata coverage, not a second ZIP/decompression parser;
/// the crate still validates/reads the selected member, CRC and ZIP64 fields.
#[cfg(feature = "zip")]
fn verify_unique_directory(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    bytes: &[u8],
) -> Result<()> {
    let mut offsets = Vec::new();
    for index in 0..archive.len() {
        offsets.push(archive.by_index_raw(index)?.central_header_start());
    }
    offsets.sort_unstable();
    let mut cursor = usize::try_from(archive.central_directory_start())
        .map_err(|_| Error::InvalidUpdaterFormat)?;
    for offset in offsets {
        if usize::try_from(offset).ok() != Some(cursor) {
            return Err(Error::InvalidUpdaterFormat);
        }
        let header = bytes
            .get(cursor..)
            .and_then(|b| b.get(..46))
            .ok_or(Error::InvalidUpdaterFormat)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(Error::InvalidUpdaterFormat);
        }
        let mut length = 46;
        for index in [28, 30, 32] {
            length += u16::from_le_bytes([header[index], header[index + 1]]) as usize;
        }
        cursor = cursor
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(Error::InvalidUpdaterFormat)?;
    }
    if bytes
        .get(cursor..)
        .is_some_and(|b| b.starts_with(b"PK\x01\x02"))
    {
        return Err(Error::InvalidUpdaterFormat);
    }
    Ok(())
}

fn installer_kind(bytes: &[u8]) -> Result<Kind> {
    if infer::app::is_exe(bytes) {
        Ok(Kind::Nsis)
    } else if infer::archive::is_msi(bytes) {
        Ok(Kind::Msi)
    } else {
        Err(Error::InvalidUpdaterFormat)
    }
}
