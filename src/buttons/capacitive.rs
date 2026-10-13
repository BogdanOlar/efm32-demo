//! Buttons using capacitive pads

use defmt::info;
use efm32xg_hal::{
    acmp::{self, Acmp},
    prs,
    timer::{self, Timer},
};
use embassy_time::Timer as EmbTimer;

pub async fn capacitive_task(
    p_timer0: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Timer0>,
    p_timer1: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Timer1>,
    p_acmp: efm32xg_hal::Peri<'static, efm32xg_hal::peripherals::Acmp0>,
    mut prs_ch0: prs::Channel<0>,
) {
    // Configure PRS channel 0 to detect ACMP0 output edges
    prs_ch0.set_source(prs::ChSource::Acmp0);
    prs_ch0.set_edge(prs::ChCtrlEdsel::Posedge);

    // Configure TIMER0 as measurement window timer
    // HFPERCLK = 40MHz, prescaler = Div512, so timer clock = 40MHz / 512 = 78125 Hz
    // TOP = 100, so measurement window = (100+1) / 78125 = 1.292ms
    let mut window_timer = Timer::new(
        p_timer0,
        &timer::Config {
            clock: timer::Clksel::Preschfperclk,
            presc: timer::Presc::Div512,
            top: 100,
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

    const ACMP_CHANNEL_COUNT: usize = 2;
    let acmp_channels: [acmp::InputSelect; ACMP_CHANNEL_COUNT] = [
        acmp::InputSelect::Aport4XCh27,
        acmp::InputSelect::Aport3XCh28,
    ];
    let mut count: [u16; ACMP_CHANNEL_COUNT] = Default::default();
    // Track maximum counts for calibration
    let mut max_count: [u16; ACMP_CHANNEL_COUNT] = Default::default();
    let mut threshold: [u16; ACMP_CHANNEL_COUNT] = Default::default();
    let mut touch: [bool; ACMP_CHANNEL_COUNT] = Default::default();

    loop {
        // Enable ACMP for the entire measurement cycle
        acmp.set_enabled(true);
        // wait for warmup (finished) interrupt
        let ints = acmp.interrupts().await;
        assert!(ints.is_set(acmp::Interrupt::Warmup));

        for (index, acmp_channel) in acmp_channels.iter().enumerate() {
            acmp.set_input_pos(*acmp_channel);

            // Reset timers
            window_timer.set_count(0);
            count_timer.set_count(0);

            // Start measurement - count_timer starts&stops automatically via SYNC
            window_timer.start();
            window_timer.interrupt().await;
            window_timer.stop();

            count[index] = count_timer.count();

            // Update maximum for calibration
            max_count[index] = max_count[index].max(count[index]);
            // Simple touch detection: count drops below 75% of maximum (25% below = touched)
            threshold[index] = max_count[index].saturating_sub(max_count[index] / 4);
            // Calculate pressed state
            touch[index] = count[index] < threshold[index] && max_count[index] > 0;
        }

        acmp.set_enabled(false);

        info!(
            "PB11={} ({}) PB12={} ({}) | Max: {} {} | T: {} {}",
            count[0],
            threshold[0],
            count[1],
            threshold[1],
            max_count[0],
            max_count[1],
            touch[0],
            touch[1]
        );

        EmbTimer::after_millis(50).await;
    }
}
