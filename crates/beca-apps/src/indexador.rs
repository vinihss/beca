use crate::desktop_entry::{Application, parse_desktop_file};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{OnceLock, RwLock},
};

static APPS_CACHE: OnceLock<RwLock<Vec<Application>>> = OnceLock::new();

fn apps_cache() -> &'static RwLock<Vec<Application>> {
    APPS_CACHE.get_or_init(|| {
        let mut apps = read_applications();
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        RwLock::new(apps)
    })
}

/// Reload the applications index from disk (invalidates in-memory cache).
pub fn reload_index() {
    let mut apps = read_applications();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if let Ok(mut cache) = apps_cache().write() {
        *cache = apps;
    }
}

fn with_cached_applications<T>(f: impl FnOnce(&[Application]) -> T) -> T {
    if let Ok(cache) = apps_cache().read() {
        f(&cache)
    } else {
        // poisoned; rebuild
        reload_index();
        if let Ok(cache) = apps_cache().read() {
            f(&cache)
        } else {
            f(&[])
        }
    }
}

pub fn search(query: &str) -> Vec<Application> {
    let query = query.trim().to_lowercase();
    let mut seen = HashSet::new();
    with_cached_applications(|apps| {
        apps.iter()
            .filter(|app| {
                let name = app.name.to_lowercase();
                let exec = app.exec.to_lowercase();
                let matches = query.is_empty() || name.contains(&query) || exec.contains(&query);
                matches && seen.insert(name.as_str().to_string())
            })
            .take(30)
            .cloned()
            .collect()
    })
}

pub fn launch(name: &str) -> anyhow::Result<Child> {
    let app = with_cached_applications(|apps| {
        apps.iter()
            .find(|app| app.name.eq_ignore_ascii_case(name))
            .cloned()
    })
    .ok_or_else(|| anyhow::anyhow!("application not found: {name}"))?;

    let mut args = shlex::split(&app.exec)
        .ok_or_else(|| anyhow::anyhow!("invalid Exec field in {}", app.desktop_file.display()))?;
    args.retain(|arg| !arg.starts_with('%'));

    let (program, arguments) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty Exec field for {}", app.name))?;

    Command::new(program)
        .args(arguments)
        .spawn()
        .map_err(Into::into)
}

fn read_applications() -> Vec<Application> {
    let mut roots = vec![PathBuf::from("/usr/share/applications")];
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }
    roots.push(PathBuf::from("/usr/local/share/applications"));
    let mut apps = Vec::new();
    for root in roots {
        collect_desktop_files(&root, &mut apps);
    }
    apps
}

fn collect_desktop_files(root: &Path, apps: &mut Vec<Application>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_files(&path, apps);
        } else if path.extension().is_some_and(|ext| ext == "desktop") {
            if let Some(app) = parse_desktop_file(&path) {
                apps.push(app);
            }
        }
    }
}
