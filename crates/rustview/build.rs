use std::env;
use std::path::PathBuf;

#[path = "build/icon_registry.rs"]
mod icon_registry;

fn main() {
    println!("cargo:rerun-if-changed=ui/");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));

    // The icon manifest generates the Slint `IconId` enum + lookup and the matching Rust enum. The returned
    // directory (`assets/`) is added to the Slint include path so the generated file can reference the SVGs.
    let assets_root = icon_registry::generate(&manifest_dir, &out_dir)
        .unwrap_or_else(|e| panic!("[Icons] icon registry generation failed: {e}"));

    // The thumbnail badge areas use Slint's FlexboxLayout (CSS-like auto-wrapping with real text
    // measurement), which Slint 1.x still gates behind this flag. Slint is pinned via Cargo.lock.
    // SAFETY: build scripts are single-threaded at this point; nothing else reads the environment.
    unsafe {
        env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    }

    // Embed all referenced resources (icons) into the binary so the executable is self-contained.
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![out_dir, assets_root])
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", config).expect("Failed to compile Slint UI");
}
