// #![allow(clippy::empty_loop)]
#![no_main]
#![no_std]

use panic_halt as _;
use rtic::app;

mod error;
mod inputs;
mod lcd;
mod pwm_fan;
mod stoptimer; // May become partially or fully unused

#[cfg(use_defmt)]
use defmt_rtt as _; // global logger

#[app(device = stm32f4xx_hal::pac, peripherals = true, dispatchers = [TIM2, TIM4, SPI1])] // Added some dispatchers, adjust as needed
mod app {
    use crate::hal::{
        self as hal, // alias hal for clarity within app mod
        gpio::{self, Alternate, Analog, Input, NoPin, PullUp},
        i2c::{I2c, Mode},
        pac,
        prelude::*,
        rcc,
        spi,
        timer::{self, Event, MonoTimerUs},
    };
    use crate::inputs::{DebouncedDInput, DebouncedOutput, PotRead};
    use crate::lcd;
    use crate::pwm_fan;
    // use crate::stoptimer; // stoptimer module is now mostly empty

    use cortex_m::peripheral::SYST;
    use defmt;

    // Define a monotonic timer based on TIM3
    #[monotonic(binds = TIM3, default = true)]
    type AppMono = MonoTimerUs<pac::TIM3>;

    #[shared]
    struct Shared {
        pwm_obj: pwm_fan::AdjustablePwmFan<
            'static,
            pac::TIM2,
            pac::SPI2,
            timer::Channel3,
            gpio::PB10<Alternate>,
            NoPin,
            gpio::PB15<Alternate>,
        >,
        lcd: lcd::I2CLcd<pac::I2C1>, // Corrected type: Removed 'static and Delay type parameter
        rgb_needs_lcd_update: bool, // Flag to signal LCD update for RGB mode
    }

    #[local]
    struct Local {
        pot_obj: PotRead<'static, pac::ADC1, gpio::PA7<Analog>>,
        user_button: DebouncedDInput<'static, gpio::PC13<Input<PullUp>>>,
        general_delay: hal::timer::Delay<SYST, 1_000_000_u32>, // For one-off delays if needed, though tasks are preferred
    }

    fn setup_clocks(rcc_dp: pac::RCC) -> rcc::Clocks {
        rcc_dp.cfgr.sysclk(48.MHz()).freeze()
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::info!("RTIC Init!\n");

        let dp: pac::Peripherals = cx.device;
        let cp: cortex_m::peripheral::Peripherals = cx.core;

        let gpioa = dp.GPIOA.split();
        let gpiob = dp.GPIOB.split();
        let gpioc = dp.GPIOC.split();

        // Sysclk
        let rcc_constrained = dp.RCC.constrain();
        let clocks = setup_clocks(rcc_constrained);

        let general_delay = cp.SYST.delay(&clocks); // General purpose delay
        let mut lcd_init_delay_timer = dp.TIM1.delay_us(&clocks); // Specific for LCD init

        // Monotonic timer setup (replaces stoptimer::init_timer)
        let mono = dp.TIM3.monotonic_us(&clocks);
        defmt::info!("Monotonic timer initialized.");

        // Pot
        let pa7_analog = gpioa.pa7.into_analog();
        let pot_obj = PotRead::with_adc01(pa7_analog, dp.ADC1);
        defmt::info!("Potentiometer initialized.");

        // User button (PC13)
        // Configure PC13 for EXTI interrupt
        let mut syscfg = dp.SYSCFG.constrain();
        let mut exti = dp.EXTI;
        let user_button_pin = gpioc.pc13.into_pull_up_input();
        user_button_pin.make_interrupt_source(&mut syscfg);
        user_button_pin.enable_interrupt(&mut exti);
        user_button_pin.trigger_on_edge(&mut exti, gpio::Edge::Falling); // Trigger on press (assuming pull-up means low when pressed)
        let user_button = DebouncedDInput::with_pullup(user_button_pin.erase());
        defmt::info!("User button PC13 initialized for EXTI.");

        // RGB fan
        // Ensure correct Alternate Function (AF) mapping for your specific STM32F411.
        // PB13 (SPI2_SCK), PB15 (SPI2_MOSI)
        // PB10 (TIM2_CH3)
        // For STM32F411: PB13 is AF5 for SPI2_SCK, PB15 is AF5 for SPI2_MOSI. PB10 is AF1 for TIM2_CH3.
        let spi_sck = gpiob.pb13.into_alternate::<5>();
        let spi_mosi = gpiob.pb15.into_alternate::<5>();
        // NoPin for MISO as it's not used for WS2812

        let spi02 = dp.SPI2.spi(
            (spi_sck, NoPin::new(), spi_mosi), // (SCK, MISO, MOSI)
            ws2812_spi::MODE,                  // Using mode from ws2812-spi crate
            3.MHz(),
            &clocks,
        );
        let pwm_pin_d6 = gpiob.pb10.into_alternate::<1>(); // AF1 for TIM2_CH3 on PB10
        let fan_pwm_channel = timer::Channel3::new(pwm_pin_d6);
        let mut pwm_obj = pwm_fan::AdjustablePwmFan::with_rgb(
            dp.TIM2,
            fan_pwm_channel,
            timer::Channel::C3, // This argument seems redundant if Channel3::new is used, check pwm_fan module
            spi02,
            &clocks,
        );
        pwm_obj.init();
        pwm_obj.set_duty(50); // Initial duty
        defmt::info!("PWM Fan initialized.");

        // LCD
        // For STM32F411: PB8 (I2C1_SCL), PB9 (I2C1_SDA) are AF4
        let i2c_scl = gpiob.pb8.into_alternate_open_drain::<4>();
        let i2c_sda = gpiob.pb9.into_alternate_open_drain::<4>();

        let i2c_01 = I2c::new(
            dp.I2C1,
            (i2c_scl, i2c_sda),
            Mode::standard(100.kHz()),
            &clocks,
        );
        let mut lcd_obj = lcd::I2CLcd::new(i2c_01, &mut lcd_init_delay_timer).unwrap();
        lcd_obj.init(&mut lcd_init_delay_timer).unwrap(); // Pass delay again for init
        lcd_obj.write_duty_cycle(50).unwrap(); // Initial duty on LCD
        defmt::info!("LCD initialized.");

        // Schedule initial tasks
        // Using `unwrap` for spawn as failure here is catastrophic
        read_pot_and_update_fan::spawn().unwrap();
        periodic_rgb_update::spawn().unwrap();
        defmt::info!("Initial tasks spawned.");

        (
            Shared {
                pwm_obj,
                lcd: lcd_obj,
                rgb_needs_lcd_update: true,
            }, // Initially true to print mode
            Local {
                pot_obj,
                user_button,
                general_delay,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [pot_obj], shared = [pwm_obj, lcd], priority = 1)]
    fn read_pot_and_update_fan(cx: read_pot_and_update_fan::Context) {
        let new_duty_percent = cx.local.pot_obj.read_percent();

        // Get current time for any functions that might need it (though not directly used here yet)
        // let current_time = monotonics::AppMono::now();
        // let current_time_ms = current_time.duration_since_epoch().to_millis() as u32;

        cx.shared.lock(|shared| {
            let pwm_obj = &mut shared.pwm_obj;
            let lcd = &mut shared.lcd;

            if new_duty_percent != pwm_obj.get_duty() {
                pwm_obj.set_duty(new_duty_percent);
                lcd.write_duty_cycle(new_duty_percent as u8).unwrap();
                // defmt::println!("Fan Duty: {}%", new_duty_percent);
            }
        });

        // Reschedule this task
        read_pot_and_update_fan::spawn_after(100.millis()).unwrap();
    }

    #[task(binds = EXTI15_10, local = [user_button], shared = [pwm_obj, lcd, rgb_needs_lcd_update], priority = 3)]
    fn user_button_handler(cx: user_button_handler::Context) {
        let current_time = monotonics::AppMono::now();
        let current_time_ms = current_time.duration_since_epoch().to_millis() as u32;

        let mut button_pressed = false;
        if let DebouncedOutput::Changed(is_low) = cx.local.user_button.is_low(current_time_ms) {
            // Pass current time
            if !is_low {
                // Assuming !is_low means button was pressed and released (rising edge of signal if active low)
                button_pressed = true;
            }
        }

        if button_pressed {
            cx.shared.lock(|shared| {
                let pwm_obj = &mut shared.pwm_obj;
                // let lcd = &mut shared.lcd; // Not directly used here for LCD write
                let rgb_update_flag = &mut shared.rgb_needs_lcd_update;

                if let Some(rgb_obj) = &mut pwm_obj.rgb {
                    rgb_obj.increment_mode(current_time_ms).unwrap(); // Pass current time
                    defmt::println!("RGB mode change via button!");
                    *rgb_update_flag = true; // Signal that LCD needs to update RGB mode text
                }
            });
        }

        // Clear the interrupt pending bit for PC13 (EXTI line 13)
        unsafe { hal::pac::EXTI::steal().pr.write(|w| w.pr13().set_bit()) };
    }

    #[task(shared = [pwm_obj, lcd, rgb_needs_lcd_update], priority = 2)]
    fn periodic_rgb_update(cx: periodic_rgb_update::Context) {
        let current_time = monotonics::AppMono::now();
        let current_time_ms = current_time.duration_since_epoch().to_millis() as u32;

        cx.shared.lock(|shared| {
            let pwm_obj = &mut shared.pwm_obj;
            let lcd = &mut shared.lcd;
            let rgb_update_flag = &mut shared.rgb_needs_lcd_update;

            if let Some(rgb_obj) = &mut pwm_obj.rgb {
                if *rgb_update_flag {
                    lcd.write_message(rgb_obj.get_mode_text(), (1, 0)).unwrap();
                    *rgb_update_flag = false; // Reset flag
                }
                rgb_obj.update(current_time_ms).unwrap(); // Pass current time
            }
        });

        periodic_rgb_update::spawn_after(50.millis()).unwrap(); // Adjust interval as needed
    }

    // Optional: Idle task
    #[idle(local = [], shared = [])]
    fn idle(_: idle::Context) -> ! {
        loop {
            // defmt::println!("Idle");
            // cortex_m::asm::delay(12_000_000); // ~0.25s delay on 48MHz for testing
            cortex_m::asm::wfi(); // Wait For Interrupt
        }
    }
}
