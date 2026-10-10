// Embed the app icon + version info into the Windows exe (x86_64-pc-windows-gnu via mingw windres).
fn main() {
    println!("cargo:rerun-if-changed=wt-collector.rc");
    println!("cargo:rerun-if-changed=assets/wt-collector.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("wt-collector.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
