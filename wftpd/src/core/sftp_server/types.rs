//! SFTP server types and constants

pub const SSH_FXF_READ: u32 = 0x00000001;
pub const SSH_FXF_WRITE: u32 = 0x00000002;
pub const SSH_FXF_APPEND: u32 = 0x00000004;
pub const SSH_FXF_CREAT: u32 = 0x00000008;
pub const SSH_FXF_TRUNC: u32 = 0x00000010;
pub const SSH_FXF_EXCL: u32 = 0x00000020;

pub const MAX_PACKET_SIZE: usize = 256 * 1024;
pub const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;
pub const MAX_HANDLES: usize = 256;
pub const SFTP_READ_BUFFER_SIZE: usize = 128 * 1024;
pub const SFTP_WRITE_FLUSH_THRESHOLD: usize = 64 * 1024;
pub const HANDLE_TIMEOUT_SECS: u64 = 1800;

pub enum SftpFileHandle {
    File {
        path: std::path::PathBuf,
        file: tokio::fs::File,
        locked: bool,
        existed: bool,
        written_bytes: u64,
        read_bytes: u64,
        pending_flush_bytes: u64,
        last_access: std::time::Instant,
    },
    Dir {
        path: std::path::PathBuf,
        entries: Vec<DirEntry>,
        index: usize,
        last_access: std::time::Instant,
    },
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u32,
}
