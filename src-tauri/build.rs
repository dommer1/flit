fn main() {
    // why: sqlx::migrate!() embeds the migrations at compile time, but a
    // proc macro can't ask the compiler to watch a directory — a new .sql
    // file alone rebuilt nothing, and the app kept the old migration set.
    // sqlx's docs prescribe exactly this line for stable Rust.
    println!("cargo:rerun-if-changed=migrations");
    tauri_build::build()
}
