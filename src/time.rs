use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;

use crate::{
    RtcDevice,
    state::{DISPLAY_CHANGED, POWER_MUTEX, RTC_TIME, Screen},
};

pub async fn get_time(rtc_device: &'static Mutex<ThreadModeRawMutex, RtcDevice>) {
    let _guard = POWER_MUTEX.lock().await;
    let result = rtc_device.lock().await.get_datetime().await.ok();
    let mut data = RTC_TIME.lock().await;
    *data = result;
}

#[embassy_executor::task]
pub async fn update_time(rtc_device: &'static Mutex<ThreadModeRawMutex, RtcDevice>) -> ! {
    loop {
        let delay = match *RTC_TIME.lock().await {
            Some(time) => 60 - time.second().clamp(0, 50),
            None => 60,
        } as u64;

        Timer::after_secs(delay).await;
        get_time(rtc_device).await;

        DISPLAY_CHANGED.signal(Screen::TopBar);
    }
}
