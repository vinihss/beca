use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct Application {
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub desktop_file: PathBuf,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn icon_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Clear the resolved-icon cache (forces re-resolution on next lookup).
pub fn clear_icon_cache() {
    if let Ok(mut cache) = icon_cache().lock() {
        cache.clear();
    }
}

pub fn parse_desktop_file(path: &Path) -> Option<Application> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut app_type = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Name" => name = Some(value.trim().to_string()),
                "Exec" => exec = Some(value.trim().to_string()),
                "Icon" => icon = Some(value.trim().to_string()),
                "NoDisplay" => no_display = value.trim() == "true",
                "Hidden" => hidden = value.trim() == "true",
                "Type" => app_type = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }

    if no_display || hidden || app_type.as_deref().is_some_and(|v| v != "Application") {
        return None;
    }
    let name = name?.trim().to_string();
    let exec = exec?;
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    Some(Application {
        name,
        exec,
        icon: icon.unwrap_or_default(),
        desktop_file: path.to_path_buf(),
    })
}

/// Resolve an Icon field value to a loadable image path (best-effort, cached).
pub fn resolve_icon(icon: &str) -> Option<String> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    if let Ok(cache) = icon_cache().lock() {
        if let Some(hit) = cache.get(icon) {
            return hit.clone();
        }
    }
    let resolved = resolve_icon_uncached(icon);
    if let Ok(mut cache) = icon_cache().lock() {
        cache.insert(icon.to_string(), resolved.clone());
    }
    resolved
}

fn resolve_icon_uncached(icon: &str) -> Option<String> {
    let p = Path::new(icon);
    if p.is_absolute() {
        if p.exists() {
            return Some(icon.to_string());
        }
        return None;
    }
    // If it already has an extension and exists in pixmaps
    if p.extension().is_some() {
        for base in ["/usr/share/pixmaps", "/usr/local/share/pixmaps"] {
            let cand = Path::new(base).join(icon);
            if cand.exists() {
                return Some(cand.to_string_lossy().to_string());
            }
        }
    }

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let exts = ["png", "svg", "xpm"];
    // Common icon roots (hicolor + pixmaps + user)
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons/Adwaita"),
        PathBuf::from("/usr/share/icons/breeze"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    if let Some(h) = &home {
        roots.push(h.join(".local/share/icons/hicolor"));
        roots.push(h.join(".icons/hicolor"));
        roots.push(h.join(".local/share/icons"));
    }

    // Prefer larger bitmap sizes and scalable
    let size_prefs = [
        "scalable/apps",
        "512x512/apps",
        "256x256/apps",
        "128x128/apps",
        "64x64/apps",
        "48x48/apps",
        "32x32/apps",
        "apps",
        "",
    ];

    for root in &roots {
        if root.as_os_str().is_empty() {
            continue;
        }
        // direct pixmaps dir
        if root.ends_with("pixmaps") {
            for ext in &exts {
                let cand = root.join(format!("{icon}.{ext}"));
                if cand.exists() {
                    return Some(cand.to_string_lossy().to_string());
                }
            }
            continue;
        }
        for sub in &size_prefs {
            let dir = if sub.is_empty() {
                root.clone()
            } else {
                root.join(sub)
            };
            for ext in &exts {
                let cand = dir.join(format!("{icon}.{ext}"));
                if cand.exists() {
                    return Some(cand.to_string_lossy().to_string());
                }
            }
        }
    }
    // fallback: shallow recursive search in common roots for <name>.png/svg
    for root in &roots {
        if let Some(found) = find_icon_shallow(root, icon, 3) {
            return Some(found);
        }
    }
    None
}

fn find_icon_shallow(root: &Path, name: &str, depth: usize) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_icon_shallow(&path, name, depth - 1) {
                return Some(found);
            }
        } else if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem == name {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "png" | "svg" | "xpm") {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    None
}
