fn main() {
    embed_resource::compile("icon.rc", embed_resource::NONE).manifest_optional().unwrap();
    
    println!("cargo:rerun-if-changed=icon.rc");
    println!("cargo:rerun-if-changed=ui/wftpg.ico");
    println!("cargo:rerun-if-changed=wftp-gui.manifest");
}
