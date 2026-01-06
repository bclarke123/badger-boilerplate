use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex, signal::Signal};
use heapless::String;
use portable_atomic::AtomicUsize;
use serde::{Deserialize, Serialize};
use time::PrimitiveDateTime;

use crate::{MutexObj, battery::BatteryState};

pub static POWER_MUTEX: MutexObj<()> = Mutex::new(());
pub static POWER_INFO: MutexObj<Option<BatteryState>> = Mutex::new(None);

pub static RTC_TIME: MutexObj<Option<PrimitiveDateTime>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, defmt::Format)]
pub enum Screen {
    None,
    TopBar,
    Image,
    Full,
    Shutdown,
}
pub static DISPLAY_CHANGED: Signal<ThreadModeRawMutex, Screen> = Signal::new();
pub static CURRENT_IMAGE: AtomicUsize = AtomicUsize::new(0);

pub enum Button {
    A,
    B,
    C,
    Up,
    Down,
}
pub static BUTTON_PRESSED: Signal<ThreadModeRawMutex, &'static Button> = Signal::new();

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct CurrentWeather {
    pub temperature: f32,
    pub weathercode: u8,
    pub relative_humidity_2m: f32,
    // pub is_day: u8,
}
pub static WEATHER: MutexObj<Option<CurrentWeather>> = Mutex::new(None);
pub static UPDATE_WEATHER: Signal<ThreadModeRawMutex, ()> = Signal::new();

pub static LABEL: MutexObj<String<64>> = Mutex::new(String::<64>::new());
