fn main() {
    println!("cargo:rerun-if-changed=icons/usage.ico");
    tauri_build::build()
}
