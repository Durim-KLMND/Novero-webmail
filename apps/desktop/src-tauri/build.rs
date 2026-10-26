fn main() {
    // Built into the app by src/updates.rs.
    println!("cargo:rerun-if-env-changed=NOVERO_UPDATER_PUBKEY");
    tauri_build::build()
}
