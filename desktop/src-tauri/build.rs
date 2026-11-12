fn main() {
    // the reader assets are compiled in (include_str!); rebuild when they are regenerated
    println!("cargo:rerun-if-changed=../src/reader/generated");
    println!("cargo:rerun-if-changed=../src/reader/bridge.js");
    tauri_build::build()
}
