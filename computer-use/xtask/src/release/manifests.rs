//! Package-manager manifests rendered with real release archive hashes.
use super::*;

pub(super) fn homebrew(v: &str, repo: &str, sums: &BTreeMap<String, String>) -> Result<String> {
    let mut platforms = String::new();
    for (os, arches) in [("macos", [0, 1]), ("linux", [4, 3])] {
        platforms.push_str(&format!("  on_{os} do\n"));
        for (arch, index) in [("arm", arches[0]), ("intel", arches[1])] {
            let name = archive_name(v, &TARGETS[index]);
            let sha = sums.get(&name).context("missing Homebrew archive hash")?;
            platforms.push_str(&format!("    on_{arch} do\n      url \"https://github.com/{repo}/releases/download/cu-v{v}/{name}\"\n      sha256 \"{sha}\"\n    end\n"));
        }
        platforms.push_str("  end\n");
    }
    Ok(include_str!("../../../packages/homebrew/gcu.rb.in")
        .replace("@VERSION@", v)
        .replace("@REPOSITORY@", repo)
        .replace("@PLATFORMS@", &platforms))
}

pub(super) fn winget(v: &str, repo: &str, sums: &BTreeMap<String, String>) -> Result<String> {
    if v.contains(['-', '+']) {
        bail!("winget requires stable numeric version");
    }
    let name = archive_name(v, &TARGETS[2]);
    Ok(include_str!("../../../packages/winget/installer.yaml.in")
        .replace("@VERSION@", v)
        .replace("@REPOSITORY@", repo)
        .replace("@ARCHIVE@", &name)
        .replace(
            "@SHA256@",
            sums.get(&name).context("missing winget archive hash")?,
        ))
}

pub(super) fn winget_locale(v: &str, repo: &str) -> String {
    include_str!("../../../packages/winget/locale.yaml.in")
        .replace("@VERSION@", v)
        .replace("@REPOSITORY@", repo)
}
