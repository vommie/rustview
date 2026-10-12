fn main() {
    println!("cargo:rerun-if-changed=ui/");

    // The thumbnail badge areas use Slint's FlexboxLayout (CSS-like auto-wrapping with real text
    // measurement), which Slint 1.x still gates behind this flag. Slint is pinned via Cargo.lock.
    // SAFETY: build scripts are single-threaded at this point; nothing else reads the environment.
    unsafe {
        std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1");
    }

    slint_build::compile("ui/main.slint").expect("Failed to compile Slint UI");
}
