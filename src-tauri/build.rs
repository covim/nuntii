fn main() {
    // Icons are embedded into the executable; tauri-build does not watch them itself.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
