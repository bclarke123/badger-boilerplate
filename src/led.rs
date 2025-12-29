use embassy_rp::pwm::Config;
use embassy_time::{Duration, Timer};

use crate::UserLed;

pub async fn blink(led: &UserLed, n_times: usize) {
    for _ in 0..n_times {
        breathe(led, Duration::from_millis(200)).await;
    }
}

pub async fn loop_breathe(led: &UserLed) {
    loop {
        breathe(led, Duration::from_secs(1)).await;
    }
}

pub async fn breathe(led: &UserLed, duration: Duration) {
    let top = 25_000;
    let delay = Duration::from_millis(10);
    let steps = (duration.as_millis() / delay.as_millis()) as u16;

    let mut config = Config::default();
    config.top = top;
    config.compare_b = 0;

    let step = top / steps;

    // Fade In
    for i in 0..steps {
        config.compare_a = i * step;

        {
            led.lock().await.set_config(&config);
        }

        Timer::after(delay).await;
    }

    // Fade Out
    for i in (0..steps).rev() {
        config.compare_a = i * step;

        {
            led.lock().await.set_config(&config);
        }

        Timer::after(delay).await;
    }

    // Ensure Off
    config.compare_a = 0;

    {
        led.lock().await.set_config(&config);
    }
}
