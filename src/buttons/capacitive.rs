//! Buttons using capacitive pads

use crate::buttons::{Event, State, BUTTON_EVENTS_CHANNEL};
use defmt::{info, warn};
use efm32_hal::{
    acmp::{self, Acmp},
    prs,
    timer::{self, Timer},
};
use embassy_executor::task;
use embassy_time::{Instant, Timer as EmbTimer};

#[task]
pub async fn task(
    p_timer0: efm32_hal::Peri<'static, efm32_hal::peripherals::Timer0>,
    p_timer1: efm32_hal::Peri<'static, efm32_hal::peripherals::Timer1>,
    p_acmp: efm32_hal::Peri<'static, efm32_hal::peripherals::Acmp0>,
    mut prs_ch0: prs::Channel<0>,
) {
    info!("Started buttons::capacitive::task...");
    let events = BUTTON_EVENTS_CHANNEL.sender();

    // Configure PRS channel 0 to detect ACMP0 output edges
    prs_ch0.set_source(prs::ChSource::Acmp0);
    prs_ch0.set_edge(prs::ChCtrlEdsel::Posedge);

    // Configure TIMER0 as measurement window timer
    // HFPERCLK = 40MHz, prescaler = Div512, so timer clock = 40MHz / 512 = 78125 Hz
    // TOP = 10, so measurement window = (10+1) / 78125 = 140us
    let mut window_timer = Timer::new(
        p_timer0,
        &timer::Config {
            clock: timer::Clksel::Preschfperclk,
            presc: timer::Presc::Div512,
            one_shot: true,
            top: 10,
            interrupts: timer::InterruptConfig {
                flags: timer::Interrupts::new(timer::Interrupt::Overflow),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    // Configure TIMER1 to count ACMP pulses via CC1+PRS, and synchronize Start/Stop to Timer0 Start/Stop
    let mut count_timer = Timer::new(
        p_timer1,
        &timer::Config {
            // synchronize the start&stop to Timer0 start&stop
            synchronized: true,
            // Clock source is CC1 (PRS pulses from ACMP)
            clock: timer::Clksel::Cc1,
            // Channel 1 takes clock input from Acmp via Prs channel 0
            channels: timer::ChannelCofigs::default().with_cc_config(
                timer::ChannelId::Ch1,
                timer::CcConfig {
                    mode: timer::CcCtrlMode::Inputcapture,
                    input_sel: timer::CcInputSel::Prs(prs::ChannelId::Ch0),
                    ic_edge_select: timer::CcCtrlIcedge::Both,
                    ic_event_control: timer::CcCtrlIcevctrl::Rising,
                },
            ),
            ..Default::default()
        },
    );

    // Configure ACMP0 for CAPACITIVE SENSE MODE
    let mut acmp = Acmp::new(
        p_acmp,
        &acmp::Config {
            full_bias: true,
            bias_prog: acmp::BiasProg::MAX,
            accuracy: acmp::Accuracy::High,
            hist_0: acmp::Hysteresis {
                div_va: acmp::DivV::try_from_u8(0x30).unwrap(),
                hyst: acmp::Hyst::Hyst8,
                ..Default::default()
            },
            hist_1: acmp::Hysteresis {
                div_va: acmp::DivV::try_from_u8(0x10).unwrap(),
                hyst: acmp::Hyst::Hyst8,
                ..Default::default()
            },
            input_sel: acmp::InputSelection {
                // Enable capacitive sense (relaxed RC oscillator), with given resistor value
                cs_res_sel: Some(acmp::CsResSel::Res7),
                va_sel: acmp::VaSel::Vdd,
                vb_sel: acmp::VbSel::V1p25,
                vlp_sel: acmp::VlpSel::VaDiv,
                neg_sel: acmp::InputSelect::VaDiv,
                ..Default::default()
            },
            interrupts: acmp::InteruptsConfig {
                warmup: true,
                conflict: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );

    let mut pads: [PadState; _] = [
        PadState::new(acmp::InputSelect::Aport3XCh28),
        PadState::new(acmp::InputSelect::Aport4XCh27),
    ];

    // DEBUG: some expected interrrupt flags
    const A_WARMUP: acmp::Interrupts = acmp::Interrupts::new(acmp::Interrupt::Warmup);
    const T_OVF: timer::Interrupts = timer::Interrupts::new(timer::Interrupt::Overflow);

    loop {
        // Enable ACMP for the entire measurement cycle
        acmp.set_enabled(true);

        // wait for warmup (finished) interrupt
        let ints = acmp.interrupts().await;
        // Only Warmup flag was raised
        assert!(ints == A_WARMUP);

        for (index, pad) in pads.iter_mut().enumerate() {
            acmp.set_input_pos(pad.ch);
            // No channel conflict expected
            assert!(!acmp.get_interrupt(acmp::Interrupt::Conflict));

            // Reset timers
            window_timer.set_count(0);
            count_timer.set_count(0);

            // Start measurement - count_timer starts&stops automatically via SYNC
            window_timer.start();
            let iflags = window_timer.interrupt().await;
            window_timer.stop();
            // Only Overflow flag was raised
            assert!(iflags == T_OVF);

            // DEBUG: window_timer count should be 0 after the Overflow interrupt event, i.e. it was stopped
            let wt_count = window_timer.count();
            if wt_count > 0 {
                warn!("{} count: {}", index, wt_count);
            }

            if let Some(state) = pad.set_count(count_timer.count()) {
                // pad changed state

                let event = match index {
                    0 => Event::Cap0(state),
                    1 => Event::Cap1(state),
                    _ => unreachable!(),
                };

                if events.try_send(event).is_err() {
                    warn!("Failed to send {}", &event);
                }
            }
        }

        acmp.set_enabled(false);

        EmbTimer::after_millis(10).await;
    }
}

struct PadState {
    ch: acmp::InputSelect,
    count: u16,
    max_count: u16,
    state: State,
}

impl PadState {
    const fn new(ch: acmp::InputSelect) -> Self {
        Self {
            ch,
            count: 0,
            max_count: 0,
            // Set initial touch to true so that we sent the expected `Released` event after first read
            state: State {
                ts: Instant::MIN,
                action: super::Action::Pressed,
            },
        }
    }

    fn set_count(&mut self, count: u16) -> Option<State> {
        self.count = count;
        // Update maximum for calibration
        self.max_count = self.max_count.max(self.count);
        // Simple touch detection: count drops below 75% of maximum (25% below = touched)
        let threshold = self.max_count.saturating_sub(self.max_count / 4);
        // Calculate pressed state
        let action = if self.count < threshold {
            super::Action::Pressed
        } else {
            super::Action::Released
        };

        if action != self.state.action {
            self.state = State {
                ts: Instant::now(),
                action,
            };
            Some(self.state)
        } else {
            None
        }
    }
}
