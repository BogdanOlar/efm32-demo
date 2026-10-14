//! Buttons management

pub mod capacitive;
pub mod switches;

use defmt::info;
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    channel::Channel,
    mutex::Mutex,
};
use embassy_time::Instant;

const CHANNEL_SIZE: usize = (ButtonId::COUNT + 2) * Action::COUNT;
pub static BUTTON_EVENTS_CHANNEL: Channel<ThreadModeRawMutex, Event, CHANNEL_SIZE> = Channel::new();
/// The shared state protected by a mutex
pub static BUTTON_STATES: Mutex<CriticalSectionRawMutex, States> =
    Mutex::new(States::const_default());

/// Button ID
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ButtonId {
    Btn0,
    Btn1,
}

impl ButtonId {
    /// ButtonId count (`mem::variant_count<T>()` is on unstable, behind `variant_count` feature gate)
    const COUNT: usize = 2;
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

impl From<bool> for Action {
    fn from(value: bool) -> Self {
        if value {
            Action::Pressed
        } else {
            Action::Released
        }
    }
}

/// Report on a button action
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event {
    Btn0(State),
    Btn1(State),
    Cap0(State),
    Cap1(State),
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct State {
    pub ts: Instant,
    pub action: Action,
}

impl State {
    const fn const_default() -> Self {
        Self {
            ts: Instant::MIN,
            action: Action::Pressed,
        }
    }
}

/// All button states
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct States {
    pub btn0: State,
    pub btn1: State,
    pub cap0: State,
    pub cap1: State,
}

impl States {
    const fn const_default() -> Self {
        Self {
            btn0: State::const_default(),
            btn1: State::const_default(),
            cap0: State::const_default(),
            cap1: State::const_default(),
        }
    }
}

#[embassy_executor::task]
pub async fn task() {
    info!("Starting buttons::task...");
    let events = BUTTON_EVENTS_CHANNEL.receiver();

    loop {
        let event = events.receive().await;
        info!("Received event: {}", &event);

        // Guard scope for automatically unlocking BUTTONS_STATE when exited
        {
            let mut state = BUTTON_STATES.lock().await;

            match event {
                Event::Btn0(btn_state) => state.btn0 = btn_state,
                Event::Btn1(btn_state) => state.btn1 = btn_state,
                Event::Cap0(btn_state) => state.cap0 = btn_state,
                Event::Cap1(btn_state) => state.cap1 = btn_state,
            }
        }
    }
}
