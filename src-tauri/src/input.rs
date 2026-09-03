use std::{
    ffi::c_void,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerActivity {
    pub delta_x: i32,
    pub delta_y: i32,
    pub synthetic: bool,
}

pub struct InputIntentGate {
    accumulated_distance: i32,
    window_started: Option<Instant>,
    last_triggered: Option<Instant>,
}

impl InputIntentGate {
    const ACTIVITY_WINDOW: Duration = Duration::from_millis(90);

    pub fn new() -> Self {
        Self {
            accumulated_distance: 0,
            window_started: None,
            last_triggered: None,
        }
    }

    pub fn observe(
        &mut self,
        activity: PointerActivity,
        threshold: i32,
        debounce: Duration,
        ignore_synthetic: bool,
        now: Instant,
    ) -> bool {
        if ignore_synthetic && activity.synthetic {
            return false;
        }

        if self
            .window_started
            .is_none_or(|started| now.duration_since(started) > Self::ACTIVITY_WINDOW)
        {
            self.window_started = Some(now);
            self.accumulated_distance = 0;
        }
        self.accumulated_distance = self.accumulated_distance.saturating_add(
            activity
                .delta_x
                .abs()
                .saturating_add(activity.delta_y.abs()),
        );

        let outside_debounce = self
            .last_triggered
            .is_none_or(|last| now.duration_since(last) >= debounce);
        if self.accumulated_distance >= threshold.max(1) && outside_debounce {
            self.accumulated_distance = 0;
            self.window_started = Some(now);
            self.last_triggered = Some(now);
            return true;
        }
        false
    }
}

type EventCallback = dyn Fn(PointerActivity) + Send + Sync + 'static;
type StatusCallback = dyn Fn(bool) + Send + Sync + 'static;

struct CallbackContext {
    on_event: Arc<EventCallback>,
    on_status: Arc<StatusCallback>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
unsafe extern "C" {
    fn keyclaim_run_input_monitor(
        callback: unsafe extern "C" fn(i32, i32, i32, *mut c_void),
        context: *mut c_void,
    ) -> i32;
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
unsafe extern "C" fn native_callback(
    delta_x: i32,
    delta_y: i32,
    synthetic: i32,
    context: *mut c_void,
) {
    if context.is_null() {
        return;
    }
    // SAFETY: context 在监听线程整个生命周期内由 Box 持有，原生端只同步回调。
    let callback = unsafe { &*(context as *const CallbackContext) };
    if synthetic == -1 {
        (callback.on_status)(true);
        return;
    }
    (callback.on_event)(PointerActivity {
        delta_x,
        delta_y,
        synthetic: synthetic != 0,
    });
}

pub fn start_monitor(
    on_event: impl Fn(PointerActivity) + Send + Sync + 'static,
    on_status: impl Fn(bool) + Send + Sync + 'static,
) {
    let on_event: Arc<EventCallback> = Arc::new(on_event);
    let on_status: Arc<StatusCallback> = Arc::new(on_status);
    std::thread::spawn(move || {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        loop {
            let context = Box::new(CallbackContext {
                on_event: on_event.clone(),
                on_status: on_status.clone(),
            });
            let context = Box::into_raw(context);
            // SAFETY: context 指向当前监听周期独占的 CallbackContext，函数返回后立即回收。
            let result =
                unsafe { keyclaim_run_input_monitor(native_callback, context.cast::<c_void>()) };
            // SAFETY: 原生监听已经停止，不会再使用该指针。
            drop(unsafe { Box::from_raw(context) });
            on_status(false);
            let _ = result;
            std::thread::sleep(Duration::from_secs(2));
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = on_event;
            on_status(false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_slow_physical_motion_inside_intent_window() {
        let mut gate = InputIntentGate::new();
        let start = Instant::now();
        let activity = PointerActivity {
            delta_x: 1,
            delta_y: 0,
            synthetic: false,
        };
        assert!(!gate.observe(activity, 3, Duration::from_millis(60), false, start));
        assert!(!gate.observe(
            activity,
            3,
            Duration::from_millis(60),
            false,
            start + Duration::from_millis(20)
        ));
        assert!(gate.observe(
            activity,
            3,
            Duration::from_millis(60),
            false,
            start + Duration::from_millis(40)
        ));
    }

    #[test]
    fn ignores_synthetic_motion_when_configured() {
        let mut gate = InputIntentGate::new();
        assert!(!gate.observe(
            PointerActivity {
                delta_x: 10,
                delta_y: 10,
                synthetic: true,
            },
            1,
            Duration::ZERO,
            true,
            Instant::now(),
        ));
    }

    #[test]
    fn debounce_prevents_immediate_second_trigger() {
        let mut gate = InputIntentGate::new();
        let start = Instant::now();
        let activity = PointerActivity {
            delta_x: 5,
            delta_y: 0,
            synthetic: false,
        };
        assert!(gate.observe(activity, 3, Duration::from_millis(60), false, start));
        assert!(!gate.observe(
            activity,
            3,
            Duration::from_millis(60),
            false,
            start + Duration::from_millis(20)
        ));
        assert!(gate.observe(
            activity,
            3,
            Duration::from_millis(60),
            false,
            start + Duration::from_millis(80)
        ));
    }
}
