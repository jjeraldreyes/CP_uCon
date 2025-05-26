use embedded_hal::blocking::delay;
use stm32f4xx_hal::i2c::{self, I2c};

use hd44780_driver::{Cursor, CursorBlink, Display, DisplayMode, HD44780, bus::I2CBus};

// Dummy delay provider using cortex_m::asm::delay
struct AsmDelay;

impl delay::DelayUs<u16> for AsmDelay {
    fn delay_us(&mut self, us: u16) {
        // Assuming a 48MHz clock, 1 cycle is approx 20ns.
        // So, 1us is approx 50 cycles.
        cortex_m::asm::delay(u32::from(us) * 50);
    }
}

impl delay::DelayUs<u8> for AsmDelay {
    fn delay_us(&mut self, us: u8) {
        cortex_m::asm::delay(u32::from(us) * 50);
    }
}

impl delay::DelayMs<u8> for AsmDelay {
    fn delay_ms(&mut self, ms: u8) {
        cortex_m::asm::delay(u32::from(ms) * 50 * 1000);
    }
}

pub struct I2CLcd<I2C>
where
    I2C: i2c::Instance,
{
    device: HD44780<I2CBus<I2c<I2C>>>,
}

impl<'a, I2C, Delay> I2CLcd<I2C>
where
    I2C: i2c::Instance,
    Delay: delay::DelayUs<u16> + delay::DelayMs<u8> + delay::DelayUs<u8>,
{
    pub fn new(
        i2c_bus: I2c<I2C>,
        delay_for_init: &'a mut Delay,
    ) -> Result<Self, crate::error::Error> {
        let lcd_1602: HD44780<I2CBus<I2c<I2C>>> = HD44780::new_i2c(i2c_bus, 0x27, delay_for_init)?;

        Ok(Self { device: lcd_1602 })
    }

    pub fn init(&mut self, delay_for_init: &'a mut Delay) -> Result<(), crate::error::Error> {
        self.device.reset(delay_for_init)?;
        self.device.clear(delay_for_init)?;

        self.device.set_display_mode(
            DisplayMode {
                display: Display::On,
                cursor_visibility: Cursor::Invisible,
                cursor_blink: CursorBlink::Off,
            },
            delay_for_init,
        )?;

        Ok(())
    }

    pub fn write_duty_cycle(&mut self, mut duty_cycle: u8) -> Result<(), crate::error::Error> {
        let mut asm_delay = AsmDelay;
        duty_cycle = duty_cycle.clamp(0, 100);
        let (duty_cycle_num_start, duty_cycle_bytes) = Self::from_number(u32::from(duty_cycle));

        self.device
            .set_cursor_pos(0, &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .write_str("Fan:     ", &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .set_cursor_pos(5, &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .write_bytes(&duty_cycle_bytes[duty_cycle_num_start..], &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .write_str("%", &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;

        Ok(())
    }

    pub fn write_number(&mut self, number: u32) -> Result<(), crate::error::Error> {
        let mut asm_delay = AsmDelay;
        self.device
            .set_cursor_pos(0, &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .write_str("DNum: ", &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;

        let (num_bytes_start, num_bytes) = Self::from_number(number);
        self.device
            .write_bytes(&num_bytes[num_bytes_start..], &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;

        Ok(())
    }

    pub fn write_message(
        &mut self,
        message: &str,
        position: (u8, u8),
    ) -> Result<(), crate::error::Error> {
        let mut asm_delay = AsmDelay;
        self.device
            .set_cursor_pos(position.0 * 40 + position.1, &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;
        self.device
            .write_str(message, &mut asm_delay)
            .map_err(|_| crate::error::Error::LcdError)?;

        Ok(())
    }

    fn from_number(mut number: u32) -> (usize, [u8; 10]) {
        let mut ans = [0u8; 10];
        let mut ans_start = 10usize;

        ans[9] = b'0';

        for v in ans.iter_mut().rev() {
            if number <= 0 {
                break;
            }

            *v = char::from_digit(u32::from(number % 10), 10)
                .map(|v| u8::try_from(v).unwrap())
                .unwrap();
            number /= 10;
            ans_start -= 1;
        }

        (ans_start, ans)
    }
}
