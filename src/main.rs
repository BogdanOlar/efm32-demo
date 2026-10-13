#![no_main]
#![no_std]

mod buttons;
mod display;

use crate::{buttons::ButtonId, display::DisplayFrame};
use cortex_m::peripheral::NVIC;
use defmt::info;
use defmt_rtt as _;
use efm32xg_hal::{
    acmp::{self, Acmp},
    cmu::{Cmu, HfClockPrescaler, HfClockSource, LfClockSource},
    dma::Dma,
    gpio::{dynamic::DynamicPin, efemb::AsyncInputPin, Analog, OutPp, Pin},
    pac::Interrupt,
    peripherals::Usart0,
    prelude::*,
    prs::{self, Prs},
    timer::{self, Timer},
    timer_le::efemb::Ticker,
    usart::spi::{BitOrder, Config, SpiParts},
};
use embassy_executor::{task, Spawner};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Receiver, Sender},
};
use embassy_time::Timer as EmbTimer;
use embedded_hal::spi::MODE_0;
use embedded_hal_async::digital::Wait;
use panic_probe as _;

const MAX_DISPLAY_BUFFER_COUNT: usize = 2;

type DisplayFrameCh = Channel<
    ThreadModeRawMutex,
    DisplayFrame<'static, { display::ls013b7dh03::BUF_SIZE }>,
    MAX_DISPLAY_BUFFER_COUNT,
>;
type DisplayFrameChReceiver = Receiver<
    'static,
    ThreadModeRawMutex,
    DisplayFrame<'static, { display::ls013b7dh03::BUF_SIZE }>,
    MAX_DISPLAY_BUFFER_COUNT,
>;
type DisplayFrameChSender = Sender<
    'static,
    ThreadModeRawMutex,
    DisplayFrame<'static, { display::ls013b7dh03::BUF_SIZE }>,
    MAX_DISPLAY_BUFFER_COUNT,
>;

static TO_SPI: DisplayFrameCh = Channel::new();
static FROM_SPI: DisplayFrameCh = Channel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = efm32xg_hal::init();

    // Initialize clocks
    let _clocks = Cmu::new(p.Cmu)
        .with_hf_clk(HfClockSource::HfXO(40_000_000), HfClockPrescaler::Div1)
        .with_lfa_clk(LfClockSource::LfRco)
        .freeze();

    // Initialize embassy time driver
    Ticker::init();

    let gpio = Gpio::new(p.Gpio);
    let dma = Dma::init(p.Ldma);
    let prs_channels = Prs::new(p.Prs).split();

    // Enable NVIC for GPIO interrupts
    unsafe {
        NVIC::unmask(Interrupt::GPIO_EVEN);
        NVIC::unmask(Interrupt::GPIO_ODD);
        NVIC::unmask(Interrupt::TIMER0);
        NVIC::unmask(Interrupt::ACMP0);
    }

    // Let this App take control of display (this is a `UG154: EFM32 Pearl Gecko Starter Kit` paticularity)
    let _ = gpio.pd15.into_mode::<OutPp>().set_high();

    // Configure capacitive sense pins as analog inputs for ACMP
    let _ = gpio.pb11.into_mode::<Analog>();
    let _ = gpio.pb12.into_mode::<Analog>();

    // ---- Capacitive buttons Task ----
    spawner.spawn(
        capacitive_task(p.Timer0, p.Timer1, p.Acmp0, prs_channels.ch0)
            .expect("Could not spawn Capacitive Task"),
    );

    // Add `DisplayFrame`s to the display task queue
    for f in display::ls013b7dh03::take_display_frames() {
        FROM_SPI.sender().send(f).await;
    }

    // ---- LED 0 and Button 0 Task ----
    spawner.spawn(
        button_led_task(
            ButtonId::Btn0,
            gpio.pf6
                .into_mode::<InFloat>()
                .into_async_input(gpio.exti4ctrl),
            gpio.pf4.into_mode::<OutPp>().into_dynamic_pin(),
        )
        .expect("Could not spawn Task 0"),
    );

    // ---- LED 1 and Button 1 Task ----
    spawner.spawn(
        button_led_task(
            ButtonId::Btn1,
            gpio.pf7
                .into_mode::<InFilt>()
                .into_async_input(gpio.exti5ctrl),
            gpio.pf5.into_mode::<OutPpAlt>().into_dynamic_pin(),
        )
        .expect("Could not spawn Task 1"),
    );

    // ---- LCD SPI Task ----
    spawner.spawn(
        lcd_task(
            TO_SPI.receiver(),
            FROM_SPI.sender(),
            Spi::new(
                SpiParts::new(
                    p.Usart0,
                    gpio.pc8.into_mode::<OutPp>(),
                    gpio.pc6.into_mode::<OutPp>(),
                    gpio.pc7.into_mode::<InFilt>(),
                ),
                &Config::new(MODE_0, 16).with_bit_order(BitOrder::MsbFirst),
                dma.ch1,
                dma.ch0,
            ),
            gpio.pd14.into_mode::<OutPp>(),
            gpio.pd13.into_mode::<OutPp>(),
        )
        .expect("Could not spawn SPI task"),
    );

    // ---- Display Task ----
    spawner.spawn(
        display_task(FROM_SPI.receiver(), TO_SPI.sender()).expect("Could not spawn Display task"),
    );
}

#[task]
async fn capacitive_task(
    p_timer0: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Timer0>,
    p_timer1: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Timer1>,
    p_acmp: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Acmp0>,
    prs_ch0: prs::Channel<0>,
) {
    defmt::info!("Starting Capacitive Touch task...");
    buttons::capacitive::capacitive_task(p_timer0, p_timer1, p_acmp, prs_ch0).await
}

#[task(pool_size = 2)]
async fn button_led_task(btn_id: ButtonId, btn: AsyncInputPin, led: DynamicPin) {
    defmt::info!("Starting Button {} task...", &btn_id);
    buttons::switches::button_led_task(btn_id, btn, led).await;
}

#[task]
async fn lcd_task(
    frames_in: DisplayFrameChReceiver,
    frames_out: DisplayFrameChSender,
    spi: Spi<'static, Usart0>,
    cs: Pin<'D', 14, OutPp>,
    disp_com_inv: Pin<'D', 13, OutPp>,
) {
    defmt::info!("Starting SPI task...");
    display::ls013b7dh03::lcd_task(frames_in, frames_out, spi, cs, disp_com_inv).await;
}

#[task]
async fn display_task(frames_in: DisplayFrameChReceiver, frames_out: DisplayFrameChSender) {
    defmt::info!("Starting Display task...");
    display::display_task(frames_in, frames_out).await;
}
