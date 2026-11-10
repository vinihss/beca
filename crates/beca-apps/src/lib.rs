//! Application discovery and launch (XDG .desktop entries).

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command},
};

#[derive(Debug, Clone)]
pub struct Application {
    pub name: String,
    pub exec: String,
    pub desktop_file: PathBuf,
}

pub fn search(query: &str) -> Vec<Application> {
    let query = query.trim().to_lowercase();
    let mut seen = HashSet::new();
    let mut apps = read_applications();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.into_iter()
        .filter(|app| {
            let key = app.name.to_lowercase();
            let matches = query.is_empty() || key.contains(&query);
            matches && seen.insert(key)
        })
        .take(30)
        .collect()
}

pub fn launch(name: &str) -> anyhow::Result<Child> {
    let app = read_applications()
        .into_iter()
        .find(|app| app.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("application not found: {name}"))?;

    let mut args = shlex::split(&app.exec)
        .ok_or_else(|| anyhow::anyhow!("invalid Exec field in {}", app.desktop_file.display()))?;
    args.retain(|arg| !arg.starts_with('%'));

    let (program, arguments) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty Exec field for {}", app.name))?;

    Command::new(program).args(arguments).spawn().map_err(Into::into)
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
    let Ok(entries) = fs::read_dir(root) else { return };
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

fn parse_desktop_file(path: &Path) -> Option<Application> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = None;
    let mut exec = None;
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
        desktop_file: path.to_path_buf(),
    })
}
