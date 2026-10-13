//! Compiled runtime contract. Source locks must match these pins; a local JSON
//! edit cannot opt the product into a different Node, Playwright or browser.

pub const REQUIRED_JS_RUNTIME_VERSION: &str = "20.18.0";
pub const REQUIRED_PLAYWRIGHT_CORE_VERSION: &str = "1.48.0";
pub const PLAYWRIGHT_CORE_TGZ_SHA256: &str =
    "60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562";
pub const REQUIRED_CHROMIUM_REVISION: &str = "1140";
pub const REQUIRED_CHROMIUM_VERSION: &str = "130.0.6723.31";

pub fn playwright_archive_name() -> String {
    format!("playwright-core-{REQUIRED_PLAYWRIGHT_CORE_VERSION}.tgz")
}

pub fn playwright_archive_relpath() -> String {
    format!("playwright/{}", playwright_archive_name())
}

#[cfg(test)]
#[path = "runtime_pins_tests.rs"]
mod tests;
