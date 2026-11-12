use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};

use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{EventLoopProxy, Platform, WindowAdapter};

pub type PlatformQueue = Arc<Mutex<VecDeque<Rc<MinimalSoftwareWindow>>>>;

type EventLoopClosure = Box<dyn FnOnce() + Send>;

struct EventLoopState {
    sender: Sender<EventLoopClosure>,
    receiver: Mutex<Receiver<EventLoopClosure>>,
    quit: AtomicBool,
}

static EVENT_LOOP: OnceLock<EventLoopState> = OnceLock::new();

fn event_loop() -> &'static EventLoopState {
    EVENT_LOOP.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        EventLoopState {
            sender,
            receiver: Mutex::new(receiver),
            quit: AtomicBool::new(false),
        }
    })
}

pub fn drain_pending_closures() -> usize {
    let Some(state) = EVENT_LOOP.get() else {
        return 0;
    };
    let Ok(rx) = state.receiver.try_lock() else {
        return 0;
    };
    let mut executed = 0;
    while let Ok(closure) = rx.try_recv() {
        closure();
        executed += 1;
    }
    executed
}

pub fn quit_requested() -> bool {
    EVENT_LOOP
        .get()
        .map(|state| state.quit.load(Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn reset_quit_flag() {
    if let Some(state) = EVENT_LOOP.get() {
        state.quit.store(false, Ordering::Relaxed);
    }
}

struct QueuedPlatform {
    queue: PlatformQueue,
}

impl Platform for QueuedPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let win = self
            .queue
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| slint::PlatformError::Other("no window adapter queued".into()))?;
        Ok(win as Rc<dyn WindowAdapter>)
    }

    fn duration_since_start(&self) -> core::time::Duration {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed()
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        struct Proxy;
        impl EventLoopProxy for Proxy {
            fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
                event_loop().quit.store(true, Ordering::Relaxed);
                Ok(())
            }

            fn invoke_from_event_loop(
                &self,
                event: Box<dyn FnOnce() + Send>,
            ) -> Result<(), slint::EventLoopError> {
                event_loop()
                    .sender
                    .send(event)
                    .map_err(|_| slint::EventLoopError::EventLoopTerminated)
            }
        }
        Some(Box::new(Proxy))
    }
}

pub fn install_platform(queue: PlatformQueue) {
    let _ = slint::platform::set_platform(Box::new(QueuedPlatform { queue }));

    let _ = event_loop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_loop_proxy_roundtrip() {
        let queue: PlatformQueue = Arc::new(Mutex::new(VecDeque::new()));
        install_platform(queue);
        reset_quit_flag();
        let _ = drain_pending_closures();

        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits2 = hits.clone();
        slint::invoke_from_event_loop(move || {
            hits2.fetch_add(1, Ordering::Relaxed);
        })
        .expect("proxy installed by set_platform must accept closures");
        assert_eq!(drain_pending_closures(), 1);
        assert_eq!(hits.load(Ordering::Relaxed), 1);

        slint::quit_event_loop().expect("proxy must accept quit requests");
        assert!(quit_requested());
        reset_quit_flag();
        assert!(!quit_requested());
    }
}
