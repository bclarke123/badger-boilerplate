use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex, signal::Signal};
use portable_atomic::AtomicUsize;
use serde::Deserialize;
use time::PrimitiveDateTime;

use crate::MutexObj;

pub static POWER_MUTEX: MutexObj<()> = Mutex::new(());
pub static RTC_TIME: MutexObj<Option<PrimitiveDateTime>> = Mutex::new(None);
pub static WEATHER: MutexObj<Option<CurrentWeather>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, defmt::Format)]
pub enum Screen {
    // Weather,
    #[allow(dead_code)]
    Time,
    TopBar,
    Image,
    Full,
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
pub static UPDATE_WEATHER: Signal<ThreadModeRawMutex, ()> = Signal::new();

#[derive(Deserialize, Copy, Clone)]
pub struct CurrentWeather {
    pub temperature: f32,
    pub weathercode: u8,
    // pub is_day: u8,
}
