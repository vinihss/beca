use slint_interpreter::{Compiler, ComponentDefinition, ComponentHandle, ComponentInstance, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

pub fn ui_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_path_home().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("beca").join("ui")
}

fn dirs_path_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn is_dev() -> bool {
    let v = std::env::var("BECA_ENVIRONMENT")
        .or_else(|_| std::env::var("BECA_ENVIROMENT"))
        .unwrap_or_else(|_| "prod".to_string());
    v.eq_ignore_ascii_case("dev") || v.eq_ignore_ascii_case("development")
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs_path_home().map(|h| h.join(".cache")))
        .unwrap_or_else(|| std::env::temp_dir())
}

include!(concat!(env!("OUT_DIR"), "/ui_bundle.rs"));

fn embedded_ui_files() -> &'static [(&'static str, &'static str)] {
    EMBEDDED_UI_FILES
}

#[derive(Clone)]
struct CachedDef {
    def: ComponentDefinition,
    signature: u64,
}

thread_local! {
    static COMPILE_CACHE: std::cell::RefCell<HashMap<String, CachedDef>> =
        std::cell::RefCell::new(HashMap::new());
    static PRELOADED_DEF: std::cell::RefCell<Option<(String, u64, ComponentDefinition)>> =
        std::cell::RefCell::new(None);
}

pub fn invalidate_cache() {
    eprintln!("[beca-ui][debug] invalidate_cache() tid={:?}", std::thread::current().id());
    COMPILE_CACHE.with(|cache| cache.borrow_mut().clear());
    PRELOADED_DEF.with(|p| *p.borrow_mut() = None);
}

pub fn set_preloaded_component(name: &str, def: ComponentDefinition) {

    PRELOADED_DEF.with(|p| *p.borrow_mut() = Some((name.to_string(), 0, def)));
}

pub fn clear_preloaded_component() {
    PRELOADED_DEF.with(|p| *p.borrow_mut() = None);
}

fn ui_signature(dir: &PathBuf) -> u64 {

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for (name, _content) in embedded_ui_files() {
        let path = dir.join(name);
        name.hash(&mut h);
        match fs::metadata(&path) {
            Ok(m) => {
                m.len().hash(&mut h);
                if let Ok(t) = m.modified() {
                    if let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) {
                        d.as_secs().hash(&mut h);
                        d.subsec_nanos().hash(&mut h);
                    }
                }
            }
            Err(_) => {
                0u64.hash(&mut h);
            }
        }
    }
    h.finish()
}

pub fn ensure_defaults() -> Result<PathBuf, String> {
    if is_dev() {
        let dir = ui_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("create ui dir {}: {e}", dir.display()))?;
        let mut changed = false;
        for (name, content) in embedded_ui_files() {
            let path = dir.join(name);
            let should_write = if *name == "library.slint" {
                match fs::read(&path) {
                    Ok(existing) => existing.as_slice() != content.as_bytes(),
                    Err(_) => true,
                }
            } else {
                !path.exists()
            };
            if should_write {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("create ui dir {}: {e}", parent.display()))?;
                }
                fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
                changed = true;
            }
        }
        if changed {
            invalidate_cache();
        }
        Ok(dir)
    } else {

        let dir = runtime_dir().join("beca").join("ui-embedded");
        fs::create_dir_all(&dir).map_err(|e| format!("create ui dir {}: {e}", dir.display()))?;
        let mut changed = false;
        for (name, content) in embedded_ui_files() {
            let path = dir.join(name);
            let should_write = match fs::read(&path) {
                Ok(existing) => existing.as_slice() != content.as_bytes(),
                Err(_) => true,
            };
            if should_write {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("create ui dir {}: {e}", parent.display()))?;
                }
                fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
                changed = true;
            }
        }
        if changed {
            eprintln!("[beca-ui][debug] prod ui sources changed, cache invalidated");
            invalidate_cache();
        }
        Ok(dir)
    }
}

pub fn compile_component(name: &str) -> Result<ComponentDefinition, String> {
    let dir = ensure_defaults()?;

    let sig = ui_signature(&dir);

    let preloaded = PRELOADED_DEF.with(|p| {
        let b = p.borrow();
        b.as_ref().and_then(|(n, _, def)| if n == name { Some(def.clone()) } else { None })
    });
    if let Some(def) = preloaded {
        eprintln!("[beca-ui][debug] using preloaded def: {name}");
        return Ok(def);
    } else {
        let pinned = PRELOADED_DEF.with(|p| p.borrow().as_ref().map(|(n,_,_)| n.clone()));
        if let Some(pn) = pinned {
            eprintln!("[beca-ui][debug] preloaded mismatch: want={name} have={pn}");
        }
    }

    let cached = COMPILE_CACHE.with(|cache| {
        let c = cache.borrow();
        eprintln!("[beca-ui][debug] cache lookup {name}: entries={} tid={:?}", c.len(), std::thread::current().id());
        c.get(name).and_then(|entry| {
            if entry.signature == sig {
                Some(entry.def.clone())
            } else {
                eprintln!("[beca-ui][debug] sig mismatch for {name}: have={} want={}", entry.signature, sig);
                None
            }
        })
    });
    if let Some(def) = cached {
        eprintln!("[beca-ui][debug] cache hit: {name}");
        return Ok(def);
    }
    eprintln!("[beca-ui][debug] cache miss: {name} (sig={sig})");

    let mut compiler = Compiler::default();
    compiler.set_include_paths(vec![dir.clone()]);
    let library = dir.join("library.slint");
    let result = spin_on::spin_on(compiler.build_from_path(&library));
    for d in result.diagnostics() {
        eprintln!("[beca-ui] slint: {d}");
    }

    let mut cached_count = 0;
    COMPILE_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        for def in result.components() {
            c.insert(
                def.name().to_string(),
                CachedDef {
                    def: def.clone(),
                    signature: sig,
                },
            );
            cached_count += 1;
        }
    });
    eprintln!("[beca-ui][debug] cached {cached_count} components (sig={sig}) tid={:?}", std::thread::current().id());

    result
        .component(name)
        .ok_or_else(|| format!("component '{name}' not found in {}", library.display()))
}

pub fn create(name: &str) -> Result<ComponentInstance, String> {
    let def = compile_component(name)?;
    def.create().map_err(|e| format!("create '{name}': {e}"))
}

pub fn set_string(instance: &ComponentInstance, prop: &str, val: &str) -> Result<(), String> {
    instance
        .set_property(prop, Value::String(val.into()))
        .map_err(|e| format!("set {prop}: {e}"))
}

pub fn get_string(instance: &ComponentInstance, prop: &str) -> String {
    match instance.get_property(prop) {
        Ok(Value::String(s)) => s.to_string(),
        _ => String::new(),
    }
}
pub fn set_bool(instance: &ComponentInstance, prop: &str, val: bool) -> Result<(), String> {
    instance
        .set_property(prop, Value::Bool(val))
        .map_err(|e| format!("set {prop}: {e}"))
}
pub fn get_bool(instance: &ComponentInstance, prop: &str) -> bool {
    matches!(instance.get_property(prop), Ok(Value::Bool(b)) if b)
}

pub use crate::theme::parse_color;

pub fn apply_theme_global(instance: &ComponentInstance, t: &crate::voice::VoiceTheme) {
    crate::theme::ThemeTokens::from_voice_theme(t).apply_interpreter(instance);
}

pub fn show(instance: &ComponentInstance) -> Result<(), String> {
    instance.show().map_err(|e| format!("show: {e}"))
}
pub fn hide(instance: &ComponentInstance) -> Result<(), String> {
    instance.hide().map_err(|e| format!("hide: {e}"))
}
thread_local! {

    static CUSTOM_EVENT_LOOP: std::cell::RefCell<Option<Box<dyn FnOnce() -> Result<(), String>>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn set_custom_event_loop(f: impl FnOnce() -> Result<(), String> + 'static) {
    CUSTOM_EVENT_LOOP.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(f));
    });
}

pub fn take_custom_event_loop() -> Option<Box<dyn FnOnce() -> Result<(), String>>> {
    CUSTOM_EVENT_LOOP.with(|cell| cell.borrow_mut().take())
}

pub fn run_event_loop() -> Result<(), String> {
    if let Some(custom) = take_custom_event_loop() {
        return custom();
    }
    slint_interpreter::run_event_loop().map_err(|e| format!("event loop: {e}"))
}

pub fn quit_event_loop() {
    let _ = slint_interpreter::quit_event_loop();
}
pub fn invoke_on_event_loop(f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    slint_interpreter::invoke_from_event_loop(f).map_err(|e| format!("invoke: {e}"))
}

#[cfg(test)]
mod tests {

    #[test]
    fn runtime_windows_compile() {
        for name in [
            "BarWindow",
            "SearchWindow",
            "VoiceCaptureWindow",
            "VoiceAssistantWindow",
            "QuickVoiceWindow",
        ] {
            if let Err(e) = super::compile_component(name) {
                panic!("{name} failed to compile: {e}");
            }
        }
    }

    #[test]
    fn quick_window_declares_rust_callbacks() {
        let def = super::compile_component("QuickVoiceWindow")
            .unwrap_or_else(|e| panic!("QuickVoiceWindow failed to compile: {e}"));
        let available: Vec<String> = def.callbacks().collect();
        for cb in ["send", "quit-quick"] {
            assert!(
                available.iter().any(|c| c == cb),
                "QuickVoiceWindow is missing callback `{cb}` (known: {available:?})"
            );
        }
    }

    #[test]
    fn voice_view_properties_exist() {
        for name in ["VoiceCaptureWindow", "VoiceAssistantWindow"] {
            let def = super::compile_component(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            let available: Vec<String> = def.properties().map(|(n, _)| n).collect();
            for prop in crate::voice::view::PROPERTIES {
                assert!(
                    available.iter().any(|n| n == prop),
                    "{name} is missing property `{prop}` (known: {available:?})"
                );
            }
        }
    }
}
