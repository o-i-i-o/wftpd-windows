//! Core module for WFTPG
//!
//! This module contains the core functionality for the WFTPG frontend.

pub mod config;
pub mod config_manager;
pub mod config_watcher;
pub mod error;
pub mod ipc;
pub mod logger;
pub mod path_utils;
pub mod server_manager;
pub mod users;
pub mod windows_ipc;

pub use error::{AppError, Result};
