fn main() {
    // Nightmoth needs admin for services, HKLM, Prefetch and uninstalls.
    let attrs = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("nightmoth.manifest")),
    );
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
