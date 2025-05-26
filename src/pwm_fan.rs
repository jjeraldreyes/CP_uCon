use core::u16;

use smart_leds::{
    RGB, RGB8, SmartLedsWrite, colors, gamma,
    hsv::{Hsv, hsv2rgb},
};

use stm32f4xx_hal::{hal::spi, prelude::*, rcc, timer};

use defmt;
use ws2812_spi as ws2812;

pub struct AdjustablePwmFan<SPI, TIM, PINS>
where
    SPI: spi::SpiBus<u8>,
    TIM: timer::PwmExt,
    PINS: timer::Pins<TIM>,
{
    device: timer::PwmHz<TIM, PINS>,
    channel: timer::Channel,
    current_duty: u16,
    pub rgb: Option<PwmFanRgb<SPI>>,
}

pub struct PwmFanRgb<SPI>
where
    SPI: spi::SpiBus<u8>,
{
    pub device: ws2812::Ws2812<SPI>,
    color_mode: u8,
    brightness: u8,
}

impl<SPI, TIM, PINS> AdjustablePwmFan<SPI, TIM, PINS>
where
    SPI: spi::SpiBus<u8>,
    TIM: timer::PwmExt,
    PINS: timer::Pins<TIM>,
{
    pub fn new(
        timer: TIM,
        pwm_pin: PINS,
        pwm_channel: timer::Channel,
        clock: &rcc::Clocks,
    ) -> Self {
        let pwm_obj = timer.pwm_hz(pwm_pin, 1000.kHz(), &clock);

        Self {
            device: pwm_obj,
            channel: pwm_channel,
            current_duty: 25,
            rgb: None,
        }
    }

    pub fn with_rgb(
        timer: TIM,
        pwm_pin: PINS,
        pwm_channel: timer::Channel,
        spi_bus: SPI,
        clock: &rcc::Clocks,
    ) -> Self {
        let mut new_obj = Self::new(timer, pwm_pin, pwm_channel, clock);
        new_obj.rgb = Some(PwmFanRgb::new(spi_bus));

        new_obj
    }

    pub fn init(&mut self) {
        self.device.enable(self.channel);
    }

    /// Set duty cycle
    ///
    /// The duty cycle should be between 0% and 100% inclusive.
    pub fn set_duty(&mut self, mut duty: u16) {
        let max_duty = match self.device.get_max_duty() {
            0 => u16::MAX,
            other => other,
        };

        duty = duty.clamp(0, 100);
        let scaled_duty = u32::from(duty)
            .saturating_mul(u32::from(max_duty))
            .saturating_mul(655)
            .wrapping_shr(16)
            .clamp(0, u32::from(u16::MAX)) as u16;

        self.device.set_duty(self.channel, scaled_duty);
        self.current_duty = duty;
    }

    /// Get duty cycle
    pub fn get_duty(&mut self) -> u16 {
        // let now_duty = u32::from(self.device.get_duty(self.channel))
        //     .saturating_mul(100)
        //     .saturating_mul(0)
        //     .wrapping_shr(16)
        //     .clamp(0, 100)
        //     as u16;

        // duty (100 / max_duty) (max_duty / 2^16) (2^16 / max_duty)

        self.current_duty
    }
}

impl<SPI> PwmFanRgb<SPI>
where
    SPI: spi::SpiBus<u8>,
{
    pub const MAX_MODES: usize = 13;
    pub const FAN_LED_QTY: usize = 8;
    pub const LED_COLOR_PALETTES: [[RGB8; 16]; 4] = [
        // Forest
        [
            colors::DARK_GREEN,
            colors::DARK_GREEN,
            colors::DARK_OLIVE_GREEN,
            colors::DARK_GREEN,
            colors::GREEN,
            colors::FOREST_GREEN,
            colors::OLIVE_DRAB,
            colors::GREEN,
            colors::SEA_GREEN,
            colors::MEDIUM_AQUAMARINE,
            colors::LIME_GREEN,
            colors::YELLOW_GREEN,
            colors::LIGHT_GREEN,
            colors::LAWN_GREEN,
            colors::MEDIUM_AQUAMARINE,
            colors::FOREST_GREEN,
        ],
        // Cloud
        [
            colors::BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::DARK_BLUE,
            colors::BLUE,
            colors::DARK_BLUE,
            colors::SKY_BLUE,
            colors::SKY_BLUE,
            colors::LIGHT_BLUE,
            colors::WHITE,
            colors::LIGHT_BLUE,
            colors::SKY_BLUE,
        ],
        // Heat
        [
            RGB::new(0, 0, 0),
            RGB::new(0x33, 0, 0),
            RGB::new(0x66, 0, 0),
            RGB::new(0x99, 0, 0),
            RGB::new(0xCC, 0, 0),
            RGB::new(0xFF, 0, 0),
            RGB::new(0xFF, 0x33, 0),
            RGB::new(0xFF, 0x66, 0),
            RGB::new(0xFF, 0x99, 0),
            RGB::new(0xFF, 0xCC, 0),
            RGB::new(0xFF, 0xFF, 0),
            RGB::new(0xFF, 0xFF, 0x33),
            RGB::new(0xFF, 0xFF, 0x66),
            RGB::new(0xFF, 0xFF, 0x99),
            RGB::new(0xFF, 0xFF, 0xCC),
            RGB::new(0xFF, 0xFF, 0xFF),
        ],
        // Rainbow
        [
            RGB::new(0xFF, 0, 0),
            RGB::new(0xD5, 0x2A, 0),
            RGB::new(0xAB, 0x55, 0),
            RGB::new(0xAB, 0x7F, 0),
            RGB::new(0xAB, 0xAB, 0),
            RGB::new(0x56, 0xD5, 0),
            RGB::new(0, 0xFF, 0),
            RGB::new(0, 0xD5, 0x2A),
            RGB::new(0, 0xAB, 0x55),
            RGB::new(0, 0x56, 0xAA),
            RGB::new(0, 0, 0xFF),
            RGB::new(0x2A, 0, 0xD5),
            RGB::new(0x55, 0, 0xAB),
            RGB::new(0x7F, 0, 0x81),
            RGB::new(0xAB, 0, 0x55),
            RGB::new(0xD5, 0, 0x2B),
        ],
    ];

    fn new(spi_bus: SPI) -> Self {
        let device = ws2812::Ws2812::new(spi_bus);

        PwmFanRgb {
            color_mode: 0,
            brightness: 128u8, // Default brightness
            device,
        }
    }

    pub fn increment_mode(&mut self, current_time_ms: u32) -> Result<(), crate::error::Error> {
        self.color_mode += 1;
        self.color_mode %= u8::try_from(Self::MAX_MODES).unwrap_or(1); // Prevent panic on bad MAX_MODES
        self.update(current_time_ms)?;

        Ok(())
    }

    pub fn get_mode_text(&self) -> &'static str {
        match self.color_mode {
            0 => "Rainbow Twirl",
            1 => "Rainbow Fade",
            2 => "Rainbow Palette",
            3 => "Forest Palette",
            4 => "Cloud Palette",
            5 => "Heat Palette",
            6 => "Red Static",
            7 => "Green Static",
            8 => "Blue Static",
            9 => "White Static",
            10 => "Yellow Static",
            11 => "Cyan Static",
            12 => "Magenta Static",
            _ => "Unknown Mode",
        }
    }

    pub fn update(&mut self, current_time_ms: u32) -> Result<(), crate::error::Error> {
        let mut leds: [RGB8; Self::FAN_LED_QTY] = [RGB8::default(); Self::FAN_LED_QTY];
        let time_val = current_time_ms;

        match self.color_mode {
            // Rainbow Twirl
            0 => {
                for i in 0..Self::FAN_LED_QTY {
                    let hue = ((time_val / 20).wrapping_add((i * 256 / Self::FAN_LED_QTY) as u32)
                        % 256) as u8;
                    let hsv_color = Hsv {
                        hue,
                        sat: 255,
                        val: self.brightness,
                    };
                    leds[i] = hsv2rgb(hsv_color);
                }
            }
            // Rainbow Fade
            1 => {
                let hue = (time_val / 30 % 256) as u8;
                let hsv_color = Hsv {
                    hue,
                    sat: 255,
                    val: self.brightness,
                };
                for i in 0..Self::FAN_LED_QTY {
                    leds[i] = hsv2rgb(hsv_color);
                }
            }
            // Palette-based modes
            2 => self.palette_cycler(&mut leds, time_val, 3), // Rainbow Palette
            3 => self.palette_cycler(&mut leds, time_val, 0), // Forest Palette
            4 => self.palette_cycler(&mut leds, time_val, 1), // Cloud Palette
            5 => self.palette_cycler(&mut leds, time_val, 2), // Heat Palette
            // Static Colors
            6 => leds = [colors::RED; Self::FAN_LED_QTY],
            7 => leds = [colors::GREEN; Self::FAN_LED_QTY],
            8 => leds = [colors::BLUE; Self::FAN_LED_QTY],
            9 => leds = [colors::WHITE; Self::FAN_LED_QTY],
            10 => leds = [colors::YELLOW; Self::FAN_LED_QTY],
            11 => leds = [colors::CYAN; Self::FAN_LED_QTY],
            12 => leds = [colors::MAGENTA; Self::FAN_LED_QTY],

            _ => {
                // Default to off or a simple pattern
                for i in 0..Self::FAN_LED_QTY {
                    leds[i] = RGB8::default();
                }
            }
        }

        // Apply gamma correction and brightness
        let bright_leds: [RGB8; Self::FAN_LED_QTY] = gamma(leds)
            .iter()
            .map(|&c| {
                c.iter()
                    .map(|cc| (u16::from(cc) * u16::from(self.brightness) / 255) as u8)
                    .collect()
            })
            .collect();
        self.device
            .write(bright_leds.iter().cloned())
            .map_err(|_| crate::error::Error::SpiError)?;

        Ok(())
    }

    fn palette_cycler(
        &self,
        leds: &mut [RGB8; Self::FAN_LED_QTY],
        time_val: u32,
        palette_idx: usize,
    ) {
        let palette = Self::LED_COLOR_PALETTES[palette_idx % Self::LED_COLOR_PALETTES.len()];
        for i in 0..Self::FAN_LED_QTY {
            let color_idx =
                ((time_val / 100).wrapping_add(i as u32) % palette.len() as u32) as usize;
            leds[i] = palette[color_idx];
        }
    }
}
