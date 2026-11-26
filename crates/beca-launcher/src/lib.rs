use anyhow::{Context, Result};
use mlua::{Function, Lua, Table};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{ComponentHandle, Global, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use beca_layer_shell::LauncherAction;
use beca_ui::{AppEntry, SearchWindow, voice::MicCapture};

const EMBEDDED_LAUNCHER: &str = include_str!("../../../scripts/launcher.lua");

fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!(e.to_string())
}

/// Eventos de voz entregues ao event loop via Timer (o caminho já usado pelo
/// socket de controle), sem depender de invoke_from_event_loop.
enum VoiceEvent {
    Endpoint(u64),
    Transcribed(u64, std::result::Result<String, String>),
}

type TypingState = Rc<RefCell<Option<(Vec<char>, usize)>>>;

fn apply_beca_theme(app: &SearchWindow) {
    let resolved = beca_core::config::load();
    let tokens = beca_ui::theme::ThemeTokens::from_voice_theme(&resolved.theme.voice_theme());
    tokens.apply_typed(&beca_ui::Theme::get(app));
}

pub fn control_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("beca").join("launcher.ctl")
    } else {
        beca_core::config::BecaPaths::resolve()
            .config_dir
            .join("launcher.ctl")
    }
}

pub fn send_control(cmd: &str) -> Result<()> {
    let path = control_socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("launcher control socket unavailable: {}", path.display()))?;
    writeln!(stream, "{cmd}")?;
    stream.flush()?;
    Ok(())
}

/// Remove o ponto final que o STT adiciona; a busca não deve herdar pontuação.
fn clean_transcript(text: &str) -> String {
    text.trim().trim_end_matches('.').trim_end().to_string()
}

fn run_search(slot: &Rc<RefCell<Option<Table>>>, query: &str) {
    if let Some(search) = slot.borrow().clone()
        && let Ok(cb) = search.get::<Function>("on_search")
        && let Err(e) = cb.call::<()>(query.to_string())
    {
        eprintln!("Lua on_search failed: {e}");
    }
}

fn clear_results(window: &SearchWindow) {
    window.set_results(ModelRc::new(VecModel::from(Vec::<AppEntry>::new())));
    window.set_selected_index(0);
}

fn web_search_url(term: &str) -> String {
    let mut url = String::from("https://www.google.com/search?q=");
    for byte in term.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                url.push(byte as char);
            }
            b' ' => url.push('+'),
            _ => {
                url.push('%');
                url.push_str(&format!("{byte:02X}"));
            }
        }
    }
    url
}

const WEB_HISTORY_LIMIT: usize = 8;

fn web_history_path() -> PathBuf {
    beca_core::config::BecaPaths::resolve()
        .config_dir
        .join("launcher_web_history.txt")
}

/// Reordena o histórico: termo repetido sobe para o topo, duplicatas somem.
fn update_web_history(existing: Vec<String>, term: &str) -> Vec<String> {
    let term = term.trim();
    if term.is_empty() {
        return existing;
    }

    let canonical = existing
        .iter()
        .map(|item| item.trim())
        .find(|item| item.eq_ignore_ascii_case(term))
        .unwrap_or(term)
        .to_string();
    let mut history = vec![canonical];
    for item in existing {
        let item = item.trim();
        if item.is_empty()
            || item.eq_ignore_ascii_case(term)
            || history.iter().any(|kept| kept.eq_ignore_ascii_case(item))
        {
            continue;
        }
        history.push(item.to_string());
        if history.len() >= WEB_HISTORY_LIMIT {
            break;
        }
    }
    history
}

fn load_web_history() -> Vec<String> {
    let Ok(content) = fs::read_to_string(web_history_path()) else {
        return Vec::new();
    };
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(WEB_HISTORY_LIMIT)
        .map(str::to_string)
        .collect()
}

/// Busca por subsequência, sem diferenciar maiúsculas de minúsculas.
fn web_history_matches(term: &str, query: &str) -> bool {
    let query = query.trim();
    query.is_empty() || term.to_lowercase().contains(&query.to_lowercase())
}

fn refresh_web_history(window: &SearchWindow) {
    let query = window.get_query();
    let history: Vec<SharedString> = load_web_history()
        .into_iter()
        .filter(|term| web_history_matches(term, query.as_ref()))
        .map(SharedString::from)
        .collect();
    window.set_web_history(ModelRc::new(VecModel::from(history)));
}

fn remember_web_search(term: &str) {
    let path = web_history_path();
    let existing = load_web_history();
    let history = update_web_history(existing, term);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut content = history.join("\n");
    content.push('\n');
    let _ = fs::write(path, content);
}

fn open_web_search(term: &str) {
    let term = term.trim();
    if term.is_empty() {
        return;
    }
    remember_web_search(term);
    let _ = Command::new("xdg-open")
        .arg(web_search_url(term))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// O layer shell intercepta Enter antes do Slint; esta função aplica a ação
/// da sessão ativa (Apps abre o resultado, Web abre o navegador padrão).
fn activate_current(window: &SearchWindow) {
    match window.get_session() {
        0 => window.invoke_activate_current(),
        1 => {
            let query = window.get_query();
            open_web_search(query.as_ref());
            refresh_web_history(window);
            let _ = window.hide();
        }
        // Files e Folders ainda não têm ação.
        _ => {}
    }
}

const SESSION_COUNT: i32 = 4;

fn next_session(current: i32) -> i32 {
    (current + 1) % SESSION_COUNT
}

/// Aplica a sessão escolhida por clique ou por Tab, com o mesmo reset.
fn select_session(window: &SearchWindow, slot: &Rc<RefCell<Option<Table>>>, session: i32) {
    window.set_session(session);
    window.set_query("".into());
    window.set_selected_index(0);
    clear_results(window);
    if session == 0 {
        run_search(slot, "");
    } else if session == 1 {
        refresh_web_history(window);
    }
}

/// Alerta visual one-shot do projeto, sem bloquear o event loop do launcher.
fn show_alert(title: &str, message: &str, timeout_ms: u64) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(exe)
        .args(["mod", "alert:show"])
        .arg(message)
        .arg("--title")
        .arg(title)
        .arg("--timeout-ms")
        .arg(timeout_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn show() -> Result<()> {
    if send_control("show").is_ok() {
        return Ok(());
    }

    run_launcher(false)
}

pub fn serve() -> Result<()> {
    run_launcher(true)
}

fn run_launcher(resident: bool) -> Result<()> {
    let queue: Rc<RefCell<VecDeque<Rc<MinimalSoftwareWindow>>>> =
        Rc::new(RefCell::new(VecDeque::new()));
    beca_layer_shell::install_platform(queue.clone());
    let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    queue.borrow_mut().push_back(msw.clone());

    let window = SearchWindow::new().context("could not create SearchWindow")?;
    apply_beca_theme(&window);
    refresh_web_history(&window);
    window.hide().ok();
    let quit = Arc::new(AtomicBool::new(false));

    let lua = Lua::new();
    let beca = beca_core::lua::bridge::install_beca_base(&lua).map_err(lua_err)?;
    let search_slot: Rc<RefCell<Option<Table>>> = Rc::new(RefCell::new(None));

    let apps: Table = beca.get("apps").map_err(lua_err)?;
    apps.set(
        "search",
        lua.create_function(|lua, query: String| {
            let results = beca_apps::search(&query);
            let t = lua.create_table()?;
            for (i, app) in results.iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("name", app.name.clone())?;
                let icon = beca_apps::desktop_entry::resolve_icon(&app.icon).unwrap_or_default();
                entry.set("icon", icon)?;
                t.set(i + 1, entry)?;
            }
            Ok(t)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;
    apps.set(
        "launch",
        lua.create_function(|_, name: String| {
            beca_apps::launch(&name)
                .map(|_| ())
                .map_err(mlua::Error::external)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    let ui: Table = beca.get("ui").map_err(lua_err)?;
    let weak = window.as_weak();
    let slot_for_create = search_slot.clone();
    ui.set(
        "search_window",
        lua.create_function(move |lua, options: Option<Table>| {
            if let Some(options) = options {
                if let Ok(placeholder) = options.get::<String>("placeholder")
                    && let Some(w) = weak.upgrade()
                {
                    w.set_placeholder(placeholder.into());
                }
                let _ = options.get::<String>("title");
            }
            let search = lua.create_table()?;
            let weak_results = weak.clone();
            let image_cache: Rc<RefCell<HashMap<String, slint::Image>>> =
                Rc::new(RefCell::new(HashMap::new()));
            search.set(
                "set_results",
                lua.create_function(move |_, args: mlua::MultiValue| {
                    let items_val = if args.len() >= 2 {
                        args.into_iter().nth(1).unwrap_or(mlua::Value::Nil)
                    } else {
                        args.into_iter().next().unwrap_or(mlua::Value::Nil)
                    };
                    if let mlua::Value::Table(items) = items_val {
                        let mut results: Vec<AppEntry> = Vec::new();
                        let len = items.len().unwrap_or(0);
                        for i in 1..=len {
                            match items.get::<mlua::Value>(i) {
                                Ok(mlua::Value::Table(t)) => {
                                    let name = t.get::<String>("name").unwrap_or_default();
                                    let icon_path = t.get::<String>("icon").unwrap_or_default();
                                    let icon = if !icon_path.is_empty() {
                                        if let Some(cached) = image_cache.borrow().get(&icon_path) {
                                            cached.clone()
                                        } else if Path::new(&icon_path).exists() {
                                            let img =
                                                slint::Image::load_from_path(Path::new(&icon_path))
                                                    .unwrap_or_default();
                                            image_cache
                                                .borrow_mut()
                                                .insert(icon_path.clone(), img.clone());
                                            img
                                        } else {
                                            slint::Image::default()
                                        }
                                    } else {
                                        slint::Image::default()
                                    };
                                    results.push(AppEntry {
                                        name: name.into(),
                                        icon,
                                    });
                                }
                                Ok(mlua::Value::String(s)) => {
                                    if let Ok(s) = s.to_str() {
                                        results.push(AppEntry {
                                            name: s.to_string().into(),
                                            icon: slint::Image::default(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Some(w) = weak_results.upgrade() {
                            w.set_results(ModelRc::new(VecModel::from(results)));
                            w.set_selected_index(0);
                        }
                    }
                    Ok(())
                })?,
            )?;
            let weak_show = weak.clone();
            let slot_show = slot_for_create.clone();
            search.set(
                "show",
                lua.create_function(move |_, ()| {
                    if let Some(w) = weak_show.upgrade() {
                        w.set_query("".into());
                        w.set_selected_index(0);
                    }
                    if let Some(search) = slot_show.borrow().clone()
                        && let Ok(cb) = search.get::<Function>("on_search")
                    {
                        let _ = cb.call::<()>(String::new());
                    }
                    if let Some(w) = weak_show.upgrade() {
                        w.window().request_redraw();
                        let _ = w.show();
                    }
                    Ok(())
                })?,
            )?;
            let weak_hide = weak.clone();
            search.set(
                "hide",
                lua.create_function(move |_, ()| {
                    if let Some(w) = weak_hide.upgrade() {
                        let _ = w.hide();
                    }
                    Ok(())
                })?,
            )?;
            *slot_for_create.borrow_mut() = Some(search.clone());
            Ok(search)
        })
        .map_err(lua_err)?,
    )
    .map_err(lua_err)?;

    {
        let weak = window.as_weak();
        let slot = search_slot.clone();
        window.on_search_requested(move |query| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_session() != 0 {
                clear_results(&w);
                if w.get_session() == 1 {
                    refresh_web_history(&w);
                }
                return;
            }
            run_search(&slot, query.as_ref());
        });
    }
    {
        let weak = window.as_weak();
        let slot = search_slot.clone();
        window.on_session_requested(move |session| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Web abre no Enter; Files/Folders ainda não implementados.
            select_session(&w, &slot, session);
        });
    }
    {
        let weak = window.as_weak();
        window.on_web_requested(move |text| {
            open_web_search(text.as_ref());
            if let Some(w) = weak.upgrade() {
                refresh_web_history(&w);
                let _ = w.hide();
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_web_history_selected(move |term| {
            open_web_search(term.as_ref());
            if let Some(w) = weak.upgrade() {
                refresh_web_history(&w);
                let _ = w.hide();
            }
        });
    }
    {
        let slot = search_slot.clone();
        let weak_hide = window.as_weak();
        window.on_result_selected(move |item| {
            if let Some(search) = slot.borrow().clone()
                && let Ok(cb) = search.get::<Function>("on_select")
                && let Err(e) = cb.call::<()>(item.to_string())
            {
                eprintln!("Lua on_select failed: {e}");
            }

            if let Some(w) = weak_hide.upgrade() {
                let _ = w.hide();
            }
        });
    }

    beca_core::lua::runtime::exec_module_script(&lua, "launcher", EMBEDDED_LAUNCHER)
        .context("launcher script failed")?;

    let mic_capture: Rc<RefCell<Option<MicCapture>>> = Rc::new(RefCell::new(None));
    let voice_busy = Arc::new(AtomicBool::new(false));
    let voice_generation = Arc::new(AtomicU64::new(0));
    let voice_auto_stop = Arc::new(AtomicBool::new(false));
    let (voice_tx, voice_rx) = mpsc::channel::<VoiceEvent>();
    let typing: TypingState = Rc::new(RefCell::new(None));
    {
        let weak = window.as_weak();
        let mic_capture = mic_capture.clone();
        let busy = voice_busy.clone();
        let generation = voice_generation.clone();
        let auto_stop = voice_auto_stop.clone();
        let endpoint_tx = voice_tx.clone();
        let result_tx = voice_tx.clone();
        let typing = typing.clone();
        window.on_voice_requested(move || {
            // Cliques durante a transcrição são ignorados: cancelar aqui
            // descartava o resultado e fazia a UI parecer inerte.
            if busy.load(Ordering::SeqCst) {
                return;
            }
            generation.fetch_add(1, Ordering::SeqCst);
            typing.borrow_mut().take();

            let recorder = mic_capture.borrow_mut().take();
            let Some(recorder) = recorder else {
                let expected_generation = generation.load(Ordering::SeqCst);
                let tx = endpoint_tx.clone();
                auto_stop.store(true, Ordering::SeqCst);
                let result = MicCapture::start_with_silence_detection(move || {
                    let _ = tx.send(VoiceEvent::Endpoint(expected_generation));
                });

                match result {
                    Ok(recorder) => {
                        eprintln!("[launcher] microphone started");
                        beca_ui::voice::play_start_sound();
                        show_alert("Microfone", "Gravando. Fale agora...", 1200);
                        *mic_capture.borrow_mut() = Some(recorder);
                        if let Some(w) = weak.upgrade() {
                            w.set_voice_active(true);
                        }
                    }
                    Err(e) => {
                        auto_stop.store(false, Ordering::SeqCst);
                        eprintln!("[launcher] microphone start failed: {e}");
                        show_alert("Microfone", &format!("Falha ao iniciar: {e}"), 4000);
                    }
                }
                return;
            };

            // Parada manual (ou automática) cancela qualquer endpoint pendente.
            auto_stop.store(false, Ordering::SeqCst);
            let generation = generation.load(Ordering::SeqCst);
            busy.store(true, Ordering::SeqCst);
            if let Some(w) = weak.upgrade() {
                w.set_voice_active(false);
                w.set_voice_busy(true);
            }

            let audio = match recorder.finish() {
                Ok(audio) => {
                    eprintln!(
                        "[launcher] microphone stopped ({} samples)",
                        audio.sample_count()
                    );
                    audio
                }
                Err(e) => {
                    eprintln!("[launcher] microphone stop failed: {e}");
                    busy.store(false, Ordering::SeqCst);
                    if let Some(w) = weak.upgrade() {
                        w.set_voice_busy(false);
                    }
                    show_alert("Microfone", &format!("Falha ao parar: {e}"), 4000);
                    return;
                }
            };

            show_alert("Transcrição", "Processando áudio...", 1200);
            let daemon_url = beca_core::config::load().config.modules.voice.daemon_url;
            let tx = result_tx.clone();
            thread::spawn(move || {
                let result = audio.transcribe(&daemon_url);
                let _ = tx.send(VoiceEvent::Transcribed(generation, result));
            });
        });
    }

    // Eventos de voz são consumidos por um Timer, como já acontece com o
    // socket de controle: isso funciona no event loop custom do layer shell.
    {
        let weak = window.as_weak();
        let busy = voice_busy.clone();
        let generation = voice_generation.clone();
        let auto_stop = voice_auto_stop.clone();
        let typing = typing.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
            while let Ok(event) = voice_rx.try_recv() {
                match event {
                    VoiceEvent::Endpoint(event_generation) => {
                        if event_generation != generation.load(Ordering::SeqCst)
                            || !auto_stop.load(Ordering::SeqCst)
                        {
                            continue;
                        }
                        if let Some(w) = weak.upgrade()
                            && w.get_voice_active()
                            && !busy.load(Ordering::SeqCst)
                        {
                            auto_stop.store(false, Ordering::SeqCst);
                            eprintln!("[launcher] voice endpoint detected");
                            beca_ui::voice::play_stop_sound();
                            w.invoke_voice_requested();
                        }
                    }
                    VoiceEvent::Transcribed(event_generation, result) => {
                        if event_generation != generation.load(Ordering::SeqCst) {
                            continue;
                        }
                        busy.store(false, Ordering::SeqCst);
                        if let Some(w) = weak.upgrade() {
                            w.set_voice_busy(false);
                            match result {
                                Ok(text) => {
                                    let text = clean_transcript(&text);
                                    if text.is_empty() {
                                        show_alert(
                                            "Erro na transcrição",
                                            "Texto vazio após limpeza",
                                            5000,
                                        );
                                        return;
                                    }
                                    w.set_query("".into());
                                    *typing.borrow_mut() = Some((text.chars().collect(), 0));
                                }
                                Err(e) => {
                                    eprintln!("[launcher] voice transcription failed: {e}");
                                    show_alert("Erro na transcrição", &e, 5000);
                                }
                            }
                        }
                    }
                }
            }
        });
        std::mem::forget(timer);
    }

    // Streaming visual: o resultado aparece digitado letra por letra.
    {
        let weak = window.as_weak();
        let typing = typing.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(18), move || {
            let mut state = typing.borrow_mut();
            let Some((chars, index)) = state.as_mut() else {
                return;
            };
            if *index >= chars.len() {
                state.take();
                return;
            }
            let ch = chars[*index];
            *index += 1;
            drop(state);

            if let Some(w) = weak.upgrade() {
                let mut query = w.get_query().to_string();
                query.push(ch);
                let query: SharedString = query.into();
                w.set_query(query.clone());
                w.invoke_search_requested(query);
            }
        });
        std::mem::forget(timer);
    }

    {
        thread::spawn(move || {
            let initial = beca_apps::search("");
            for app in initial.iter().take(40) {
                let _ = beca_apps::desktop_entry::resolve_icon(&app.icon);
            }
        });
    }

    let (nav_tx, nav_rx) = mpsc::channel::<LauncherAction>();

    if resident {
        window
            .window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        start_control_listener(&window, search_slot.clone(), quit.clone(), Some(nav_rx))?;

        let _ = window.hide();
    } else {
        let weak_quit = window.as_weak();
        let quit_close = quit.clone();
        window.window().on_close_requested(move || {
            if let Some(w) = weak_quit.upgrade() {
                let _ = w.hide();
            }
            quit_close.store(true, Ordering::Relaxed);
            slint::CloseRequestResponse::HideWindow
        });
        start_nav_timer(&window, search_slot.clone(), nav_rx);
        let _ = window.show();
    }

    beca_layer_shell::run_launcher_with_nav(msw, quit, Some(nav_tx))
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

fn start_nav_timer(
    window: &SearchWindow,
    slot: Rc<RefCell<Option<Table>>>,
    nav_rx: mpsc::Receiver<LauncherAction>,
) {
    let weak = window.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        while let Ok(action) = nav_rx.try_recv() {
            if let Some(w) = weak.upgrade() {
                match action {
                    LauncherAction::Navigate(0) | LauncherAction::Activate => {
                        activate_current(&w);
                    }
                    LauncherAction::Navigate(delta) => w.invoke_move_selection(delta),
                    LauncherAction::CycleSession => {
                        let session = next_session(w.get_session());
                        select_session(&w, &slot, session);
                    }
                    LauncherAction::VoiceRequested => w.invoke_voice_requested(),
                }
            }
        }
    });
    std::mem::forget(timer);
}

fn start_control_listener(
    window: &SearchWindow,
    slot: Rc<RefCell<Option<Table>>>,
    quit: Arc<AtomicBool>,
    nav_rx: Option<mpsc::Receiver<LauncherAction>>,
) -> Result<()> {
    let path = control_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind launcher control socket {}", path.display()))?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let cmd = line.trim().to_string();
            if cmd.is_empty() {
                continue;
            }
            if tx.send(cmd.clone()).is_err() {
                break;
            }
            if cmd == "quit" || cmd == "stop" {
                break;
            }
        }
        let _ = std::fs::remove_file(control_socket_path());
    });

    let weak = window.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        if let Some(nav_rx) = nav_rx.as_ref() {
            while let Ok(action) = nav_rx.try_recv() {
                if let Some(w) = weak.upgrade() {
                    match action {
                        LauncherAction::Navigate(0) | LauncherAction::Activate => {
                            activate_current(&w);
                        }
                        LauncherAction::Navigate(delta) => w.invoke_move_selection(delta),
                        LauncherAction::CycleSession => {
                            let session = next_session(w.get_session());
                            select_session(&w, &slot, session);
                        }
                        LauncherAction::VoiceRequested => w.invoke_voice_requested(),
                    }
                }
            }
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd.as_str() {
                "show" => {
                    if let Some(w) = weak.upgrade() {
                        select_session(&w, &slot, 0);
                        w.window().request_redraw();
                        let _ = w.show();
                    }
                }
                "hide" => {
                    if let Some(w) = weak.upgrade() {
                        let _ = w.hide();
                    }
                }
                "quit" | "stop" => {
                    if let Some(w) = weak.upgrade() {
                        let _ = w.hide();
                    }
                    quit.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    });

    std::mem::forget(timer);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        WEB_HISTORY_LIMIT, clean_transcript, next_session, update_web_history, web_history_matches,
        web_search_url,
    };

    #[test]
    fn clean_transcript_removes_trailing_dot() {
        assert_eq!(clean_transcript("abrir firefox."), "abrir firefox");
    }

    #[test]
    fn clean_transcript_preserves_internal_punctuation() {
        assert_eq!(clean_transcript("  abrir, firefox.  "), "abrir, firefox");
    }

    #[test]
    fn web_search_url_encodes_term() {
        assert_eq!(
            web_search_url("rust & slint"),
            "https://www.google.com/search?q=rust+%26+slint"
        );
    }

    #[test]
    fn web_history_moves_repeat_to_top_without_duplicates() {
        let history = update_web_history(
            vec![
                "rust".into(),
                "slint".into(),
                "RUST".into(),
                "wayland".into(),
            ],
            "Slint",
        );
        assert_eq!(history, ["slint", "rust", "wayland"]);
    }

    #[test]
    fn web_history_is_limited() {
        let existing: Vec<String> = (0..20).map(|i| format!("term {i}")).collect();
        let history = update_web_history(existing, "novo");
        assert_eq!(history.len(), WEB_HISTORY_LIMIT);
        assert_eq!(history[0], "novo");
        assert_eq!(history[1], "term 0");
    }

    #[test]
    fn web_history_filter_ignores_case_and_matches_middle() {
        assert!(web_history_matches("Rust e Slint", "slint"));
        assert!(web_history_matches("wayland", "LAND"));
        assert!(!web_history_matches("wayland", "gtk"));
        assert!(web_history_matches("qualquer", "  "));
    }

    #[test]
    fn tab_cycles_all_sessions() {
        assert_eq!(next_session(0), 1);
        assert_eq!(next_session(1), 2);
        assert_eq!(next_session(2), 3);
        assert_eq!(next_session(3), 0);
    }
}
