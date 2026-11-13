fn main() {
    println!("cargo:rerun-if-changed=assets/dockodile.rc");
    println!("cargo:rerun-if-changed=assets/dockodile.ico");
    println!("cargo:rerun-if-changed=assets/dockodile.manifest");
    embed_resource::compile("assets/dockodile.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
