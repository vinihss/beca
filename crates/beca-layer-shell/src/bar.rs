use std::collections::VecDeque;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use beca_core::config::BarModuleConfig;
use beca_core::theme::Theme as BecaTheme;
use beca_ui::{BarWindow, Theme};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Global, LogicalPosition, Timer, TimerMode};

use crate::platform::{install_platform, PlatformQueue};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPosition {
    Top,
    Bottom,
}

pub fn run_bar(
    position: BarPosition,
    bar_cfg: &BarModuleConfig,
    theme: &BecaTheme,
) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|e| format!("wlr-layer-shell unavailable: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;

    let queue: PlatformQueue = Arc::new(Mutex::new(VecDeque::new()));

    install_platform(queue.clone());

    let height = (bar_cfg.height.max(1.0)) as u32;
    let anchor = match position {
        BarPosition::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        BarPosition::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
    };

    let mut state = BarState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        instances: Vec::new(),
        queue,
        height,
        anchor,
        exclusive: bar_cfg.exclusive,
        bar_cfg: bar_cfg.clone(),
        theme: theme.clone(),
        position,
        exit: false,
        quick_child: std::sync::Arc::new(Mutex::new(None)),
        converse_child: std::sync::Arc::new(Mutex::new(None)),
        pointer: None,
    };

    let _ = event_queue.roundtrip(&mut state);
    let outputs: Vec<_> = state.output_state.outputs().collect();
    if outputs.is_empty() {
        state.create_instance(None, &qh)?;
    } else {
        for out in outputs {
            state.create_instance(Some(&out), &qh)?;
        }
    }

    loop {
        slint::platform::update_timers_and_animations();
        state.maybe_render_all(&qh);
        if state.exit {
            break;
        }
        event_queue
            .flush()
            .map_err(|e| format!("flush: {e}"))?;
        event_queue
            .dispatch_pending(&mut state)
            .map_err(|e| format!("dispatch: {e}"))?;
        if state.exit {
            break;
        }

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e}"))?;

        std::thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}

struct Timers {
    _clock: Option<Timer>,
    _volume: Option<Timer>,
    _quick: Option<Timer>,
    _converse: Option<Timer>,
}

struct BarInstance {
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    configured: bool,
    window: Rc<MinimalSoftwareWindow>,
    _bar: BarWindow,
    _timers: Timers,
}

struct BarState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    instances: Vec<BarInstance>,
    queue: PlatformQueue,
    height: u32,
    anchor: Anchor,
    exclusive: bool,
    bar_cfg: BarModuleConfig,
    theme: BecaTheme,
    position: BarPosition,
    exit: bool,
    quick_child: std::sync::Arc<Mutex<Option<std::process::Child>>>,
    converse_child: std::sync::Arc<Mutex<Option<std::process::Child>>>,

    pointer: Option<wl_pointer::WlPointer>,
}

impl BarState {
    fn create_instance(&mut self, output: Option<&wl_output::WlOutput>, qh: &QueueHandle<Self>) -> Result<(), String> {

        let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        self.queue.lock().unwrap().push_back(msw.clone());

        let bar = BarWindow::new().map_err(|e| format!("BarWindow: {e}"))?;
        apply_theme(&bar, &self.theme);
        configure_bar(&bar, &self.bar_cfg, self.position);
        setup_callbacks(&bar, self.quick_child.clone(), self.converse_child.clone());
        let _ = bar.show();
        let _timers = setup_timers(&bar, &self.bar_cfg, self.quick_child.clone(), self.converse_child.clone());

        msw.set_size(slint::PhysicalSize::new(800, self.height));

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("beca-bar"),
            output,
        );
        layer.set_anchor(self.anchor);
        layer.set_size(0, self.height);
        layer.set_exclusive_zone(if self.exclusive { self.height as i32 } else { 0 });
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();

        let pool = SlotPool::new(800 * self.height as usize * 4, &self.shm)
            .map_err(|e| format!("shm pool: {e}"))?;

        self.instances.push(BarInstance {
            layer,
            pool,
            width: 800,
            height: self.height,
            configured: false,
            window: msw,
            _bar: bar,
            _timers,
        });
        Ok(())
    }

    fn instance_for_layer_mut(&mut self, layer: &LayerSurface) -> Option<&mut BarInstance> {
        self.instances.iter_mut().find(|i| &i.layer == layer)
    }
    fn instance_for_surface_mut(&mut self, surface: &wl_surface::WlSurface) -> Option<&mut BarInstance> {
        self.instances.iter_mut().find(|i| i.layer.wl_surface() == surface)
    }

    fn maybe_render_all(&mut self, qh: &QueueHandle<Self>) {
        for idx in 0..self.instances.len() {
            if self.instances[idx].configured {
                let _ = draw_instance_if_needed(&mut self.instances[idx], qh);
            }
        }
    }
}

fn draw_instance_if_needed(inst: &mut BarInstance, qh: &QueueHandle<BarState>) -> bool {
    let width = inst.width.max(1);
    let height = inst.height.max(1);
    let stride = width as i32 * 4;
    let mut did = false;

    let rendered = inst.window.draw_if_needed(|renderer| {
        let (buffer, canvas) = match inst.pool.create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut rgba = vec![PremultipliedRgbaColor { red: 0, green: 0, blue: 0, alpha: 0 }; (width * height) as usize];
        renderer.render(rgba.as_mut_slice(), width as usize);
        for (dst, src) in canvas.chunks_exact_mut(4).zip(rgba.iter()) {
            dst[0] = src.blue;
            dst[1] = src.green;
            dst[2] = src.red;
            dst[3] = src.alpha;
        }
        inst.layer.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
        inst.layer.wl_surface().frame(qh, inst.layer.wl_surface().clone());
        let _ = buffer.attach_to(inst.layer.wl_surface());
        inst.layer.commit();
        did = true;
    });
    rendered && did
}

impl CompositorHandler for BarState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _: u32) {

        self.maybe_render_all(qh);
    }
}

impl OutputHandler for BarState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for BarState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.instances.iter().any(|i| &i.layer == layer) { self.exit = true; }
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (w, h) = (configure.new_size.0, configure.new_size.1);
        if let Some(inst) = self.instance_for_layer_mut(layer) {
            if w > 0 { inst.width = w; }
            if h > 0 { inst.height = h; }
            inst.window.set_size(slint::PhysicalSize::new(inst.width, inst.height));
            inst.configured = true;
            inst.window.request_redraw();
        }
        if let Some(idx) = self.instances.iter().position(|i| &i.layer == layer) {
            let needed = (self.instances[idx].width as usize) * (self.instances[idx].height as usize) * 4;
            if let Ok(new_pool) = SlotPool::new(needed.max(4096), &self.shm) {
                self.instances[idx].pool = new_pool;
            }
        }
        self.maybe_render_all(qh);
    }
}

impl SeatHandler for BarState {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("failed to create wl_pointer");
            self.pointer = Some(pointer);
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for BarState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        use PointerEventKind::*;
        for event in events {
            if let Some(inst) = self.instance_for_surface_mut(&event.surface) {
                let pos = LogicalPosition::new(event.position.0 as f32, event.position.1 as f32);
                match event.kind {
                    Enter { .. } | Motion { .. } => {
                        inst.window.dispatch_event(WindowEvent::PointerMoved { position: pos });
                    }
                    Leave { .. } => {
                        inst.window.dispatch_event(WindowEvent::PointerExited);
                    }
                    Press { button, .. } => {
                        if let Some(btn) = map_button(button) {
                            inst.window.dispatch_event(WindowEvent::PointerPressed { position: pos, button: btn });
                        }
                    }
                    Release { button, .. } => {
                        if let Some(btn) = map_button(button) {
                            inst.window.dispatch_event(WindowEvent::PointerReleased { position: pos, button: btn });
                        }
                    }
                    Axis { .. } => {}
                }
                inst.window.request_redraw();
            }
        }
    }
}

impl ShmHandler for BarState {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

delegate_compositor!(BarState);
delegate_output!(BarState);
delegate_shm!(BarState);
delegate_seat!(BarState);
delegate_pointer!(BarState);
delegate_layer!(BarState);
delegate_registry!(BarState);

impl ProvidesRegistryState for BarState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}

fn map_button(button: u32) -> Option<PointerEventButton> {
    match button {
        0x110 => Some(PointerEventButton::Left),
        0x111 => Some(PointerEventButton::Right),
        0x112 => Some(PointerEventButton::Middle),
        _ => Some(PointerEventButton::Other),
    }
}

fn apply_theme(bar: &BarWindow, theme: &BecaTheme) {
    let tokens = beca_ui::theme::ThemeTokens::from_voice_theme(&theme.voice_theme());
    tokens.apply_typed(&Theme::get(bar));
}

fn configure_bar(bar: &BarWindow, cfg: &BarModuleConfig, position: BarPosition) {
    bar.set_position(match position { BarPosition::Top => "top".into(), BarPosition::Bottom => "bottom".into() });
    bar.set_show_clock(cfg.show_clock);
    bar.set_show_launcher(cfg.show_launcher);
    bar.set_launcher_label(cfg.launcher_label.clone().into());
    bar.set_show_volume_out(cfg.show_volume_out);
    bar.set_show_volume_in(cfg.show_volume_in);
    bar.set_show_voice(cfg.show_voice);
    bar.set_voice_label(cfg.voice_label.clone().into());
    bar.set_show_voice_assistant(true);
    bar.set_voice_assistant_label("VA".into());
    bar.set_clock_section(cfg.clock_section.clone().into());
    bar.set_launcher_section(cfg.launcher_section.clone().into());
    bar.set_volume_out_section(cfg.volume_out_section.clone().into());
    bar.set_volume_in_section(cfg.volume_in_section.clone().into());
    bar.set_voice_section(cfg.voice_section.clone().into());
    bar.set_voice_assistant_section(cfg.voice_section.clone().into());
    bar.set_conversation_active(false);
    bar.set_left_align(cfg.left_align.clone().into());
    bar.set_center_align(cfg.center_align.clone().into());
    bar.set_right_align(cfg.right_align.clone().into());
    bar.set_background_opacity(cfg.background_opacity);
    bar.set_show_border(cfg.show_border);
    bar.set_clock_text(format_clock(&cfg.clock_format).into());
    if cfg.show_volume_out { let v = read_volume("@DEFAULT_AUDIO_SINK@"); bar.set_volume_out_text(v.text.into()); bar.set_volume_out_muted(v.muted); }
    if cfg.show_volume_in { let v = read_volume("@DEFAULT_AUDIO_SOURCE@"); bar.set_volume_in_text(v.text.into()); bar.set_volume_in_muted(v.muted); }
}

fn quick_spawn_or_signal(slot: &std::sync::Arc<Mutex<Option<std::process::Child>>>) {
    let mut slot = match slot.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut alive = false;
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(None) => alive = true,
            Ok(Some(_)) => {
                *slot = None;
            }
            Err(_) => alive = true,
        }
    }
    if alive {

        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(dir).join("beca").join("quick.ctl");
            if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
                use std::io::Write;
                let _ = s.write_all(b"send\n");
            } else {
                eprintln!("[beca-bar] quick session ctl unreachable: {}", path.display());
            }
        }
        return;
    }

    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .env("BECA_NO_DAEMON", "1")
            .arg("mod")
            .arg("--local")
            .arg("voice:quick")
            .arg("--respond")
            .spawn();
        match child {
            Ok(c) => *slot = Some(c),
            Err(e) => eprintln!("[beca-bar] failed to spawn voice:quick: {e}"),
        }
    }
}

fn converse_spawn_or_signal(slot: &std::sync::Arc<Mutex<Option<std::process::Child>>>) -> bool {
    let mut slot = match slot.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut alive = false;
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(None) => alive = true,
            Ok(Some(_)) => {
                *slot = None;
            }
            Err(_) => alive = true,
        }
    }
    if alive {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(dir).join("beca").join("converse.ctl");
            if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
                use std::io::Write;
                let _ = s.write_all(b"quit\n");
            } else {
                eprintln!("[beca-bar] converse ctl unreachable: {}", path.display());
            }
        }
        return false;
    }

    if let Ok(exe) = std::env::current_exe() {
        let child = Command::new(exe)
            .env("BECA_NO_DAEMON", "1")
            .arg("mod")
            .arg("--local")
            .arg("voice:converse")
            .spawn();
        match child {
            Ok(c) => {
                *slot = Some(c);
                return true;
            }
            Err(e) => eprintln!("[beca-bar] failed to spawn voice:converse: {e}"),
        }
    }
    false
}

fn setup_callbacks(bar: &BarWindow, quick_slot: std::sync::Arc<Mutex<Option<std::process::Child>>>, converse_slot: std::sync::Arc<Mutex<Option<std::process::Child>>>) {
    bar.on_launcher_clicked(|| {
        let exe = std::env::current_exe().ok().and_then(|p| p.to_str().map(|s| s.to_string())).unwrap_or_else(|| "beca".into());
        let _ = Command::new(exe).args(["mod", "launcher:show"]).spawn();
    });
    let weak_voice = bar.as_weak();
    bar.on_voice_clicked(move || {
        quick_spawn_or_signal(&quick_slot);

        if let Some(bar) = weak_voice.upgrade() {
            bar.set_voice_recording(true);
        }
    });
    bar.on_voice_assistant_clicked(|| {
        let req = serde_json::json!({"type":"mod","target":"voice:assistant","args":[]});
        let _ = send_daemon(&req);
    });
    let weak_converse = bar.as_weak();
    bar.on_conversation_clicked(move || {
        let active = converse_spawn_or_signal(&converse_slot);

        if let Some(bar) = weak_converse.upgrade() {
            bar.set_conversation_active(active);
        }
    });
}

fn send_daemon(req: &serde_json::Value) {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = std::path::PathBuf::from(dir).join("beca").join("beca.sock");
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(path) {
            use std::io::Write;
            let _ = writeln!(stream, "{}", req);
        }
    }
}

fn setup_timers(bar: &BarWindow, cfg: &BarModuleConfig, quick_slot: std::sync::Arc<Mutex<Option<std::process::Child>>>, converse_slot: std::sync::Arc<Mutex<Option<std::process::Child>>>) -> Timers {
    let clock = if cfg.show_clock {
        let weak = bar.as_weak();
        let fmt = cfg.clock_format.clone();
        let interval = if fmt.contains("%S") { Duration::from_secs(1) } else { Duration::from_secs(30) };
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, interval, move || {
            if let Some(bar) = weak.upgrade() { bar.set_clock_text(format_clock(&fmt).into()); }
        });
        Some(timer)
    } else { None };

    let volume = if cfg.show_volume_out || cfg.show_volume_in {
        let weak = bar.as_weak();
        let out = cfg.show_volume_out;
        let inp = cfg.show_volume_in;
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
            if let Some(bar) = weak.upgrade() {
                if out { let v = read_volume("@DEFAULT_AUDIO_SINK@"); bar.set_volume_out_text(v.text.into()); bar.set_volume_out_muted(v.muted); }
                if inp { let v = read_volume("@DEFAULT_AUDIO_SOURCE@"); bar.set_volume_in_text(v.text.into()); bar.set_volume_in_muted(v.muted); }
            }
        });
        Some(timer)
    } else { None };

    let quick = {
        let weak = bar.as_weak();
        let timer = Timer::default();
        let mut was_running = false;
        timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            let mut run = false;
            if let Ok(mut slot) = quick_slot.lock() {
                if let Some(child) = slot.as_mut() {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            *slot = None;
                        }
                        Ok(None) => {
                            run = true;
                        }
                        Err(_) => {
                            run = true;
                        }
                    }
                }
            }
            if run != was_running {
                was_running = run;
                if let Some(bar) = weak.upgrade() {
                    bar.set_voice_recording(run);
                }
            }
        });
        Some(timer)
    };

    let converse = {
        let weak = bar.as_weak();
        let timer = Timer::default();
        let mut was_running = false;
        timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            let mut run = false;
            if let Ok(mut slot) = converse_slot.lock() {
                if let Some(child) = slot.as_mut() {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            *slot = None;
                        }
                        Ok(None) => {
                            run = true;
                        }
                        Err(_) => {
                            run = true;
                        }
                    }
                }
            }
            if run != was_running {
                was_running = run;
                if let Some(bar) = weak.upgrade() {
                    bar.set_conversation_active(run);
                }
            }
        });
        Some(timer)
    };

    Timers { _clock: clock, _volume: volume, _quick: quick, _converse: converse }
}

fn format_clock(format: &str) -> String {
    let out = Command::new("date").arg(format!("+{}", format)).output();
    match out { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(), _ => "--:--".to_string() }
}

#[derive(Clone, Debug)]
struct VolumeInfo { text: String, muted: bool }

fn read_volume_wpctl(target: &str) -> Option<VolumeInfo> {
    let out = Command::new("wpctl").args(["get-volume", target]).output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let muted = s.contains("[MUTED]");
    let vol = s.split_whitespace().find_map(|p| p.parse::<f32>().ok()).unwrap_or(0.0);
    Some(VolumeInfo { text: format!("{}%", (vol * 100.0).round().clamp(0.0, 999.0) as i32), muted })
}

fn read_volume_pactl(target: &str) -> Option<VolumeInfo> {
    let (cmd_target, mute_flag) = if target == "@DEFAULT_AUDIO_SINK@" { ("@DEFAULT_SINK@", "get-sink-mute") } else { ("@DEFAULT_SOURCE@", "get-source-mute") };
    let vol_out = if target == "@DEFAULT_AUDIO_SINK@" {
        Command::new("pactl").args(["get-sink-volume", cmd_target]).output().ok()
    } else {
        Command::new("pactl").args(["get-source-volume", cmd_target]).output().ok()
    }?;
    if !vol_out.status.success() { return None; }
    let vs = String::from_utf8_lossy(&vol_out.stdout);
    let pct = vs.split('%').next().and_then(|h| h.split_whitespace().last()).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
    let muted = Command::new("pactl").args([mute_flag, cmd_target]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).contains("yes")).unwrap_or(false);
    Some(VolumeInfo { text: format!("{pct}%"), muted })
}

fn read_volume(target: &str) -> VolumeInfo {
    read_volume_wpctl(target).or_else(|| read_volume_pactl(target)).unwrap_or(VolumeInfo { text: "--%".into(), muted: false })
}
