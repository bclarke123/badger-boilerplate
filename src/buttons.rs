use embassy_rp::gpio::Input;
use embassy_time::Timer;

use crate::{
    FlashDevice, UserLed, flash, image,
    led::blink,
    state::{BUTTON_PRESSED, Button, DISPLAY_CHANGED, Screen, UPDATE_WEATHER},
};

#[embassy_executor::task(pool_size = 5)]
pub async fn listen_to_button(mut button: Input<'static>, btn_type: &'static Button) -> ! {
    loop {
        button.wait_for_high().await;
        Timer::after_millis(50).await;

        if button.is_high() {
            BUTTON_PRESSED.signal(btn_type);
        }

        button.wait_for_low().await;
    }
}

#[embassy_executor::task]
pub async fn handle_presses(user_led: &'static UserLed, flash: &'static FlashDevice) -> ! {
    loop {
        let btn = BUTTON_PRESSED.wait().await;

        match btn {
            Button::A => UPDATE_WEATHER.signal(()),
            Button::B => {
                blink(user_led, 1).await;

                DISPLAY_CHANGED.signal(Screen::Full);
            }
            Button::C => {}
            Button::Down => {
                blink(user_led, 1).await;

                image::next();
                flash::save_state(flash).await;
                DISPLAY_CHANGED.signal(Screen::Image);
            }
            Button::Up => {
                blink(user_led, 1).await;

                image::prev();
                flash::save_state(flash).await;
                DISPLAY_CHANGED.signal(Screen::Image);
            }
        }
    }
}
