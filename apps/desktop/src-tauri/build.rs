fn main() {
    // Built into the app by src/updates.rs.
    println!("cargo:rerun-if-env-changed=NOVERO_UPDATER_PUBKEY");
    // Built into the app by src/plan.rs. A license covers versions built
    // within a year of buying it; SOURCE_DATE_EPOCH pins the date.
    for var in [
        "NOVERO_PLANS_STORE_ID",
        "NOVERO_PLANS_PRODUCT_ID",
        "NOVERO_PLANS_CHECKOUT_URL",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let released = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default()
    });
    println!("cargo:rustc-env=NOVERO_RELEASED={released}");
    tauri_build::build()
}
