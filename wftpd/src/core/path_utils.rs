//! Path utility functions
//!
//! Provides safe path resolution, logical chroot isolation, and path normalization

use std::path::{Path, PathBuf};

#[path = "path_resolve.rs"]
mod path_resolve;

pub use path_resolve::{
    MAX_PATH_DEPTH, PathResolveError, build_safe_path, canonicalize_and_validate,
    check_path_components_for_symlinks, is_absolute_ftp_path, is_valid_path_component,
    normalize_windows_path, path_starts_with_ignore_case, paths_equal_ignore_case,
    resolve_path_internal, to_ftp_path, validate_parent_and_build_path, validate_symlink_chain,
};

pub fn safe_resolve_path(
    cwd: &str,
    home_dir: &str,
    path: &str,
    allow_symlinks: bool,
) -> Result<PathBuf, PathResolveError> {
    let input_desc = format!("cwd={}, home={}, path={}", cwd, home_dir, path);

    let home = PathBuf::from(home_dir);
    let home_canon = home.canonicalize().map_err(|e| {
        tracing::error!(
            "safe_resolve_path: Failed to canonicalize home directory - home: {:?}, error: {}",
            home_dir,
            e
        );
        PathResolveError::HomeDirectoryNotFound
    })?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    match canonicalize_and_validate(&resolved, &home_canon, &input_desc, allow_symlinks) {
        Ok(canon) => Ok(canon),
        Err(PathResolveError::SymlinkNotAllowed) if allow_symlinks => {
            validate_symlink_chain(&resolved, &home_canon, &input_desc)
        }
        Err(PathResolveError::CanonicalizeFailed) => {
            tracing::debug!(
                "safe_resolve_path: Path does not exist, validating parent directory - resolved: {:?}, input: {:?}",
                resolved,
                input_desc
            );
            validate_parent_and_build_path(&resolved, &home_canon, &input_desc)
        }
        Err(e) => Err(e),
    }
}

pub fn resolve_directory_path(
    cwd: &str,
    home_dir: &str,
    path: &str,
) -> Result<PathBuf, PathResolveError> {
    let input_desc = format!("cwd={}, home={}, path={}", cwd, home_dir, path);

    let home = PathBuf::from(home_dir);
    let home_canon = home.canonicalize().map_err(|e| {
        tracing::error!(
            "resolve_directory_path: Failed to canonicalize home directory - home: {:?}, error: {}",
            home_dir,
            e
        );
        PathResolveError::HomeDirectoryNotFound
    })?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    match canonicalize_and_validate(&resolved, &home_canon, &input_desc, false) {
        Ok(canon) => {
            if !canon.is_dir() {
                tracing::warn!(
                    "resolve_directory_path: Path is not a directory - path: {:?}, input: {:?}",
                    canon,
                    input_desc
                );
                return Err(PathResolveError::NotADirectory);
            }
            Ok(canon)
        }
        Err(PathResolveError::CanonicalizeFailed) => {
            tracing::warn!(
                "resolve_directory_path: Directory does not exist - resolved: {:?}, input: {:?}",
                resolved,
                input_desc
            );
            Err(PathResolveError::NotFound)
        }
        Err(e) => Err(e),
    }
}

pub fn safe_resolve_path_with_cwd(
    cwd: &str,
    home_dir: &str,
    path: &str,
    allow_symlinks: bool,
) -> Result<PathBuf, PathResolveError> {
    safe_resolve_path(cwd, home_dir, path, allow_symlinks)
}

pub fn safe_resolve_path_no_symlink(
    cwd: &str,
    home_dir: &str,
    path: &str,
) -> Result<PathBuf, PathResolveError> {
    let input_desc = format!("cwd={}, home={}, path={}", cwd, home_dir, path);

    let home = PathBuf::from(home_dir);
    let home_canon = home.canonicalize().map_err(|e| {
        tracing::error!(
            "safe_resolve_path_no_symlink: Failed to canonicalize home directory - home: {:?}, error: {}",
            home_dir,
            e
        );
        PathResolveError::HomeDirectoryNotFound
    })?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    match canonicalize_and_validate(&resolved, &home_canon, &input_desc, false) {
        Ok(canon) => Ok(canon),
        Err(PathResolveError::CanonicalizeFailed) => {
            tracing::warn!(
                "safe_resolve_path_no_symlink: Path does not exist - resolved: {:?}, input: {:?}",
                resolved,
                input_desc
            );
            Err(PathResolveError::NotFound)
        }
        Err(e) => Err(e),
    }
}

pub fn validate_existing_path(path: &Path, home_canon: &Path) -> Result<PathBuf, PathResolveError> {
    let canon = path.canonicalize().map_err(|e| {
        tracing::warn!(
            "validate_existing_path: Canonicalize failed - path: {:?}, error: {}",
            path,
            e
        );
        PathResolveError::CanonicalizeFailed
    })?;

    if !path_starts_with_ignore_case(&canon, home_canon) {
        tracing::warn!(
            "validate_existing_path: Path escape detected - canonicalized: {:?}, home: {:?}",
            canon,
            home_canon
        );
        return Err(PathResolveError::PathEscape);
    }

    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_absolute_ftp_path() {
        assert!(is_absolute_ftp_path("/home/user"));
        assert!(is_absolute_ftp_path("\\home\\user"));
        assert!(is_absolute_ftp_path("/"));
        assert!(!is_absolute_ftp_path("relative/path"));
        assert!(!is_absolute_ftp_path(""));
    }

    #[test]
    fn test_is_valid_path_component() {
        assert!(is_valid_path_component(std::ffi::OsStr::new("valid_name")));
        assert!(is_valid_path_component(std::ffi::OsStr::new("..")));
        assert!(is_valid_path_component(std::ffi::OsStr::new(".")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new(
            "invalid:name"
        )));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("")));
    }

    #[test]
    fn test_to_ftp_path_windows() {
        let home = Path::new("C:\\share_test");
        assert_eq!(
            to_ftp_path(Path::new("C:\\share_test\\file.txt"), home).unwrap(),
            "/file.txt"
        );
        assert_eq!(to_ftp_path(Path::new("C:\\share_test"), home).unwrap(), "/");
        assert_eq!(
            to_ftp_path(Path::new("C:\\share_test\\subdir\\file.txt"), home).unwrap(),
            "/subdir/file.txt"
        );
    }

    #[test]
    fn test_resolve_path_internal_absolute() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();

            let result = resolve_path_internal("", &home_canon, "/subdir/file.txt").unwrap();
            assert!(result.starts_with(&home_canon));
            assert!(result.to_string_lossy().contains("subdir"));

            let result2 = resolve_path_internal("", &home_canon, "/").unwrap();
            assert_eq!(result2, home_canon);
        }
    }

    #[test]
    fn test_resolve_path_internal_relative() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();

            let result = resolve_path_internal("", &home_canon, "file.txt").unwrap();
            assert!(result.starts_with(&home_canon));
            assert!(result.to_string_lossy().ends_with("file.txt"));
        }
    }

    #[test]
    fn test_normalize_windows_path() {
        let path = Path::new(r"\\?\C:\Users\test");
        let normalized = normalize_windows_path(path);
        assert_eq!(normalized, PathBuf::from("C:\\Users\\test"));

        let path2 = Path::new("C:\\Users\\test");
        let normalized2 = normalize_windows_path(path2);
        assert_eq!(normalized2, PathBuf::from("C:\\Users\\test"));
    }

    #[test]
    fn test_path_starts_with_ignore_case() {
        let path = Path::new("C:\\Users\\Test");
        assert!(path_starts_with_ignore_case(path, "C:\\Users"));
        assert!(path_starts_with_ignore_case(path, "c:\\users"));
        assert!(!path_starts_with_ignore_case(path, "D:\\Data"));
    }

    #[test]
    fn test_paths_equal_ignore_case() {
        assert!(paths_equal_ignore_case(
            "C:\\Users\\Test",
            "c:\\users\\test"
        ));
        assert!(!paths_equal_ignore_case("C:\\Users", "D:\\Users"));
    }

    #[test]
    fn test_to_ftp_path_root() {
        let home = Path::new("C:\\share_test");
        assert_eq!(to_ftp_path(Path::new("C:\\share_test"), home).unwrap(), "/");
    }

    #[test]
    fn test_to_ftp_path_not_under_home() {
        let home = Path::new("C:\\share_test");
        let result = to_ftp_path(Path::new("D:\\other"), home);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_absolute_ftp_path_edge_cases() {
        assert!(!is_absolute_ftp_path(""));
        assert!(is_absolute_ftp_path("/"));
        assert!(is_absolute_ftp_path("\\"));
        assert!(!is_absolute_ftp_path("relative"));
    }

    #[test]
    fn test_is_valid_path_component_windows_reserved() {
        assert!(!is_valid_path_component(std::ffi::OsStr::new("CON")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("PRN")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("AUX")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("NUL")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("COM1")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("LPT1")));
        assert!(is_valid_path_component(std::ffi::OsStr::new("con1")));
    }

    #[test]
    fn test_is_valid_path_component_invalid_chars() {
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file<name")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file>name")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file\"name")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file|name")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file?name")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("file*name")));
    }

    #[test]
    fn test_is_valid_path_component_empty() {
        assert!(!is_valid_path_component(std::ffi::OsStr::new("")));
        assert!(!is_valid_path_component(std::ffi::OsStr::new("  ")));
    }

    #[test]
    fn test_path_resolve_error_display() {
        assert_eq!(
            format!("{}", PathResolveError::PathEscape),
            "Path escape detected"
        );
        assert_eq!(
            format!("{}", PathResolveError::NotADirectory),
            "Path is not a directory"
        );
        assert_eq!(format!("{}", PathResolveError::NotFound), "Path not found");
        assert_eq!(
            format!("{}", PathResolveError::PathTooDeep),
            "Path depth exceeds maximum limit"
        );
        assert_eq!(
            format!("{}", PathResolveError::HomeDirectoryNotFound),
            "Home directory not found"
        );
        assert_eq!(
            format!("{}", PathResolveError::CanonicalizeFailed),
            "Path canonicalization failed"
        );
        assert_eq!(format!("{}", PathResolveError::InvalidPath), "Invalid path");
        assert_eq!(
            format!("{}", PathResolveError::SymlinkNotAllowed),
            "Symlinks not allowed"
        );
        assert_eq!(
            format!("{}", PathResolveError::PathNotUnderHome),
            "Path not under home directory"
        );
    }

    #[test]
    fn test_path_resolve_error_debug() {
        let err = PathResolveError::PathEscape;
        assert!(format!("{:?}", err).contains("PathEscape"));
    }

    #[test]
    fn test_path_resolve_error_clone_copy() {
        let err = PathResolveError::InvalidPath;
        let copied = err;
        assert_eq!(err, copied);
    }

    #[test]
    fn test_resolve_path_internal_empty_path() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let result = resolve_path_internal("", &home_canon, "").unwrap();
            assert_eq!(result, home_canon);
        }
    }

    #[test]
    fn test_resolve_path_internal_dot_path() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let result = resolve_path_internal("", &home_canon, ".").unwrap();
            assert_eq!(result, home_canon);
        }
    }

    #[test]
    fn test_resolve_path_internal_absolute_with_backslash() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let result = resolve_path_internal("", &home_canon, "\\subdir\\file.txt").unwrap();
            assert!(result.to_string_lossy().contains("subdir"));
        }
    }

    #[test]
    fn test_build_safe_path_normal() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let resolved = home_canon.join("subdir").join("file.txt");
            let result = build_safe_path(&home_canon, &resolved, "test").unwrap();
            assert!(result.to_string_lossy().contains("subdir"));
        }
    }

    #[test]
    fn test_build_safe_path_parent_escape() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let resolved = home_canon.join("..").join("..");
            let result = build_safe_path(&home_canon, &resolved, "test");
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_build_safe_path_too_deep() {
        let home = PathBuf::from("C:\\share_test");
        if home.exists() {
            let home_canon = home.canonicalize().unwrap();
            let mut resolved = home_canon.clone();
            for _ in 0..70 {
                resolved = resolved.join("a");
            }
            let result = build_safe_path(&home_canon, &resolved, "test");
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_validate_existing_path_not_found() {
        let home = PathBuf::from("C:\\share_test");
        let nonexistent = PathBuf::from("C:\\share_test\\nonexistent_file_12345.txt");
        let result = validate_existing_path(&nonexistent, &home);
        assert!(result.is_err());
    }
}
