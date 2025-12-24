use embassy_rp::gpio::Input;
use embassy_time::Timer;

use crate::{
    UserLed,
    badge_display::display_image::DisplayImage,
    helpers::blink,
    state::{BUTTON_PRESSED, Button, CURRENT_IMAGE, DISPLAY_CHANGED, Screen},
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
pub async fn handle_presses(user_led: &'static UserLed) -> ! {
    loop {
        let btn = BUTTON_PRESSED.wait().await;

        blink(user_led, 1).await;

        match btn {
            Button::A => {}
            Button::B => DISPLAY_CHANGED.signal(Screen::Full),
            Button::C => {
                let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
                let new_image = DisplayImage::from_u8(current_image).unwrap().next();
                CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
                DISPLAY_CHANGED.signal(Screen::Image);
            }
            Button::Down => {}
            Button::Up => {}
        }
    }
}
