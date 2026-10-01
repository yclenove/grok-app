//! Test-only selection of a prepared native seed. WSL and Windows can validate
//! separate seeds without rewriting each other's packaged resources. Product
//! resolution never reads this environment variable; integrity checks remain on.
use std::path::PathBuf;

pub(crate) fn path() -> PathBuf {
    let path = std::env::var_os("GROK_CU_TEST_SEED")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/computer-use/seed")
        });
    assert!(
        path.is_absolute(),
        "GROK_CU_TEST_SEED must be an absolute prepared-seed path"
    );
    path
}

pub(crate) fn node() -> PathBuf {
    let target = crate::runtime_lock::RuntimeTarget::parse(&crate::runtime::host_arch()).unwrap();
    path().join(target.node_relpath())
}
