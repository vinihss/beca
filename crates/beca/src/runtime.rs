use anyhow::Result;
use mlua::Lua;
use std::fs;

/// Load a module script from XDG (`$XDG_CONFIG_HOME/beca/scripts/<name>.lua`)
/// falling back to the embedded default.
pub fn load_module_script(name: &str, embedded: &str) -> String {
    let paths = beca_core::BecaPaths::resolve();
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

/// Install base `beca` table with domain namespaces.
/// Domains: apps, ui, voice, events, system, hyprland (stubs for future).
pub fn install_beca_base(lua: &Lua) -> mlua::Result<mlua::Table> {
    let beca = lua.create_table()?;

    // Domain tables (extensible)
    let ui = lua.create_table()?;
    let apps = lua.create_table()?;
    let voice = lua.create_table()?;
    let events = lua.create_table()?;
    let system = lua.create_table()?;
    let hyprland = lua.create_table()?;

    beca.set("ui", ui)?;
    beca.set("apps", apps)?;
    beca.set("voice", voice)?;
    beca.set("events", events)?;
    beca.set("system", system)?;
    beca.set("hyprland", hyprland)?;

    lua.globals().set("beca", beca.clone())?;

    Ok(beca)
}

/// Execute a named script with embedded fallback.
/// `name` is used for XDG lookup and chunk name.
pub fn exec_module_script(lua: &Lua, name: &str, embedded: &str) -> Result<()> {
    let code = load_module_script(name, embedded);
    lua.load(&code)
        .set_name(name)
        .exec()
        .map_err(|e| anyhow::anyhow!("failed to execute script '{name}': {e}"))?;
    Ok(())
}
