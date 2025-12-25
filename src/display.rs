use crate::image;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as AsyncSpiDevice;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
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
use gpio::Output;
use heapless::String;
use time::PrimitiveDateTime;
use tinybmp::Bmp;
use uc8151::{HEIGHT, LUT, WIDTH, asynch::Uc8151};

use crate::{
    Spi0Bus,
    helpers::easy_format,
    state::{DISPLAY_CHANGED, POWER_MUTEX, RTC_TIME, Screen, WEATHER},
};

type Display<SPI> = Uc8151<SPI, Output<'static>, Input<'static>, Output<'static>, Delay>;

#[embassy_executor::task]
pub async fn run(
    spi_bus: &'static Spi0Bus,
    cs: Output<'static>,
    dc: Output<'static>,
    busy: Input<'static>,
    reset: Output<'static>,
) {
    let spi_dev = AsyncSpiDevice::new(spi_bus, cs);
    let mut display = Display::new(spi_dev, dc, busy, reset, Delay);

    loop {
        let to_update = DISPLAY_CHANGED.wait().await;
        update_screen(&mut display, &to_update).await;
    }
}

async fn update_screen<SPI: SpiDevice>(display: &mut Display<SPI>, to_update: &Screen) {
    let _guard = POWER_MUTEX.lock().await;
    display.enable();
    display.reset().await;

    match to_update {
        Screen::Full => {
            display.setup(LUT::Medium).await.ok();
        }
        _ => {
            display.setup(LUT::Fast).await.ok();
        }
    }

    Timer::after_millis(50).await;

    match to_update {
        Screen::Full => {
            draw_badge(display).await;
        }
        Screen::TopBar => {
            draw_top_bar(display, true).await;
        }
        // Screen::Weather => {
        //     draw_weather(display, true).await;
        // }
        Screen::Time => {
            draw_time(display, true).await;
        }
        Screen::Image => {
            draw_current_image(display, true).await;
        }
    }

    display.disable();

    Timer::after_millis(50).await;
}

async fn draw_weather<SPI: SpiDevice>(display: &mut Display<SPI>, partial: bool) {
    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);

    {
        let data = *WEATHER.lock().await;
        if let Some(data) = data {
            let top_text: String<64> = easy_format::<64>(format_args!(
                "{}C | {}",
                data.temperature,
                weather_description(data.weathercode)
            ));

            let text = Text::new(top_text.as_str(), Point::new(8, 16), character_style);
            let rect = text.bounding_box();

            text.draw(display).unwrap();

            if partial {
                display.partial_update(rect.try_into().unwrap()).await.ok();
            }
        }
    }
}

async fn draw_time<SPI: SpiDevice>(display: &mut Display<SPI>, partial: bool) {
    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);

    {
        let date = RTC_TIME.lock().await;
        if let Some(when) = *date {
            let str = get_display_time(when);

            let text = Text::new(
                str.as_str(),
                Point::new((WIDTH - 98) as i32, 16),
                character_style,
            );

            if partial {
                Rectangle::new(Point::new(192, 1), Size::new(88, 22))
                    .into_styled(
                        PrimitiveStyleBuilder::default()
                            .stroke_color(BinaryColor::On)
                            .fill_color(BinaryColor::On)
                            .build(),
                    )
                    .draw(display)
                    .ok();
            }

            text.draw(display).unwrap();

            if partial {
                let bounds = Rectangle::new(Point::new(192, 0), Size::new(104, 24));
                display
                    .partial_update(bounds.try_into().unwrap())
                    .await
                    .ok();
            }
        };
    }
}

async fn draw_top_bar<SPI: SpiDevice>(display: &mut Display<SPI>, partial: bool) {
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

    draw_weather(display, false).await;
    draw_time(display, false).await;

    if partial {
        display
            .partial_update(top_bounds.try_into().unwrap())
            .await
            .ok();
    }
}

async fn draw_current_image<SPI: SpiDevice>(display: &mut Display<SPI>, partial: bool) {
    let current_image = image::get_image();
    let position = image::get_position();

    let tga: Bmp<BinaryColor> = Bmp::from_slice(current_image).unwrap();
    let image = Image::new(&tga, position.into());

    // clear image location by writing a white rectangle over previous image location
    let clear_rectangle = Rectangle::new(Point::new(0, 24), Size::new(WIDTH, HEIGHT - 24));
    clear_rectangle
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(display)
        .unwrap();

    image.draw(display).ok();

    if partial {
        display
            .partial_update(clear_rectangle.try_into().unwrap())
            .await
            .ok();
    }
}

async fn draw_badge<SPI: SpiDevice>(display: &mut Display<SPI>) {
    draw_top_bar(display, false).await;
    draw_current_image(display, false).await;

    display.update().await.ok();
}

fn get_display_time(time: PrimitiveDateTime) -> String<10> {
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

    easy_format::<10>(format_args!(
        "| {:02}:{:02} {}",
        twelve_hour,
        time.minute(),
        am_pm
    ))
}

fn weather_description(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly Clear",
        2 => "Part Cloudy",
        3 => "Cloudy",
        45..=48 => "Fog",
        51..=55 => "Drizzle",
        56 | 57 => "Frizzle",
        61 => "Light Rain",
        63 => "Rain",
        65 => "Heavy Rain",
        66 | 67 => "Frzing Rain",
        71 => "Light Snow",
        73 => "Snow",
        75 => "Heavy Snow",
        77 => "Snow Grains",
        80..=82 => "Rain Showers",
        85 | 86 => "Snow Showers",
        95 => "Thunderstorm",
        96 | 99 => "Hailstorm",
        _ => "Unknown",
    }
}
