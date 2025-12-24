use core::sync::atomic::AtomicU8;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex, signal::Signal};
use serde::Deserialize;
use time::PrimitiveDateTime;

pub static POWER_MUTEX: Mutex<ThreadModeRawMutex, ()> = Mutex::new(());
pub static RTC_TIME: Mutex<ThreadModeRawMutex, Option<PrimitiveDateTime>> = Mutex::new(None);
pub static WEATHER: Mutex<ThreadModeRawMutex, Option<CurrentWeather>> = Mutex::new(None);

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
pub static CURRENT_IMAGE: AtomicU8 = AtomicU8::new(0);

pub enum Button {
    A,
    B,
    C,
    Up,
    Down,
}
pub static BUTTON_PRESSED: Signal<ThreadModeRawMutex, &'static Button> = Signal::new();

#[derive(Deserialize, Copy, Clone)]
pub struct CurrentWeather {
    pub temperature: f32,
    pub weathercode: u8,
    // pub is_day: u8,
}
