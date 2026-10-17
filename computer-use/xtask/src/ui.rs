//! Embed the onboarding page and export locale JSON into the App catalog.
//!
//! The compressor and the TypeScript renderer live in `gcu-cli` and are
//! included by path so this crate does not depend on `gcu-cli`.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../../crates/gcu-cli/src/ui/compress.rs"]
mod compress;
#[path = "../../crates/gcu-cli/src/ui/export_ts.rs"]
mod export_ts;

/// `cargo xtask ui <embed|export-i18n|check>`.
///
/// Blocking: reads and writes the worktree. Timeout: none. Cancellation: none.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    match (args.next().as_deref(), args.next()) {
        (Some("embed"), None) => embed(),
        (Some("export-i18n"), None) => export_i18n(),
        (Some("check"), None) => check(),
        _ => bail!("usage: cargo xtask ui <embed|export-i18n|check>"),
    }
}

fn embed() -> Result<()> {
    let document = render_embed()?;
    fs::write(embed_path(), document).context("write onboarding embed")?;
    Ok(())
}

fn export_i18n() -> Result<()> {
    let root = i18n_root();
    if !root.is_dir() {
        bail!(
            "App i18n directory is not in this checkout: {}",
            root.display()
        );
    }
    for (locale, source) in locale_sources()? {
        let module = export_ts::export_module(&locale, &source)
            .map_err(|error| anyhow::anyhow!("export {locale}: {error}"))?;
        let path = root.join(&locale).join("computer-use.ts");
        fs::write(&path, module).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn check() -> Result<()> {
    let rendered = render_embed()?;
    let shipped = fs::read_to_string(embed_path()).context("read onboarding embed")?;
    if rendered.replace("\r\n", "\n") != shipped.replace("\r\n", "\n") {
        bail!("onboarding embed is stale; run cargo xtask ui embed");
    }
    let root = i18n_root();
    if !root.is_dir() {
        return Ok(());
    }
    for (locale, source) in locale_sources()? {
        let module = export_ts::export_module(&locale, &source)
            .map_err(|error| anyhow::anyhow!("export {locale}: {error}"))?;
        let path = root.join(&locale).join("computer-use.ts");
        let shipped =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if shipped.replace("\r\n", "\n") != module.replace("\r\n", "\n") {
            bail!(
                "{} is stale; run cargo xtask ui export-i18n",
                path.display()
            );
        }
    }
    Ok(())
}

fn render_embed() -> Result<String> {
    let html = fs::read_to_string(ui_path("onboarding.html")).context("read onboarding.html")?;
    let css = fs::read_to_string(ui_path("onboarding.css")).context("read onboarding.css")?;
    compress::embed_document(&html, &css).map_err(|error| anyhow::anyhow!("embed: {error}"))
}

fn locale_sources() -> Result<Vec<(String, String)>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(ui_path("locales")).context("read locales")? {
        let entry = entry.context("read locale entry")?;
        let name = entry.file_name();
        let Some(file_name) = name.to_str() else {
            continue;
        };
        if let Some(locale) = file_name.strip_suffix(".json") {
            names.push(locale.to_string());
        }
    }
    names.sort();
    let mut rows = Vec::new();
    for locale in names {
        let path = ui_path("locales").join(format!("{locale}.json"));
        let source =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        rows.push((locale, source));
    }
    Ok(rows)
}

fn ui_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ui")
        .join(name)
}

fn embed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/gcu-cli/src/ui/onboarding.embed.html")
}

fn i18n_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/i18n/messages")
}
