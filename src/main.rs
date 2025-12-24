#![no_std]
#![no_main]

use badge_display::display_image::DisplayImage;
use badge_display::{CURRENT_IMAGE, DISPLAY_CHANGED, Screen, run_the_display};
use cyw43::{Control, JoinOptions};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use defmt::info;
use defmt::*;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::Input;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::{self, DMA_CH0, I2C0, PIO0, SPI0};
use embassy_rp::pio::Pio;
use embassy_rp::spi::Spi;
use embassy_rp::{bind_interrupts, gpio, i2c, pio, spi};
use embassy_sync::blocking_mutex::raw::{NoopRawMutex, ThreadModeRawMutex};
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use env::env_value;
use gpio::{Level, Output, Pull};
use heapless::Vec;
use http::http_get;
use pcf85063a::PCF85063;
use serde::Deserialize;
use static_cell::StaticCell;
use time::{Date, Month, PrimitiveDateTime, Time};
use {defmt_rtt as _, panic_reset as _};

mod badge_display;
mod env;
mod helpers;
mod http;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

type AsyncI2c0 = I2c<'static, I2C0, i2c::Async>;
type I2c0Bus = Mutex<ThreadModeRawMutex, AsyncI2c0>;
type SharedI2c = I2cDevice<'static, ThreadModeRawMutex, AsyncI2c0>;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    I2C0_IRQ => i2c::InterruptHandler<peripherals::I2C0>;
});

enum Button {
    A,
    B,
    C,
    Up,
    Down,
}
static BUTTON_PRESSED: Signal<ThreadModeRawMutex, &'static Button> = Signal::new();

pub static POWER_MUTEX: Mutex<ThreadModeRawMutex, ()> = Mutex::new(());
pub static RTC_TIME: Mutex<ThreadModeRawMutex, Option<PrimitiveDateTime>> = Mutex::new(None);
pub static WEATHER: Mutex<ThreadModeRawMutex, Option<CurrentWeather>> = Mutex::new(None);

static RTC_DEVICE: StaticCell<Mutex<ThreadModeRawMutex, PCF85063<SharedI2c>>> = StaticCell::new();
static USER_LED: StaticCell<Mutex<ThreadModeRawMutex, Output<'static>>> = StaticCell::new();

static FW: &[u8] = include_bytes!("../cyw43-firmware/43439A0.bin");
static CLM: &[u8] = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut power_latch = Output::new(p.PIN_10, Level::High);
    power_latch.set_high();
    core::mem::forget(power_latch); // Send this to space so it's not dropped and set low

    let user_pin = Output::new(p.PIN_22, Level::High);
    let user_led = USER_LED.init(Mutex::new(user_pin));
    let rtc_device;

    blink(&user_led, 1).await;

    // I2C RTC
    {
        let config = embassy_rp::i2c::Config::default();
        let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_5, p.PIN_4, Irqs, config);
        static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
        let i2c_bus = Mutex::new(i2c);
        let i2c_bus = I2C_BUS.init(i2c_bus);

        let i2c_dev = I2cDevice::new(i2c_bus);
        let rtc = PCF85063::new(i2c_dev);
        rtc_device = RTC_DEVICE.init(Mutex::new(rtc));

        get_time(rtc_device).await;
        spawner.spawn(update_time(rtc_device)).ok();
    }

    Timer::after_millis(100).await;

    // SPI e-ink display
    {
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

        DISPLAY_CHANGED.signal(Screen::Full);
        spawner.must_spawn(run_the_display(spi_bus, cs, dc, busy, reset));
    }

    Timer::after_millis(100).await;

    // Button handlers
    {
        let btn_up = Input::new(p.PIN_15, Pull::Down);
        let btn_down = Input::new(p.PIN_11, Pull::Down);
        let btn_a = Input::new(p.PIN_12, Pull::Down);
        let btn_b = Input::new(p.PIN_13, Pull::Down);
        let btn_c = Input::new(p.PIN_14, Pull::Down);

        spawner.spawn(handle_presses(user_led)).ok();

        spawner.spawn(listen_to_button(btn_a, &Button::A)).ok();
        spawner.spawn(listen_to_button(btn_b, &Button::B)).ok();
        spawner.spawn(listen_to_button(btn_c, &Button::C)).ok();
        spawner.spawn(listen_to_button(btn_up, &Button::Up)).ok();
        spawner
            .spawn(listen_to_button(btn_down, &Button::Down))
            .ok();
    }

    // Screen refresh must complete before we set up wifi
    {
        Timer::after_millis(100).await;
        let _guard = POWER_MUTEX.lock().await;
        blink(user_led, 2).await;
    }

    // Wifi driver and cyw43 setup
    {
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
        let (net_device, mut control, cywrunner) = cyw43::new(state, pwr, spi, FW).await;
        spawner.must_spawn(cyw43_task(cywrunner));

        control.init(CLM).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        // Wifi setup
        let config = embassy_net::Config::dhcpv4(Default::default());

        // Init network stack
        static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
        let (stack, netrunner) = embassy_net::new(
            net_device,
            config,
            RESOURCES.init(StackResources::new()),
            RoscRng.next_u64(),
        );

        spawner.must_spawn(net_task(netrunner));

        spawner
            .spawn(run_network(control, stack, user_led, rtc_device))
            .ok();
    }
}

async fn get_time(rtc_device: &'static Mutex<ThreadModeRawMutex, PCF85063<SharedI2c>>) {
    let _guard = POWER_MUTEX.lock().await;
    let result = rtc_device.lock().await.get_datetime().await.ok();
    let mut data = RTC_TIME.lock().await;
    *data = result;

    Timer::after_millis(50).await;
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
async fn handle_presses(user_led: &'static Mutex<ThreadModeRawMutex, Output<'static>>) -> ! {
    loop {
        let btn = BUTTON_PRESSED.wait().await;

        match btn {
            Button::A => {
                user_led.lock().await.toggle();
            }
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

#[embassy_executor::task]
async fn update_time(rtc_device: &'static Mutex<ThreadModeRawMutex, PCF85063<SharedI2c>>) -> ! {
    loop {
        Timer::after_secs(60).await;
        get_time(&rtc_device).await;
        DISPLAY_CHANGED.signal(Screen::TopBar);
    }
}

#[embassy_executor::task]
async fn run_network(
    mut control: Control<'static>,
    stack: Stack<'static>,
    user_led: &'static Mutex<ThreadModeRawMutex, Output<'static>>,
    rtc_device: &'static Mutex<ThreadModeRawMutex, PCF85063<SharedI2c>>,
) -> ! {
    let mut rx_buffer = [0; 8192];

    loop {
        if let Ok(_) = connect_to_wifi(&mut control, &stack).await {
            blink(user_led, 3).await;

            let (time_buf, weather_buf) = rx_buffer.split_at_mut(4096);

            join(
                fetch_time(&stack, time_buf, rtc_device),
                fetch_weather(&stack, weather_buf),
            )
            .await;

            control.leave().await;

            blink(user_led, 4).await;

            DISPLAY_CHANGED.signal(Screen::TopBar);
        }

        Timer::after_secs(3600).await;
    }
}

async fn connect_to_wifi(control: &mut Control<'_>, stack: &Stack<'_>) -> Result<(), ()> {
    let _guard = POWER_MUTEX.lock().await;

    let mut connected_to_wifi = false;

    let wifi_ssid = env_value("WIFI_SSID");
    let wifi_password = env_value("WIFI_PASSWORD");

    for _ in 0..30 {
        match control
            .join(wifi_ssid, JoinOptions::new(wifi_password.as_bytes()))
            .await
        {
            Ok(_) => {
                connected_to_wifi = true;
                info!("join successful");
                break;
            }
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
        Timer::after(Duration::from_secs(1)).await;
    }

    if !connected_to_wifi {
        return Err(());
    }

    stack.wait_config_up().await;

    Ok(())
}

async fn fetch_time(
    stack: &Stack<'_>,
    rx_buf: &mut [u8],
    rtc_device: &Mutex<ThreadModeRawMutex, PCF85063<SharedI2c>>,
) {
    let url = env_value("TIME_API");
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<TimeApiResponse>(&stack, rx_buf, url).await {
        let datetime: PrimitiveDateTime = response.into();

        rtc_device
            .lock()
            .await
            .set_datetime(&datetime)
            .await
            .expect("TODO: panic message");

        let mut data = RTC_TIME.lock().await;
        *data = Some(datetime);
    }
}

async fn fetch_weather(stack: &Stack<'_>, rx_buf: &mut [u8]) {
    let url = env_value("TEMP_API");
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<OpenMeteoResponse>(&stack, rx_buf, url).await {
        info!(
            "Temp: {}C, Code: {}",
            response.current.temperature, response.current.weathercode
        );

        let weather = CurrentWeather {
            temperature: response.current.temperature,
            weathercode: response.current.weathercode,
            is_day: response.current.is_day,
        };

        let mut data = WEATHER.lock().await;
        *data = Some(weather);
    }
}

async fn fetch_api<'a, T>(stack: &Stack<'_>, rx_buf: &'a mut [u8], url: &str) -> Result<T, ()>
where
    T: Deserialize<'a>,
{
    match http_get(&stack, url, rx_buf).await {
        Ok(bytes) => match serde_json_core::de::from_slice::<T>(bytes) {
            Ok((response, _)) => {
                return Ok(response);
            }
            Err(_e) => {
                error!("Failed to parse response body");
                return Err(());
            }
        },
        Err(e) => {
            error!("Failed to make weather API request: {:?}", e);
            return Err(());
        }
    }
}

async fn blink(pin: &Mutex<ThreadModeRawMutex, Output<'_>>, n_times: usize) {
    for _ in 0..n_times {
        pin.lock().await.set_high();
        Timer::after_millis(100).await;
        pin.lock().await.set_low();
        Timer::after_millis(100).await;
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

#[derive(Deserialize)]
pub struct OpenMeteoResponse {
    pub current: CurrentWeather,
}

#[derive(Deserialize, Copy, Clone)]
pub struct CurrentWeather {
    pub temperature: f32,
    pub weathercode: u8,
    pub is_day: u8,
}
