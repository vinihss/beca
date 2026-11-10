//! Runtime UI loader using slint-interpreter (XDG + embedded fallbacks).

use slint_interpreter::{Compiler, ComponentDefinition, ComponentHandle, ComponentInstance, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Resolve XDG UI directory for beca: $XDG_CONFIG_HOME/beca/ui (or ~/.config/beca/ui).
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

/// Runtime environment: `BECA_ENVIRONMENT=dev` (or `BECA_ENVIROMENT=dev`) enables dev behavior.
/// Default is `prod`: UI is loaded from embedded/bundled Slint, not from XDG config files.
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

fn embedded_ui_files() -> &'static [(&'static str, &'static str)] {
    &[
        ("library.slint", include_str!("../ui/library.slint")),
        ("theme.slint", include_str!("../ui/theme.slint")),
        (
            "desktop_button.slint",
            include_str!("../ui/desktop_button.slint"),
        ),
        ("info_card.slint", include_str!("../ui/info_card.slint")),
        (
            "media_button.slint",
            include_str!("../ui/media_button.slint"),
        ),
        (
            "waveform_view.slint",
            include_str!("../ui/waveform_view.slint"),
        ),
        (
            "voice_capture_window.slint",
            include_str!("../ui/voice_capture_window.slint"),
        ),
        (
            "search_window.slint",
            include_str!("../ui/search_window.slint"),
        ),
        ("bar_window.slint", include_str!("../ui/bar_window.slint")),
        (
            "clock_widget.slint",
            include_str!("../ui/clock_widget.slint"),
        ),
        (
            "launcher_widget.slint",
            include_str!("../ui/launcher_widget.slint"),
        ),
        (
            "volume_widget.slint",
            include_str!("../ui/volume_widget.slint"),
        ),
        (
            "voice_widget.slint",
            include_str!("../ui/voice_widget.slint"),
        ),
    ]
}

#[derive(Clone)]
struct CachedDef {
    def: ComponentDefinition,
    signature: u64,
}

thread_local! {
    static COMPILE_CACHE: std::cell::RefCell<HashMap<String, CachedDef>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Invalidate cached compiled UI definitions (forces recompilation on next use).
/// Note: cache is thread-local; call from the UI thread that owns the definitions.
pub fn invalidate_cache() {
    COMPILE_CACHE.with(|cache| cache.borrow_mut().clear());
}

fn ui_signature(dir: &PathBuf) -> u64 {
    // Cheap signature: combine file sizes and modified times for known UI files.
    // Changes to any of these invalidate the whole compiled cache.
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

/// Ensure embedded UI source files are available for compilation.
/// - dev (`BECA_ENVIRONMENT=dev`): read/write from XDG config UI dir (editable, live).
/// - prod (default): use bundled embedded files only (written to a runtime cache dir), ignoring XDG edits.
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
                fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
                changed = true;
            }
        }
        if changed {
            invalidate_cache();
        }
        Ok(dir)
    } else {
        // Prod: never read user XDG .slint overrides; compile from embedded bundle.
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

/// Compile a component by name.
/// Results are cached per-process and invalidated when UI source files change.
/// In prod (default) sources come from embedded bundle; in dev from XDG config UI dir.
pub fn compile_component(name: &str) -> Result<ComponentDefinition, String> {
    let dir = ensure_defaults()?;
    // In prod signature is effectively stable (embedded), but we still compute over dir for safety.
    let sig = ui_signature(&dir);
    let cached = COMPILE_CACHE.with(|cache| {
        cache.borrow().get(name).and_then(|entry| {
            if entry.signature == sig {
                Some(entry.def.clone())
            } else {
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
    let def = result
        .component(name)
        .ok_or_else(|| format!("component '{name}' not found in {}", library.display()))?;

    COMPILE_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            name.to_string(),
            CachedDef {
                def: def.clone(),
                signature: sig,
            },
        );
    });
    Ok(def)
}

/// Create a component instance by name.
pub fn create(name: &str) -> Result<ComponentInstance, String> {
    let def = compile_component(name)?;
    def.create().map_err(|e| format!("create '{name}': {e}"))
}

/// Helper: set string property.
pub fn set_string(instance: &ComponentInstance, prop: &str, val: &str) -> Result<(), String> {
    instance
        .set_property(prop, Value::String(val.into()))
        .map_err(|e| format!("set {prop}: {e}"))
}
/// Helper: get string property.
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

/// Parse "#RRGGBB" or "#RRGGBBAA" to slint Color.
pub fn parse_color(hex: &str) -> Option<slint_interpreter::Color> {
    let h = hex.trim().trim_start_matches('#');
    match h.len() {
        6 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some(slint_interpreter::Color::from_rgb_u8(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            let a = u8::from_str_radix(&h[6..8], 16).ok()?;
            Some(slint_interpreter::Color::from_argb_u8(a, r, g, b))
        }
        _ => None,
    }
}

/// Apply theme values to the global `Theme` of a component instance.
pub fn apply_theme_global(instance: &ComponentInstance, t: &crate::voice::VoiceTheme) {
    let set_brush = |name: &str, hex: &str| {
        if let Some(c) = parse_color(hex) {
            let _ = instance.set_global_property("Theme", name, Value::Brush(c.into()));
        }
    };
    // apply as brush for broad compatibility (color/brush props accept Brush values)
    set_brush("background", &t.background);
    set_brush("text-main", &t.text_main);
    set_brush("card-bg", &t.card_bg);
    set_brush("card-border", &t.card_border);
    set_brush("surface", &t.surface);
    set_brush("surface-alt", &t.surface_alt);
    set_brush("border", &t.border);
    set_brush("text-strong", &t.text_strong);
    set_brush("accent", &t.accent);
    set_brush("accent-success", &t.accent_success);
    set_brush("accent-danger", &t.accent_danger);
    set_brush("recording", &t.recording);
    let _ = instance.set_global_property("Theme", "radius", Value::Number(t.radius as f64));
    let _ = instance.set_global_property("Theme", "dark-mode", Value::Bool(t.dark_mode));
}

pub fn show(instance: &ComponentInstance) -> Result<(), String> {
    instance.show().map_err(|e| format!("show: {e}"))
}
pub fn hide(instance: &ComponentInstance) -> Result<(), String> {
    instance.hide().map_err(|e| format!("hide: {e}"))
}
pub fn run_event_loop() -> Result<(), String> {
    slint_interpreter::run_event_loop().map_err(|e| format!("event loop: {e}"))
}
pub fn quit_event_loop() {
    let _ = slint_interpreter::quit_event_loop();
}
pub fn invoke_on_event_loop(f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    slint_interpreter::invoke_from_event_loop(f).map_err(|e| format!("invoke: {e}"))
}
