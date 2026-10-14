//! Display

pub mod ls013b7dh03;

use crate::buttons;
use defmt::{error, info};
use embassy_executor::task;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::Instant;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Dimensions, Point},
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    primitives::{Primitive, PrimitiveStyle, PrimitiveStyleBuilder, StrokeAlignment, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
    Drawable,
};
use heapless::format;

const MAX_DISPLAY_BUFFER_COUNT: usize = 2;

pub(crate) type DisplayFrameCh = Channel<
    ThreadModeRawMutex,
    DisplayFrame<'static, { ls013b7dh03::BUF_SIZE }>,
    MAX_DISPLAY_BUFFER_COUNT,
>;

pub(crate) static TO_SPI: DisplayFrameCh = Channel::new();
pub(crate) static FROM_SPI: DisplayFrameCh = Channel::new();

/// Generic display frame, parametrized over the size of the undelying `u8` buffer (`N: usize`)
pub struct DisplayFrame<'a, const N: usize> {
    pub buffer: &'a mut [u8; N],
}

impl<'a, const N: usize> DisplayFrame<'a, N> {
    pub fn new(buffer: &'a mut [u8; N]) -> Self {
        Self { buffer }
    }

    /// Get the raw bytes of the display frame.
    ///
    /// These include the line address bytes that the LCD expects
    pub fn as_bytes(&self) -> &[u8] {
        self.buffer.as_slice()
    }
}

#[task]
pub async fn task() {
    info!("Started display::task ...");
    let frames_in = FROM_SPI.receiver();
    let frames_out = TO_SPI.sender();

    // Create styles used by the drawing operations.
    let thin_stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let border_stroke = PrimitiveStyleBuilder::new()
        .stroke_color(BinaryColor::On)
        .stroke_width(3)
        .stroke_alignment(StrokeAlignment::Inside)
        .build();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let left_aligned = TextStyleBuilder::new()
        .alignment(Alignment::Left)
        .baseline(Baseline::Top)
        .build();
    let mut y_offset = 20;

    let sec_us: f32 = 1000000.0;
    let mut prev_frame_start = Instant::now();

    let mut x_offset = 100;
    const T_WIDTH: i32 = 16;
    const T_HEIGHT: i32 = 16;

    loop {
        let mut frame = frames_in.receive().await;
        let frame_start = Instant::now();
        let frame_us = (frame_start - prev_frame_start).as_micros() as f32;
        prev_frame_start = frame_start;

        let x_dir = {
            // buttons state lock will be dropped a the end of this scope
            let bs = buttons::BUTTON_STATES.lock().await;

            if bs.cap1.action == buttons::Action::Pressed
                && bs.cap0.action == buttons::Action::Pressed
            {
                if bs.cap1.ts > bs.cap0.ts {
                    -1
                } else {
                    1
                }
            } else if bs.cap1.action == buttons::Action::Pressed {
                -1
            } else if bs.cap0.action == buttons::Action::Pressed {
                1
            } else {
                0
            }
        };

        x_offset += x_dir;
        x_offset = x_offset.clamp(0, ls013b7dh03::WIDTH as i32);

        y_offset = if y_offset <= -16 {
            ls013b7dh03::HEIGHT as i32
        } else {
            y_offset - 1
        };

        frame.clear(BinaryColor::Off);

        // Draw a 3px wide outline around the display.
        frame
            .bounding_box()
            .into_styled(border_stroke)
            .draw(&mut frame);

        // Draw a triangle.
        Triangle::new(
            Point::new(x_offset, y_offset),
            Point::new(x_offset - (T_WIDTH / 2), y_offset + T_HEIGHT),
            Point::new(x_offset + (T_WIDTH / 2), y_offset + T_HEIGHT),
        )
        .into_styled(thin_stroke)
        .draw(&mut frame);

        // FPS
        let fps = if frame_us.is_normal() {
            sec_us / frame_us
        } else {
            0.0
        };

        if let Ok(fps_str) = format!(10; "{:.1} fps", fps) {
            let fps_txt = Text::with_text_style(
                fps_str.as_str(),
                frame.bounding_box().top_left,
                character_style,
                left_aligned,
            );
            frame.fill_solid(&fps_txt.bounding_box(), BinaryColor::On);
            fps_txt.draw(&mut frame);
        } else {
            error!("Could not format fps {}", &fps);
        }

        frames_out.send(frame).await;
    }
}
