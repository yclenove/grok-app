//! App-owned runtime packs: hashed files in a private directory, never PATH.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const JS_RUNTIME: &str = "js-runtime";
pub const MCP_SERVER: &str = "mcp-server";
pub const MCP_PROTOCOL: &str = "mcp-protocol";
pub const PLAYWRIGHT_ARCHIVE: &str = "playwright-archive";
/// Compatibility alias: the archive is no longer a runnable Playwright tree.
pub const PLAYWRIGHT: &str = PLAYWRIGHT_ARCHIVE;
pub const PLAYWRIGHT_RUNTIME: &str = "playwright-runtime";
pub const BROWSER_WORKER: &str = "browser-worker";
pub const BROWSER_WORKER_MODULES: &str = "browser-worker-modules";
pub const CHROMIUM: &str = "chromium";
pub const DRIVER: &str = "driver";
/// Pack format version. Independent from Computer Use wire `compatibility.protocol`.
pub const PACK_SCHEMA_VERSION: u32 = 2;

const CURRENT_FILE: &str = "current.json";

#[cfg(test)]
#[path = "runtime_rollback_tests.rs"]
mod rollback_tests;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ComponentKind {
    #[default]
    File,
    Tree,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeComponent {
    pub id: String,
    pub version: String,
    pub arch: String,
    pub relpath: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub kind: ComponentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCompatibility {
    #[serde(default)]
    pub protocol: u32,
    #[serde(default)]
    pub js_runtime_version: String,
    #[serde(default)]
    pub playwright_core_version: String,
    #[serde(default)]
    pub worker_source_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeManifest {
    pub pack_id: String,
    #[serde(default)]
    pub schema_version: u32,
    pub components: BTreeMap<String, RuntimeComponent>,
    #[serde(default)]
    pub compatibility: RuntimeCompatibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CurrentPointer {
    active: Option<String>,
    previous: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewComponent {
    pub id: String,
    pub version: String,
    pub relpath: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RuntimeStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIssue {
    pub code: String,
    pub component: String,
    pub action: String,
}

impl RuntimeIssue {
    pub fn missing_file(component: &str) -> Self {
        Self {
            code: "missing_file".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }

    pub fn hash_mismatch(component: &str) -> Self {
        Self {
            code: "hash_mismatch".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }

    pub fn arch_mismatch(component: &str) -> Self {
        Self {
            code: "arch_mismatch".into(),
            component: component.into(),
            action: "reinstall".into(),
        }
    }

    pub fn version_mismatch(component: &str) -> Self {
        Self {
            code: "version_mismatch".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }

    pub fn permission_denied(component: &str) -> Self {
        Self {
            code: "permission_denied".into(),
            component: component.into(),
            action: "reinstall".into(),
        }
    }

    pub fn port_failed() -> Self {
        Self {
            code: "port_failed".into(),
            component: "ipc".into(),
            action: "repair".into(),
        }
    }

    pub fn interrupted_upgrade() -> Self {
        Self {
            code: "interrupted_upgrade".into(),
            component: "pack".into(),
            action: "repair".into(),
        }
    }

    pub fn placeholder(component: &str) -> Self {
        Self {
            code: "placeholder".into(),
            component: component.into(),
            action: "reinstall".into(),
        }
    }

    pub fn not_executable(component: &str) -> Self {
        Self {
            code: "not_executable".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }

    pub fn import_failed(component: &str) -> Self {
        Self {
            code: "import_failed".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }

    pub fn missing_sibling(component: &str) -> Self {
        Self {
            code: "missing_sibling".into(),
            component: component.into(),
            action: "repair".into(),
        }
    }
}

pub const PLACEHOLDER_JS_RUNTIME: &[u8] = b"GROK-APP-OWNED-JS-RUNTIME\n";
pub const VERSION_PIN_JS_RUNTIME_STUB: &[u8] = b"grok-cu-windows-x64-js-runtime-20.18.0\n";
pub const PLACEHOLDER_PLAYWRIGHT: &[u8] = b"export const grokComputerUsePlaywrightWorker = true;\n";
pub const PLAYWRIGHT_CORE_TGZ_SHA256: &str =
    "60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562";
pub const REQUIRED_JS_RUNTIME_VERSION: &str = "20.18.0";
pub const REQUIRED_PLAYWRIGHT_CORE_VERSION: &str = "1.48.0";
/// Four App-owned pack architectures. macOS/Linux verification stays not_run on Windows.
pub const REQUIRED_PACK_ARCHS: &[&str] = &[
    "x86_64-windows",
    "aarch64-macos",
    "x86_64-macos",
    "x86_64-linux",
];

pub fn is_placeholder_runtime(bytes: &[u8]) -> bool {
    if looks_like_native_binary(bytes) {
        return false;
    }
    bytes.starts_with(b"GROK-APP-OWNED-JS-RUNTIME") || bytes == VERSION_PIN_JS_RUNTIME_STUB || {
        let text = std::str::from_utf8(bytes).unwrap_or("").trim();
        text.starts_with("grok-cu-") && text.contains("js-runtime")
    }
}

pub fn is_placeholder_playwright(bytes: &[u8]) -> bool {
    bytes == PLACEHOLDER_PLAYWRIGHT
        || (bytes.len() < 256
            && bytes
                .windows(32)
                .any(|w| w == b"grokComputerUsePlaywrightWorker"))
}

fn component_is_placeholder(id: &str, bytes: &[u8]) -> bool {
    is_placeholder_runtime(bytes)
        || (id == BROWSER_WORKER && is_placeholder_playwright(bytes))
        || ((id == PLAYWRIGHT || id == PLAYWRIGHT_ARCHIVE) && is_placeholder_playwright(bytes))
}

fn is_materialized_playwright_runtime(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    value.get("name").and_then(|v| v.as_str()) == Some("playwright-core")
        && value.get("version").and_then(|v| v.as_str()) == Some(REQUIRED_PLAYWRIGHT_CORE_VERSION)
}

fn check_sibling_listing(listing_path: &Path, bytes: &[u8]) -> Result<(), RuntimeIssue> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RuntimeIssue::import_failed(BROWSER_WORKER_MODULES))?;
    let root = listing_path.parent().unwrap_or(listing_path);
    let canonical_root = fs::canonicalize(root)
        .map_err(|_| RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES))?;
    let mut any = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        any = true;
        let mut parts = line.split('\t');
        let name = parts.next().unwrap_or("");
        if name.is_empty()
            || name.contains("..")
            || name.contains('/')
            || name.contains('\\')
            || name.contains(':')
        {
            return Err(RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES));
        }
        let file = root.join(name);
        let metadata = fs::symlink_metadata(&file)
            .map_err(|_| RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || !fs::canonicalize(&file)
                .map_err(|_| RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES))?
                .starts_with(&canonical_root)
        {
            return Err(RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES));
        }
        if let Some(size_s) = parts.next() {
            if let Ok(size) = size_s.parse::<u64>() {
                if metadata.len() != size {
                    return Err(RuntimeIssue::hash_mismatch(BROWSER_WORKER_MODULES));
                }
            }
        }
        if let Some(hash) = parts.next() {
            if sha256_file(&file).ok().as_deref() != Some(hash) {
                return Err(RuntimeIssue::hash_mismatch(BROWSER_WORKER_MODULES));
            }
        }
    }
    if !any {
        return Err(RuntimeIssue::missing_sibling(BROWSER_WORKER_MODULES));
    }
    Ok(())
}

fn component_health(id: &str, path: &Path, bytes: &[u8]) -> Result<(), RuntimeIssue> {
    if component_is_placeholder(id, bytes) {
        return Err(RuntimeIssue::placeholder(id));
    }
    if id == CHROMIUM && !looks_like_native_binary(bytes) {
        return Err(RuntimeIssue::not_executable(id));
    }
    if id == PLAYWRIGHT_RUNTIME && !is_materialized_playwright_runtime(bytes) {
        return Err(RuntimeIssue::import_failed(id));
    }
    if id == BROWSER_WORKER_MODULES {
        check_sibling_listing(path, bytes)?;
    }
    Ok(())
}

fn issue_message(issue: &RuntimeIssue) -> String {
    match issue.code.as_str() {
        "placeholder" => format!(
            "Computer Use runtime `{}` is a placeholder and is not healthy. Repair or reinstall Grok App. The system PATH is not used.",
            issue.component
        ),
        "not_executable" => format!(
            "Computer Use runtime `{}` is not executable. Repair or reinstall Grok App. The system PATH is not used.",
            issue.component
        ),
        "import_failed" => format!(
            "Computer Use runtime `{}` is not a materialized Playwright package and cannot be imported. Repair or reinstall Grok App. The system PATH is not used.",
            issue.component
        ),
        "missing_sibling" => format!(
            "Computer Use runtime `{}` is missing a required sibling module. Repair or reinstall Grok App. The system PATH is not used.",
            issue.component
        ),
        "hash_mismatch" => format!(
            "Computer Use runtime `{}` failed integrity check. Repair or reinstall Grok App.",
            issue.component
        ),
        _ => format!(
            "Computer Use runtime `{}` is not healthy ({}). Repair or reinstall Grok App. The system PATH is not used.",
            issue.component, issue.code
        ),
    }
}

pub fn looks_like_native_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ")
        || bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(&[0xfe, 0xed, 0xfa, 0xce])
        || bytes.starts_with(&[0xfe, 0xed, 0xfa, 0xcf])
        || bytes.starts_with(&[0xce, 0xfa, 0xed, 0xfe])
        || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
        || bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe])
}

pub fn required_runtime_components() -> &'static [&'static str] {
    &[
        JS_RUNTIME,
        MCP_SERVER,
        MCP_PROTOCOL,
        PLAYWRIGHT_ARCHIVE,
        PLAYWRIGHT_RUNTIME,
        BROWSER_WORKER,
        BROWSER_WORKER_MODULES,
        CHROMIUM,
    ]
}

pub fn host_arch() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

fn chromium_executable_path(tree: &Path, arch: &str) -> Result<PathBuf, String> {
    let target = crate::runtime_lock::RuntimeTarget::parse(arch).map_err(|e| e.message)?;
    Ok(tree.join(target.chromium_executable()))
}

fn component_tree_manifest(
    path: &Path,
    component: &RuntimeComponent,
) -> Result<(String, u64, u32), crate::playwright_materialize::MaterializeError> {
    if component.id == CHROMIUM {
        crate::runtime_chromium::tree_manifest(path, &component.arch)
    } else {
        crate::playwright_materialize::tree_manifest(path)
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn unique_pack_id(prefix: &str) -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &hex[..12])
}

fn mcp_pack_id(script: &str, protocol: &str, parts: &[NewComponent]) -> String {
    let mut stamp = format!("{script}\n{protocol}");
    let mut ids: Vec<(String, String)> = parts
        .iter()
        .map(|part| (part.id.clone(), sha256_bytes(&part.bytes)))
        .collect();
    ids.sort();
    for (id, hash) in ids {
        stamp.push('\n');
        stamp.push_str(&id);
        stamp.push(':');
        stamp.push_str(&hash);
    }
    format!("mcp-{}", &sha256_bytes(stamp.as_bytes())[..12])
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    Ok(sha256_bytes(&bytes))
}

fn missing_runtime(what: &str) -> String {
    format!(
        "Computer Use runtime ({what}) is missing from the App private directory. Repair or reinstall Grok App. System Node on PATH is not used."
    )
}

impl RuntimeStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn pointer_path(&self) -> PathBuf {
        self.root.join(CURRENT_FILE)
    }

    fn pack_dir(&self, pack_id: &str) -> PathBuf {
        self.root.join("packs").join(pack_id)
    }

    fn read_pointer(&self) -> CurrentPointer {
        fs::read_to_string(self.pointer_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_pointer(&self, pointer: &CurrentPointer) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let tmp = self.root.join(format!(".{CURRENT_FILE}.tmp"));
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(pointer).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&tmp, self.pointer_path()).map_err(|e| e.to_string())
    }

    pub fn install_pack(
        &self,
        pack_id: &str,
        components: Vec<NewComponent>,
    ) -> Result<PathBuf, String> {
        self.install_pack_full(pack_id, components, &[])
    }

    fn install_pack_full(
        &self,
        pack_id: &str,
        components: Vec<NewComponent>,
        trees: &[(RuntimeComponent, PathBuf)],
    ) -> Result<PathBuf, String> {
        if pack_id.trim().is_empty() || pack_id.contains(['/', '\\', '.']) {
            return Err("invalid computer-use runtime pack id".into());
        }
        let dest = self.pack_dir(pack_id);
        if dest.exists() {
            return Err(format!(
                "computer-use runtime pack {pack_id} already exists"
            ));
        }
        let staging = self
            .root
            .join(".staging")
            .join(format!("{pack_id}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let arch = host_arch();
        let mut manifest = RuntimeManifest {
            pack_id: pack_id.to_string(),
            schema_version: PACK_SCHEMA_VERSION,
            components: BTreeMap::new(),
            compatibility: RuntimeCompatibility {
                protocol: 1,
                js_runtime_version: "20.18.0".into(),
                playwright_core_version: "1.48.0".into(),
                worker_source_sha256: String::new(),
            },
        };
        for part in components {
            if Path::new(&part.relpath).is_absolute()
                || part.relpath.split(['/', '\\']).any(|s| s == "..")
            {
                let _ = fs::remove_dir_all(&staging);
                return Err("runtime component path must be relative and stay in the pack".into());
            }
            let file = staging.join(&part.relpath);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&file, &part.bytes).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            if matches!(part.id.as_str(), JS_RUNTIME | CHROMIUM | DRIVER) {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&file, fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            manifest.components.insert(
                part.id.clone(),
                RuntimeComponent {
                    id: part.id,
                    version: part.version,
                    arch: arch.clone(),
                    relpath: part.relpath,
                    sha256: sha256_bytes(&part.bytes),
                    size: part.bytes.len() as u64,
                    kind: ComponentKind::File,
                },
            );
        }
        self.write_trees_into_staging(&staging, &mut manifest, trees)?;
        if let Some(worker) = manifest.components.get(BROWSER_WORKER) {
            manifest.compatibility.worker_source_sha256 = worker.sha256.clone();
        }
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::rename(&staging, &dest).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            e.to_string()
        })?;
        Ok(dest)
    }

    fn write_trees_into_staging(
        &self,
        staging: &Path,
        manifest: &mut RuntimeManifest,
        trees: &[(RuntimeComponent, PathBuf)],
    ) -> Result<(), String> {
        for (component, src) in trees {
            if Path::new(&component.relpath).is_absolute()
                || component.relpath.split(['/', '\\']).any(|s| s == "..")
            {
                return Err("runtime component path must be relative and stay in the pack".into());
            }
            let dest = staging.join(&component.relpath);
            let copy = if component.id == CHROMIUM {
                crate::runtime_chromium::copy_tree(src, &dest, &component.arch)
            } else {
                crate::playwright_materialize::copy_tree_secure(src, &dest)
            };
            copy.map_err(|e| {
                format!(
                    "tree {} -> {}: {}",
                    src.display(),
                    dest.display(),
                    e.message
                )
            })?;
            let (digest, total, _) =
                component_tree_manifest(&dest, component).map_err(|e| e.message)?;
            if component.sha256 != digest {
                return Err(format!(
                    "Computer Use installer seed `{}` failed integrity check. Reinstall Grok App. The system PATH is not used.",
                    component.id
                ));
            }
            let mut stored = component.clone();
            stored.size = total;
            stored.kind = ComponentKind::Tree;
            stored.arch = if component.arch == "any" {
                "any".into()
            } else {
                host_arch()
            };
            manifest.components.insert(component.id.clone(), stored);
        }
        Ok(())
    }

    fn copy_listing_siblings(
        &self,
        bundle: &Path,
        manifest: &RuntimeManifest,
        pack_id: &str,
    ) -> Result<(), String> {
        let Some(component) = manifest.components.get(BROWSER_WORKER_MODULES) else {
            return Ok(());
        };
        let src_listing = bundle.join(&component.relpath);
        if !src_listing.is_file() {
            return Ok(());
        }
        let dest_listing = self.pack_dir(pack_id).join(&component.relpath);
        let bytes = fs::read(&src_listing).map_err(|e| e.to_string())?;
        let root = src_listing
            .parent()
            .ok_or_else(|| "modules listing parent missing".to_string())?;
        let dest_root = dest_listing
            .parent()
            .ok_or_else(|| "pack modules listing parent missing".to_string())?;
        fs::create_dir_all(dest_root).map_err(|e| e.to_string())?;
        for line in std::str::from_utf8(&bytes).unwrap_or("").lines() {
            let name = line.split('\t').next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if name.contains("..")
                || name.contains('/')
                || name.contains('\\')
                || name.contains(':')
            {
                return Err("modules listing path must be a sibling filename".into());
            }
            fs::copy(root.join(name), dest_root.join(name)).map_err(|e| {
                format!(
                    "copy sibling {} -> {}: {e}",
                    root.join(name).display(),
                    dest_root.join(name).display()
                )
            })?;
        }
        Ok(())
    }

    pub fn activate_pack(&self, pack_id: &str) -> Result<(), String> {
        if !self.pack_dir(pack_id).join("manifest.json").is_file() {
            return Err(format!(
                "computer-use runtime pack {pack_id} is missing a manifest"
            ));
        }
        let mut pointer = self.read_pointer();
        if pointer.active.as_deref() != Some(pack_id) {
            pointer.previous = pointer.active.take();
            pointer.active = Some(pack_id.to_string());
        }
        self.write_pointer(&pointer)
    }

    pub fn rollback_pack(&self) -> Result<String, String> {
        let mut pointer = self.read_pointer();
        let previous = pointer
            .previous
            .take()
            .ok_or_else(|| "no previous computer-use runtime pack to roll back to".to_string())?;
        // A previous pack can have been removed, corrupted, or replaced since
        // activation. Never make it active merely because manifest.json exists.
        self.validate_rollback_pack(&previous)?;
        let old_active = pointer.active.take();
        pointer.active = Some(previous.clone());
        pointer.previous = old_active;
        self.write_pointer(&pointer)?;
        Ok(previous)
    }

    fn validate_rollback_pack(&self, pack_id: &str) -> Result<(), String> {
        if pack_id.is_empty()
            || pack_id.len() > 128
            || !pack_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("invalid previous computer-use runtime pack id; cannot roll back".into());
        }
        let packs = self.root.join("packs");
        let pack = self.pack_dir(pack_id);
        for path in [&packs, &pack, &pack.join("manifest.json")] {
            let meta = fs::symlink_metadata(path)
                .map_err(|_| "previous computer-use runtime pack is missing; cannot roll back")?;
            if meta.file_type().is_symlink() {
                return Err(
                    "previous computer-use runtime pack contains a symlink; cannot roll back"
                        .into(),
                );
            }
        }
        let manifest: RuntimeManifest = serde_json::from_slice(
            &fs::read(pack.join("manifest.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|_| "previous computer-use runtime manifest is invalid; cannot roll back")?;
        if manifest.pack_id != pack_id
            || manifest.schema_version != PACK_SCHEMA_VERSION
            || manifest.compatibility.protocol != 1
            || manifest.compatibility.js_runtime_version != REQUIRED_JS_RUNTIME_VERSION
            || manifest.compatibility.playwright_core_version != REQUIRED_PLAYWRIGHT_CORE_VERSION
        {
            return Err(
                "previous computer-use runtime pack is incompatible; cannot roll back".into(),
            );
        }
        let canonical_pack = fs::canonicalize(&pack).map_err(|e| e.to_string())?;
        for (id, component) in &manifest.components {
            let rel = &component.relpath;
            if component.id != *id
                || rel.is_empty()
                || rel.contains(':')
                || Path::new(rel).is_absolute()
                || rel
                    .split(['/', '\\'])
                    .any(|p| p.is_empty() || p == "." || p == "..")
            {
                return Err(
                    "previous runtime component path must stay in its pack; cannot roll back"
                        .into(),
                );
            }
            let mut path = pack.clone();
            for segment in rel.split(['/', '\\']) {
                path.push(segment);
                let meta = fs::symlink_metadata(&path).map_err(|_| {
                    format!("previous runtime component `{id}` is missing; cannot roll back")
                })?;
                if meta.file_type().is_symlink() {
                    return Err(
                        "previous runtime component path contains a symlink; cannot roll back"
                            .into(),
                    );
                }
            }
            if !fs::canonicalize(path)
                .map_err(|e| e.to_string())?
                .starts_with(&canonical_pack)
            {
                return Err(
                    "previous runtime component path escaped its pack; cannot roll back".into(),
                );
            }
        }
        let issues = self.diagnose_pack(pack_id, required_runtime_components());
        if !issues.is_empty() {
            return Err(format!(
                "previous computer-use runtime pack is unhealthy ({issues:?}); cannot roll back"
            ));
        }
        Ok(())
    }

    pub fn load_active_manifest(&self) -> Result<RuntimeManifest, String> {
        let pack_id = self
            .read_pointer()
            .active
            .ok_or_else(|| missing_runtime("active pack"))?;
        let path = self.pack_dir(&pack_id).join("manifest.json");
        let raw = fs::read(&path).map_err(|_| missing_runtime("manifest"))?;
        serde_json::from_slice(&raw).map_err(|e| e.to_string())
    }

    pub fn resolve(&self, component_id: &str) -> Result<PathBuf, String> {
        let manifest = self.load_active_manifest()?;
        let component = manifest.components.get(component_id).ok_or_else(|| {
            format!(
                "Computer Use component `{component_id}` is not in the App runtime manifest. Repair or reinstall Grok App. The system PATH is not used."
            )
        })?;
        if component.arch != host_arch() && component.arch != "any" {
            return Err(format!(
                "Computer Use runtime arch {} does not match this App ({}). Repair or reinstall Grok App.",
                component.arch,
                host_arch()
            ));
        }
        let path = self.pack_dir(&manifest.pack_id).join(&component.relpath);
        if component.kind == ComponentKind::Tree {
            if !path.is_dir() {
                return Err(missing_runtime(component_id));
            }
            let (digest, total, _) = component_tree_manifest(&path, component).map_err(|e| {
                issue_message(&RuntimeIssue {
                    code: e.code,
                    component: component_id.into(),
                    action: "repair".into(),
                })
            })?;
            if digest != component.sha256 || (component.size != 0 && component.size != total) {
                return Err(format!(
                    "Computer Use runtime `{component_id}` failed integrity check. Repair or reinstall Grok App."
                ));
            }
            if component_id == PLAYWRIGHT_RUNTIME {
                let pkg = fs::read(path.join("package.json"))
                    .map_err(|_| missing_runtime(component_id))?;
                if !is_materialized_playwright_runtime(&pkg) {
                    return Err(issue_message(&RuntimeIssue::import_failed(component_id)));
                }
            }
            if component_id == CHROMIUM {
                let exe = chromium_executable_path(&path, &component.arch)?;
                let bytes = fs::read(&exe).map_err(|_| missing_runtime(component_id))?;
                if !looks_like_native_binary(&bytes) {
                    return Err(issue_message(&RuntimeIssue::not_executable(component_id)));
                }
            }
            return Ok(path);
        }
        if !path.is_file() {
            return Err(missing_runtime(component_id));
        }
        let actual = sha256_file(&path)?;
        if actual != component.sha256 {
            return Err(format!(
                "Computer Use runtime `{component_id}` failed integrity check. Repair or reinstall Grok App."
            ));
        }
        let bytes = fs::read(&path).map_err(|_| missing_runtime(component_id))?;
        if component.size != 0 && component.size != bytes.len() as u64 {
            return Err(format!(
                "Computer Use runtime `{component_id}` failed integrity check. Repair or reinstall Grok App."
            ));
        }
        if let Err(issue) = component_health(component_id, &path, &bytes) {
            return Err(issue_message(&issue));
        }
        Ok(path)
    }

    fn write_mcp_into_active(&self, script: &str, protocol: &str) -> Result<(), String> {
        let mut manifest = self.load_active_manifest()?;
        let pack = self.pack_dir(&manifest.pack_id);
        for part in mcp_parts(script, protocol) {
            let dest = pack.join(&part.relpath);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&dest, &part.bytes).map_err(|e| e.to_string())?;
            manifest.components.insert(
                part.id.clone(),
                RuntimeComponent {
                    id: part.id,
                    version: part.version,
                    arch: "any".into(),
                    relpath: part.relpath,
                    sha256: sha256_bytes(&part.bytes),
                    size: part.bytes.len() as u64,
                    kind: ComponentKind::File,
                },
            );
        }
        fs::write(
            pack.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn ensure_embedded_mcp(&self, script: &str, protocol: &str) -> Result<(), String> {
        let required = required_runtime_components();
        if let Ok(manifest) = self.load_active_manifest() {
            if component_hash_matches(&manifest, MCP_SERVER, script.as_bytes())
                && component_hash_matches(&manifest, MCP_PROTOCOL, protocol.as_bytes())
                && self.pack_reusable(&manifest.pack_id, required)
            {
                return Ok(());
            }
            if self.write_mcp_into_active(script, protocol).is_ok() {
                return Ok(());
            }
            let mut parts = Vec::new();
            let mut trees = Vec::new();
            for (id, component) in &manifest.components {
                if id == MCP_SERVER || id == MCP_PROTOCOL {
                    continue;
                }
                let src = self.pack_dir(&manifest.pack_id).join(&component.relpath);
                if component.kind == ComponentKind::Tree || src.is_dir() {
                    trees.push((component.clone(), src));
                    continue;
                }
                let bytes = fs::read(&src).map_err(|e| e.to_string())?;
                parts.push(NewComponent {
                    id: id.clone(),
                    version: component.version.clone(),
                    relpath: component.relpath.clone(),
                    bytes,
                });
            }
            parts.extend(mcp_parts(script, protocol));
            let canonical = mcp_pack_id(script, protocol, &parts);
            let pack_id = if self.pack_reusable(&canonical, required) {
                canonical
            } else if self.pack_dir(&canonical).exists() {
                unique_pack_id("mcp")
            } else {
                canonical
            };
            if !self.pack_reusable(&pack_id, required) {
                self.install_pack_full(&pack_id, parts, &trees)?;
            }
            self.copy_listing_siblings(&self.pack_dir(&manifest.pack_id), &manifest, &pack_id)?;
            return self.activate_pack(&pack_id);
        }
        let parts = mcp_parts(script, protocol);
        let pack_id = mcp_pack_id(script, protocol, &parts);
        if !self.pack_reusable(&pack_id, required) {
            if self.pack_dir(&pack_id).exists() {
                let pack_id = unique_pack_id("mcp");
                self.install_pack(&pack_id, parts)?;
                return self.activate_pack(&pack_id);
            }
            self.install_pack(&pack_id, parts)?;
        }
        self.activate_pack(&pack_id)
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(".staging")
    }

    pub fn staging_present(&self) -> bool {
        self.staging_dir()
            .read_dir()
            .map(|mut it| it.next().is_some())
            .unwrap_or(false)
    }

    pub fn discard_interrupted_upgrade(&self) -> Result<(), String> {
        let staging = self.staging_dir();
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|e| {
                format!(
                    "cannot clear interrupted Computer Use runtime update ({e}). Repair stays inside the App private directory. System Node on PATH is not used."
                )
            })?;
        }
        Ok(())
    }

    fn probe_write(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let probe = self.root.join(".write-probe");
        fs::write(&probe, b"ok").map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&probe);
        Ok(())
    }

    pub fn diagnose(&self, required: &[&str]) -> Vec<RuntimeIssue> {
        let mut issues = Vec::new();
        if self.staging_present() {
            issues.push(RuntimeIssue::interrupted_upgrade());
        }
        if self.probe_write().is_err() {
            issues.push(RuntimeIssue::permission_denied("runtime"));
            return issues;
        }
        let Some(pack_id) = self.read_pointer().active else {
            for id in required {
                issues.push(RuntimeIssue::missing_file(id));
            }
            return issues;
        };
        issues.extend(self.diagnose_pack(&pack_id, required));
        issues
    }

    pub fn diagnose_pack(&self, pack_id: &str, required: &[&str]) -> Vec<RuntimeIssue> {
        let mut issues = Vec::new();
        let path = self.pack_dir(pack_id).join("manifest.json");
        let Ok(raw) = fs::read(&path) else {
            for id in required {
                issues.push(RuntimeIssue::missing_file(id));
            }
            return issues;
        };
        let Ok(manifest) = serde_json::from_slice::<RuntimeManifest>(&raw) else {
            for id in required {
                issues.push(RuntimeIssue::missing_file(id));
            }
            return issues;
        };
        for id in required {
            let Some(component) = manifest.components.get(*id) else {
                issues.push(RuntimeIssue::missing_file(id));
                continue;
            };
            if component.arch != host_arch() && component.arch != "any" {
                issues.push(RuntimeIssue::arch_mismatch(id));
                continue;
            }
            let path = self.pack_dir(pack_id).join(&component.relpath);
            if component.kind == ComponentKind::Tree {
                if !path.is_dir() {
                    issues.push(RuntimeIssue::missing_file(id));
                    continue;
                }
                match component_tree_manifest(&path, component) {
                    Ok((digest, total, _)) => {
                        if digest != component.sha256
                            || (component.size != 0 && component.size != total)
                        {
                            issues.push(RuntimeIssue::hash_mismatch(id));
                        } else if *id == PLAYWRIGHT_RUNTIME {
                            match fs::read(path.join("package.json")) {
                                Ok(pkg) if is_materialized_playwright_runtime(&pkg) => {}
                                _ => issues.push(RuntimeIssue::import_failed(id)),
                            }
                        } else if *id == CHROMIUM {
                            match chromium_executable_path(&path, &component.arch)
                                .and_then(|path| fs::read(path).map_err(|e| e.to_string()))
                            {
                                Ok(bytes) if looks_like_native_binary(&bytes) => {}
                                _ => issues.push(RuntimeIssue::not_executable(id)),
                            }
                        }
                    }
                    Err(error) => issues.push(RuntimeIssue {
                        code: error.code,
                        component: (*id).into(),
                        action: "repair".into(),
                    }),
                }
                continue;
            }
            if !path.is_file() {
                issues.push(RuntimeIssue::missing_file(id));
                continue;
            }
            match sha256_file(&path) {
                Ok(hash) if hash != component.sha256 => {
                    issues.push(RuntimeIssue::hash_mismatch(id));
                }
                Err(_) => issues.push(RuntimeIssue::permission_denied(id)),
                Ok(_) => {
                    if let Ok(bytes) = fs::read(&path) {
                        if component.size != 0 && component.size != bytes.len() as u64 {
                            issues.push(RuntimeIssue::hash_mismatch(id));
                        } else if let Err(issue) = component_health(id, &path, &bytes) {
                            issues.push(issue);
                        }
                    }
                }
            }
        }
        issues
    }

    pub fn diagnose_versions(&self, expected: &[(&str, &str)]) -> Vec<RuntimeIssue> {
        let mut issues = self.diagnose(&expected.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        if let Ok(manifest) = self.load_active_manifest() {
            for (id, version) in expected {
                if let Some(component) = manifest.components.get(*id) {
                    if component.version != *version {
                        issues.push(RuntimeIssue::version_mismatch(id));
                    }
                }
            }
        }
        issues
    }

    pub fn install_from_bundle(&self, bundle_dir: &Path) -> Result<String, String> {
        let canon_bundle =
            fs::canonicalize(bundle_dir).map_err(|_| missing_runtime("installer seed"))?;
        let manifest_path = canon_bundle.join("manifest.json");
        let raw = fs::read(&manifest_path).map_err(|_| missing_runtime("installer seed"))?;
        let manifest: RuntimeManifest = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        let mut parts = Vec::new();
        let mut trees = Vec::new();
        for (id, component) in &manifest.components {
            if component.relpath.split(['/', '\\']).any(|s| s == "..")
                || Path::new(&component.relpath).is_absolute()
            {
                return Err("installer seed path must stay inside the App bundle".into());
            }
            if component.arch != host_arch() && component.arch != "any" {
                return Err(format!(
                    "Computer Use installer seed arch {} does not match this App ({}). Reinstall Grok App. System Node on PATH is not used.",
                    component.arch,
                    host_arch()
                ));
            }
            let file = canon_bundle.join(&component.relpath);
            if component.kind == ComponentKind::Tree || file.is_dir() {
                trees.push((component.clone(), file));
                continue;
            }
            let bytes = fs::read(&file).map_err(|_| missing_runtime(id))?;
            if sha256_bytes(&bytes) != component.sha256 {
                return Err(format!(
                    "Computer Use installer seed `{id}` failed integrity check. Reinstall Grok App. System Node on PATH is not used."
                ));
            }
            parts.push(NewComponent {
                id: id.clone(),
                version: component.version.clone(),
                relpath: component.relpath.clone(),
                bytes,
            });
        }
        let digest = sha256_bytes(&raw);
        let canonical = format!("{}-{}", manifest.pack_id, &digest[..12]);
        self.discard_interrupted_upgrade()?;
        let required = required_runtime_components();
        let pack_id = if self.pack_reusable(&canonical, required) {
            canonical
        } else if self.pack_dir(&canonical).exists() {
            unique_pack_id("rpr")
        } else {
            canonical
        };
        if !self.pack_reusable(&pack_id, required) {
            self.install_pack_full(&pack_id, parts, &trees)?;
        }
        self.copy_listing_siblings(&canon_bundle, &manifest, &pack_id)?;
        self.activate_pack(&pack_id)?;
        Ok(pack_id)
    }

    pub fn repair_from_bundle(&self, bundle_dir: &Path) -> Result<String, String> {
        self.discard_interrupted_upgrade()?;
        let required = required_runtime_components();
        if let Some(active) = self.read_pointer().active.clone() {
            if self.pack_reusable(&active, required)
                && self.pack_matches_bundle(&active, bundle_dir)?
            {
                return Ok(active);
            }
        }
        let pointer = self.read_pointer();
        let previous = pointer.active.clone();
        match self.install_from_bundle(bundle_dir) {
            Ok(pack) => Ok(pack),
            Err(err) => {
                if let Some(prev) = previous {
                    let mut restored = self.read_pointer();
                    if restored.active.as_deref() != Some(prev.as_str()) {
                        restored.active = Some(prev);
                        let _ = self.write_pointer(&restored);
                    }
                }
                let _ = self.discard_interrupted_upgrade();
                Err(err)
            }
        }
    }

    fn pack_reusable(&self, pack_id: &str, required: &[&str]) -> bool {
        self.pack_dir(pack_id).join("manifest.json").is_file()
            && self.diagnose_pack(pack_id, required).is_empty()
    }

    fn pack_matches_bundle(&self, pack_id: &str, bundle_dir: &Path) -> Result<bool, String> {
        let pack_raw = fs::read(self.pack_dir(pack_id).join("manifest.json"))
            .map_err(|_| missing_runtime("manifest"))?;
        let pack: RuntimeManifest = serde_json::from_slice(&pack_raw).map_err(|e| e.to_string())?;
        let bundle_raw = fs::read(bundle_dir.join("manifest.json"))
            .map_err(|_| missing_runtime("installer seed"))?;
        let bundle: RuntimeManifest =
            serde_json::from_slice(&bundle_raw).map_err(|e| e.to_string())?;
        for (id, bundle_comp) in &bundle.components {
            if id == MCP_SERVER || id == MCP_PROTOCOL {
                continue;
            }
            let Some(pack_comp) = pack.components.get(id) else {
                return Ok(false);
            };
            if pack_comp.sha256 != bundle_comp.sha256 || pack_comp.kind != bundle_comp.kind {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn has_previous_pack(&self) -> bool {
        self.read_pointer().previous.is_some()
    }

    /// Product spawn inputs. Must resolve node, worker, materialized Playwright,
    /// and Chromium independently — never guess worker from an archive parent.
    pub fn product_runtime_paths(&self) -> Result<ProductRuntimePaths, String> {
        let browser = self.resolve(CHROMIUM)?;
        let browser = if browser.is_dir() {
            let exe = chromium_executable_path(&browser, &host_arch())?;
            if !exe.is_file() {
                return Err(issue_message(&RuntimeIssue::missing_file(CHROMIUM)));
            }
            exe
        } else {
            browser
        };
        Ok(ProductRuntimePaths {
            node: self.resolve(JS_RUNTIME)?,
            worker: self.resolve(BROWSER_WORKER)?,
            playwright_runtime: self.resolve(PLAYWRIGHT_RUNTIME)?,
            browser,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProductRuntimePaths {
    pub node: PathBuf,
    pub worker: PathBuf,
    pub playwright_runtime: PathBuf,
    pub browser: PathBuf,
}

fn mcp_parts(script: &str, protocol: &str) -> Vec<NewComponent> {
    vec![
        NewComponent {
            id: MCP_SERVER.into(),
            version: "app-embed".into(),
            relpath: "mcp/server.mjs".into(),
            bytes: script.as_bytes().to_vec(),
        },
        NewComponent {
            id: MCP_PROTOCOL.into(),
            version: "app-embed".into(),
            relpath: "mcp/protocol.mjs".into(),
            bytes: protocol.as_bytes().to_vec(),
        },
    ]
}

fn component_hash_matches(manifest: &RuntimeManifest, id: &str, bytes: &[u8]) -> bool {
    manifest.components.get(id).is_some_and(|c| {
        c.sha256 == sha256_bytes(bytes) && (c.arch == host_arch() || c.arch == "any")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static PATH_LOCK: Mutex<()> = Mutex::new(());
    static SEED_LOCK: Mutex<()> = Mutex::new(());

    pub(super) fn store() -> (RuntimeStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("grok-cu-rt-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        (RuntimeStore::new(dir.clone()), dir)
    }

    #[test]
    fn computer_use_runtime_missing_pack_does_not_use_system_node() {
        let (store, dir) = store();
        let err = store.resolve(JS_RUNTIME).unwrap_err();
        assert!(err.contains("App private directory"), "{err}");
        assert!(!err.to_ascii_lowercase().contains("node.js"), "{err}");
        assert!(err.contains("PATH is not used"), "{err}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_placeholder_is_not_healthy() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-placeholder",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "0".into(),
                    relpath: "bin/js-runtime".into(),
                    bytes: PLACEHOLDER_JS_RUNTIME.to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-placeholder").unwrap();
        let err = store.resolve(JS_RUNTIME).unwrap_err();
        assert!(err.contains("placeholder"), "{err}");
        assert!(err.contains("PATH is not used"), "{err}");
        let issues = store.diagnose(&[JS_RUNTIME]);
        assert!(issues.iter().any(|i| i.code == "placeholder"), "{issues:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_rejects_wrong_hash_and_arch() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-a",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "1".into(),
                    relpath: "bin/node.txt".into(),
                    bytes: b"runtime-a".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-a").unwrap();
        let path = store.resolve(JS_RUNTIME).unwrap();
        fs::write(&path, b"tampered").unwrap();
        let err = store.resolve(JS_RUNTIME).unwrap_err();
        assert!(err.contains("integrity check"), "{err}");

        let mut manifest = store.load_active_manifest().unwrap();
        manifest.components.get_mut(JS_RUNTIME).unwrap().sha256 = sha256_file(&path).unwrap();
        manifest.components.get_mut(JS_RUNTIME).unwrap().arch = "sparc-unknown".into();
        fs::write(
            store.pack_dir("pack-a").join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let err = store.resolve(JS_RUNTIME).unwrap_err();
        assert!(err.contains("arch"), "{err}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_activate_is_atomic_and_rollback_restores_previous() {
        let (store, dir) = store();
        let bundle = write_full_seed_bundle(&dir);
        let previous = store.install_from_bundle(&bundle).unwrap();
        let original = fs::read(store.resolve(JS_RUNTIME).unwrap()).unwrap();
        store
            .install_pack(
                "pack-b",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "2".into(),
                    relpath: "bin/node.txt".into(),
                    bytes: b"runtime-b".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-b").unwrap();
        assert_eq!(
            fs::read(store.resolve(JS_RUNTIME).unwrap()).unwrap(),
            b"runtime-b"
        );
        assert_eq!(store.rollback_pack().unwrap(), previous);
        assert_eq!(
            fs::read(store.resolve(JS_RUNTIME).unwrap()).unwrap(),
            original
        );
        assert_eq!(store.read_pointer().previous.as_deref(), Some("pack-b"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_embed_mcp_keeps_existing_js_runtime() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-a",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "1".into(),
                    relpath: "bin/node.txt".into(),
                    bytes: b"node-stub".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-a").unwrap();
        store
            .ensure_embedded_mcp("export const s=1\n", "export const p=1\n")
            .unwrap();
        assert_eq!(
            fs::read(store.resolve(JS_RUNTIME).unwrap()).unwrap(),
            b"node-stub"
        );
        assert!(store
            .resolve(MCP_SERVER)
            .unwrap()
            .ends_with(Path::new("server.mjs")));
        assert!(store
            .resolve(MCP_PROTOCOL)
            .unwrap()
            .ends_with(Path::new("protocol.mjs")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_refuses_path_escape_and_does_not_write_outside_home() {
        let (store, dir) = store();
        let err = store
            .install_pack(
                "pack-x",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "1".into(),
                    relpath: "../outside.bin".into(),
                    bytes: b"nope".to_vec(),
                }],
            )
            .unwrap_err();
        assert!(err.contains("relative"), "{err}");
        assert!(!dir.join("outside.bin").exists());
        let _ = fs::remove_dir_all(dir);
    }

    fn write_bundle(root: &Path, files: &[(&str, &str, &[u8])]) -> PathBuf {
        let bundle = root.join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        let mut components = BTreeMap::new();
        for (id, relpath, bytes) in files {
            let path = bundle.join(relpath);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, bytes).unwrap();
            components.insert(
                (*id).to_string(),
                RuntimeComponent {
                    id: (*id).to_string(),
                    version: "seed-1".into(),
                    arch: "any".into(),
                    relpath: (*relpath).to_string(),
                    sha256: sha256_bytes(bytes),
                    size: bytes.len() as u64,
                    kind: ComponentKind::File,
                },
            );
        }
        let manifest = RuntimeManifest {
            pack_id: "seed".into(),
            schema_version: PACK_SCHEMA_VERSION,
            components,
            compatibility: RuntimeCompatibility {
                protocol: 1,
                js_runtime_version: "20.18.0".into(),
                playwright_core_version: "1.48.0".into(),
                worker_source_sha256: String::new(),
            },
        };
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        bundle
    }

    fn file_component(id: &str, relpath: &str, bytes: &[u8]) -> RuntimeComponent {
        RuntimeComponent {
            id: id.into(),
            version: "1".into(),
            arch: "any".into(),
            relpath: relpath.into(),
            sha256: sha256_bytes(bytes),
            size: bytes.len() as u64,
            kind: ComponentKind::File,
        }
    }

    pub(super) fn fixture_target() -> crate::runtime_lock::RuntimeTarget {
        crate::runtime_lock::RuntimeTarget::parse(&host_arch()).expect("supported test target")
    }

    fn fixture_chromium_relpath() -> String {
        format!("chromium/{}", fixture_target().chromium_root())
    }

    pub(super) fn write_full_seed_bundle(root: &Path) -> PathBuf {
        let bundle = root.join("bundle");
        let target = fixture_target();
        let chrome_root = bundle.join(fixture_chromium_relpath());
        let chrome_executable = chrome_root.join(target.chromium_executable());
        fs::create_dir_all(chrome_executable.parent().unwrap()).unwrap();
        fs::create_dir_all(bundle.join("playwright/node_modules/playwright-core")).unwrap();
        fs::create_dir_all(bundle.join("mcp")).unwrap();
        fs::create_dir_all(bundle.join("bin")).unwrap();
        let chrome = b"MZ\x90\x00fake-chrome";
        let credits = b"credits-ok";
        let node = b"MZ\x90\x00fake-node";
        let worker = b"export const grokComputerUseWorkerEntry = 1;\n";
        let sibling = b"export const loopback = 1;\n";
        let listing = format!(
            "loopback.mjs\t{}\t{}\n",
            sibling.len(),
            sha256_bytes(sibling)
        );
        let tgz = b"\x1f\x8b\x08gzip-stub";
        let pkg = br#"{"name":"playwright-core","version":"1.48.0"}"#;
        let mcp_s = b"export const s=1\n";
        let mcp_p = b"export const p=1\n";
        fs::write(&chrome_executable, chrome).unwrap();
        fs::write(chrome_root.join("CREDITS.html"), credits).unwrap();
        if crate::runtime_chromium::is_macos(target.arch()) {
            let framework = chrome_root
                .join("Chromium.app/Contents/Frameworks/Chromium Framework.framework/Versions/130.0.6723.31");
            for name in ["Resources", "Libraries", "Helpers"] {
                fs::create_dir_all(framework.join(name)).unwrap();
            }
            fs::write(framework.join("Chromium Framework"), chrome).unwrap();
            let mut links: Vec<_> = crate::runtime_chromium::macos_links().into_iter().collect();
            links.sort_by_key(|(path, _)| !path.ends_with("/Versions/Current"));
            for (path, destination) in links {
                crate::runtime_chromium::create_link(
                    &chrome_root.join(&path),
                    Path::new(&destination),
                    !path.ends_with("/Chromium Framework"),
                )
                .unwrap();
            }
        }
        fs::write(bundle.join(target.node_relpath()), node).unwrap();
        fs::write(bundle.join("playwright/worker.mjs"), worker).unwrap();
        fs::write(bundle.join("playwright/loopback.mjs"), sibling).unwrap();
        fs::write(bundle.join("playwright/modules.sha256list"), &listing).unwrap();
        fs::write(bundle.join("playwright/playwright-core-1.48.0.tgz"), tgz).unwrap();
        fs::write(
            bundle.join("playwright/node_modules/playwright-core/package.json"),
            pkg,
        )
        .unwrap();
        fs::write(bundle.join("mcp/server.mjs"), mcp_s).unwrap();
        fs::write(bundle.join("mcp/protocol.mjs"), mcp_p).unwrap();
        let (chrome_digest, chrome_bytes, _) =
            crate::runtime_chromium::tree_manifest(&chrome_root, target.arch()).unwrap();
        let (pw_digest, pw_bytes, _) = crate::playwright_materialize::tree_manifest(
            &bundle.join("playwright/node_modules/playwright-core"),
        )
        .unwrap();
        let mut components = BTreeMap::new();
        components.insert(
            JS_RUNTIME.into(),
            file_component(JS_RUNTIME, target.node_relpath(), node),
        );
        components.insert(
            PLAYWRIGHT_ARCHIVE.into(),
            file_component(
                PLAYWRIGHT_ARCHIVE,
                "playwright/playwright-core-1.48.0.tgz",
                tgz,
            ),
        );
        components.insert(
            BROWSER_WORKER.into(),
            file_component(BROWSER_WORKER, "playwright/worker.mjs", worker),
        );
        components.insert(
            BROWSER_WORKER_MODULES.into(),
            file_component(
                BROWSER_WORKER_MODULES,
                "playwright/modules.sha256list",
                listing.as_bytes(),
            ),
        );
        components.insert(
            MCP_SERVER.into(),
            file_component(MCP_SERVER, "mcp/server.mjs", mcp_s),
        );
        components.insert(
            MCP_PROTOCOL.into(),
            file_component(MCP_PROTOCOL, "mcp/protocol.mjs", mcp_p),
        );
        components.insert(
            PLAYWRIGHT_RUNTIME.into(),
            RuntimeComponent {
                id: PLAYWRIGHT_RUNTIME.into(),
                version: "1.48.0".into(),
                arch: "any".into(),
                relpath: "playwright/node_modules/playwright-core".into(),
                sha256: pw_digest,
                size: pw_bytes,
                kind: ComponentKind::Tree,
            },
        );
        components.insert(
            CHROMIUM.into(),
            RuntimeComponent {
                id: CHROMIUM.into(),
                version: "130.0.6723.31".into(),
                arch: host_arch(),
                relpath: fixture_chromium_relpath(),
                sha256: chrome_digest,
                size: chrome_bytes,
                kind: ComponentKind::Tree,
            },
        );
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&RuntimeManifest {
                pack_id: "seed".into(),
                schema_version: PACK_SCHEMA_VERSION,
                components,
                compatibility: RuntimeCompatibility {
                    protocol: 1,
                    js_runtime_version: "20.18.0".into(),
                    playwright_core_version: "1.48.0".into(),
                    worker_source_sha256: sha256_bytes(worker),
                },
            })
            .unwrap(),
        )
        .unwrap();
        bundle
    }

    #[test]
    fn same_version_corrupt_tree_rebuilds_pack_healthy_pack_is_reused() {
        let (store, dir) = store();
        let bundle = write_full_seed_bundle(&dir);
        let credits = b"credits-ok";

        let first = store.repair_from_bundle(&bundle).unwrap();
        let second = store.repair_from_bundle(&bundle).unwrap();
        assert_eq!(first, second, "healthy pack must be reused");

        let credits_path = store
            .pack_dir(&first)
            .join(fixture_chromium_relpath())
            .join("CREDITS.html");
        fs::write(&credits_path, b"tampered-tree").unwrap();
        let issues = store.diagnose_pack(&first, &[CHROMIUM]);
        assert!(
            issues
                .iter()
                .any(|i| i.component == CHROMIUM && i.code == "hash_mismatch"),
            "{issues:?}"
        );
        let pointer_before = fs::read(dir.join("current.json")).unwrap();
        let repaired = store.repair_from_bundle(&bundle).unwrap();
        assert_ne!(repaired, first, "corrupt pack must not be reused");
        let restored = fs::read(
            store
                .pack_dir(&repaired)
                .join(fixture_chromium_relpath())
                .join("CREDITS.html"),
        )
        .unwrap();
        assert_eq!(restored, credits);
        assert!(
            store.diagnose_pack(&repaired, &[CHROMIUM]).is_empty(),
            "{:?}",
            store.diagnose_pack(&repaired, &[CHROMIUM])
        );
        assert_eq!(fs::read(&credits_path).unwrap(), b"tampered-tree");

        let bad_bundle = dir.join("bad-bundle");
        fs::create_dir_all(&bad_bundle).unwrap();
        let mut bad_manifest: RuntimeManifest =
            serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
        bad_manifest.pack_id = "broken".into();
        if let Some(js) = bad_manifest.components.get_mut(JS_RUNTIME) {
            js.sha256 = "0".repeat(64);
        }
        fs::create_dir_all(bad_bundle.join("bin")).unwrap();
        fs::write(
            bad_bundle.join(fixture_target().node_relpath()),
            b"MZ\x90\x00fake-node",
        )
        .unwrap();
        fs::write(
            bad_bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&bad_manifest).unwrap(),
        )
        .unwrap();
        let pointer = fs::read(dir.join("current.json")).unwrap();
        let err = store.repair_from_bundle(&bad_bundle).unwrap_err();
        assert!(
            err.contains("missing") || err.contains("integrity") || err.contains("hash"),
            "{err}"
        );
        assert_eq!(
            fs::read(dir.join("current.json")).unwrap(),
            pointer,
            "failed repair must keep current pointer"
        );
        let _ = pointer_before;
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_diagnose_missing_hash_arch_version_and_port() {
        let (store, dir) = store();
        let missing = store.diagnose(&[JS_RUNTIME, PLAYWRIGHT]);
        assert!(missing.iter().any(|i| i.code == "missing_file"));
        assert!(missing.iter().all(|i| i.action == "repair"));

        store
            .install_pack(
                "pack-a",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "old".into(),
                    relpath: "bin/js-runtime".into(),
                    bytes: b"runtime-a".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-a").unwrap();
        let path = store.resolve(JS_RUNTIME).unwrap();
        fs::write(&path, b"tampered").unwrap();
        let hashed = store.diagnose(&[JS_RUNTIME]);
        assert!(hashed.iter().any(|i| i.code == "hash_mismatch"));

        let versions = store.diagnose_versions(&[(JS_RUNTIME, "seed-1")]);
        assert!(versions.iter().any(|i| i.code == "version_mismatch"));

        let port = RuntimeIssue::port_failed();
        assert_eq!(port.code, "port_failed");
        assert_ne!(port.action, "download");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_interrupted_upgrade_is_discarded_without_activating() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-a",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "1".into(),
                    relpath: "bin/js-runtime".into(),
                    bytes: b"stable".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-a").unwrap();
        let staging = dir.join(".staging").join("partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("broken.bin"), b"partial").unwrap();
        let issues = store.diagnose(&[JS_RUNTIME]);
        assert!(issues.iter().any(|i| i.code == "interrupted_upgrade"));
        store.discard_interrupted_upgrade().unwrap();
        assert!(!store.staging_present());
        assert_eq!(
            fs::read(store.resolve(JS_RUNTIME).unwrap()).unwrap(),
            b"stable"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_repair_from_bundle_ignores_path_node_and_restores_files() {
        let (store, dir) = store();
        let decoy_dir = dir.join("path-decoy");
        fs::create_dir_all(&decoy_dir).unwrap();
        fs::write(decoy_dir.join("node.exe"), b"SYSTEM-NODE").unwrap();
        fs::write(decoy_dir.join("node"), b"SYSTEM-NODE").unwrap();
        let bundle = write_bundle(
            &dir,
            &[
                (JS_RUNTIME, "bin/js-runtime", VERSION_PIN_JS_RUNTIME_STUB),
                (
                    PLAYWRIGHT_ARCHIVE,
                    "playwright/playwright-core-1.48.0.tgz",
                    b"\x1f\x8b\x08gzip-stub",
                ),
            ],
        );
        store
            .install_pack(
                "pack-bad",
                vec![NewComponent {
                    id: JS_RUNTIME.into(),
                    version: "0".into(),
                    relpath: "bin/js-runtime".into(),
                    bytes: b"corrupt".to_vec(),
                }],
            )
            .unwrap();
        store.activate_pack("pack-bad").unwrap();
        fs::write(store.resolve(JS_RUNTIME).unwrap(), b"tampered").unwrap();

        let _path_guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_path = std::env::var_os("PATH");
        let decoy_path = decoy_dir.to_string_lossy().into_owned();
        std::env::set_var("PATH", &decoy_path);
        let pack = store.repair_from_bundle(&bundle).unwrap();
        match prev_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        assert!(pack.starts_with("seed-"));
        let restored = store
            .pack_dir(&store.load_active_manifest().unwrap().pack_id)
            .join("bin/js-runtime");
        let js = fs::read(&restored).unwrap();
        assert_eq!(js, VERSION_PIN_JS_RUNTIME_STUB);
        assert_ne!(js.as_slice(), b"SYSTEM-NODE");
        assert!(
            is_placeholder_runtime(&js),
            "version-pin text stub must not count as a healthy js-runtime"
        );
        let err = store.resolve(JS_RUNTIME).unwrap_err();
        assert!(err.contains("placeholder"), "{err}");
        assert!(err.contains("PATH is not used"), "{err}");
        assert!(
            store
                .diagnose(&[JS_RUNTIME])
                .iter()
                .any(|i| i.code == "placeholder"),
            "{:?}",
            store.diagnose(&[JS_RUNTIME])
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_installer_seed_repairs_from_shipped_resources() {
        let _seed_guard = SEED_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let seed = crate::runtime_test_seed::path();
        assert!(
            seed.join("manifest.json").is_file(),
            "installer seed missing at {}",
            seed.display()
        );
        let (store, dir) = store();
        store.repair_from_bundle(&seed).unwrap();
        let issues = store.diagnose(&[JS_RUNTIME, PLAYWRIGHT]);
        assert!(
            issues.iter().all(|i| i.code != "placeholder"),
            "shipped native js-runtime/playwright must not be text stubs: {issues:?}"
        );
        let js = store.resolve(JS_RUNTIME).expect("js-runtime must resolve");
        assert!(js.starts_with(&dir), "{}", js.display());
        let bytes = fs::read(&js).unwrap();
        assert!(
            looks_like_native_binary(&bytes),
            "js-runtime must be a native executable, got {} bytes",
            bytes.len()
        );
        assert!(!is_placeholder_runtime(&bytes));
        let mut command = std::process::Command::new(&js);
        command.arg("--version").env_clear();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        let out = command.output().expect("run pack js-runtime");
        let ver = String::from_utf8_lossy(&out.stdout);
        assert!(
            ver.trim() == "v20.18.0",
            "pack js-runtime --version {ver:?} status={}",
            out.status
        );
        let pw = store
            .resolve(PLAYWRIGHT_ARCHIVE)
            .expect("playwright-core archive must resolve");
        assert!(pw.starts_with(&dir), "{}", pw.display());
        let pw_bytes = fs::read(&pw).unwrap();
        assert!(
            !is_placeholder_playwright(&pw_bytes),
            "shipped PLAYWRIGHT archive must not be the 53-byte worker stub"
        );
        assert_eq!(sha256_bytes(&pw_bytes), PLAYWRIGHT_CORE_TGZ_SHA256);
        let runtime = store
            .resolve(PLAYWRIGHT_RUNTIME)
            .expect("materialized playwright-runtime must resolve");
        assert!(runtime.starts_with(&dir), "{}", runtime.display());
        assert!(runtime.is_dir(), "{}", runtime.display());
        let pkg: serde_json::Value =
            serde_json::from_slice(&fs::read(runtime.join("package.json")).unwrap()).unwrap();
        assert_eq!(pkg["name"], "playwright-core");
        assert_eq!(pkg["version"], REQUIRED_PLAYWRIGHT_CORE_VERSION);
        let worker = store
            .resolve(BROWSER_WORKER)
            .expect("production browser-worker must resolve");
        let worker_bytes = fs::read(&worker).unwrap();
        assert!(!is_placeholder_playwright(&worker_bytes));
        store
            .resolve(BROWSER_WORKER_MODULES)
            .expect("browser-worker-modules listing must resolve");
        let manifest = store.load_active_manifest().unwrap();
        let pw_comp = manifest.components.get(PLAYWRIGHT).unwrap();
        assert_eq!(pw_comp.version, REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert_eq!(
            manifest.compatibility.playwright_core_version,
            REQUIRED_PLAYWRIGHT_CORE_VERSION
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn computer_use_runtime_version_pin_and_owned_label_are_placeholders() {
        assert!(is_placeholder_runtime(PLACEHOLDER_JS_RUNTIME));
        assert!(is_placeholder_runtime(VERSION_PIN_JS_RUNTIME_STUB));
        assert!(is_placeholder_runtime(
            b"grok-cu-aarch64-macos-js-runtime-20.18.0\n"
        ));
        assert!(!is_placeholder_runtime(b"runtime-a"));
        assert!(!is_placeholder_runtime(b"MZ\x90\x00this-is-not-a-label"));
        assert!(is_placeholder_playwright(PLACEHOLDER_PLAYWRIGHT));
        assert!(!is_placeholder_playwright(&[0x1f, 0x8b, 0x08]));
    }

    #[test]
    fn computer_use_runtime_four_target_contract_refuses_cross_os_windows_blob() {
        assert_eq!(REQUIRED_PACK_ARCHS.len(), 4);
        assert!(REQUIRED_PACK_ARCHS.contains(&"x86_64-windows"));
        assert!(REQUIRED_PACK_ARCHS.contains(&"aarch64-macos"));
        assert!(REQUIRED_PACK_ARCHS.contains(&"x86_64-macos"));
        assert!(REQUIRED_PACK_ARCHS.contains(&"x86_64-linux"));
        assert_eq!(REQUIRED_JS_RUNTIME_VERSION, "20.18.0");
        assert_eq!(REQUIRED_PLAYWRIGHT_CORE_VERSION, "1.48.0");
        let contract = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("resources")
            .join("computer-use")
            .join("pack-targets.json");
        let spec: serde_json::Value =
            serde_json::from_slice(&fs::read(&contract).unwrap()).unwrap();
        assert_eq!(spec["jsRuntimeVersion"], "20.18.0");
        assert_eq!(spec["playwrightCoreVersion"], "1.48.0");
        let targets = spec["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 4);
        for arch in REQUIRED_PACK_ARCHS {
            assert!(
                targets.iter().any(|t| t["arch"] == *arch),
                "missing {arch} in {spec}"
            );
        }
        assert_eq!(targets[1]["status"], "not_run");
        assert_eq!(targets[2]["status"], "not_run");
        assert_eq!(targets[3]["status"], "not_run");

        let (store, dir) = store();
        let bundle = dir.join("macos-fake");
        fs::create_dir_all(bundle.join("bin")).unwrap();
        fs::write(bundle.join("bin/js-runtime"), VERSION_PIN_JS_RUNTIME_STUB).unwrap();
        let mut components = BTreeMap::new();
        components.insert(
            JS_RUNTIME.to_string(),
            RuntimeComponent {
                id: JS_RUNTIME.into(),
                version: REQUIRED_JS_RUNTIME_VERSION.into(),
                arch: "aarch64-macos".into(),
                relpath: "bin/js-runtime".into(),
                sha256: sha256_bytes(VERSION_PIN_JS_RUNTIME_STUB),
                size: VERSION_PIN_JS_RUNTIME_STUB.len() as u64,
                kind: ComponentKind::File,
            },
        );
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&RuntimeManifest {
                pack_id: "seed-macos".into(),
                schema_version: PACK_SCHEMA_VERSION,
                components,
                compatibility: RuntimeCompatibility {
                    protocol: 1,
                    js_runtime_version: REQUIRED_JS_RUNTIME_VERSION.into(),
                    playwright_core_version: REQUIRED_PLAYWRIGHT_CORE_VERSION.into(),
                    worker_source_sha256: String::new(),
                },
            })
            .unwrap(),
        )
        .unwrap();
        let err = store.install_from_bundle(&bundle).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("arch"), "{err}");
        assert!(err.contains("PATH is not used"), "{err}");
        assert!(store.resolve(JS_RUNTIME).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    fn part(id: &str, relpath: &str, bytes: &[u8]) -> NewComponent {
        NewComponent {
            id: id.into(),
            version: "1".into(),
            relpath: relpath.into(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn pack_schema_version_is_independent_of_wire_protocol() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-schema",
                vec![part(JS_RUNTIME, "bin/node.txt", b"runtime-a")],
            )
            .unwrap();
        store.activate_pack("pack-schema").unwrap();
        let manifest = store.load_active_manifest().unwrap();
        assert_eq!(manifest.schema_version, PACK_SCHEMA_VERSION);
        assert_eq!(manifest.compatibility.protocol, 1);
        assert_ne!(
            manifest.schema_version, manifest.compatibility.protocol,
            "pack schema must not reuse the wire protocol number"
        );
        let js = manifest.components.get(JS_RUNTIME).unwrap();
        assert_eq!(js.size, b"runtime-a".len() as u64);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn required_runtime_components_include_worker_runtime_and_chromium() {
        let required = required_runtime_components();
        for id in [
            JS_RUNTIME,
            MCP_SERVER,
            MCP_PROTOCOL,
            PLAYWRIGHT_ARCHIVE,
            PLAYWRIGHT_RUNTIME,
            BROWSER_WORKER,
            BROWSER_WORKER_MODULES,
            CHROMIUM,
        ] {
            assert!(
                required.contains(&id),
                "{id} missing from required {required:?}"
            );
        }
    }

    #[test]
    fn placeholder_browser_worker_is_not_healthy() {
        let (store, dir) = store();
        store
            .install_pack(
                "pack-worker",
                vec![part(
                    BROWSER_WORKER,
                    "playwright/worker.mjs",
                    PLACEHOLDER_PLAYWRIGHT,
                )],
            )
            .unwrap();
        store.activate_pack("pack-worker").unwrap();
        let err = store.resolve(BROWSER_WORKER).unwrap_err();
        assert!(err.contains("placeholder"), "{err}");
        assert!(err.contains("PATH is not used"), "{err}");
        let issues = store.diagnose(&[BROWSER_WORKER]);
        assert!(
            issues
                .iter()
                .any(|i| i.code == "placeholder" && i.component == BROWSER_WORKER),
            "{issues:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tgz_bytes_are_not_a_materialized_playwright_runtime() {
        let (store, dir) = store();
        let tgz = {
            let mut bytes = vec![0x1f, 0x8b, 0x08];
            bytes.extend_from_slice(&[0u8; 64]);
            bytes
        };
        store
            .install_pack(
                "pack-tgz",
                vec![
                    part(
                        PLAYWRIGHT_ARCHIVE,
                        "playwright/playwright-core-1.48.0.tgz",
                        &tgz,
                    ),
                    part(
                        PLAYWRIGHT_RUNTIME,
                        "playwright/playwright-core-1.48.0.tgz",
                        &tgz,
                    ),
                ],
            )
            .unwrap();
        store.activate_pack("pack-tgz").unwrap();
        assert!(store.resolve(PLAYWRIGHT_ARCHIVE).is_ok());
        let err = store.resolve(PLAYWRIGHT_RUNTIME).unwrap_err();
        assert!(
            err.contains("placeholder")
                || err.to_ascii_lowercase().contains("import")
                || err.to_ascii_lowercase().contains("material"),
            "{err}"
        );
        let issues = store.diagnose(&[PLAYWRIGHT_ARCHIVE, PLAYWRIGHT_RUNTIME]);
        assert!(
            issues.iter().any(|i| i.component == PLAYWRIGHT_RUNTIME
                && (i.code == "import_failed" || i.code == "placeholder")),
            "{issues:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_worker_sibling_is_unhealthy() {
        let (store, dir) = store();
        let listing =
            b"loopback.mjs\t4\tabcd000000000000000000000000000000000000000000000000000000000000\n";
        store
            .install_pack(
                "pack-sib",
                vec![
                    part(
                        BROWSER_WORKER,
                        "playwright/worker.mjs",
                        b"export const grokComputerUseWorkerEntry = 1;\n",
                    ),
                    part(
                        BROWSER_WORKER_MODULES,
                        "playwright/modules.sha256list",
                        listing,
                    ),
                ],
            )
            .unwrap();
        store.activate_pack("pack-sib").unwrap();
        let issues = store.diagnose(&[BROWSER_WORKER, BROWSER_WORKER_MODULES]);
        assert!(
            issues
                .iter()
                .any(|i| i.component == BROWSER_WORKER_MODULES && i.code == "missing_sibling"),
            "{issues:?}"
        );
        let err = store.resolve(BROWSER_WORKER_MODULES).unwrap_err();
        assert!(
            err.to_ascii_lowercase().contains("sibling") || err.contains("missing"),
            "{err}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn chromium_missing_wrong_arch_hash_or_not_executable_is_unhealthy() {
        let (store, dir) = store();
        let missing = store.diagnose(&[CHROMIUM]);
        assert!(
            missing
                .iter()
                .any(|i| i.component == CHROMIUM && i.code == "missing_file"),
            "{missing:?}"
        );

        store
            .install_pack(
                "pack-chrome",
                vec![part(CHROMIUM, "chromium/chrome.exe", b"not-a-browser")],
            )
            .unwrap();
        store.activate_pack("pack-chrome").unwrap();
        let issues = store.diagnose(&[CHROMIUM]);
        assert!(
            issues
                .iter()
                .any(|i| i.component == CHROMIUM && i.code == "not_executable"),
            "{issues:?}"
        );
        let err = store.resolve(CHROMIUM).unwrap_err();
        assert!(
            err.to_ascii_lowercase().contains("executable") || err.contains("placeholder"),
            "{err}"
        );

        let path = store.pack_dir("pack-chrome").join("chromium/chrome.exe");
        fs::write(&path, b"MZ\x90\x00fake-chrome").unwrap();
        let mut manifest = store.load_active_manifest().unwrap();
        manifest.components.get_mut(CHROMIUM).unwrap().sha256 = sha256_file(&path).unwrap();
        manifest.components.get_mut(CHROMIUM).unwrap().size = fs::metadata(&path).unwrap().len();
        manifest.components.get_mut(CHROMIUM).unwrap().arch = "sparc-unknown".into();
        fs::write(
            store.pack_dir("pack-chrome").join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let arch_issues = store.diagnose(&[CHROMIUM]);
        assert!(
            arch_issues
                .iter()
                .any(|i| i.component == CHROMIUM && i.code == "arch_mismatch"),
            "{arch_issues:?}"
        );

        manifest.components.get_mut(CHROMIUM).unwrap().arch = host_arch();
        fs::write(
            store.pack_dir("pack-chrome").join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(&path, b"MZ\x90\x00tampered-chrome").unwrap();
        let hashed = store.diagnose(&[CHROMIUM]);
        assert!(
            hashed
                .iter()
                .any(|i| i.component == CHROMIUM && i.code == "hash_mismatch"),
            "{hashed:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn product_runtime_paths_resolve_worker_not_archive_parent() {
        let (store, dir) = store();
        let archive = {
            let mut bytes = vec![0x1f, 0x8b, 0x08];
            bytes.extend_from_slice(&[0u8; 64]);
            bytes
        };
        let decoy_worker = b"export const grokComputerUseWrongWorker = 1;\n";
        let real_worker = b"export const grokComputerUseWorkerEntry = 1;\n";
        let runtime_pkg = br#"{"name":"playwright-core","version":"1.48.0"}"#;
        let chrome = b"MZ\x90\x00fake-chrome";
        store
            .install_pack(
                "pack-paths",
                vec![
                    part(JS_RUNTIME, "bin/node.exe", b"MZ\x90\x00fake-node"),
                    part(
                        PLAYWRIGHT_ARCHIVE,
                        "playwright/playwright-core-1.48.0.tgz",
                        &archive,
                    ),
                    part(BROWSER_WORKER, "playwright/worker.mjs", decoy_worker),
                    part(BROWSER_WORKER, "worker/entry.mjs", real_worker),
                    part(
                        PLAYWRIGHT_RUNTIME,
                        "playwright/node_modules/playwright-core/package.json",
                        runtime_pkg,
                    ),
                    part(CHROMIUM, "chromium/chrome.exe", chrome),
                ],
            )
            .unwrap();
        store.activate_pack("pack-paths").unwrap();
        // Last insert of duplicate id wins in install_pack; rewrite worker to entry.mjs.
        let mut manifest = store.load_active_manifest().unwrap();
        manifest.components.insert(
            BROWSER_WORKER.into(),
            RuntimeComponent {
                id: BROWSER_WORKER.into(),
                version: "1".into(),
                arch: host_arch(),
                relpath: "worker/entry.mjs".into(),
                sha256: sha256_bytes(real_worker),
                size: real_worker.len() as u64,
                kind: ComponentKind::File,
            },
        );
        fs::write(
            store.pack_dir("pack-paths").join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(store.pack_dir("pack-paths").join("worker")).unwrap();
        fs::write(
            store.pack_dir("pack-paths").join("worker/entry.mjs"),
            real_worker,
        )
        .unwrap();

        let _path_guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let decoy = dir.join("path-decoy");
        fs::create_dir_all(&decoy).unwrap();
        fs::write(decoy.join("node.exe"), b"SYSTEM-NODE").unwrap();
        let prev_path = std::env::var_os("PATH");
        let prev_node = std::env::var_os("GROK_CU_NODE_FILE");
        std::env::set_var("PATH", decoy.as_os_str());
        std::env::set_var("GROK_CU_NODE_FILE", decoy.join("node.exe"));
        let result = store.product_runtime_paths();
        match prev_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        match prev_node {
            Some(v) => std::env::set_var("GROK_CU_NODE_FILE", v),
            None => std::env::remove_var("GROK_CU_NODE_FILE"),
        }
        let paths = result.expect("product paths should resolve independent worker");
        assert_eq!(
            paths.worker.file_name().and_then(|n| n.to_str()),
            Some("entry.mjs"),
            "must not guess worker.mjs from tgz parent: {}",
            paths.worker.display()
        );
        assert!(paths.node.starts_with(&dir), "{}", paths.node.display());
        assert!(!paths.node.starts_with(&decoy), "{}", paths.node.display());
        let worker_s = paths.worker.to_string_lossy().replace('\\', "/");
        assert!(
            !worker_s.contains("/tools/computer-use-browser/"),
            "{worker_s}"
        );
        assert!(!paths
            .playwright_runtime
            .ends_with("playwright-core-1.48.0.tgz"));
        assert!(
            paths.browser.ends_with("chrome.exe") || paths.browser.ends_with("chrome"),
            "{}",
            paths.browser.display()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn shipped_seed_js_runtime_relpath_matches_native_target() {
        let seed = crate::runtime_test_seed::path().join("manifest.json");
        let spec: serde_json::Value = serde_json::from_slice(&fs::read(&seed).unwrap()).unwrap();
        assert_eq!(
            spec["components"]["js-runtime"]["relpath"],
            fixture_target().node_relpath(),
            "{spec}"
        );
        assert!(
            spec["components"].get(PLAYWRIGHT_ARCHIVE).is_some(),
            "{spec}"
        );
        assert!(
            spec["components"].get(PLAYWRIGHT_RUNTIME).is_some(),
            "{spec}"
        );
        assert_eq!(spec["components"][PLAYWRIGHT_RUNTIME]["kind"], "tree");
        assert!(spec["components"].get(BROWSER_WORKER).is_some(), "{spec}");
        assert!(spec["components"].get(CHROMIUM).is_some(), "{spec}");
        assert_eq!(spec["components"][CHROMIUM]["kind"], "tree");
        assert!(
            spec["components"].get(BROWSER_WORKER_MODULES).is_some(),
            "{spec}"
        );
        assert_eq!(
            spec["schemaVersion"]
                .as_u64()
                .unwrap_or(spec["schema_version"].as_u64().unwrap_or(0)),
            PACK_SCHEMA_VERSION as u64
        );
    }

    #[test]
    fn shipped_browser_worker_is_not_placeholder() {
        let worker = crate::runtime_test_seed::path().join("playwright/worker.mjs");
        let bytes = fs::read(&worker).unwrap();
        assert!(!is_placeholder_playwright(&bytes), "len={}", bytes.len());
        assert!(bytes.len() > 1000, "len={}", bytes.len());
        assert!(!std::str::from_utf8(&bytes)
            .unwrap()
            .contains("pathname === \"/state\""));
    }

    #[test]
    fn prepare_check_fails_when_production_sibling_is_removed() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let shared = crate::runtime_test_seed::path();
        let sibling = shared.join("playwright").join("loopback.mjs");
        let before = fs::read(&sibling).expect("shared sibling");
        let isolated_root = if std::env::var_os("GROK_CU_TEST_SEED").is_some() {
            crate::runtime_mutation::isolated_clone_dir(shared.parent().expect("seed parent"))
        } else {
            crate::runtime_mutation::isolated_clone_dir(&repo)
        };
        crate::runtime_mutation::clone_tree_hardlink(&shared, &isolated_root.join("seed"))
            .expect("same-volume hardlink clone");
        fs::remove_file(isolated_root.join("seed/playwright/loopback.mjs")).unwrap();
        let mut ctx = crate::runtime_prepare::PrepareContext::shipped(repo, host_arch());
        ctx.seed_dir = isolated_root.join("seed");
        let err = crate::runtime_prepare::check(&ctx).expect_err("missing sibling");
        assert_eq!(err.code, "missing_sibling", "{err:?}");
        assert_eq!(fs::read(&sibling).expect("shared sibling after"), before);
        let _ = fs::remove_dir_all(&isolated_root);
    }
}
