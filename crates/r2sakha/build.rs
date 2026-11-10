// Embed the R2 icon and version metadata into ArdonStudio.exe, so it shows
// branded in Explorer, the taskbar and Alt-Tab.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ico_path = manifest_dir.join("../../installer/r2.ico");
    println!("cargo:rerun-if-changed={}", ico_path.display());
    if !ico_path.exists() {
        println!("cargo:warning=installer/r2.ico not found; building without an embedded icon.");
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().unwrap());
        res.set("ProductName",     "Ardon Studio");
        res.set("FileDescription", "Ardon Studio — the IDE for Ardon-R2");
        res.set("CompanyName",     "Devendra Tandale");
        res.set("LegalCopyright",  "AGPL-3.0");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed icon resource: {}", e);
        }
    }
}
