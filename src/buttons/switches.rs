//! Buttons usin HW switches

use crate::buttons::{Action, ButtonId, Event, State, BUTTON_EVENTS_CHANNEL};
use defmt::{info, warn};
use efm32_hal::gpio::{dynamic::DynamicPin, efemb::AsyncInputPin};
use embassy_executor::task;
use embassy_time::{Instant, Timer};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::digital::Wait;

#[task(pool_size = 2)]
pub async fn task(id: ButtonId, mut btn: AsyncInputPin, mut led: DynamicPin) {
    info!("Starting Button {} task...", &id);
    let events = BUTTON_EVENTS_CHANNEL.sender();
    loop {
        // Wait for button press (active low)
        let _ = btn.wait_for_low().await;
        let _ = led.set_high();

        let state = State {
            ts: Instant::now(),
            action: Action::Pressed,
        };
        let event = match id {
            ButtonId::Btn0 => Event::Btn0(state),
            ButtonId::Btn1 => Event::Btn1(state),
        };
        if events.try_send(event).is_err() {
            warn!("Failed to send {}", &event);
        }

        // Small delay to debounce
        Timer::after_millis(50).await;

        // Wait for button release
        let _ = btn.wait_for_high().await;
        let _ = led.set_low();
        let state = State {
            ts: Instant::now(),
            action: Action::Released,
        };
        let event = match id {
            ButtonId::Btn0 => Event::Btn0(state),
            ButtonId::Btn1 => Event::Btn1(state),
        };
        if events.try_send(event).is_err() {
            warn!("Failed to send {}", &event);
        }

        // Small delay to debounce
        Timer::after_millis(50).await;
    }
}
