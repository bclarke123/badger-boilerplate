pub mod display_image;

use core::sync::atomic::AtomicU8;
use display_image::get_current_image;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as AsyncSpiDevice;
use embassy_futures::select::{Either, select};
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    image::Image,
    mono_font::{MonoTextStyle, ascii::*},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use embedded_hal_async::spi::SpiDevice;
use embedded_text::{
    TextBox,
    alignment::HorizontalAlignment,
    style::{HeightMode, TextBoxStyleBuilder},
};
use gpio::Output;
use heapless::String;
use time::PrimitiveDateTime;
use tinybmp::Bmp;
use uc8151::{LUT, WIDTH, asynch::Uc8151};

use crate::{
    CurrentWeather, POWER_MUTEX, Spi0Bus,
    env::env_value,
    helpers::{easy_format, easy_format_str},
};

pub static CURRENT_IMAGE: AtomicU8 = AtomicU8::new(0);
pub static RTC_TIME: Mutex<ThreadModeRawMutex, Option<PrimitiveDateTime>> = Mutex::new(None);
pub static WEATHER: Mutex<ThreadModeRawMutex, Option<CurrentWeather>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, defmt::Format)]
pub enum Screen {
    Badge,
    WifiList,
}

pub static DISPLAY_CHANGED: Signal<ThreadModeRawMutex, Screen> = Signal::new();

#[embassy_executor::task]
pub async fn run_the_display(
    spi_bus: &'static Spi0Bus,
    cs: Output<'static>,
    dc: Output<'static>,
    busy: Input<'static>,
    reset: Output<'static>,
) {
    let spi_dev = AsyncSpiDevice::new(&spi_bus, cs);
    let mut display = Uc8151::new(spi_dev, dc, busy, reset, Delay);
    let mut current_screen = Screen::Badge;

    loop {
        let result = select(DISPLAY_CHANGED.wait(), Timer::after_secs(60)).await;

        if let Either::First(new_screen) = result {
            current_screen = new_screen;
        }

        draw_current_screen(&mut display, &current_screen).await;
    }
}

async fn draw_current_screen<SPI>(
    display: &mut Uc8151<SPI, Output<'static>, Input<'static>, Output<'static>, Delay>,
    current_screen: &Screen,
) where
    SPI: SpiDevice,
{
    let _guard = POWER_MUTEX.lock().await;
    display.enable();
    display.reset().await;
    display.setup(LUT::Medium).await.ok();

    Timer::after_millis(50).await;

    match current_screen {
        Screen::Badge => {
            draw_badge(display).await;
        }
        Screen::WifiList => {
            draw_wifi(display).await;
        }
    }

    display.disable();

    Timer::after_millis(50).await;
}

async fn draw_badge<SPI>(
    display: &mut Uc8151<SPI, Output<'static>, Input<'static>, Output<'static>, Delay>,
) where
    SPI: SpiDevice,
{
    let mut name_and_details_buffer = [0; 128];
    let name_and_details = easy_format_str(
        format_args!("{}\n{}", env_value("NAME"), env_value("DETAILS")),
        &mut name_and_details_buffer,
    );

    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);
    let textbox_style = TextBoxStyleBuilder::new()
        .height_mode(HeightMode::FitToText)
        .alignment(HorizontalAlignment::Left)
        .paragraph_spacing(6)
        .build();

    let name_and_detail_bounds = Rectangle::new(Point::new(0, 40), Size::new(WIDTH - 75, 0));
    let name_and_detail_box = TextBox::with_textbox_style(
        &name_and_details.unwrap(),
        name_and_detail_bounds,
        character_style,
        textbox_style,
    );

    name_and_detail_box.draw(display).unwrap();

    {
        let data = *WEATHER.lock().await;
        if data.is_some() {
            let data = data.unwrap();
            let top_text: String<64> = easy_format::<64>(format_args!(
                "{} C | {}",
                data.temperature,
                weather_description(data.weathercode)
            ));
            let top_bounds = Rectangle::new(Point::new(0, 0), Size::new(WIDTH, 24));
            top_bounds
                .into_styled(
                    PrimitiveStyleBuilder::default()
                        .stroke_color(BinaryColor::Off)
                        .fill_color(BinaryColor::On)
                        .stroke_width(1)
                        .build(),
                )
                .draw(display)
                .unwrap();

            Text::new(top_text.as_str(), Point::new(8, 16), character_style)
                .draw(display)
                .unwrap();
        }
    }

    // Draw the text box.
    // let result = display.partial_update(top_bounds.try_into().unwrap()).await;
    // match result {
    //     Ok(_) => {}
    //     Err(_) => {
    //         info!("Error updating display");
    //     }
    // }

    let time_box_rectangle_location = Point::new(0, 96);

    //The bounds of the box for time and refresh area
    let time_bounds = Rectangle::new(time_box_rectangle_location, Size::new(88, 24));
    time_bounds
        .into_styled(
            PrimitiveStyleBuilder::default()
                .stroke_color(BinaryColor::Off)
                .fill_color(BinaryColor::On)
                .stroke_width(1)
                .build(),
        )
        .draw(display)
        .unwrap();

    let date = RTC_TIME.lock().await;
    match *date {
        Some(when) => {
            let str = get_display_time(when);

            Text::new(
                str.as_str(),
                (
                    time_box_rectangle_location.x + 8,
                    time_box_rectangle_location.y + 16,
                )
                    .into(),
                character_style,
            )
            .draw(display)
            .unwrap();
        }
        None => {
            Text::new(
                "00:00 AM",
                (
                    time_box_rectangle_location.x + 8,
                    time_box_rectangle_location.y + 16,
                )
                    .into(),
                character_style,
            )
            .draw(display)
            .unwrap();
        }
    };

    //Adding a y offset to the box location to fit inside the box

    // let result = display
    //     .partial_update(time_bounds.try_into().unwrap())
    //     .await;
    // match result {
    //     Ok(_) => {}
    //     Err(_) => {
    //         info!("Error updating display");
    //     }
    // }

    //Manually triggered display events

    let current_image = get_current_image();
    let tga: Bmp<BinaryColor> = Bmp::from_slice(&current_image.image()).unwrap();
    let image = Image::new(&tga, current_image.image_location());
    //clear image location by writing a white rectangle over previous image location
    let clear_rectangle = Rectangle::new(
        current_image.previous().image_location(),
        Size::new(157, 101),
    );
    clear_rectangle
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(display)
        .unwrap();

    image.draw(display).ok();
    display.update().await.ok();
}

async fn draw_wifi<SPI>(
    display: &mut Uc8151<SPI, Output<'static>, Input<'static>, Output<'static>, Delay>,
) where
    SPI: SpiDevice,
{
    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);

    let top_bounds = Rectangle::new(Point::new(0, 0), Size::new(WIDTH, 24));
    top_bounds
        .into_styled(
            PrimitiveStyleBuilder::default()
                .stroke_color(BinaryColor::Off)
                .fill_color(BinaryColor::On)
                .stroke_width(1)
                .build(),
        )
        .draw(display)
        .unwrap();

    Text::new("Wifi found: 0", Point::new(8, 16), character_style)
        .draw(display)
        .unwrap();

    // let result = display.partial_update(top_bounds.try_into().unwrap()).await;
    // match result {
    //     Ok(_) => {}
    //     Err(_) => {
    //         info!("Error updating display");
    //     }
    // }

    //write the wifi list
    // let mut y_offset = 24;
    // let wifi_list = RECENT_WIFI_NETWORKS.lock(|x| x.borrow().clone());
    // for wifi in wifi_list.iter() {
    //     // let wifi_text: String<64> = easy_format::<64>(format_args!("{}", wifi));
    //     let wifi_bounds = Rectangle::new(Point::new(0, y_offset), Size::new(WIDTH, 24));
    //     wifi_bounds
    //         .into_styled(
    //             PrimitiveStyleBuilder::default()
    //                 .stroke_color(BinaryColor::Off)
    //                 .fill_color(BinaryColor::On)
    //                 .stroke_width(1)
    //                 .build(),
    //         )
    //         .draw(display)
    //         .unwrap();

    //     Text::new(wifi.trim(), Point::new(8, y_offset + 16), character_style)
    //         .draw(display)
    //         .unwrap();

    // let result = display
    //     .partial_update(wifi_bounds.try_into().unwrap())
    //     .await;
    // match result {
    //     Ok(_) => {}
    //     Err(_) => {
    //         info!("Error updating display");
    //     }
    // }
    //     y_offset += 24;
    // }

    display.update().await.ok();
}

fn get_display_time(time: PrimitiveDateTime) -> String<8> {
    let mut am = true;
    let twelve_hour = if time.hour() == 0 {
        12
    } else if time.hour() == 12 {
        am = false;
        12
    } else if time.hour() > 12 {
        am = false;
        time.hour() - 12
    } else {
        time.hour()
    };

    let am_pm = if am { "AM" } else { "PM" };

    let formatted_time = easy_format::<8>(format_args!(
        "{:02}:{:02} {}",
        twelve_hour,
        time.minute(),
        am_pm
    ));

    formatted_time
}

fn weather_description(code: u8) -> &'static str {
    match code {
        0 => "Clear Sky",
        1 => "Mainly Clear",
        2 => "Partly Cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing Drizzle",
        61 => "Slight Rain",
        63 => "Rain",
        65 => "Heavy Rain",
        66 | 67 => "Freezing Rain",
        71 => "Slight Snow",
        73 => "Snow",
        75 => "Heavy Snow",
        77 => "Snow Grains",
        80 | 81 | 82 => "Rain Showers",
        85 | 86 => "Snow Showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm & Hail",
        _ => "Unknown",
    }
}
