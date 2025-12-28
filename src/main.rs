#![no_std]
#![no_main]

mod buttons;
mod display;
mod flash;
mod helpers;
mod http;
mod image;
mod led;
mod state;
mod time;
mod wifi;

use crate::buttons::{handle_presses, listen_to_button};
use crate::flash::FlashDriver;
use crate::image::Shift;
use crate::led::blink;
use crate::state::{Button, DISPLAY_CHANGED, POWER_MUTEX, Screen};
use crate::time::{check_trust_time, get_time, update_time};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::StackResources;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::Input;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::{self, DMA_CH0, I2C0, PIO0, SPI0};
use embassy_rp::pio::Pio;
use embassy_rp::pwm::{Config, Pwm};
use embassy_rp::spi::Spi;
use embassy_rp::{bind_interrupts, gpio, i2c, pio, spi};
use embassy_sync::blocking_mutex::raw::{NoopRawMutex, ThreadModeRawMutex};
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use gpio::{Level, Output, Pull};
use pcf85063a::{Control, PCF85063};
use static_cell::StaticCell;

use {defmt_rtt as _, panic_reset as _};

type MutexObj<T> = Mutex<ThreadModeRawMutex, T>;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

type AsyncI2c0 = I2c<'static, I2C0, i2c::Async>;
type I2c0Bus = MutexObj<AsyncI2c0>;
type SharedI2c = I2cDevice<'static, ThreadModeRawMutex, AsyncI2c0>;
type RtcDriver = PCF85063<SharedI2c>;

pub type RtcDevice = MutexObj<RtcDriver>;
static RTC_DEVICE: StaticCell<RtcDevice> = StaticCell::new();

pub type UserLed = MutexObj<Pwm<'static>>;
static USER_LED: StaticCell<UserLed> = StaticCell::new();

pub type FlashDevice = MutexObj<FlashDriver>;
static FLASH_DEVICE: StaticCell<FlashDevice> = StaticCell::new();

static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
static SPI_BUS: StaticCell<Spi0Bus> = StaticCell::new();
static STATE: StaticCell<cyw43::State> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    I2C0_IRQ => i2c::InterruptHandler<peripherals::I2C0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut power_latch = Output::new(p.PIN_10, Level::High);
    power_latch.set_high();

    let rtc_device;
    let flash_device;
    let user_led;

    let mut sync_wifi = false;
    let mut is_rtc_alarm = false;
    let mut external_power = false;
    let mut screen_refresh_type = Screen::None;
    let mut image_dir = Shift::None;

    // Button handlers
    let mut up = Input::new(p.PIN_15, Pull::Down);
    let mut down = Input::new(p.PIN_11, Pull::Down);
    let mut a = Input::new(p.PIN_12, Pull::Down);
    let mut b = Input::new(p.PIN_13, Pull::Down);
    let mut c = Input::new(p.PIN_14, Pull::Down);
    let rtc_alarm = Input::new(p.PIN_8, Pull::Down);

    if up.is_high() {
        // Up
        image_dir = Shift::Prev;
        screen_refresh_type = Screen::Image;
        up.wait_for_low().await;
    } else if down.is_high() {
        // Down
        image_dir = Shift::Next;
        screen_refresh_type = Screen::Image;
        down.wait_for_low().await;
    } else if a.is_high() {
        // A
        sync_wifi = true;
        screen_refresh_type = Screen::TopBar;
        a.wait_for_low().await;
    } else if b.is_high() {
        // B
        screen_refresh_type = Screen::Full;
        b.wait_for_low().await;
    } else if c.is_high() {
        // C
        screen_refresh_type = Screen::TopBar;
        c.wait_for_low().await;
    } else if rtc_alarm.is_high() {
        // RTC wake
        is_rtc_alarm = true;
    } else {
        // We must be on external power
        external_power = true;
    }

    // User LED
    {
        let config = Config::default();
        let pwm = Pwm::new_output_a(p.PWM_SLICE3, p.PIN_22, config);
        user_led = USER_LED.init(Mutex::new(pwm));
    }

    // Load most recent flash data
    {
        let flashdev = FlashDriver::new(p.FLASH, p.DMA_CH3);
        flash_device = FLASH_DEVICE.init(Mutex::new(flashdev));

        join(flash::load_state(flash_device), blink(user_led, 1)).await;
    }

    // I2C RTC
    {
        let config = embassy_rp::i2c::Config::default();
        let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_5, p.PIN_4, Irqs, config);
        let i2c_bus = Mutex::new(i2c);
        let i2c_bus = I2C_BUS.init(i2c_bus);

        let i2c_dev = I2cDevice::new(i2c_bus);
        let rtc = RtcDriver::new(i2c_dev);
        rtc_device = RTC_DEVICE.init(Mutex::new(rtc));

        check_trust_time(rtc_device).await;
        get_time(rtc_device).await;

        let mut rtc = rtc_device.lock().await;

        if is_rtc_alarm {
            let now = rtc.get_datetime().await;

            match now {
                Ok(now) if now.minute() == 0 => {
                    sync_wifi = true;
                    screen_refresh_type = Screen::Full;
                }
                _ => {
                    screen_refresh_type = Screen::TopBar;
                }
            }
        }

        // Pull image index from RTC ram byte, shift if we need, save it
        image::set(rtc.read_ram_byte().await.unwrap_or(0) as usize);
        image::shift(image_dir);
        rtc.write_ram_byte(image::get() as u8).await.ok();
    }

    // Long running tasks if we're on mains power
    if external_power {
        spawner.spawn(handle_presses(user_led, flash_device)).ok();

        spawner.spawn(listen_to_button(a, &Button::A)).ok();
        spawner.spawn(listen_to_button(b, &Button::B)).ok();
        spawner.spawn(listen_to_button(c, &Button::C)).ok();
        spawner.spawn(listen_to_button(up, &Button::Up)).ok();
        spawner.spawn(listen_to_button(down, &Button::Down)).ok();

        spawner.spawn(update_time(rtc_device)).ok();
    }

    // SPI e-ink display
    {
        let miso = p.PIN_16;
        let mosi = p.PIN_19;
        let clk = p.PIN_18;
        let dc = Output::new(p.PIN_20, Level::Low);
        let cs = Output::new(p.PIN_17, Level::High);
        let busy = Input::new(p.PIN_26, Pull::Up);
        let reset = Output::new(p.PIN_21, Level::Low);

        let spi = Spi::new(
            p.SPI0,
            clk,
            mosi,
            miso,
            p.DMA_CH1,
            p.DMA_CH2,
            spi::Config::default(),
        );

        let spi_bus = SPI_BUS.init(Mutex::new(spi));

        // If we're on mains, put something on the display
        if external_power {
            DISPLAY_CHANGED.signal(Screen::Full);
        }

        spawner.must_spawn(display::run(spi_bus, cs, dc, busy, reset));
    }

    // Screen refresh must complete before we set up wifi
    {
        let _ = join(blink(user_led, 1), POWER_MUTEX.lock()).await;
    }

    // Connect to wifi and sync
    if sync_wifi || external_power {
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

        let state = STATE.init(cyw43::State::new());
        let (net_device, mut control, cywrunner) = cyw43::new(state, pwr, spi, wifi::FW).await;
        spawner.must_spawn(cyw43_task(cywrunner));

        control.init(wifi::CLM).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let config = embassy_net::Config::dhcpv4(Default::default());

        let (stack, netrunner) = embassy_net::new(
            net_device,
            config,
            RESOURCES.init(StackResources::new()),
            RoscRng.next_u64(),
        );

        spawner.must_spawn(net_task(netrunner));

        if external_power {
            spawner
                .spawn(wifi::run(
                    control,
                    stack,
                    user_led,
                    rtc_device,
                    flash_device,
                ))
                .ok();
        } else {
            wifi::run_once(control, stack, user_led, rtc_device, flash_device).await;
        }
    }

    if !external_power {
        DISPLAY_CHANGED.signal(screen_refresh_type);
        Timer::after_secs(3).await;
        nighty_night(&mut power_latch, rtc_device).await;
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

async fn nighty_night(power_latch: &mut Output<'static>, rtc_device: &'static RtcDevice) {
    DISPLAY_CHANGED.signal(Screen::Shutdown);

    let mut rtc = rtc_device.lock().await;

    rtc.disable_all_alarms().await.ok();
    rtc.clear_alarm_flag().await.ok();

    if let Ok(now) = rtc.get_datetime().await
        && now.second() == 0
    {
        Timer::after_millis(1000 - now.millisecond() as u64).await
    }

    rtc.set_alarm_seconds(0).await.ok();
    rtc.control_alarm_seconds(Control::On).await.ok();
    rtc.control_alarm_interrupt(Control::On).await.ok();

    Timer::after_secs(1).await;
    power_latch.set_low();
}
