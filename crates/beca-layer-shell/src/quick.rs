use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor};
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

const WIDTH: u32 = 480;
const HEIGHT: u32 = 150;

fn extra_top_margin() -> i32 {
    std::env::var("BECA_QUICK_TOP_MARGIN")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|&h| h >= 0)
        .unwrap_or(0)
}

struct QuickInstance {
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    configured: bool,
}

struct QuickState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<Self>,

    window: Rc<MinimalSoftwareWindow>,
    instance: Option<QuickInstance>,
    exit: bool,
}

impl QuickState {
    fn show_surface(&mut self) -> Result<(), String> {
        if self.instance.is_some() {
            return Ok(());
        }

        let input_region =
            Region::new(&self.compositor).map_err(|e| format!("input region: {e}"))?;

        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("beca-quick"),
            None,
        );
        layer.set_anchor(Anchor::TOP);
        layer.set_margin(extra_top_margin(), 0, 0, 0);
        layer.set_size(WIDTH, HEIGHT);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_input_region(Some(input_region.wl_region()));
        layer.commit();

        self.window
            .set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
        let pool = SlotPool::new((WIDTH * HEIGHT * 4) as usize, &self.shm)
            .map_err(|e| format!("shm pool: {e}"))?;

        self.instance = Some(QuickInstance {
            layer,
            pool,
            width: WIDTH,
            height: HEIGHT,
            configured: false,
        });
        Ok(())
    }

    fn hide_surface(&mut self) {
        self.instance = None;
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

    fn instance_for_layer_mut(&mut self, layer: &LayerSurface) -> Option<&mut QuickInstance> {
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
        let _ = rendered;
    }
}

pub fn run_quick_layer_shell(window: Rc<MinimalSoftwareWindow>) -> Result<(), String> {
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

    let mut state = QuickState {
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

impl CompositorHandler for QuickState {
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
    }
}

impl OutputHandler for QuickState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for QuickState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        let _ = self.window.hide();
        self.hide_surface();
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

impl ShmHandler for QuickState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(QuickState);
delegate_output!(QuickState);
delegate_shm!(QuickState);
delegate_layer!(QuickState);
delegate_registry!(QuickState);

impl ProvidesRegistryState for QuickState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
