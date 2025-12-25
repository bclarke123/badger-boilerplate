use cyw43::{Control, JoinOptions};
use embassy_futures::{join::join, select::select};
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use log::info;

use crate::{
    RtcDevice, UserLed,
    helpers::blink,
    http::{fetch_time, fetch_weather},
    state::{DISPLAY_CHANGED, POWER_MUTEX, Screen, UPDATE_WEATHER},
};

pub static FW: &[u8] = include_bytes!("../cyw43-firmware/43439A0.bin");
pub static CLM: &[u8] = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

static WIFI_SSID: &str = env!("WIFI_SSID");
static WIFI_PASSWORD: &[u8] = include_bytes!("../.wifi");

async fn connect(control: &mut Control<'_>, stack: &Stack<'_>) -> Result<(), ()> {
    let _guard = POWER_MUTEX.lock().await;

    let mut connected_to_wifi = false;

    for _ in 0..30 {
        match control
            .join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD))
            .await
        {
            Ok(_) => {
                connected_to_wifi = true;
                info!("join successful");
                break;
            }
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
        Timer::after(Duration::from_secs(1)).await;
    }

    if !connected_to_wifi {
        return Err(());
    }

    stack.wait_config_up().await;

    Ok(())
}

#[embassy_executor::task]
pub async fn run(
    mut control: Control<'static>,
    stack: Stack<'static>,
    user_led: &'static UserLed,
    rtc_device: &'static RtcDevice,
) -> ! {
    let mut rx_buffer = [0; 8192];

    loop {
        if connect(&mut control, &stack).await.is_ok() {
            blink(user_led, 3).await;

            let (time_buf, weather_buf) = rx_buffer.split_at_mut(4096);

            join(
                fetch_time(&stack, time_buf, rtc_device),
                fetch_weather(&stack, weather_buf),
            )
            .await;

            control.leave().await;

            blink(user_led, 4).await;

            DISPLAY_CHANGED.signal(Screen::TopBar);
        }

        select(Timer::after_secs(3600), UPDATE_WEATHER.wait()).await;
    }
}
