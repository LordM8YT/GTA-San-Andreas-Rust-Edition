//! Give the Windows executable its Explorer/taskbar icon without extra build
//! dependencies: the MSVC linker accepts the precompiled resource directly.
//! Regenerate it with `python tools/make-icon.py`.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/sare.res");
    let target = |name: &str| std::env::var(name).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") == "windows" && target("CARGO_CFG_TARGET_ENV") == "msvc" {
        let resource = std::path::Path::new(&target("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("sare.res");
        println!("cargo:rustc-link-arg-bins={}", resource.display());
    }
}
