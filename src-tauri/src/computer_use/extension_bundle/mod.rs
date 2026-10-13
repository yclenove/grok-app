//! Embedded source extension export. No download, install, pairing or grant.
mod files;
mod safety;
use base64::Engine;
use fs2::FileExt;
use safety::{directories, error, make_directory, plain, read, sync, write};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = ".grok-extension-bundle.json";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtensionInfo {
    pub version: String,
    pub extension_id: String,
    pub minimum_browser_version: String,
    pub sha256: String,
    pub file_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportedExtension {
    #[serde(flatten)]
    pub info: ExtensionInfo,
    pub path: String,
}

pub(crate) fn info() -> Result<ExtensionInfo, String> {
    let manifest = files::FILES
        .iter()
        .find(|(name, _)| *name == "manifest.json")
        .ok_or_else(|| error("manifest missing"))?
        .1;
    let manifest: serde_json::Value = serde_json::from_slice(manifest).map_err(error)?;
    let string = |key: &str| {
        manifest[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| error("invalid manifest"))
    };
    let key = base64::engine::general_purpose::STANDARD
        .decode(string("key")?)
        .map_err(error)?;
    let digest = Sha256::digest(key);
    let id: String = digest[..16]
        .iter()
        .flat_map(|byte| {
            [
                char::from(b'a' + (byte >> 4)),
                char::from(b'a' + (byte & 15)),
            ]
        })
        .collect();
    if id != grok_computer_use_core::pairing::EXTENSION_ID || manifest["manifest_version"] != 3 {
        return Err(error("extension identity mismatch"));
    }
    let version = string("version")?;
    if version.split('.').count() != 3
        || version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(error("invalid extension version"));
    }
    let mut hash = Sha256::new();
    for (name, bytes) in files::FILES {
        hash.update(format!(
            "{name}\0{}\0{:x}\n",
            bytes.len(),
            Sha256::digest(bytes)
        ));
    }
    Ok(ExtensionInfo {
        version,
        extension_id: id,
        minimum_browser_version: string("minimum_chrome_version")?,
        sha256: format!("{:x}", hash.finalize()),
        file_count: files::FILES.len(),
    })
}
fn root() -> PathBuf {
    crate::paths::app_data_root()
        .join("computer-use")
        .join("browser-extension")
}
fn destination(root: &Path, info: &ExtensionInfo) -> PathBuf {
    root.join(format!("v{}-{}", info.version, info.sha256))
}
fn marker(info: &ExtensionInfo) -> Result<Vec<u8>, String> {
    serde_json::to_vec(
        &serde_json::json!({ "owner": "com.grokapp.desktop", "format": 1, "extension": info }),
    )
    .map_err(error)
}
fn verify(path: &Path, info: &ExtensionInfo) -> Result<(), String> {
    let _pins = directories(path)?;
    if read(&path.join(MARKER))? != marker(info)? {
        return Err(error("unowned or changed extension export"));
    }
    let mut expected = BTreeSet::from([MARKER.to_owned()]);
    for (name, bytes) in files::FILES {
        if read(&path.join(name))?.as_slice() != *bytes {
            return Err(error("changed extension export"));
        }
        let mut item = Path::new(name);
        expected.insert(name.to_string());
        while let Some(parent) = item
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            expected.insert(parent.to_string_lossy().replace('\\', "/"));
            item = parent;
        }
    }
    fn walk(root: &Path, current: &Path, found: &mut BTreeSet<String>) -> Result<(), String> {
        let _pins = directories(current)?;
        for entry in fs::read_dir(current).map_err(error)? {
            let path = entry.map_err(error)?.path();
            let meta = fs::symlink_metadata(&path).map_err(error)?;
            if !plain(&meta) || (!meta.is_dir() && !meta.is_file()) {
                return Err(error("unsafe export entry"));
            }
            found.insert(
                path.strip_prefix(root)
                    .map_err(error)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if meta.is_dir() {
                walk(root, &path, found)?;
            }
        }
        Ok(())
    }
    let mut found = BTreeSet::new();
    walk(path, path, &mut found)?;
    if found != expected {
        return Err(error("unexpected export contents"));
    }
    Ok(())
}
fn exported(path: &Path, info: ExtensionInfo) -> Result<ExportedExtension, String> {
    Ok(ExportedExtension {
        path: path
            .to_str()
            .ok_or_else(|| error("App path is not Unicode"))?
            .into(),
        info,
    })
}

fn export_at(root: &Path) -> Result<ExportedExtension, String> {
    let info = info()?;
    make_directory(root)?;
    let _pins = directories(root)?;
    let lock_path = root.join(".publication.lock");
    let lock = match safety::open(&lock_path, true) {
        Ok(file) => file,
        Err(_) => safety::open(&lock_path, false)?,
    };
    lock.try_lock_exclusive()
        .map_err(|_| error("another export is busy"))?;
    let target = destination(root, &info);
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            verify(&target, &info)?;
            return exported(&target, info);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    // Keep an interrupted staging directory as evidence; never repair/delete
    // foreign contents. Explicit retry creates a new private staging identity.
    let staging = root.join(format!(".pending-{}", uuid::Uuid::new_v4()));
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&staging).map_err(error)?;
    for (name, bytes) in files::FILES {
        let path = staging.join(name);
        make_directory(
            path.parent()
                .ok_or_else(|| error("missing bundle parent"))?,
        )?;
        write(&path, bytes)?;
    }
    write(&staging.join(MARKER), &marker(&info)?)?;
    verify(&staging, &info)?;
    sync(&staging)?;
    safety::publish(&staging, &target)?;
    sync(root)?;
    verify(&target, &info)?;
    exported(&target, info)
}
pub(crate) fn export() -> Result<ExportedExtension, String> {
    export_at(&root())
}
pub(crate) fn published_path() -> Result<PathBuf, String> {
    let info = info()?;
    let path = destination(&root(), &info);
    verify(&path, &info)?;
    Ok(path)
}

#[cfg(test)]
mod tests;
