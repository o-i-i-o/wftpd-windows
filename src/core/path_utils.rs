use std::path::{Path, PathBuf};

const MAX_PATH_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathResolveError {
    PathEscape,
    NotADirectory,
    NotFound,
    PathTooDeep,
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

pub fn is_path_safe(resolved: &Path, home: &Path) -> bool {
    match resolved.canonicalize() {
        Ok(canon) => canon.starts_with(home),
        Err(_) => {
            if resolved.exists() {
                false
            } else {
                let mut safe_path = home.to_path_buf();
                for component in resolved.components() {
                    match component {
                        std::path::Component::Normal(name) => {
                            safe_path.push(name);
                        }
                        std::path::Component::ParentDir => {
                            if !safe_path.pop() || safe_path.as_os_str().is_empty() {
                                return false;
                            }
                            if !safe_path.starts_with(home) {
                                return false;
                            }
                        }
                        std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                            return false;
                        }
                        std::path::Component::CurDir => {}
                    }
                }
                safe_path.starts_with(home)
            }
        }
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

fn resolve_path_internal(cwd: &str, home_canon: &Path, path: &str) -> PathBuf {
    let clean_path = path.trim();

    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return home_canon.to_path_buf();
    }

    if is_absolute_ftp_path(clean_path) {
        let relative = clean_path
            .trim_start_matches('/')
            .trim_start_matches('\\');
        if relative.contains(':') {
            home_canon.to_path_buf()
        } else {
            home_canon.join(relative)
        }
    } else {
        Path::new(cwd).join(clean_path)
    }
}

fn build_safe_path(home_canon: &Path, resolved: &Path) -> Result<PathBuf, PathResolveError> {
    let mut safe_path = home_canon.to_path_buf();
    let mut depth: usize = 0;
    for component in resolved.components() {
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
                safe_path.pop();
                depth = depth.saturating_sub(1);
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                safe_path = home_canon.to_path_buf();
                depth = 0;
            }
            std::path::Component::CurDir => {}
        }
    }
    Ok(safe_path)
}

pub fn safe_resolve_path(cwd: &str, home_dir: &str, path: &str) -> PathBuf {
    let home = PathBuf::from(home_dir);
    let home_canon = match home.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to canonicalize home directory: {}", e);
            return home;
        }
    };

    let resolved = resolve_path_internal(cwd, &home_canon, path);

    if resolved.exists() {
        if is_path_safe(&resolved, &home_canon) {
            match resolved.canonicalize() {
                Ok(canon) => canon,
                Err(_) => home_canon,
            }
        } else {
            log::warn!("Path escape attempt detected: {:?}", resolved);
            home_canon
        }
    } else {
        match build_safe_path(&home_canon, &resolved) {
            Ok(safe_path) => {
                if is_path_safe(&safe_path, &home_canon) {
                    safe_path
                } else {
                    log::warn!(
                        "Path escape attempt detected in non-existent path: {:?}",
                        safe_path
                    );
                    home_canon
                }
            }
            Err(PathResolveError::PathTooDeep) => {
                log::warn!("Path too deep: {:?}", resolved);
                home_canon
            }
            Err(_) => home_canon,
        }
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
        .map_err(|_| PathResolveError::NotFound)?;

    let resolved = resolve_path_internal(cwd, &home_canon, path);

    if !resolved.exists() {
        return Err(PathResolveError::NotFound);
    }

    let canon = resolved
        .canonicalize()
        .map_err(|_| PathResolveError::NotFound)?;

    if !canon.starts_with(&home_canon) {
        log::warn!("Path escape attempt detected: {:?}", resolved);
        return Err(PathResolveError::PathEscape);
    }

    if !canon.is_dir() {
        return Err(PathResolveError::NotADirectory);
    }

    Ok(canon)
}

pub fn safe_resolve_path_with_cwd(cwd: &str, home_dir: &str, path: &str) -> PathBuf {
    let home = PathBuf::from(home_dir);
    let home_canon = if home.exists() {
        match home.canonicalize() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to canonicalize home directory: {}", e);
                return PathBuf::from(cwd);
            }
        }
    } else {
        log::warn!("Home directory does not exist: {:?}", home);
        return PathBuf::from(cwd);
    };

    let resolved = resolve_path_internal(cwd, &home_canon, path);

    if resolved.exists() {
        if is_path_safe(&resolved, &home_canon) {
            match resolved.canonicalize() {
                Ok(canon) => canon,
                Err(_) => home_canon,
            }
        } else {
            log::warn!("Path escape attempt detected: {:?}", resolved);
            home_canon
        }
    } else {
        match build_safe_path(&home_canon, &resolved) {
            Ok(safe_path) => {
                if is_path_safe(&safe_path, &home_canon) {
                    safe_path
                } else {
                    log::warn!(
                        "Path escape attempt detected in non-existent path: {:?}",
                        safe_path
                    );
                    home_canon
                }
            }
            Err(PathResolveError::PathTooDeep) => {
                log::warn!("Path too deep: {:?}", resolved);
                home_canon
            }
            Err(_) => home_canon,
        }
    }
}

