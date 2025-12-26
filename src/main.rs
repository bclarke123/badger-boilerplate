#![no_std]
#![no_main]

mod buttons;
mod display;
mod helpers;
mod http;
mod image;
mod led;
mod state;
mod time;
mod wifi;

use crate::buttons::{handle_presses, listen_to_button};
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
use gpio::{Level, Output, Pull};
use pcf85063a::PCF85063;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_reset as _};

type MutexObj<T> = Mutex<ThreadModeRawMutex, T>;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

type AsyncI2c0 = I2c<'static, I2C0, i2c::Async>;
type I2c0Bus = MutexObj<AsyncI2c0>;
type SharedI2c = I2cDevice<'static, ThreadModeRawMutex, AsyncI2c0>;
type RtcDriver = PCF85063<SharedI2c>;

pub type RtcDevice = MutexObj<RtcDriver>;
pub type UserLed = MutexObj<Pwm<'static>>;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<peripherals::PIO0>;
    I2C0_IRQ => i2c::InterruptHandler<peripherals::I2C0>;
});

static RTC_DEVICE: StaticCell<RtcDevice> = StaticCell::new();
static USER_LED: StaticCell<UserLed> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut power_latch = Output::new(p.PIN_10, Level::High);
    power_latch.set_high();
    core::mem::forget(power_latch); // Send this to space so it's not dropped and set low

    let config = Config::default();
    let pwm = Pwm::new_output_a(p.PWM_SLICE3, p.PIN_22, config);

    let user_led = USER_LED.init(Mutex::new(pwm));
    let rtc_device;

    blink(user_led, 1).await;

    // I2C RTC
    {
        let config = embassy_rp::i2c::Config::default();
        let i2c = i2c::I2c::new_async(p.I2C0, p.PIN_5, p.PIN_4, Irqs, config);
        static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
        let i2c_bus = Mutex::new(i2c);
        let i2c_bus = I2C_BUS.init(i2c_bus);

        let i2c_dev = I2cDevice::new(i2c_bus);
        let rtc = RtcDriver::new(i2c_dev);
        rtc_device = RTC_DEVICE.init(Mutex::new(rtc));

        check_trust_time(rtc_device).await;
        get_time(rtc_device).await;

        spawner.spawn(update_time(rtc_device)).ok();
    }

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

        static SPI_BUS: StaticCell<Spi0Bus> = StaticCell::new();
        let spi_bus = SPI_BUS.init(Mutex::new(spi));

        DISPLAY_CHANGED.signal(Screen::Full);
        spawner.must_spawn(display::run(spi_bus, cs, dc, busy, reset));
    }

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
        let _ = join(blink(user_led, 1), POWER_MUTEX.lock()).await;
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
        let (net_device, mut control, cywrunner) = cyw43::new(state, pwr, spi, wifi::FW).await;
        spawner.must_spawn(cyw43_task(cywrunner));

        control.init(wifi::CLM).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let config = embassy_net::Config::dhcpv4(Default::default());
        static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();

        let (stack, netrunner) = embassy_net::new(
            net_device,
            config,
            RESOURCES.init(StackResources::new()),
            RoscRng.next_u64(),
        );

        spawner.must_spawn(net_task(netrunner));

        spawner
            .spawn(wifi::run(control, stack, user_led, rtc_device))
            .ok();
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
