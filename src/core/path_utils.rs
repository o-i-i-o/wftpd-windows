use std::path::{Path, PathBuf};

const MAX_PATH_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathResolveError {
    PathEscape,
    NotADirectory,
    NotFound,
    PathTooDeep,
    HomeDirectoryNotFound,
    CanonicalizeFailed,
    InvalidPath,
}

impl std::fmt::Display for PathResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathResolveError::PathEscape => write!(f, "Path escapes home directory"),
            PathResolveError::NotADirectory => write!(f, "Path is not a directory"),
            PathResolveError::NotFound => write!(f, "Path not found"),
            PathResolveError::PathTooDeep => write!(f, "Path depth exceeds maximum"),
            PathResolveError::HomeDirectoryNotFound => write!(f, "Home directory not found"),
            PathResolveError::CanonicalizeFailed => write!(f, "Failed to canonicalize path"),
            PathResolveError::InvalidPath => write!(f, "Invalid path"),
        }
    }
}

pub fn to_ftp_path(path: &Path, home_dir: &Path) -> String {
    let relative = match path.strip_prefix(home_dir) {
        Ok(r) => r,
        Err(_) => {
            log::warn!(
                "to_ftp_path: path {:?} is not under home_dir {:?}",
                path,
                home_dir
            );
            path
        }
    };
    let path_str = relative.to_string_lossy();
    let normalized = path_str.replace('\\', "/");
    if normalized.is_empty() || normalized == "." {
        "/".to_string()
    } else {
        format!("/{}", normalized.trim_start_matches('/'))
    }
}

fn is_absolute_ftp_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let first_char = path.chars().next().unwrap();
    if first_char == '/' || first_char == '\\' {
        return true;
    }
    if let Some(colon_pos) = path.find(':')
        && colon_pos == 1
    {
        if path.len() > 2 {
            let third_char = path.chars().nth(2).unwrap();
            return third_char == '/' || third_char == '\\';
        }
        return true;
    }
    false
}

fn resolve_path_internal(cwd: &str, home_canon: &Path, path: &str) -> Result<PathBuf, PathResolveError> {
    let clean_path = path.trim();

    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return Ok(home_canon.to_path_buf());
    }

    if clean_path.starts_with("\\\\?\\") {
        let path_buf = PathBuf::from(clean_path);
        if !path_buf.starts_with(home_canon) {
            log::warn!("Windows extended path outside home directory: {:?}", path_buf);
            return Err(PathResolveError::PathEscape);
        }
        return Ok(path_buf);
    }

    if is_absolute_ftp_path(clean_path) {
        let relative = clean_path
            .trim_start_matches('/')
            .trim_start_matches('\\');
        if relative.is_empty() {
            return Ok(home_canon.to_path_buf());
        }
        if relative.contains(':') {
            log::warn!("Invalid path contains drive letter: {}", relative);
            return Err(PathResolveError::InvalidPath);
        }
        Ok(home_canon.join(relative))
    } else {
        if clean_path.contains(':') {
            log::warn!("Invalid relative path contains drive letter: {}", clean_path);
            return Err(PathResolveError::InvalidPath);
        }
        if cwd.is_empty() {
            Ok(home_canon.join(clean_path))
        } else {
            Ok(Path::new(cwd).join(clean_path))
        }
    }
}

fn build_safe_path(home_canon: &Path, resolved: &Path) -> Result<PathBuf, PathResolveError> {
    let mut safe_path = home_canon.to_path_buf();
    let mut depth: usize = 0;
    
    let home_components: Vec<_> = home_canon.components().collect();
    let resolved_components: Vec<_> = resolved.components().collect();
    
    if resolved_components.len() < home_components.len() {
        log::warn!("Path escape attempt: resolved path shorter than home - {:?}", resolved);
        return Err(PathResolveError::PathEscape);
    }
    
    for (i, component) in resolved_components.iter().enumerate() {
        if i < home_components.len() {
            if *component != home_components[i] {
                log::warn!("Path escape attempt: component mismatch at index {} - {:?}", i, resolved);
                return Err(PathResolveError::PathEscape);
            }
            continue;
        }
        
        match component {
            std::path::Component::Normal(name) => {
                safe_path.push(name);
                depth += 1;
                if depth > MAX_PATH_DEPTH {
                    log::warn!("Path depth exceeded maximum: {:?}", resolved);
                    return Err(PathResolveError::PathTooDeep);
                }
            }
            std::path::Component::ParentDir => {
                if !safe_path.pop() || safe_path.as_os_str().is_empty() {
                    log::warn!("Path escape attempt via parent dir: {:?}", resolved);
                    return Err(PathResolveError::PathEscape);
                }
                if !safe_path.starts_with(home_canon) {
                    log::warn!("Path escape attempt: {:?}", resolved);
                    return Err(PathResolveError::PathEscape);
                }
                depth = depth.saturating_sub(1);
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
            }
            std::path::Component::CurDir => {}
        }
    }
    Ok(safe_path)
}

pub fn safe_resolve_path(cwd: &str, home_dir: &str, path: &str) -> Result<PathBuf, PathResolveError> {
    let home = PathBuf::from(home_dir);
    let home_canon = home
        .canonicalize()
        .map_err(|e| {
            log::error!("Failed to canonicalize home directory '{}': {}", home_dir, e);
            PathResolveError::HomeDirectoryNotFound
        })?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    if resolved.exists() {
        let canon = resolved
            .canonicalize()
            .map_err(|e| {
                log::warn!("Failed to canonicalize path '{:?}': {}", resolved, e);
                PathResolveError::CanonicalizeFailed
            })?;

        if !canon.starts_with(&home_canon) {
            log::warn!("Path escape attempt detected: {:?}", resolved);
            return Err(PathResolveError::PathEscape);
        }
        Ok(canon)
    } else {
        let safe_path = build_safe_path(&home_canon, &resolved)?;

        if !safe_path.starts_with(&home_canon) {
            log::warn!(
                "Path escape attempt detected in non-existent path: {:?}",
                safe_path
            );
            return Err(PathResolveError::PathEscape);
        }
        Ok(safe_path)
    }
}

pub fn resolve_directory_path(
    cwd: &str,
    home_dir: &str,
    path: &str,
) -> Result<PathBuf, PathResolveError> {
    let home = PathBuf::from(home_dir);
    let home_canon = home
        .canonicalize()
        .map_err(|_| PathResolveError::HomeDirectoryNotFound)?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    if !resolved.exists() {
        return Err(PathResolveError::NotFound);
    }

    let canon = resolved
        .canonicalize()
        .map_err(|_| PathResolveError::CanonicalizeFailed)?;

    if !canon.starts_with(&home_canon) {
        log::warn!("Path escape attempt detected: {:?}", resolved);
        return Err(PathResolveError::PathEscape);
    }

    if !canon.is_dir() {
        return Err(PathResolveError::NotADirectory);
    }

    Ok(canon)
}

pub fn safe_resolve_path_with_cwd(cwd: &str, home_dir: &str, path: &str) -> Result<PathBuf, PathResolveError> {
    let home = PathBuf::from(home_dir);
    let home_canon = home
        .canonicalize()
        .map_err(|e| {
            log::warn!("Failed to canonicalize home directory: {}", e);
            PathResolveError::HomeDirectoryNotFound
        })?;

    let resolved = resolve_path_internal(cwd, &home_canon, path)?;

    if resolved.exists() {
        let canon = resolved
            .canonicalize()
            .map_err(|e| {
                log::warn!("Failed to canonicalize path: {}", e);
                PathResolveError::CanonicalizeFailed
            })?;

        if !canon.starts_with(&home_canon) {
            log::warn!("Path escape attempt detected: {:?}", resolved);
            return Err(PathResolveError::PathEscape);
        }
        Ok(canon)
    } else {
        let safe_path = build_safe_path(&home_canon, &resolved)?;

        if !safe_path.starts_with(&home_canon) {
            log::warn!(
                "Path escape attempt detected in non-existent path: {:?}",
                safe_path
            );
            return Err(PathResolveError::PathEscape);
        }
        Ok(safe_path)
    }
}
