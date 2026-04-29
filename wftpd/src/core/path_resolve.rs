//! Path utility types and constants

use std::path::{Component, Path, PathBuf};

pub const MAX_PATH_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathResolveError {
    PathEscape,
    NotADirectory,
    NotFound,
    PathTooDeep,
    HomeDirectoryNotFound,
    CanonicalizeFailed,
    InvalidPath,
    SymlinkNotAllowed,
    PathNotUnderHome,
}

impl std::fmt::Display for PathResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathResolveError::PathEscape => write!(f, "Path escape detected"),
            PathResolveError::NotADirectory => write!(f, "Path is not a directory"),
            PathResolveError::NotFound => write!(f, "Path not found"),
            PathResolveError::PathTooDeep => write!(f, "Path depth exceeds maximum limit"),
            PathResolveError::HomeDirectoryNotFound => write!(f, "Home directory not found"),
            PathResolveError::CanonicalizeFailed => write!(f, "Path canonicalization failed"),
            PathResolveError::InvalidPath => write!(f, "Invalid path"),
            PathResolveError::SymlinkNotAllowed => write!(f, "Symlinks not allowed"),
            PathResolveError::PathNotUnderHome => write!(f, "Path not under home directory"),
        }
    }
}

impl std::error::Error for PathResolveError {}

pub fn normalize_windows_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    let stripped = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str);
    PathBuf::from(stripped)
}

#[cfg(windows)]
pub fn path_starts_with_ignore_case<P: AsRef<Path>>(path: &Path, prefix: P) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    let prefix_str = prefix.as_ref().to_string_lossy().to_lowercase();
    path_str.starts_with(&prefix_str)
}

#[cfg(not(windows))]
pub fn path_starts_with_ignore_case<P: AsRef<Path>>(path: &Path, prefix: P) -> bool {
    path.starts_with(prefix.as_ref())
}

#[cfg(windows)]
pub fn paths_equal_ignore_case<P1: AsRef<Path>, P2: AsRef<Path>>(path1: P1, path2: P2) -> bool {
    let str1 = path1.as_ref().to_string_lossy().to_lowercase();
    let str2 = path2.as_ref().to_string_lossy().to_lowercase();
    str1 == str2
}

#[cfg(not(windows))]
pub fn paths_equal_ignore_case<P1: AsRef<Path>, P2: AsRef<Path>>(path1: P1, path2: P2) -> bool {
    path1.as_ref() == path2.as_ref()
}

pub fn to_ftp_path(path: &Path, home_dir: &Path) -> Result<String, PathResolveError> {
    let normalized_path = normalize_windows_path(path);
    let normalized_home = normalize_windows_path(home_dir);

    let relative = match normalized_path.strip_prefix(&normalized_home) {
        Ok(r) => r,
        Err(_) => {
            tracing::warn!(
                "to_ftp_path: path {:?} is not under home_dir {:?} (normalized: path={:?}, home={:?})",
                path,
                home_dir,
                normalized_path,
                normalized_home
            );
            return Err(PathResolveError::PathNotUnderHome);
        }
    };
    let path_str = relative.to_string_lossy();
    let normalized = path_str.replace('\\', "/");
    if normalized.is_empty() || normalized == "." {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", normalized.trim_start_matches('/')))
    }
}

pub fn is_absolute_ftp_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }

    let path_buf = Path::new(path);
    if path_buf.is_absolute() {
        return true;
    }

    let first_char = path.chars().next().unwrap();
    first_char == '/' || first_char == '\\'
}

pub fn is_valid_path_component(name: &std::ffi::OsStr) -> bool {
    let name_str = name.to_string_lossy();
    let name_str = name_str.trim();

    if name_str.is_empty() {
        return false;
    }

    if name_str == "." || name_str == ".." {
        return true;
    }

    if name_str.contains(':') {
        return false;
    }

    if cfg!(windows) {
        let invalid_chars = ['<', '>', '"', '|', '?', '*'];
        if name_str.chars().any(|c| invalid_chars.contains(&c)) {
            return false;
        }

        let reserved_names = [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        let upper = name_str.to_uppercase();
        if reserved_names.contains(&upper.as_str()) {
            return false;
        }
    }

    true
}

pub fn resolve_path_internal(
    cwd: &str,
    home_canon: &Path,
    path: &str,
) -> Result<PathBuf, PathResolveError> {
    let clean_path = path.trim();

    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return Ok(home_canon.to_path_buf());
    }

    if clean_path.starts_with("\\\\?\\") {
        let path_buf = PathBuf::from(clean_path);
        if !path_buf.starts_with(home_canon) {
            tracing::warn!(
                "resolve_path_internal: Windows extended path outside home - input: {:?}, home: {:?}",
                clean_path,
                home_canon
            );
            return Err(PathResolveError::PathEscape);
        }
        return Ok(path_buf);
    }

    if is_absolute_ftp_path(clean_path) {
        let relative = clean_path.trim_start_matches('/').trim_start_matches('\\');
        if relative.is_empty() {
            return Ok(home_canon.to_path_buf());
        }

        let relative_path = Path::new(relative);
        for component in relative_path.components() {
            if let Component::Prefix(_) | Component::RootDir = component {
                tracing::warn!(
                    "resolve_path_internal: Invalid prefix/root in absolute path - input: {:?}",
                    clean_path
                );
                return Err(PathResolveError::InvalidPath);
            }
            if let Component::Normal(name) = component
                && !is_valid_path_component(name)
            {
                tracing::warn!(
                    "resolve_path_internal: Invalid component in path - component: {:?}, input: {:?}",
                    name,
                    clean_path
                );
                return Err(PathResolveError::InvalidPath);
            }
        }

        let resolved_path = home_canon.join(relative);

        if !resolved_path.starts_with(home_canon) {
            tracing::warn!(
                "resolve_path_internal: Absolute path outside home directory - resolved: {:?}, home: {:?}, input: {:?}",
                resolved_path,
                home_canon,
                clean_path
            );
            return Err(PathResolveError::PathEscape);
        }

        Ok(resolved_path)
    } else {
        let clean_path_path = Path::new(clean_path);
        for component in clean_path_path.components() {
            if let Component::Prefix(_) | Component::RootDir = component {
                tracing::warn!(
                    "resolve_path_internal: Invalid prefix/root in relative path - input: {:?}",
                    clean_path
                );
                return Err(PathResolveError::InvalidPath);
            }
            if let Component::Normal(name) = component
                && !is_valid_path_component(name)
            {
                tracing::warn!(
                    "resolve_path_internal: Invalid component in relative path - component: {:?}, input: {:?}",
                    name,
                    clean_path
                );
                return Err(PathResolveError::InvalidPath);
            }
        }

        let base = if cwd.is_empty() {
            home_canon.to_path_buf()
        } else {
            let cwd_path = PathBuf::from(cwd);
            if !path_starts_with_ignore_case(&cwd_path, home_canon) {
                tracing::warn!(
                    "resolve_path_internal: CWD outside home - cwd: {:?}, home: {:?}",
                    cwd,
                    home_canon.display()
                );
                return Err(PathResolveError::PathEscape);
            }
            cwd_path
        };

        let resolved_path = base.join(clean_path);

        Ok(resolved_path)
    }
}

pub fn build_safe_path(
    home_canon: &Path,
    resolved: &Path,
    input_desc: &str,
) -> Result<PathBuf, PathResolveError> {
    let mut safe_path = home_canon.to_path_buf();
    let home_components: Vec<_> = home_canon.components().collect();
    let resolved_components: Vec<_> = resolved.components().collect();

    let mut relative_depth: usize = 0;

    for (i, component) in resolved_components.iter().enumerate() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                continue;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if safe_path == *home_canon {
                    tracing::warn!(
                        "SECURITY: Path escape via parent dir at root - input: {:?}, resolved: {:?}",
                        input_desc,
                        resolved
                    );
                    return Err(PathResolveError::PathEscape);
                }

                if !safe_path.pop() {
                    tracing::warn!(
                        "SECURITY: Failed to pop path - input: {:?}, resolved: {:?}",
                        input_desc,
                        resolved
                    );
                    return Err(PathResolveError::PathEscape);
                }

                if !path_starts_with_ignore_case(&safe_path, home_canon) {
                    tracing::warn!(
                        "SECURITY: Path escape after pop - input: {:?}, safe_path: {:?}, home: {:?}",
                        input_desc,
                        safe_path,
                        home_canon
                    );
                    return Err(PathResolveError::PathEscape);
                }

                relative_depth = relative_depth.saturating_sub(1);
            }
            Component::Normal(name) => {
                if i < home_components.len() {
                    continue;
                }

                if !is_valid_path_component(name) {
                    tracing::warn!(
                        "build_safe_path: Invalid component name - component: {:?}, input: {:?}",
                        name,
                        input_desc
                    );
                    return Err(PathResolveError::InvalidPath);
                }

                safe_path.push(name);
                relative_depth += 1;

                if relative_depth > MAX_PATH_DEPTH {
                    tracing::warn!(
                        "SECURITY: Path depth exceeded - depth: {}, max: {}, input: {:?}",
                        relative_depth,
                        MAX_PATH_DEPTH,
                        input_desc
                    );
                    return Err(PathResolveError::PathTooDeep);
                }
            }
        }
    }

    if !path_starts_with_ignore_case(&safe_path, home_canon) {
        tracing::warn!(
            "SECURITY: Final path outside home - safe_path: {:?}, home: {:?}, input: {:?}",
            safe_path,
            home_canon,
            input_desc
        );
        return Err(PathResolveError::PathEscape);
    }

    Ok(safe_path)
}

pub fn canonicalize_and_validate(
    path: &Path,
    home_canon: &Path,
    input_desc: &str,
    allow_symlink: bool,
) -> Result<PathBuf, PathResolveError> {
    match path.canonicalize() {
        Ok(canon) => {
            if !path_starts_with_ignore_case(&canon, home_canon) {
                tracing::warn!(
                    "SECURITY: Path escape detected - canonicalized: {:?}, home: {:?}, input: {:?}",
                    canon,
                    home_canon,
                    input_desc
                );
                return Err(PathResolveError::PathEscape);
            }

            if !allow_symlink {
                if let Ok(metadata) = path.symlink_metadata()
                    && metadata.file_type().is_symlink()
                {
                    tracing::warn!(
                        "SECURITY: Symlink not allowed - path: {:?}, input: {:?}",
                        path,
                        input_desc
                    );
                    return Err(PathResolveError::SymlinkNotAllowed);
                }

                check_path_components_for_symlinks(path, home_canon, input_desc)?;
            }

            Ok(canon)
        }
        Err(e) => {
            tracing::debug!(
                "canonicalize_and_validate: Canonicalize failed - path: {:?}, error: {}, input: {:?}",
                path,
                e,
                input_desc
            );
            Err(PathResolveError::CanonicalizeFailed)
        }
    }
}

pub fn check_path_components_for_symlinks(
    path: &Path,
    home_canon: &Path,
    input_desc: &str,
) -> Result<(), PathResolveError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
            }
            Component::RootDir => {
                current.push(component);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                current.push(name);

                if let Ok(metadata) = current.symlink_metadata()
                    && metadata.file_type().is_symlink()
                {
                    tracing::warn!(
                        "SECURITY: Symlink component detected - component: {:?}, full_path: {:?}, input: {:?}",
                        current,
                        path,
                        input_desc
                    );
                    return Err(PathResolveError::SymlinkNotAllowed);
                }

                if !path_starts_with_ignore_case(&current, home_canon) {
                    tracing::warn!(
                        "SECURITY: Path component outside home - component: {:?}, home: {:?}, input: {:?}",
                        current,
                        home_canon,
                        input_desc
                    );
                    return Err(PathResolveError::PathEscape);
                }
            }
        }
    }
    Ok(())
}

pub fn validate_symlink_chain(
    path: &Path,
    home_canon: &Path,
    input_desc: &str,
) -> Result<PathBuf, PathResolveError> {
    let mut current = PathBuf::new();
    let mut components = path.components().peekable();

    while let Some(component) = components.next() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                current.push(name);

                if let Ok(metadata) = current.symlink_metadata()
                    && metadata.file_type().is_symlink()
                {
                    let link_target = match std::fs::read_link(&current) {
                        Ok(target) => target,
                        Err(e) => {
                            tracing::warn!(
                                "validate_symlink_chain: Failed to read symlink - path: {:?}, error: {}, input: {:?}",
                                current,
                                e,
                                input_desc
                            );
                            return Err(PathResolveError::SymlinkNotAllowed);
                        }
                    };

                    let resolved_target = if link_target.is_absolute() {
                        link_target.clone()
                    } else {
                        let parent = current.parent().unwrap_or(Path::new("/"));
                        parent.join(&link_target)
                    };

                    let canon_target = match resolved_target.canonicalize() {
                        Ok(canon) => canon,
                        Err(_) => {
                            let parent = current.parent().unwrap_or(Path::new("/"));
                            build_safe_path(home_canon, &parent.join(&link_target), input_desc)?
                        }
                    };

                    if !path_starts_with_ignore_case(&canon_target, home_canon) {
                        tracing::warn!(
                            "validate_symlink_chain: Symlink target outside home - link: {:?}, target: {:?}, home: {:?}, input: {:?}",
                            current,
                            canon_target,
                            home_canon,
                            input_desc
                        );
                        return Err(PathResolveError::SymlinkNotAllowed);
                    }

                    tracing::debug!(
                        "validate_symlink_chain: Valid symlink - link: {:?}, target: {:?}",
                        current,
                        canon_target
                    );

                    if components.peek().is_some() {
                        current = canon_target;
                    }
                }
            }
        }
    }

    let final_path = match path.canonicalize() {
        Ok(canon) => canon,
        Err(_) => current,
    };

    if !path_starts_with_ignore_case(&final_path, home_canon) {
        tracing::warn!(
            "validate_symlink_chain: Final path outside home - path: {:?}, home: {:?}, input: {:?}",
            final_path,
            home_canon,
            input_desc
        );
        return Err(PathResolveError::PathEscape);
    }

    Ok(final_path)
}

pub fn validate_parent_and_build_path(
    path: &Path,
    home_canon: &Path,
    input_desc: &str,
) -> Result<PathBuf, PathResolveError> {
    let parent = path.parent().ok_or_else(|| {
        tracing::warn!(
            "validate_parent: Path has no parent - path: {:?}, input: {:?}",
            path,
            input_desc
        );
        PathResolveError::InvalidPath
    })?;

    if paths_equal_ignore_case(parent, home_canon) {
        if !path_starts_with_ignore_case(path, home_canon) {
            tracing::warn!(
                "SECURITY: Path outside home - path: {:?}, home: {:?}, input: {:?}",
                path,
                home_canon,
                input_desc
            );
            return Err(PathResolveError::PathEscape);
        }
        return Ok(path.to_path_buf());
    }

    match parent.canonicalize() {
        Ok(canon_parent) => {
            if !path_starts_with_ignore_case(&canon_parent, home_canon) {
                tracing::warn!(
                    "SECURITY: Parent directory outside home - parent: {:?}, home: {:?}, input: {:?}",
                    canon_parent,
                    home_canon,
                    input_desc
                );
                return Err(PathResolveError::PathEscape);
            }

            if let Ok(metadata) = parent.symlink_metadata()
                && metadata.file_type().is_symlink()
            {
                tracing::warn!(
                    "validate_parent: Parent is symlink - parent: {:?}, input: {:?}",
                    parent,
                    input_desc
                );
                return Err(PathResolveError::SymlinkNotAllowed);
            }

            let file_name = path.file_name().ok_or_else(|| {
                tracing::warn!(
                    "validate_parent: Path has no file name - path: {:?}, input: {:?}",
                    path,
                    input_desc
                );
                PathResolveError::InvalidPath
            })?;

            let final_path = canon_parent.join(file_name);

            if !path_starts_with_ignore_case(&final_path, home_canon) {
                tracing::warn!(
                    "SECURITY: Final path outside home - final: {:?}, home: {:?}, input: {:?}",
                    final_path,
                    home_canon,
                    input_desc
                );
                return Err(PathResolveError::PathEscape);
            }

            tracing::debug!(
                "validate_parent: Successfully built path for new file - final: {:?}, input: {:?}",
                final_path,
                input_desc
            );

            Ok(final_path)
        }
        Err(e) => {
            tracing::warn!(
                "validate_parent: Parent directory does not exist or cannot be canonicalized - parent: {:?}, error: {}, input: {:?}",
                parent,
                e,
                input_desc
            );
            Err(PathResolveError::NotFound)
        }
    }
}
