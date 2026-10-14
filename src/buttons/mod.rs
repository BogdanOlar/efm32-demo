//! Buttons management

pub mod capacitive;
pub mod switches;

use defmt::info;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::Instant;

const CHANNEL_SIZE: usize = ButtonId::COUNT * Action::COUNT;
pub static BUTTON_EVENTS_CHANNEL: Channel<ThreadModeRawMutex, Event, CHANNEL_SIZE> = Channel::new();

/// Button ID
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ButtonId {
    Btn0,
    Btn1,
    Cap0,
    Cap1,
}

impl ButtonId {
    /// ButtonId count (`mem::variant_count<T>()` is on unstable, behind `variant_count` feature gate)
    const COUNT: usize = 4;
}

/// Report on a button action
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Action {
    Released,
    Pressed,
}

impl Action {
    const COUNT: usize = 2;
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Event {
    ts: Instant,
    id: ButtonId,
    action: Action,
}

#[embassy_executor::task]
pub async fn task() {
    info!("Starting buttons::task...");
    let events = BUTTON_EVENTS_CHANNEL.receiver();

    loop {
        let event = events.receive().await;
        info!("Received event: {}", &event);
    }
}
