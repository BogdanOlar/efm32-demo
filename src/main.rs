#![no_main]
#![no_std]

mod buttons;
mod display;

use cortex_m::peripheral::NVIC;
use defmt_rtt as _;
use efm32_hal::{
    cmu::{Cmu, HfClockPrescaler, HfClockSource, LfClockSource},
    dma::Dma,
    gpio::{Analog, OutPp},
    pac::Interrupt,
    prelude::*,
    prs::Prs,
    timer_le::efemb::Ticker,
    usart::spi::{BitOrder, Config, SpiParts},
};
use embassy_executor::Spawner;
use embedded_hal::spi::MODE_0;
use panic_probe as _;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = efm32_hal::init();

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
    }

    // Let this App take control of display (this is a `UG154: EFM32 Pearl Gecko Starter Kit` paticularity)
    let _ = gpio.pd15.into_mode::<OutPp>().set_high();

    // Configure capacitive sense pins as analog inputs for ACMP
    let _ = gpio.pb11.into_mode::<Analog>();
    let _ = gpio.pb12.into_mode::<Analog>();

    // ---- Buttons Task ----
    spawner.spawn(buttons::task().expect("Could not spawn buttons::task"));

    // ---- Capacitive buttons Task ----
    spawner.spawn(
        buttons::capacitive::task(p.Timer0, p.Timer1, p.Acmp0, prs_channels.ch0)
            .expect("Could not spawn Capacitive Task"),
    );

    // ---- LED 0 and Button 0 Task ----
    spawner.spawn(
        buttons::switches::task(
            buttons::ButtonId::Btn0,
            gpio.pf6
                .into_mode::<InFloat>()
                .into_async_input(gpio.exti4ctrl),
            gpio.pf4.into_mode::<OutPp>().into_dynamic_pin(),
        )
        .expect("Could not spawn buttons::switches::task 0"),
    );

    // ---- LED 1 and Button 1 Task ----
    spawner.spawn(
        buttons::switches::task(
            buttons::ButtonId::Btn1,
            gpio.pf7
                .into_mode::<InFilt>()
                .into_async_input(gpio.exti5ctrl),
            gpio.pf5.into_mode::<OutPpAlt>().into_dynamic_pin(),
        )
        .expect("Could not spawn buttons::switches::task 1"),
    );

    // ---- LCD SPI Task ----
    spawner.spawn(
        display::ls013b7dh03::task(
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
    spawner.spawn(display::task().expect("Could not spawn Display task"));
}
