fn main() {
    println!("cargo:rerun-if-changed=assets/serein.rc");
    println!("cargo:rerun-if-changed=assets/serein.ico");

    embed_resource::compile("assets/serein.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("failed to embed the Serein Windows icon");
}
