use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint_interpreter::Value;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};

const WIDTH: u32 = 420;
const HEIGHT: u32 = 112;

/// Options for a one-shot layer-shell alert.
pub struct AlertOptions {
    pub title: String,
    pub message: String,
    pub timeout_ms: u64,
    pub accent: Option<slint::Color>,
}

fn top_margin() -> i32 {
    std::env::var("BECA_ALERT_TOP_MARGIN")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|&h| h >= 0)
        .unwrap_or(64)
}

struct AlertInstance {
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    configured: bool,
}

struct AlertState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<Self>,

    window: Rc<MinimalSoftwareWindow>,
    instance: Option<AlertInstance>,
    exit: bool,
}

impl AlertState {
    fn show_surface(&mut self) -> Result<(), String> {
        if self.instance.is_some() {
            return Ok(());
        }

        // Empty input region: the alert is informational and must not steal pointer input.
        let input_region =
            Region::new(&self.compositor).map_err(|e| format!("input region: {e}"))?;
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("beca-alert"),
            None,
        );
        layer.set_anchor(Anchor::TOP);
        layer.set_margin(top_margin(), 0, 0, 0);
        layer.set_size(WIDTH, HEIGHT);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_input_region(Some(input_region.wl_region()));
        layer.commit();

        self.window
            .set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
        let pool = SlotPool::new((WIDTH * HEIGHT * 4) as usize, &self.shm)
            .map_err(|e| format!("shm pool: {e}"))?;

        self.instance = Some(AlertInstance {
            layer,
            pool,
            width: WIDTH,
            height: HEIGHT,
            configured: false,
        });
        Ok(())
    }

    fn instance_for_layer_mut(&mut self, layer: &LayerSurface) -> Option<&mut AlertInstance> {
        self.instance.as_mut().filter(|i| &i.layer == layer)
    }

    fn draw_if_needed(&mut self) {
        let Some(inst) = self.instance.as_mut() else {
            return;
        };
        if !inst.configured {
            return;
        }

        let width = inst.width.max(1);
        let height = inst.height.max(1);
        let stride = width as i32 * 4;
        self.window.draw_if_needed(|renderer| {
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
                    alpha: 0
                };
                (width * height) as usize
            ];
            renderer.render(rgba.as_mut_slice(), width as usize);
            for (dst, src) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(rgba.iter()) {
                dst[0] = src.blue;
                dst[1] = src.green;
                dst[2] = src.red;
                dst[3] = src.alpha;
            }
            inst.layer
                .wl_surface()
                .damage_buffer(0, 0, width as i32, height as i32);
            inst.layer
                .wl_surface()
                .frame(&self.qh, inst.layer.wl_surface().clone());
            let _ = buffer.attach_to(inst.layer.wl_surface());
            inst.layer.commit();
        });
    }
}

/// Shows `AlertWindow` as a click-through Overlay layer surface until its timer fires.
/// Requires a Wayland session with `wlr-layer-shell`.
pub fn run_alert(options: &AlertOptions) -> Result<(), String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err("alert requires a Wayland session with wlr-layer-shell".into());
    }

    let queue = Rc::new(RefCell::new(VecDeque::<Rc<MinimalSoftwareWindow>>::new()));
    crate::platform::install_platform(queue.clone());
    let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    queue.borrow_mut().push_back(msw.clone());

    let app = beca_ui::ui_loader::create("AlertWindow")?;
    let set = |name: &str, value: Value| -> Result<(), String> {
        app.set_property(name, value)
            .map_err(|e| format!("set {name}: {e}"))
    };
    set("title-text", Value::String(options.title.clone().into()))?;
    set(
        "message-text",
        Value::String(options.message.clone().into()),
    )?;
    set("timeout", Value::Number(options.timeout_ms as f64))?;
    if let Some(accent) = options.accent {
        set("accent-color", Value::Brush(accent.into()))?;
    }
    let theme = beca_core::config::load().theme.voice_theme();
    beca_ui::ui_loader::apply_theme_global(&app, &theme);
    app.set_callback("dismiss", |_| {
        let _ = slint::quit_event_loop();
        Value::Void
    })
    .map_err(|e| format!("set dismiss callback: {e}"))?;

    beca_ui::ui_loader::set_custom_event_loop(move || run_alert_surface(msw.clone()));
    beca_ui::ui_loader::show(&app)?;
    beca_ui::ui_loader::run_event_loop()
}

fn run_alert_surface(window: Rc<MinimalSoftwareWindow>) -> Result<(), String> {
    crate::platform::reset_quit_flag();
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|e| format!("wlr-layer-shell unavailable: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;

    let mut state = AlertState {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        qh: qh.clone(),
        window,
        instance: None,
        exit: false,
    };

    loop {
        crate::platform::drain_pending_closures();
        slint::platform::update_timers_and_animations();
        if crate::platform::quit_requested() || state.exit {
            break;
        }
        if state.window.is_visible() && state.instance.is_none() {
            state.show_surface()?;
        }
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

impl CompositorHandler for AlertState {
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
}

impl OutputHandler for AlertState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for AlertState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        let _ = self.window.hide();
        self.instance = None;
        self.exit = true;
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
            self.window
                .set_size(slint::PhysicalSize::new(width, height));
            let needed = (width as usize) * (height as usize) * 4;
            if let Ok(new_pool) = SlotPool::new(needed.max(4096), &self.shm)
                && let Some(inst) = self.instance_for_layer_mut(layer)
            {
                inst.pool = new_pool;
            }
            self.window.request_redraw();
        }
    }
}

impl ShmHandler for AlertState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(AlertState);
delegate_output!(AlertState);
delegate_shm!(AlertState);
delegate_layer!(AlertState);
delegate_registry!(AlertState);

impl ProvidesRegistryState for AlertState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
