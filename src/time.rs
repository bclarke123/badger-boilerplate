use core::sync::atomic::Ordering;
use embassy_time::Timer;
use portable_atomic::AtomicBool;

use crate::{
    RtcDevice,
    state::{DISPLAY_CHANGED, RTC_TIME, Screen},
};

pub static TRUST_TIME: AtomicBool = AtomicBool::new(false);

pub async fn get_time(rtc_device: &'static RtcDevice) {
    if !TRUST_TIME.load(Ordering::Relaxed) {
        return;
    }

    let result = rtc_device.lock().await.get_datetime().await.ok();
    let mut data = RTC_TIME.lock().await;
    *data = result;
}

pub async fn check_trust_time(rtc_device: &'static RtcDevice) {
    // Check if the oscillator stopped, if not, we can
    // use the existing time right away
    let osc_did_stop = rtc_device
        .lock()
        .await
        .is_register_bit_flag_high(0x04, 0x80)
        .await
        .unwrap_or(false);

    TRUST_TIME.store(!osc_did_stop, Ordering::Relaxed);
}

#[embassy_executor::task]
pub async fn update_time(rtc_device: &'static RtcDevice) -> ! {
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
