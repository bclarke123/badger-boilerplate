use embassy_rp::pwm::Config;
use embassy_time::{Duration, Timer};

use crate::UserLed;

pub async fn blink(led: &UserLed, n_times: usize) {
    let mut config = Config::default();
    config.compare_a = 0;
    config.compare_b = 0;
    config.top = 25_000;

    let mut locked = led.lock().await;

    for i in 0..n_times {
        config.compare_a = config.top;
        locked.set_config(&config);

        Timer::after_millis(100).await;

        config.compare_a = 0;
        locked.set_config(&config);

        if i < n_times - 1 {
            Timer::after_millis(100).await;
        }
    }
}

pub async fn breathe(led: &UserLed) {
    let mut config = Config::default();

    config.top = 25_000;
    config.compare_b = 0;

    let steps = 100;
    let top = config.top;
    let step = top / steps;
    let delay = Duration::from_millis(10);

    let mut locked = led.lock().await;

    // Fade In
    for i in 0..steps {
        config.compare_a = i * step;
        locked.set_config(&config);
        Timer::after(delay).await;
    }

    // Fade Out
    for i in (0..steps).rev() {
        config.compare_a = i * step;
        locked.set_config(&config);
        Timer::after(delay).await;
    }

    // Ensure Off
    config.compare_a = 0;
    locked.set_config(&config);
}

pub async fn loop_breathe(led: &UserLed) {
    loop {
        breathe(led).await;
    }
}
