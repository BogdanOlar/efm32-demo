//! Buttons usin HW switches

use crate::ButtonId;
use efm32xg_hal::gpio::{dynamic::DynamicPin, efemb::AsyncInputPin};
use embassy_time::Timer;
use embedded_hal::digital::OutputPin;
use embedded_hal_async::digital::Wait;

pub async fn button_led_task(btn_id: ButtonId, mut btn: AsyncInputPin, mut led: DynamicPin) {
    loop {
        // Wait for button press (active low)
        let _ = btn.wait_for_low().await;
        let _ = led.set_high();
        defmt::info!("{} pressed", &btn_id);

        // Small delay to debounce
        Timer::after_millis(50).await;

        // Wait for button release
        let _ = btn.wait_for_high().await;
        let _ = led.set_low();
        defmt::info!("{} released", &btn_id);

        // Small delay to debounce
        Timer::after_millis(50).await;
    }
}
