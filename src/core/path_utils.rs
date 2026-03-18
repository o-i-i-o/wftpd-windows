use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathResolveError {
    PathEscape,
    NotADirectory,
    NotFound,
}

pub fn to_ftp_path(path: &Path, home_dir: &Path) -> String {
    let relative = path.strip_prefix(home_dir).unwrap_or(path);
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
        Err(_) => false,
    }
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
    
    let clean_path = path.trim();
    
    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return home_canon;
    }
    
    let resolved = if clean_path.starts_with('/') {
        home_canon.join(clean_path.trim_start_matches('/'))
    } else {
        Path::new(cwd).join(clean_path)
    };
    
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
        let mut safe_path = home_canon.clone();
        for component in resolved.components() {
            match component {
                std::path::Component::Normal(name) => {
                    safe_path.push(name);
                }
                std::path::Component::ParentDir => {
                    safe_path.pop();
                }
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    safe_path = home_canon.clone();
                }
                std::path::Component::CurDir => {}
            }
        }
        if is_path_safe(&safe_path, &home_canon) {
            safe_path
        } else {
            log::warn!("Path escape attempt detected in non-existent path: {:?}", safe_path);
            home_canon
        }
    }
}

pub fn resolve_directory_path(cwd: &str, home_dir: &str, path: &str) -> Result<PathBuf, PathResolveError> {
    let home = PathBuf::from(home_dir);
    let home_canon = home.canonicalize().map_err(|_| PathResolveError::NotFound)?;
    
    let clean_path = path.trim();
    
    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return Ok(home_canon);
    }
    
    let resolved = if clean_path.starts_with('/') {
        home_canon.join(clean_path.trim_start_matches('/'))
    } else {
        Path::new(cwd).join(clean_path)
    };
    
    if !resolved.exists() {
        return Err(PathResolveError::NotFound);
    }
    
    let canon = resolved.canonicalize().map_err(|_| PathResolveError::NotFound)?;
    
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
    
    let clean_path = path.trim();
    if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
        return PathBuf::from(cwd);
    }

    let resolved = if clean_path.starts_with('/') {
        home_canon.join(clean_path.trim_start_matches('/'))
    } else {
        PathBuf::from(cwd).join(clean_path)
    };

    if resolved.exists() {
        if is_path_safe(&resolved, &home_canon) {
            match resolved.canonicalize() {
                Ok(canon) => canon,
                Err(_) => PathBuf::from(cwd),
            }
        } else {
            log::warn!("Path escape attempt detected: {:?}", resolved);
            PathBuf::from(cwd)
        }
    } else {
        let mut safe_path = PathBuf::from(cwd);
        for component in resolved.components() {
            match component {
                std::path::Component::Normal(name) => {
                    safe_path.push(name);
                }
                std::path::Component::ParentDir => {
                    safe_path.pop();
                }
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    safe_path = PathBuf::from(cwd);
                }
                std::path::Component::CurDir => {}
            }
        }
        
        if is_path_safe(&safe_path, &home_canon) {
            safe_path
        } else {
            log::warn!("Path escape attempt detected in non-existent path: {:?}", safe_path);
            PathBuf::from(cwd)
        }
    }
}
