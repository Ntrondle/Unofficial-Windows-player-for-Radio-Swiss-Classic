fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        println!("cargo:rerun-if-changed=windows/app.manifest");
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set_manifest_file("windows/app.manifest")
            .set("ProductName", "Radio Swiss Classic")
            .set("FileDescription", "Native Radio Swiss Classic player")
            .set("LegalCopyright", "Nico. Streams and metadata by SRG SSR.")
            .compile()
            .expect("compile Windows resources");
    }
}
