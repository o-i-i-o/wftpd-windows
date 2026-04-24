fn main() -> Result<(), Box<dyn std::error::Error>> {
    embed_resource::compile("icon.rc", embed_resource::NONE).manifest_optional()?;

    println!("cargo:rerun-if-changed=icon.rc");
    println!("cargo:rerun-if-changed=ui/wftpg.ico");
    println!("cargo:rerun-if-changed=wftpg.manifest");

    Ok(())
}
