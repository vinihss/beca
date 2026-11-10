use anyhow::Result;
use mlua::{Function, Lua, Table};
use std::fs;

/// Load a module script from XDG (`$XDG_CONFIG_HOME/beca/scripts/<name>.lua`)
/// falling back to the embedded default.
pub fn load_module_script(name: &str, embedded: &str) -> String {
    let paths = crate::config::BecaPaths::resolve();
    let filename = if name.ends_with(".lua") || name.ends_with(".luau") {
        name.to_string()
    } else {
        format!("{name}.lua")
    };
    let disk = paths.scripts_dir.join(&filename);
    match fs::read_to_string(&disk) {
        Ok(s) => {
            eprintln!("[beca] loaded script: {}", disk.display());
            s
        }
        Err(_) => embedded.to_string(),
    }
}

/// Call a lifecycle hook if defined: checks `beca.hooks.<name>` then `beca.<name>`.
pub fn call_hook(lua: &Lua, name: &str) {
    let Ok(beca) = lua.globals().get::<Table>("beca") else {
        return;
    };
    if let Ok(hooks) = beca.get::<Table>("hooks") {
        if let Ok(f) = hooks.get::<Function>(name) {
            if let Err(e) = f.call::<()>(()) {
                eprintln!("[beca] hook '{name}' failed: {e}");
            }
            return;
        }
    }
    if let Ok(f) = beca.get::<Function>(name) {
        if let Err(e) = f.call::<()>(()) {
            eprintln!("[beca] hook '{name}' failed: {e}");
        }
    }
}

/// Execute a named script with embedded fallback.
/// Sets module context for reload, then runs on_load hook.
pub fn exec_module_script(lua: &Lua, name: &str, embedded: &str) -> Result<()> {
    // store context for reload
    if let Ok(beca) = lua.globals().get::<Table>("beca") {
        if let Ok(runtime) = beca.get::<Table>("runtime") {
            let _ = runtime.set("_module", name);
            let _ = runtime.set("_embedded", embedded);
        }
    }
    let code = load_module_script(name, embedded);
    lua.load(&code)
        .set_name(name)
        .exec()
        .map_err(|e| anyhow::anyhow!("failed to execute script '{name}': {e}"))?;
    call_hook(lua, "on_load");
    Ok(())
}
