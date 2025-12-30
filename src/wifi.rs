use cyw43::{Control, JoinOptions};
use embassy_futures::{join::join, select::select};
use embassy_net::Stack;
use embassy_time::{Duration, Timer, with_timeout};
use log::info;

use crate::{
    FlashDevice, RtcDevice, UserLed,
    http::{fetch_time, fetch_weather},
    led,
    state::{DISPLAY_CHANGED, POWER_MUTEX, Screen, UPDATE_WEATHER},
};

pub static FW: &[u8] = include_bytes!("../cyw43-firmware/43439A0.bin");
pub static CLM: &[u8] = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

static WIFI_SSID: &str = env!("WIFI_SSID");
static WIFI_PASSWORD: &[u8] = include_bytes!("../.wifi");

async fn connect(control: &mut Control<'_>, stack: &Stack<'_>) -> Result<(), ()> {
    let _guard = POWER_MUTEX.lock().await;

    let mut connected_to_wifi = false;

    match with_timeout(
        Duration::from_secs(20),
        control.join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD)),
    )
    .await
    {
        Ok(Ok(_)) => {
            connected_to_wifi = true;
            info!("join successful");
        }
        Ok(Err(err)) => {
            info!("join failed with status={}", err.status);
        }
        Err(_) => {
            info!("join timed out");
        }
    }

    if !connected_to_wifi {
        return Err(());
    }

    stack.wait_config_up().await;

    Ok(())
}

async fn sync(
    rx_buffer: &mut [u8],
    control: &mut Control<'static>,
    stack: Stack<'static>,
    rtc_device: &'static RtcDevice,
    flash_driver: &'static FlashDevice,
) {
    if connect(control, &stack).await.is_ok() {
        let (time_buf, weather_buf) = rx_buffer.split_at_mut(4096);

        join(
            fetch_time(&stack, time_buf, rtc_device),
            fetch_weather(&stack, weather_buf, flash_driver),
        )
        .await;

        control.leave().await;
    }
}

async fn blink_sync(
    rx_buffer: &mut [u8],
    control: &mut Control<'static>,
    stack: Stack<'static>,
    user_led: &'static UserLed,
    rtc_device: &'static RtcDevice,
    flash_driver: &'static FlashDevice,
) {
    select(
        led::loop_breathe(user_led),
        with_timeout(
            Duration::from_secs(30),
            sync(rx_buffer, control, stack, rtc_device, flash_driver),
        ),
    )
    .await;
}

#[embassy_executor::task]
pub async fn run(
    mut control: Control<'static>,
    stack: Stack<'static>,
    user_led: &'static UserLed,
    rtc_device: &'static RtcDevice,
    flash_driver: &'static FlashDevice,
) -> ! {
    let mut rx_buffer = [0; 8192];

    loop {
        blink_sync(
            &mut rx_buffer,
            &mut control,
            stack,
            user_led,
            rtc_device,
            flash_driver,
        )
        .await;

        DISPLAY_CHANGED.signal(Screen::TopBar);
        led::blink(user_led, 2).await;

        select(Timer::after_secs(3600), UPDATE_WEATHER.wait()).await;
    }
}

pub async fn run_once(
    mut control: Control<'static>,
    stack: Stack<'static>,
    user_led: &'static UserLed,
    rtc_device: &'static RtcDevice,
    flash_driver: &'static FlashDevice,
) {
    let mut rx_buffer = [0; 8192];

    blink_sync(
        &mut rx_buffer,
        &mut control,
        stack,
        user_led,
        rtc_device,
        flash_driver,
    )
    .await;

    led::blink(user_led, 2).await;
}
