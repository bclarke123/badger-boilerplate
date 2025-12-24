use core::fmt::Arguments;
use embassy_time::Timer;
use heapless::String;

use crate::UserLed;

/// Makes it easier to format strings in a single line method
pub fn easy_format<const N: usize>(args: Arguments<'_>) -> String<N> {
    let mut formatted_string: String<N> = String::<N>::new();
    let result = core::fmt::write(&mut formatted_string, args);
    match result {
        Ok(_) => formatted_string,
        Err(_) => {
            panic!("Error formatting the string")
        }
    }
}

pub async fn blink(pin: &UserLed, n_times: usize) {
    for i in 0..n_times {
        pin.lock().await.set_high();
        Timer::after_millis(100).await;
        pin.lock().await.set_low();

        if i < n_times - 1 {
            Timer::after_millis(100).await;
        }
    }
}
