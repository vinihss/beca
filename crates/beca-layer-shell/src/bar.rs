use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use beca_core::BarProperties;
use beca_core::config::{BarModuleConfig, WidgetsConfig};
use beca_core::theme::Theme as BecaTheme;
use beca_ui::{BarWindow, Theme};
use beca_widgets::{WidgetAction, WidgetController, WidgetId, WidgetLayout, WidgetSink};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Global, LogicalPosition, Timer, TimerMode};

use crate::platform::{PlatformQueue, install_platform};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
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
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPosition {
    Top,
    Bottom,
}

pub fn run_bar(
    position: BarPosition,
    bar_cfg: &BarModuleConfig,
    widgets: &WidgetsConfig,
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

    let props = BarProperties::from_config(bar_cfg);
    let layout = WidgetLayout::from_lookup(|name| props.section(name));
    let controller = WidgetController::new(layout, widgets.clock.format.clone());

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
        widgets: widgets.clone(),
        theme: theme.clone(),
        position,
        exit: false,
        controller,
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
        event_queue.flush().map_err(|e| format!("flush: {e}"))?;
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
    _timers: Vec<Timer>,
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
    widgets: WidgetsConfig,
    theme: BecaTheme,
    position: BarPosition,
    exit: bool,
    controller: WidgetController,

    pointer: Option<wl_pointer::WlPointer>,
}

impl BarState {
    fn create_instance(
        &mut self,
        output: Option<&wl_output::WlOutput>,
        qh: &QueueHandle<Self>,
    ) -> Result<(), String> {
        let msw = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        self.queue.lock().unwrap().push_back(msw.clone());

        let bar = BarWindow::new().map_err(|e| format!("BarWindow: {e}"))?;
        apply_theme(&bar, &self.theme);
        configure_bar(
            &bar,
            &self.bar_cfg,
            &self.widgets,
            self.position,
            &self.controller,
        );
        setup_callbacks(&bar, self.controller.clone());
        let _ = bar.show();
        let _timers = setup_timers(&bar, self.controller.clone());

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
        layer.set_exclusive_zone(if self.exclusive {
            self.height as i32
        } else {
            0
        });
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
    fn instance_for_surface_mut(
        &mut self,
        surface: &wl_surface::WlSurface,
    ) -> Option<&mut BarInstance> {
        self.instances
            .iter_mut()
            .find(|i| i.layer.wl_surface() == surface)
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
        for (dst, src) in canvas.chunks_exact_mut(4).zip(rgba.iter()) {
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
            .frame(qh, inst.layer.wl_surface().clone());
        let _ = buffer.attach_to(inst.layer.wl_surface());
        inst.layer.commit();
        did = true;
    });
    rendered && did
}

impl CompositorHandler for BarState {
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
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.maybe_render_all(qh);
    }
}

impl OutputHandler for BarState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for BarState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.instances.iter().any(|i| &i.layer == layer) {
            self.exit = true;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = (configure.new_size.0, configure.new_size.1);
        if let Some(inst) = self.instance_for_layer_mut(layer) {
            if w > 0 {
                inst.width = w;
            }
            if h > 0 {
                inst.height = h;
            }
            inst.window
                .set_size(slint::PhysicalSize::new(inst.width, inst.height));
            inst.configured = true;
            inst.window.request_redraw();
        }
        if let Some(idx) = self.instances.iter().position(|i| &i.layer == layer) {
            let needed =
                (self.instances[idx].width as usize) * (self.instances[idx].height as usize) * 4;
            if let Ok(new_pool) = SlotPool::new(needed.max(4096), &self.shm) {
                self.instances[idx].pool = new_pool;
            }
        }
        self.maybe_render_all(qh);
    }
}

impl SeatHandler for BarState {
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
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("failed to create wl_pointer");
            self.pointer = Some(pointer);
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
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for BarState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            if let Some(inst) = self.instance_for_surface_mut(&event.surface) {
                let pos = LogicalPosition::new(event.position.0 as f32, event.position.1 as f32);
                match event.kind {
                    Enter { .. } | Motion { .. } => {
                        inst.window
                            .dispatch_event(WindowEvent::PointerMoved { position: pos });
                    }
                    Leave { .. } => {
                        inst.window.dispatch_event(WindowEvent::PointerExited);
                    }
                    Press { button, .. } => {
                        if let Some(btn) = map_button(button) {
                            inst.window.dispatch_event(WindowEvent::PointerPressed {
                                position: pos,
                                button: btn,
                            });
                        }
                    }
                    Release { button, .. } => {
                        if let Some(btn) = map_button(button) {
                            inst.window.dispatch_event(WindowEvent::PointerReleased {
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
                            inst.window.dispatch_event(WindowEvent::PointerScrolled {
                                position: pos,
                                delta_x: dx,
                                delta_y: dy,
                            });
                        }
                    }
                }
                inst.window.request_redraw();
            }
        }
    }
}

impl ShmHandler for BarState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(BarState);
delegate_output!(BarState);
delegate_shm!(BarState);
delegate_seat!(BarState);
delegate_pointer!(BarState);
delegate_layer!(BarState);
delegate_registry!(BarState);

impl ProvidesRegistryState for BarState {
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

fn apply_theme(bar: &BarWindow, theme: &BecaTheme) {
    let tokens = beca_ui::theme::ThemeTokens::from_voice_theme(&theme.voice_theme());
    tokens.apply_typed(&Theme::get(bar));
}

struct TypedSink<'a> {
    bar: &'a BarWindow,
}

impl WidgetSink for TypedSink<'_> {
    fn set_visible(&self, id: WidgetId, visible: bool) {
        match id {
            WidgetId::Launcher => self.bar.set_show_launcher(visible),
            WidgetId::Clock => self.bar.set_show_clock(visible),
            WidgetId::Conversation => self.bar.set_show_conversation(visible),
            WidgetId::VolumeOut => self.bar.set_show_volume_out(visible),
            WidgetId::VolumeIn => self.bar.set_show_volume_in(visible),
            WidgetId::Network => self.bar.set_show_network(visible),
            WidgetId::Alerts => self.bar.set_show_alerts(visible),
            WidgetId::Bluetooth => self.bar.set_show_bluetooth(visible),
            WidgetId::Cpu => self.bar.set_show_cpu(visible),
            WidgetId::Memory => self.bar.set_show_memory(visible),
            WidgetId::Disk => self.bar.set_show_disk(visible),
        }
    }

    fn set_section(&self, id: WidgetId, section: &'static str) {
        match id {
            WidgetId::Launcher => self.bar.set_launcher_section(section.into()),
            WidgetId::Clock => self.bar.set_clock_section(section.into()),
            WidgetId::Conversation => self.bar.set_conversation_section(section.into()),
            WidgetId::VolumeOut => self.bar.set_volume_out_section(section.into()),
            WidgetId::VolumeIn => self.bar.set_volume_in_section(section.into()),
            WidgetId::Network => self.bar.set_network_section(section.into()),
            WidgetId::Alerts => self.bar.set_alerts_section(section.into()),
            WidgetId::Bluetooth => self.bar.set_bluetooth_section(section.into()),
            WidgetId::Cpu => self.bar.set_cpu_section(section.into()),
            WidgetId::Memory => self.bar.set_memory_section(section.into()),
            WidgetId::Disk => self.bar.set_disk_section(section.into()),
        }
    }

    fn set_text(&self, id: WidgetId, text: &str) {
        match id {
            WidgetId::Clock => self.bar.set_clock_text(text.into()),
            WidgetId::VolumeOut => self.bar.set_volume_out_text(text.into()),
            WidgetId::VolumeIn => self.bar.set_volume_in_text(text.into()),
            WidgetId::Network => self.bar.set_network_text(text.into()),
            WidgetId::Alerts => self.bar.set_alerts_text(text.into()),
            WidgetId::Bluetooth => self.bar.set_bluetooth_text(text.into()),
            WidgetId::Cpu => self.bar.set_cpu_text(text.into()),
            WidgetId::Memory => self.bar.set_memory_text(text.into()),
            WidgetId::Disk => self.bar.set_disk_text(text.into()),
            WidgetId::Launcher | WidgetId::Conversation => {}
        }
    }

    fn set_muted(&self, id: WidgetId, muted: bool) {
        match id {
            WidgetId::VolumeOut => self.bar.set_volume_out_muted(muted),
            WidgetId::VolumeIn => self.bar.set_volume_in_muted(muted),
            _ => {}
        }
    }

    fn set_active(&self, id: WidgetId, active: bool) {
        if id == WidgetId::Conversation {
            self.bar.set_conversation_active(active);
        }
    }
}

fn configure_bar(
    bar: &BarWindow,
    cfg: &BarModuleConfig,
    widgets: &WidgetsConfig,
    position: BarPosition,
    controller: &WidgetController,
) {
    bar.set_position(match position {
        BarPosition::Top => "top".into(),
        BarPosition::Bottom => "bottom".into(),
    });

    let sink = TypedSink { bar };
    controller.apply_layout(&sink);
    controller.sample_all(&sink);

    bar.set_launcher_label(widgets.launcher.label.clone().into());
    bar.set_begin_align(cfg.begin_align.clone().into());
    bar.set_center_align(cfg.center_align.clone().into());
    bar.set_end_align(cfg.end_align.clone().into());
    bar.set_background_opacity(cfg.background_opacity);
    bar.set_show_border(cfg.show_border);
    if let Some(bg) = cfg
        .background
        .as_ref()
        .and_then(|s| beca_ui::theme::parse_color(s))
    {
        bar.set_bar_background(slint::Brush::SolidColor(bg));
    }
}

fn apply_action(weak: slint::Weak<BarWindow>, controller: &WidgetController, action: WidgetAction) {
    if let Some(bar) = weak.upgrade() {
        let sink = TypedSink { bar: &bar };
        if let Some(update) = controller.act(action) {
            update.apply(&sink);
        }
    }
}

fn setup_callbacks(bar: &BarWindow, controller: WidgetController) {
    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_launcher_clicked(move || {
            apply_action(weak.clone(), &controller, WidgetAction::Launcher);
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_alerts_clicked(move || {
            apply_action(weak.clone(), &controller, WidgetAction::Alerts);
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_conversation_clicked(move || {
            apply_action(weak.clone(), &controller, WidgetAction::ConversationToggle);
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_volume_out_scrolled(move |delta| {
            if delta == 0.0 {
                return;
            }
            apply_action(
                weak.clone(),
                &controller,
                WidgetAction::VolumeScroll {
                    id: WidgetId::VolumeOut,
                    up: delta > 0.0,
                },
            );
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_volume_in_scrolled(move |delta| {
            if delta == 0.0 {
                return;
            }
            apply_action(
                weak.clone(),
                &controller,
                WidgetAction::VolumeScroll {
                    id: WidgetId::VolumeIn,
                    up: delta > 0.0,
                },
            );
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_volume_out_clicked(move || {
            apply_action(
                weak.clone(),
                &controller,
                WidgetAction::VolumeMute {
                    id: WidgetId::VolumeOut,
                },
            );
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_volume_in_clicked(move || {
            apply_action(
                weak.clone(),
                &controller,
                WidgetAction::VolumeMute {
                    id: WidgetId::VolumeIn,
                },
            );
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_network_clicked(move || {
            apply_action(weak.clone(), &controller, WidgetAction::Network);
        });
    }

    {
        let weak = bar.as_weak();
        let controller = controller.clone();
        bar.on_bluetooth_clicked(move || {
            apply_action(weak.clone(), &controller, WidgetAction::Bluetooth);
        });
    }
}

fn setup_timers(bar: &BarWindow, controller: WidgetController) -> Timers {
    let mut timers = Vec::new();
    for (interval, group) in controller.timer_specs() {
        let weak = bar.as_weak();
        let controller = controller.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, interval, move || {
            let Some(bar) = weak.upgrade() else {
                return;
            };
            let Some(update) = controller.sample(group) else {
                return;
            };
            let sink = TypedSink { bar: &bar };
            update.apply(&sink);
        });
        timers.push(timer);
    }
    Timers { _timers: timers }
}
