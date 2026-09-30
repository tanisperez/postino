//! Embeds the Windows resources (the executable icon, `assets/postino.rc`) into `postino.exe`.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/postino.rc");
        println!("cargo:rerun-if-changed=assets/postino.ico");
        if let Err(error) =
            embed_resource::compile("assets/postino.rc", embed_resource::NONE).manifest_optional()
        {
            panic!("failed to embed the Windows resources: {error}");
        }
    }
}
