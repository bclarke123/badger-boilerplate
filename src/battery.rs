use embassy_rp::{
    adc::{self, Adc, Channel},
    gpio::{Level, Output, Pull},
};
use embassy_time::Timer;

use crate::{Irqs, state::POWER_INFO};

pub enum BatteryState {
    Error,
    UsbPower,
    Battery(u8),
}

fn get_battery_state(voltage: f32) -> BatteryState {
    match voltage {
        x if x > 4.5 => BatteryState::UsbPower,
        x => {
            let max_v = 4.2;
            let min_v = 3.1;

            let actual = x.clamp(min_v, max_v);
            let percentage = (actual - min_v) / (max_v - min_v);

            BatteryState::Battery((percentage * 100.0) as u8)
        }
    }
}

pub async fn get_power_state() {
    let p = unsafe { embassy_rp::Peripherals::steal() };

    let mut wifi_switch = Output::new(p.PIN_25, Level::High);

    let mut adc = Adc::new(p.ADC, Irqs, adc::Config::default());
    let mut channel = Channel::new_pin(p.PIN_29, Pull::None);

    let mut val = 0;
    for _ in 0..10 {
        match adc.read(&mut channel).await {
            Ok(x) => val += x,
            Err(_) => {
                *POWER_INFO.lock().await = Some(BatteryState::Error);
                return;
            }
        }

        Timer::after_millis(1).await;
    }

    val /= 10;

    let ret = get_battery_state(((val as f32) / 4095.0) * 3.3 * 3.0);
    *POWER_INFO.lock().await = Some(ret);

    wifi_switch.set_low();
}
