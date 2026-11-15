use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, SharedString};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RepeatInfo},
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
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use xkeysym::Keysym;

const WIDTH: u32 = 560;
const HEIGHT: u32 = 420;

struct LauncherInstance {
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    configured: bool,
}

struct LauncherState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<Self>,

    window: Rc<MinimalSoftwareWindow>,
    instance: Option<LauncherInstance>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_focused: bool,
    exit: bool,
    nav_tx: Option<std::sync::mpsc::Sender<i32>>,
}

impl LauncherState {
    fn show_surface(&mut self) -> Result<(), String> {
        if self.instance.is_some() {
            return Ok(());
        }
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("beca-launcher"),
            None,
        );

        layer.set_anchor(Anchor::empty());
        layer.set_size(WIDTH, HEIGHT);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();

        self.window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
        let pool = SlotPool::new((WIDTH * HEIGHT * 4) as usize, &self.shm)
            .map_err(|e| format!("shm pool: {e}"))?;

        self.instance = Some(LauncherInstance {
            layer,
            pool,
            width: WIDTH,
            height: HEIGHT,
            configured: false,
        });
        self.keyboard_focused = false;
        Ok(())
    }

    fn hide_surface(&mut self) {

        self.instance = None;
        self.keyboard_focused = false;
    }

    fn maybe_sync_visibility(&mut self) -> Result<(), String> {
        let visible = self.window.is_visible();
        if visible && self.instance.is_none() {
            self.show_surface()?;
        } else if !visible && self.instance.is_some() {
            self.hide_surface();
        }
        Ok(())
    }

    fn instance_for_layer_mut(&mut self, layer: &LayerSurface) -> Option<&mut LauncherInstance> {
        self.instance
            .as_mut()
            .filter(|i| &i.layer == layer)
    }

    fn instance_for_surface(&self, surface: &wl_surface::WlSurface) -> bool {
        self.instance
            .as_ref()
            .map(|i| i.layer.wl_surface() == surface)
            .unwrap_or(false)
    }

    fn draw_if_needed(&mut self) {
        let Some(inst) = self.instance.as_mut() else { return };
        if !inst.configured {
            return;
        }
        let width = inst.width.max(1);
        let height = inst.height.max(1);
        let stride = width as i32 * 4;
        let mut did = false;
        let rendered = self.window.draw_if_needed(|renderer| {
            let (buffer, canvas) = match inst.pool.create_buffer(
                width as i32,
                height as i32,
                stride,
                wl_shm::Format::Argb8888,
            ) {
                Ok(b) => b,
                Err(_) => return,
            };
            let mut rgba = vec![
                PremultipliedRgbaColor {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 0,
                };
                (width * height) as usize
            ];
            renderer.render(rgba.as_mut_slice(), width as usize);
            for (dst, src) in canvas.chunks_exact_mut(4).zip(rgba.iter()) {
                dst[0] = src.blue;
                dst[1] = src.green;
                dst[2] = src.red;
                dst[3] = src.alpha;
            }
            inst.layer
                .wl_surface()
                .damage_buffer(0, 0, width as i32, height as i32);
            inst.layer.wl_surface().frame(&self.qh, inst.layer.wl_surface().clone());
            let _ = buffer.attach_to(inst.layer.wl_surface());
            inst.layer.commit();
            did = true;
        });
        let _ = (rendered, did);
    }
}

pub fn run_launcher(
    window: Rc<MinimalSoftwareWindow>,
    quit: Arc<AtomicBool>,
) -> Result<(), String> {
    run_launcher_with_nav(window, quit, None)
}

pub fn run_launcher_with_nav(
    window: Rc<MinimalSoftwareWindow>,
    quit: Arc<AtomicBool>,
    nav_tx: Option<std::sync::mpsc::Sender<i32>>,
) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("wlr-layer-shell unavailable: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;

    let mut state = LauncherState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        qh: qh.clone(),
        window,
        instance: None,
        pointer: None,
        keyboard: None,
        keyboard_focused: false,
        exit: false,
        nav_tx,
    };

    loop {
        slint::platform::update_timers_and_animations();
        if quit.load(Ordering::Relaxed) || state.exit {
            break;
        }
        state.maybe_sync_visibility()?;
        state.draw_if_needed();

        event_queue.flush().map_err(|e| format!("flush: {e}"))?;
        event_queue
            .dispatch_pending(&mut state)
            .map_err(|e| format!("dispatch: {e}"))?;

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e}"))?;

        std::thread::sleep(Duration::from_millis(4));
    }

    Ok(())
}

impl CompositorHandler for LauncherState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.draw_if_needed();
    }
}

impl OutputHandler for LauncherState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for LauncherState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        let _ = self.window.hide();
        self.hide_surface();

    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = (configure.new_size.0, configure.new_size.1);

        let dims = if let Some(inst) = self.instance_for_layer_mut(layer) {
            inst.width = if w > 0 { w } else { WIDTH };
            inst.height = if h > 0 { h } else { HEIGHT };
            inst.configured = true;
            Some((inst.width, inst.height))
        } else {
            None
        };
        if let Some((width, height)) = dims {
            self.window.set_size(PhysicalSize::new(width, height));
            let needed = (width as usize) * (height as usize) * 4;
            if let Ok(new_pool) = SlotPool::new(needed.max(4096), &self.shm) {
                if let Some(inst) = self.instance_for_layer_mut(layer) {
                    inst.pool = new_pool;
                }
            }
            self.window.request_redraw();
        }
    }
}

impl SeatHandler for LauncherState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(pointer);
            }
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            if let Ok(kb) = self.seat_state.get_keyboard(qh, &seat, None) {
                self.keyboard = Some(kb);
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(kb) = self.keyboard.take() {
                kb.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for LauncherState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            if !self.instance_for_surface(&event.surface) {
                continue;
            }
            let pos = LogicalPosition::new(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                Enter { .. } | Motion { .. } => {
                    self.window
                        .dispatch_event(WindowEvent::PointerMoved { position: pos });
                }
                Leave { .. } => {
                    self.window.dispatch_event(WindowEvent::PointerExited);
                }
                Press { button, .. } => {
                    if let Some(btn) = map_button(button) {
                        self.window.dispatch_event(WindowEvent::PointerPressed {
                            position: pos,
                            button: btn,
                        });
                    }
                }
                Release { button, .. } => {
                    if let Some(btn) = map_button(button) {
                        self.window.dispatch_event(WindowEvent::PointerReleased {
                            position: pos,
                            button: btn,
                        });
                    }
                }
                Axis {
                    horizontal,
                    vertical,
                    ..
                } => {

                    let dx = -horizontal.absolute as f32;
                    let dy = -vertical.absolute as f32;
                    if dx != 0.0 || dy != 0.0 {
                        self.window.dispatch_event(WindowEvent::PointerScrolled {
                            position: pos,
                            delta_x: dx,
                            delta_y: dy,
                        });
                    }
                }
            }
            self.window.request_redraw();
        }
    }
}

impl KeyboardHandler for LauncherState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        if self.instance_for_surface(surface) {
            self.keyboard_focused = true;
            self.window
                .dispatch_event(WindowEvent::WindowActiveChanged(true));
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        if self.instance_for_surface(surface) {
            self.keyboard_focused = false;
            self.window
                .dispatch_event(WindowEvent::WindowActiveChanged(false));
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if !self.keyboard_focused {
            return;
        }
        if let Some(text) = keysym_to_slint_text(&event) {
            let esc: SharedString = Key::Escape.into();
            let up: SharedString = Key::UpArrow.into();
            let down: SharedString = Key::DownArrow.into();
            let ret: SharedString = Key::Return.into();
            if text == esc {
                let _ = self.window.hide();
                self.hide_surface();
                return;
            }
            if let Some(tx) = &self.nav_tx {
                if text == up {
                    let _ = tx.send(-1);
                    self.window.request_redraw();
                    return;
                }
                if text == down {
                    let _ = tx.send(1);
                    self.window.request_redraw();
                    return;
                }
                if text == ret {
                    let _ = tx.send(0);
                    self.window.request_redraw();
                    return;
                }
            }
            self.window.dispatch_event(WindowEvent::KeyPressed { text });
        }
        self.window.request_redraw();
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if !self.keyboard_focused {
            return;
        }
        if let Some(text) = keysym_to_slint_text(&event) {
            let esc: SharedString = Key::Escape.into();
            let up: SharedString = Key::UpArrow.into();
            let down: SharedString = Key::DownArrow.into();
            let ret: SharedString = Key::Return.into();
            if text == esc || text == up || text == down || text == ret {
                self.window.request_redraw();
                return;
            }
            self.window.dispatch_event(WindowEvent::KeyReleased { text });
        }
        self.window.request_redraw();
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
    ) {
    }

    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _info: RepeatInfo,
    ) {
    }
}

impl ShmHandler for LauncherState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(LauncherState);
delegate_output!(LauncherState);
delegate_shm!(LauncherState);
delegate_seat!(LauncherState);
delegate_pointer!(LauncherState);
smithay_client_toolkit::delegate_keyboard!(LauncherState);
delegate_layer!(LauncherState);
delegate_registry!(LauncherState);

impl ProvidesRegistryState for LauncherState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
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

fn keysym_to_slint_text(event: &KeyEvent) -> Option<SharedString> {

    let special: Option<SharedString> = match event.keysym {
        Keysym::Escape => Some(Key::Escape.into()),
        Keysym::Return | Keysym::KP_Enter => Some(Key::Return.into()),
        Keysym::BackSpace => Some(Key::Backspace.into()),
        Keysym::Tab | Keysym::ISO_Left_Tab => Some(Key::Tab.into()),
        Keysym::Delete | Keysym::KP_Delete => Some(Key::Delete.into()),
        Keysym::Left | Keysym::KP_Left => Some(Key::LeftArrow.into()),
        Keysym::Right | Keysym::KP_Right => Some(Key::RightArrow.into()),
        Keysym::Up | Keysym::KP_Up => Some(Key::UpArrow.into()),
        Keysym::Down | Keysym::KP_Down => Some(Key::DownArrow.into()),
        Keysym::Home | Keysym::KP_Home => Some(Key::Home.into()),
        Keysym::End | Keysym::KP_End => Some(Key::End.into()),
        Keysym::Page_Up | Keysym::KP_Page_Up => Some(Key::PageUp.into()),
        Keysym::Page_Down | Keysym::KP_Page_Down => Some(Key::PageDown.into()),
        _ => None,
    };
    if special.is_some() {
        return special;
    }

    if let Some(ref utf8) = event.utf8 {
        if !utf8.is_empty() {
            return Some(SharedString::from(utf8.as_str()));
        }
    }
    event.keysym.key_char().map(|c| SharedString::from(c.to_string().as_str()))
}
