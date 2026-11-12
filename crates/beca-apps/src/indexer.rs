use crate::desktop_entry::{Application, parse_desktop_file};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{OnceLock, RwLock},
};

static APPS_CACHE: OnceLock<RwLock<Vec<Application>>> = OnceLock::new();
const HISTORY_LIMIT: usize = 30;

fn apps_cache() -> &'static RwLock<Vec<Application>> {
    APPS_CACHE.get_or_init(|| {
        let mut apps = read_applications();
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        RwLock::new(apps)
    })
}

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
        reload_index();
        if let Ok(cache) = apps_cache().read() {
            f(&cache)
        } else {
            f(&[])
        }
    }
}

fn history_path() -> PathBuf {
    if let Some(dir) = env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(dir).join("beca").join("launcher_history")
    } else if let Some(home) = env::var_os("HOME") {
        PathBuf::from(home).join(".cache/beca/launcher_history")
    } else {
        PathBuf::from("/tmp/beca_launcher_history")
    }
}

fn read_history() -> Vec<String> {
    let path = history_path();
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

fn write_history(names: &[String]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, names.join("\n"));
}

pub fn record_launch(name: &str) {
    let mut hist = read_history();
    hist.retain(|n| !n.eq_ignore_ascii_case(name));
    hist.insert(0, name.to_string());
    if hist.len() > HISTORY_LIMIT {
        hist.truncate(HISTORY_LIMIT);
    }
    write_history(&hist);
}

pub fn recent() -> Vec<Application> {
    let names = read_history();
    if names.is_empty() {
        return Vec::new();
    }
    with_cached_applications(|apps| {
        let mut out = Vec::new();
        for name in names {
            if let Some(app) = apps.iter().find(|a| a.name.eq_ignore_ascii_case(&name)) {
                out.push(app.clone());
            }
            if out.len() >= HISTORY_LIMIT {
                break;
            }
        }
        out
    })
}

pub fn search(query: &str) -> Vec<Application> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        let rec = recent();
        if !rec.is_empty() {
            return rec;
        }
    }
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

    let child = Command::new(program)
        .args(arguments)
        .spawn()?;
    record_launch(&app.name);
    Ok(child)
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
