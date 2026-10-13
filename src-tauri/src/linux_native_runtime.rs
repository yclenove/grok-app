//! Private PipeWire before any App/session/UI work. No portal or desktop access.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
#[cfg(grok_private_pipewire)]
use std::ffi::CStr;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
#[cfg(grok_private_pipewire)]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

const RESOURCE: &str = "resources/computer-use/native-pipewire";

fn native_policy(arch: &str) -> Result<(&'static str, &'static str), String> {
    match arch {
        "x86_64" => Ok(("x86_64-unknown-linux-gnu", "2.35")),
        "aarch64" => Ok(("aarch64-unknown-linux-gnu", "2.39")),
        _ => Err("Unsupported private PipeWire target architecture".into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    target: String,
    pipewire_version: String,
    glibc_baseline: String,
    files: Vec<FileRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    path: String,
    bytes: u64,
    sha256: String,
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    parse_manifest_for_arch(bytes, std::env::consts::ARCH)
}

fn parse_manifest_for_arch(bytes: &[u8], arch: &str) -> Result<Manifest, String> {
    let (target, glibc_baseline) = native_policy(arch)?;
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if manifest.schema_version != 1
        || manifest.target != target
        || manifest.pipewire_version != "1.6.9"
        || manifest.glibc_baseline != glibc_baseline
        || manifest.files.len() != 16
    {
        return Err("Private PipeWire manifest policy mismatch".into());
    }
    let mut paths = BTreeSet::new();
    for file in &manifest.files {
        let path = Path::new(&file.path);
        if file.bytes == 0
            || file.sha256.len() != 64
            || !file
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || path.as_os_str().is_empty()
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
            || !paths.insert(file.path.clone())
        {
            return Err("Invalid or duplicate private PipeWire file record".into());
        }
    }
    for required in [
        "lib/libpipewire-0.3.so.0",
        "lib/spa-0.2/support/libspa-support.so",
        "lib/spa-0.2/audioconvert/libspa-audioconvert.so",
        "share/pipewire/client.conf",
        "share/licenses/pipewire/COPYING",
        "share/licenses/pipewire/LICENSE",
        "share/licenses/pipewire/0001-CVE-2026-14330.patch",
        "share/licenses/pipewire/SOURCE.txt",
        "share/licenses/libei/COPYING",
    ] {
        if !paths.contains(required) {
            return Err(format!(
                "Required private PipeWire file not compiled: {required}"
            ));
        }
    }
    for module in [
        "rt",
        "protocol-native",
        "client-node",
        "client-device",
        "adapter",
        "metadata",
        "session-manager",
    ] {
        if !paths.contains(&format!("lib/pipewire-0.3/libpipewire-module-{module}.so")) {
            return Err(format!("Required client module not compiled: {module}"));
        }
    }
    Ok(manifest)
}

fn resource_from_exe(exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or("Executable directory missing")?;
    // Match Tauri's normal installed/relocated resource layout, then Cargo's
    // output resource layout. Never trust APPDIR or a user-selected SDK path.
    let candidates = [
        parent.join("../lib/grok-app").join(RESOURCE),
        parent.join(RESOURCE),
    ];
    for candidate in candidates {
        if fs::symlink_metadata(&candidate).is_ok() {
            let canonical = candidate.canonicalize().map_err(|e| e.to_string())?;
            // Normalize the intentional '..' in installed layout first, then
            // inspect every original component rather than hiding parent links.
            let normalized = normalize_absolute(&candidate)?;
            reject_links(&normalized)?;
            // A symlink at the original candidate must not be canonicalized
            // into a different resource tree and treated as trusted.
            if fs::symlink_metadata(&candidate)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("Private PipeWire resource directory is a symlink".into());
            }
            return Ok(canonical);
        }
    }
    Err("Private PipeWire resources missing; repair this installation".into())
}

fn normalize_absolute(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Absolute native resource path required".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => result.push(component.as_os_str()),
            Component::CurDir => (),
            Component::ParentDir if result.parent().is_some() => {
                result.pop();
            }
            _ => return Err("Native resource path escapes root".into()),
        }
    }
    Ok(result)
}

fn reject_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if fs::symlink_metadata(ancestor)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Private PipeWire path contains a symlink".into());
        }
    }
    Ok(())
}

fn verify_files(root: &Path, manifest: &Manifest) -> Result<(), String> {
    reject_links(root)?;
    verify_runtime_tree(root, manifest)?;
    for record in &manifest.files {
        let path = root.join("runtime").join(&record.path);
        reject_links(&path)?;
        let file = open_regular(&path)?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        if before.len() != record.bytes {
            return Err(format!(
                "Private PipeWire component size mismatch: {}",
                record.path
            ));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        // Bound reads even if a component grows while being inspected.
        let mut reader = file.take(record.bytes.saturating_add(1));
        let mut bytes_read = 0u64;
        loop {
            let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            bytes_read += count as u64;
            hash.update(&buffer[..count]);
        }
        let after = reader.get_ref().metadata().map_err(|e| e.to_string())?;
        if bytes_read != record.bytes
            || after.len() != record.bytes
            || format!("{:x}", hash.finalize()) != record.sha256
        {
            return Err(format!(
                "Private PipeWire component hash mismatch: {}",
                record.path
            ));
        }
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<fs::File, String> {
    let expected = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !expected.is_file() {
        return Err("Private PipeWire component is not a regular file".into());
    }
    // NONBLOCK also prevents a replaced FIFO from hanging startup between the
    // metadata check and open. This is an integrity diagnostic, not a sandbox
    // against a concurrent attacker replacing already-mapped ELF pages.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let actual = file.metadata().map_err(|e| e.to_string())?;
    if !actual.is_file() || actual.dev() != expected.dev() || actual.ino() != expected.ino() {
        return Err("Private PipeWire component changed during open".into());
    }
    Ok(file)
}

fn verify_runtime_tree(root: &Path, manifest: &Manifest) -> Result<(), String> {
    let runtime = root.join("runtime");
    if !fs::symlink_metadata(&runtime)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("Private PipeWire runtime is not a directory".into());
    }
    let files: BTreeSet<PathBuf> = manifest
        .files
        .iter()
        .map(|f| PathBuf::from(&f.path))
        .collect();
    let directories: BTreeSet<PathBuf> = files
        .iter()
        .flat_map(|f| f.ancestors().skip(1))
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    let mut pending = vec![runtime.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(&runtime).map_err(|e| e.to_string())?;
            let kind = fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .file_type();
            if kind.is_symlink() {
                return Err("Private PipeWire runtime contains a symlink".into());
            }
            if kind.is_dir() && directories.contains(relative) {
                pending.push(path);
            } else if !kind.is_file() || !files.contains(relative) {
                return Err(format!(
                    "Unexpected private PipeWire runtime entry: {}",
                    relative.display()
                ));
            }
        }
    }
    Ok(())
}

/// Process-local search order also protects installed/extracted AppImage launch
/// when a host-WebKit wrapper prepends the system multiarch library directory.
pub(super) fn library_search_path(
    private: &Path,
    old: Option<&std::ffi::OsStr>,
) -> Result<OsString, String> {
    let mut paths = vec![private.to_path_buf()];
    if let Some(old) = old {
        paths.extend(
            std::env::split_paths(old).filter(|p| !p.as_os_str().is_empty() && p != private),
        );
    }
    std::env::join_paths(paths).map_err(|e| e.to_string())
}

#[cfg(grok_private_pipewire)]
pub(crate) fn prepare() -> Result<(), String> {
    static COMPILED: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/computer-use/native-pipewire/manifest.json"
    ));
    let manifest = parse_manifest(COMPILED)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let resource = resource_from_exe(&exe)?;
    let entries: BTreeSet<OsString> = fs::read_dir(&resource)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if entries
        != [OsString::from("runtime"), OsString::from("manifest.json")]
            .into_iter()
            .collect()
    {
        return Err("Unexpected private PipeWire resource entry".into());
    }
    let index = resource.join("manifest.json");
    reject_links(&index)?;
    let mut installed = Vec::new();
    open_regular(&index)?
        .take(COMPILED.len() as u64 + 1)
        .read_to_end(&mut installed)
        .map_err(|e| e.to_string())?;
    if installed != COMPILED {
        return Err("Private PipeWire installation manifest differs from compiled policy".into());
    }
    verify_files(&resource, &manifest)?;
    verify_loaded(
        &resource.join("runtime/lib/libpipewire-0.3.so.0"),
        &manifest.pipewire_version,
    )?;
    let runtime = resource.join("runtime");
    let search = library_search_path(
        &runtime.join("lib"),
        std::env::var_os("LD_LIBRARY_PATH").as_deref(),
    )?;
    // Called from run(), before session API, logging, Tauri or worker threads.
    std::env::set_var("LD_LIBRARY_PATH", search);
    std::env::set_var("SPA_PLUGIN_DIR", runtime.join("lib/spa-0.2"));
    std::env::set_var("PIPEWIRE_MODULE_DIR", runtime.join("lib/pipewire-0.3"));
    std::env::set_var("PIPEWIRE_CONFIG_DIR", runtime.join("share/pipewire"));
    std::env::set_var("PIPEWIRE_CONFIG_PREFIX", "");
    std::env::set_var("PIPEWIRE_CONFIG_NAME", "client.conf");
    Ok(())
}

#[cfg(grok_private_pipewire)]
fn verify_loaded(expected: &Path, version: &str) -> Result<(), String> {
    #[link(name = "pipewire-0.3")]
    unsafe extern "C" {
        fn pw_get_library_version() -> *const libc::c_char;
    }
    // No pw_init/context/connect: inspect only the already-loaded ELF identity.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    unsafe {
        if libc::dladdr(
            pw_get_library_version as *const () as *const libc::c_void,
            &mut info,
        ) == 0
            || info.dli_fname.is_null()
        {
            return Err("Loaded PipeWire identity unavailable".into());
        }
        let actual_version = pw_get_library_version();
        if actual_version.is_null() {
            return Err("Loaded PipeWire version unavailable".into());
        }
        let actual = Path::new(std::ffi::OsStr::from_bytes(
            CStr::from_ptr(info.dli_fname).to_bytes(),
        ));
        if actual.canonicalize().map_err(|e| e.to_string())? != expected
            || CStr::from_ptr(actual_version).to_bytes() != version.as_bytes()
        {
            return Err("Wrong PipeWire library loaded; refusing system fallback".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "linux_native_runtime/tests.rs"]
mod tests;
