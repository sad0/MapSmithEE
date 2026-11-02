fn main() {
    // The dialog plugin needs Common Controls v6. Tauri embeds that manifest in the app only, so test
    // binaries crashed at start (STATUS_ENTRYPOINT_NOT_FOUND): embed it for every target instead.
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    let attrs = tauri_build::Attributes::new();
    #[cfg(windows)]
    let attrs = attrs.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attrs).expect("tauri build");
}
