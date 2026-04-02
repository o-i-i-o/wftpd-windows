//! WFTPG - SFTP/FTP Management Frontend
//!
//! This library provides the core functionality for the WFTPG management frontend.
//!
//! # Architecture
//! 
//! - Configuration management with hot-reload support
//! - User management with Argon2 password hashing
//! - Comprehensive logging with tracing
//! - Windows service integration

pub mod core;
pub mod gui_egui;
