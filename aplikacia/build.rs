fn main() {
    // okno smie volať len jediný príkaz aplikácie – `volaj` (povolený v capabilities/default.json)
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["volaj"])),
    )
    .expect("tauri-build");
}
