#![no_std]
#![no_main]

use crate::http::http_get;
use badge_display::display_image::DisplayImage;
use badge_display::{CURRENT_IMAGE, DISPLAY_CHANGED, RTC_TIME, Screen, run_the_display};
use cyw43::JoinOptions;
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use defmt::info;
use defmt::*;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::Input;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIO0, SPI0};
use embassy_rp::pio::Pio;
use embassy_rp::spi::Spi;
use embassy_rp::spi::{self};
use embassy_rp::{bind_interrupts, gpio, i2c};
use embassy_sync::blocking_mutex::raw::{NoopRawMutex, ThreadModeRawMutex};
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use env::env_value;
use gpio::{Level, Output, Pull};
use heapless::Vec;
use pcf85063a::PCF85063;
use serde::Deserialize;
use static_cell::StaticCell;
use time::{Date, Month, PrimitiveDateTime, Time};
use {defmt_rtt as _, panic_probe as _};

mod badge_display;
mod env;
mod helpers;
mod http;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

type AsyncI2c0 = I2c<'static, I2C0, i2c::Async>;
type I2c0Bus = Mutex<NoopRawMutex, AsyncI2c0>;
type SharedI2c = I2cDevice<'static, NoopRawMutex, AsyncI2c0>;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<embassy_rp::peripherals::PIO0>;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<embassy_rp::peripherals::I2C0>;
});

enum Button {
    A,
    B,
    C,
    Up,
    Down,
}
static BUTTON_PRESSED: Signal<ThreadModeRawMutex, &'static Button> = Signal::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut power_latch = Output::new(p.PIN_10, Level::High);
    power_latch.set_high();

    let mut user_led = Output::new(p.PIN_22, Level::High);

    blink(&mut user_led, 1).await;

    //Wifi driver and cyw43 setup
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    spawner.must_spawn(cyw43_task(runner));

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::Aggressive)
        .await;

    let miso = p.PIN_16;
    let mosi = p.PIN_19;
    let clk = p.PIN_18;
    let dc = p.PIN_20;
    let cs = p.PIN_17;
    let busy = p.PIN_26;
    let reset = Output::new(p.PIN_21, Level::Low);

    let dc = Output::new(dc, Level::Low);
    let cs = Output::new(cs, Level::High);
    let busy = Input::new(busy, Pull::Up);

    let btn_up = Input::new(p.PIN_15, Pull::Down);
    let btn_down = Input::new(p.PIN_11, Pull::Down);
    let btn_a = Input::new(p.PIN_12, Pull::Down);
    let btn_b = Input::new(p.PIN_13, Pull::Down);
    let btn_c = Input::new(p.PIN_14, Pull::Down);

    //Setup i2c bus
    let config = embassy_rp::i2c::Config::default();
    let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_5, p.PIN_4, Irqs, config);
    static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
    let i2c_bus = Mutex::new(i2c);
    let i2c_bus = I2C_BUS.init(i2c_bus);

    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut rtc_device = PCF85063::new(i2c_dev);

    let spi = Spi::new(
        p.SPI0,
        clk,
        mosi,
        miso,
        p.DMA_CH1,
        p.DMA_CH2,
        spi::Config::default(),
    );

    //SPI Bus setup to run the e-ink display
    static SPI_BUS: StaticCell<Spi0Bus> = StaticCell::new();
    let spi_bus = SPI_BUS.init(Mutex::new(spi));

    blink(&mut user_led, 2).await;

    //wifi setup
    let config = embassy_net::Config::dhcpv4(Default::default());

    // Init network stack
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        RoscRng.next_u64(),
    );

    spawner.must_spawn(net_task(runner));

    //Attempt to connect to wifi to get RTC time loop for 2 minutes
    let mut wifi_connection_attempts = 0;
    let mut connected_to_wifi = false;

    let wifi_ssid = env_value("WIFI_SSID");
    let wifi_password = env_value("WIFI_PASSWORD");
    while wifi_connection_attempts < 30 {
        match control
            .join(wifi_ssid, JoinOptions::new(wifi_password.as_bytes()))
            .await
        {
            Ok(_) => {
                connected_to_wifi = true;
                info!("join successful");

                blink(&mut user_led, 3).await;

                break;
            }
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
        Timer::after(Duration::from_secs(1)).await;
        wifi_connection_attempts += 1;
    }

    if connected_to_wifi {
        //Feed the dog if it makes it this far
        info!("waiting for DHCP...");
        while !stack.is_config_up() {
            Timer::after_millis(100).await;
        }
        info!("DHCP is now up!");

        info!("waiting for link up...");
        while !stack.is_link_up() {
            Timer::after_millis(500).await;
        }
        info!("Link is up!");

        info!("waiting for stack to be up...");
        stack.wait_config_up().await;
        info!("Stack is up!");

        //RTC Web request
        let mut rx_buffer = [0; 8192];
        let url = env_value("TIME_API");
        info!("connecting to {}", &url);

        //If the call goes through set the rtc
        match http_get(&stack, url, &mut rx_buffer).await {
            Ok(bytes) => {
                match serde_json_core::de::from_slice::<TimeApiResponse>(bytes) {
                    Ok((output, _used)) => {
                        let datetime: PrimitiveDateTime = output.into();

                        rtc_device
                            .set_datetime(&datetime)
                            .await
                            .expect("TODO: panic message");

                        let mut data = RTC_TIME.lock().await;
                        *data = Some(datetime);

                        blink(&mut user_led, 4).await;
                    }
                    Err(_e) => {
                        error!("Failed to parse response body");
                        // return; // handle the error

                        blink(&mut user_led, 1).await;
                    }
                }
            }
            Err(e) => {
                error!("Failed to make HTTP request: {:?}", e);
                // return; // handle the error
            }
        };

        // control.leave().await;
    }

    Timer::after_millis(100).await;

    DISPLAY_CHANGED.signal(Screen::Badge);
    spawner.must_spawn(run_the_display(spi_bus, cs, dc, busy, reset));

    Timer::after_millis(500).await;

    spawner.spawn(handle_presses(user_led)).ok();

    spawner.spawn(listen_to_button(btn_a, &Button::A)).ok();
    spawner.spawn(listen_to_button(btn_b, &Button::B)).ok();
    spawner.spawn(listen_to_button(btn_c, &Button::C)).ok();
    spawner.spawn(listen_to_button(btn_up, &Button::Up)).ok();
    spawner
        .spawn(listen_to_button(btn_down, &Button::Down))
        .ok();

    spawner.spawn(update_time(rtc_device)).ok();
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task(pool_size = 5)]
async fn listen_to_button(mut button: Input<'static>, btn_type: &'static Button) -> ! {
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
async fn handle_presses(mut user_led: Output<'static>) -> ! {
    loop {
        let btn = BUTTON_PRESSED.wait().await;

        match btn {
            Button::A => {
                user_led.toggle();
            }
            Button::B => {}
            Button::C => {
                let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
                let new_image = DisplayImage::from_u8(current_image).unwrap().next();
                CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
                DISPLAY_CHANGED.signal(Screen::Badge);
            }
            Button::Down => {
                DISPLAY_CHANGED.signal(Screen::WifiList);
            }
            Button::Up => {
                DISPLAY_CHANGED.signal(Screen::Badge);
            }
        }
    }
}

#[embassy_executor::task]
async fn update_time(mut rtc_device: PCF85063<SharedI2c>) -> ! {
    loop {
        Timer::after_secs(60).await;

        let result = rtc_device.get_datetime().await.ok();
        let mut data = RTC_TIME.lock().await;
        *data = result;
    }
}

#[derive(Deserialize)]
struct TimeApiResponse<'a> {
    datetime: &'a str,
}

impl<'a> From<TimeApiResponse<'a>> for PrimitiveDateTime {
    fn from(response: TimeApiResponse) -> Self {
        info!("Datetime: {:?}", response.datetime);
        //split at T
        let datetime = response.datetime.split('T').collect::<Vec<&str, 2>>();
        //split at -
        let date = datetime[0].split('-').collect::<Vec<&str, 3>>();
        let year = date[0].parse::<i32>().unwrap();
        let month = date[1].parse::<u8>().unwrap();
        let day = date[2].parse::<u8>().unwrap();
        //split at :
        let time = datetime[1].split(':').collect::<Vec<&str, 4>>();
        let hour = time[0].parse::<u8>().unwrap();
        let minute = time[1].parse::<u8>().unwrap();
        //split at .
        let second_split = time[2].split('.').collect::<Vec<&str, 2>>();
        let second = second_split[0].parse::<u8>().unwrap();

        let date = Date::from_calendar_date(year, Month::try_from(month).unwrap(), day).unwrap();
        let time = Time::from_hms(hour, minute, second).unwrap();

        PrimitiveDateTime::new(date, time)
    }
}

async fn blink(pin: &mut Output<'_>, n_times: usize) {
    for _ in 0..n_times {
        pin.set_high();
        Timer::after_millis(100).await;
        pin.set_low();
        Timer::after_millis(100).await;
    }
}
