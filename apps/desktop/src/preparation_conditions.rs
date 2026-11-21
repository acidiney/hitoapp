use std::time::Duration;
use tokio::sync::oneshot;

const PERIOD: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Conditions {
    pub(crate) on_battery: bool,
    pub(crate) metered: bool,
}

impl Conditions {
    fn hold(self) -> bool {
        self.on_battery || self.metered
    }
}

pub(crate) trait Read: Send + 'static {
    fn read(&mut self) -> Conditions;
}

pub(crate) async fn watch<S, H>(mut source: S, hold: H, mut stopped: oneshot::Receiver<()>)
where
    S: Read,
    H: Fn(bool),
{
    let mut last = None;
    loop {
        let reading = tokio::task::spawn_blocking(move || {
            let now = source.read();
            (source, now)
        });
        let (returned, now) = tokio::select! {
            _ = &mut stopped => return,
            reading = reading => match reading {
                Ok(pair) => pair,
                Err(_) => return,
            },
        };
        source = returned;
        if last != Some(now) {
            tracing::info!(
                event = "desktop_preparation_conditions_changed",
                on_battery = now.on_battery,
                metered = now.metered,
                held = now.hold(),
                "Background preparation conditions changed"
            );
            hold(now.hold());
            last = Some(now);
        }
        tokio::select! {
            _ = &mut stopped => return,
            _ = tokio::time::sleep(PERIOD) => {}
        }
    }
}

pub(crate) fn system() -> impl Read {
    platform::System::new()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Conditions, Read};
    use block2::RcBlock;
    use core_foundation::{
        base::{CFType, CFTypeRef, TCFType},
        string::{CFString, CFStringRef},
    };
    use dispatch2::{DispatchQueue, DispatchRetained};
    use std::{
        ffi::c_void,
        ptr::NonNull,
        sync::{Arc, Condvar, Mutex},
        time::Duration,
    };

    const FIRST_PATH: Duration = Duration::from_secs(2);

    type Cost = Arc<(Mutex<Option<bool>>, Condvar)>;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    }

    #[link(name = "Network", kind = "framework")]
    unsafe extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *const DispatchQueue);
        fn nw_path_monitor_set_update_handler(
            monitor: *mut c_void,
            handler: &block2::Block<dyn Fn(*mut c_void)>,
        );
        fn nw_path_monitor_start(monitor: *mut c_void);
        fn nw_path_monitor_cancel(monitor: *mut c_void);
        fn nw_path_is_expensive(path: *mut c_void) -> bool;
        fn nw_path_is_constrained(path: *mut c_void) -> bool;
        fn nw_release(object: *mut c_void);
    }

    struct Monitor(NonNull<c_void>);

    unsafe impl Send for Monitor {}

    impl Drop for Monitor {
        fn drop(&mut self) {
            unsafe {
                nw_path_monitor_cancel(self.0.as_ptr());
                nw_release(self.0.as_ptr());
            }
        }
    }

    pub(crate) struct System {
        metered: Cost,
        _queue: DispatchRetained<DispatchQueue>,
        _monitor: Option<Monitor>,
    }

    impl System {
        pub(crate) fn new() -> Self {
            let metered: Cost = Arc::new((Mutex::new(None), Condvar::new()));
            let queue = DispatchQueue::new("org.hito.desktop.network-cost", None);
            let monitor = NonNull::new(unsafe { nw_path_monitor_create() }).map(Monitor);
            if let Some(monitor) = &monitor {
                let seen = metered.clone();
                let handler = RcBlock::new(move |path: *mut c_void| {
                    let costly =
                        unsafe { nw_path_is_expensive(path) || nw_path_is_constrained(path) };
                    let (cost, changed) = &*seen;
                    if let Ok(mut cost) = cost.lock() {
                        *cost = Some(costly);
                        changed.notify_all();
                    }
                });
                unsafe {
                    nw_path_monitor_set_queue(monitor.0.as_ptr(), &*queue);
                    nw_path_monitor_set_update_handler(monitor.0.as_ptr(), &handler);
                    nw_path_monitor_start(monitor.0.as_ptr());
                }
            }
            Self {
                metered,
                _queue: queue,
                _monitor: monitor,
            }
        }
    }

    impl Read for System {
        fn read(&mut self) -> Conditions {
            Conditions {
                on_battery: on_battery(),
                metered: self.metered(),
            }
        }
    }

    impl System {
        fn metered(&self) -> bool {
            let (cost, changed) = &*self.metered;
            let Ok(cost) = cost.lock() else {
                return false;
            };
            changed
                .wait_timeout_while(cost, FIRST_PATH, |cost| cost.is_none())
                .map(|(cost, _)| cost.unwrap_or(false))
                .unwrap_or(false)
        }
    }

    fn on_battery() -> bool {
        unsafe {
            let info = IOPSCopyPowerSourcesInfo();
            if info.is_null() {
                return false;
            }
            let info = CFType::wrap_under_create_rule(info);
            let kind = IOPSGetProvidingPowerSourceType(info.as_CFTypeRef());
            if kind.is_null() {
                return false;
            }
            let source = CFString::wrap_under_get_rule(kind).to_string();
            source == "Battery Power"
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Conditions, Read};
    use windows::{
        Networking::Connectivity::{NetworkCostType, NetworkInformation},
        Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
    };

    pub(crate) struct System;

    impl System {
        pub(crate) fn new() -> Self {
            Self
        }
    }

    impl Read for System {
        fn read(&mut self) -> Conditions {
            Conditions {
                on_battery: on_battery(),
                metered: metered().unwrap_or(false),
            }
        }
    }

    fn on_battery() -> bool {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == 0
    }

    fn metered() -> windows::core::Result<bool> {
        let cost = NetworkInformation::GetInternetConnectionProfile()?.GetConnectionCost()?;
        let kind = cost.NetworkCostType()?;
        Ok(kind == NetworkCostType::Fixed
            || kind == NetworkCostType::Variable
            || cost.Roaming()?
            || cost.OverDataLimit()?)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{Conditions, Read};
    use dbus::blocking::{Connection, stdintf::org_freedesktop_dbus::Properties};
    use std::time::Duration;

    const CALL: Duration = Duration::from_secs(2);

    pub(crate) struct System {
        bus: Option<Connection>,
    }

    impl System {
        pub(crate) fn new() -> Self {
            Self {
                bus: Connection::new_system().ok(),
            }
        }
    }

    impl Read for System {
        fn read(&mut self) -> Conditions {
            let Some(bus) = &self.bus else {
                return Conditions::default();
            };
            let on_battery = bus
                .with_proxy("org.freedesktop.UPower", "/org/freedesktop/UPower", CALL)
                .get::<bool>("org.freedesktop.UPower", "OnBattery")
                .unwrap_or(false);
            let metered = bus
                .with_proxy(
                    "org.freedesktop.NetworkManager",
                    "/org/freedesktop/NetworkManager",
                    CALL,
                )
                .get::<u32>("org.freedesktop.NetworkManager", "Metered")
                .is_ok_and(|metered| matches!(metered, 1 | 3));
            Conditions {
                on_battery,
                metered,
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::{Conditions, Read};

    pub(crate) struct System;

    impl System {
        pub(crate) fn new() -> Self {
            Self
        }
    }

    impl Read for System {
        fn read(&mut self) -> Conditions {
            Conditions::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    struct Fake(Arc<Mutex<Conditions>>);

    impl Read for Fake {
        fn read(&mut self) -> Conditions {
            *self.0.lock().unwrap()
        }
    }

    #[test]
    #[ignore = "reads this computer's own power source and network cost"]
    fn this_computer_reports_its_power_source_and_network_cost() {
        let now = system().read();
        eprintln!(
            "this_computer on_battery={} metered={}",
            now.on_battery, now.metered
        );
    }

    #[tokio::test(start_paused = true)]
    async fn preparation_waits_on_battery_or_a_metered_network_and_resumes_when_both_clear() {
        let now = Arc::new(Mutex::new(Conditions::default()));
        let (held, mut holds) = mpsc::unbounded_channel();
        let (stop, stopped) = oneshot::channel();
        let watching = tokio::spawn(watch(
            Fake(now.clone()),
            move |hold| {
                let _ = held.send(hold);
            },
            stopped,
        ));
        assert_eq!(holds.recv().await, Some(false));
        now.lock().unwrap().on_battery = true;
        assert_eq!(holds.recv().await, Some(true), "on battery it waits");
        *now.lock().unwrap() = Conditions {
            on_battery: false,
            metered: true,
        };
        assert_eq!(
            holds.recv().await,
            Some(true),
            "plugged in on a metered network it still waits"
        );
        *now.lock().unwrap() = Conditions::default();
        assert_eq!(
            holds.recv().await,
            Some(false),
            "it resumes once both clear"
        );
        let _ = stop.send(());
        watching.await.unwrap();
        assert_eq!(holds.recv().await, None, "it stops with the backend");
    }
}
