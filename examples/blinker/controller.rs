// Copyright (c) 2026 Jolla Mobile Ltd

//! LED controller code.

// Written based on the original hack in CSD.

// This is just a quickly cobbled up version. A proper version would attempt to have the registers
// behind statically checked interfaces.

use num_enum::IntoPrimitive;
use std::io::{Error, Read, Result, Write};
use std::time::Duration;
use symbiosis::i2cdev::I2cDev;
use tokio::time::sleep;

const AW2023_TARGET_ADDRESS: u32 = 0x45;

#[derive(IntoPrimitive, Copy, Clone, Debug)]
#[repr(u8)]
enum AW2023 {
    Gcr1 = 0x01,
    LCtr = 0x30,
    Gcr2 = 0x04,
    Pwm0 = 0x34,
    Pwm1 = 0x35,
    Pwm2 = 0x36,
    LCfg0 = 0x31,
    LCfg1 = 0x32,
    LCfg2 = 0x33,
    Led0T0 = 0x37,
    Led0T1 = 0x38,
    Led0T2 = 0x39,
}

pub struct LedController<'dev> {
    dev: &'dev mut I2cDev,
}

impl<'dev> LedController<'dev> {
    pub fn new(dev: &'dev mut I2cDev) -> Self {
        Self { dev }
    }

    fn read_register(&mut self, register: AW2023) -> Result<u8> {
        self.dev.set_target_address(AW2023_TARGET_ADDRESS)?;
        let input = [register.into()];
        self.dev.write_all(&input)?;
        let mut output = [0u8; 1];
        self.dev.read_exact(&mut output)?;
        Ok(output[0])
    }

    fn write_register(&mut self, register: AW2023, value: u8) -> Result<()> {
        self.dev.set_target_address(AW2023_TARGET_ADDRESS)?;
        let input = [register.into(), value];
        self.dev.write_all(&input)?;
        Ok(())
    }

    pub async fn turn_led_on(&mut self) -> Result<()> {
        // Try a few times
        for _ in 0..3 {
            // Write CHIPEN = 1
            self.write_register(AW2023::Gcr1, 0x01)?;
            // Check that the chip was enabled
            if (self.read_register(AW2023::Gcr1)? & 0x01) > 0 {
                return Ok(());
            }
            sleep(Duration::from_millis(10)).await;
        }
        Err(Error::other("failed after three retries"))
    }

    pub fn turn_led_off(&mut self) -> Result<()> {
        // Write CHIPEN = 0
        self.write_register(AW2023::Gcr1, 0x00)
    }

    pub async fn start_breathing(&mut self) -> Result<()> {
        self.turn_led_on()
            .await
            // GCR2.IMAX = 1 (30 mA)
            .and_then(|_| self.write_register(AW2023::Gcr2, 0x01))
            // LCFG2.CUR = 15 -> led 2 max current
            .and_then(|_| self.write_register(AW2023::LCfg2, 0x0f))
            // LCFG1.CUR = 15 -> led 1 max current
            .and_then(|_| self.write_register(AW2023::LCfg1, 0x0f))
            // LCFG0.CUR = 15 -> led 0 max current
            // LCFG0.SYNC = 1 -> enable sync mode
            // LCFG0.MD = 1 -> enable pattern mode
            .and_then(|_| self.write_register(AW2023::LCfg0, 0x9f))
            // Setup brightness
            // PWM[012].PWM = 255 -> max pwm brightness
            .and_then(|_| self.write_register(AW2023::Pwm0, 0xff))
            .and_then(|_| self.write_register(AW2023::Pwm1, 0xff))
            .and_then(|_| self.write_register(AW2023::Pwm2, 0xff))
            // Setup breathing time constants
            // LED0T0.T1 = 6 (1.04 s) (rise time)
            // LED0T0.T2 = 6 (1.04 s) (on time)
            .and_then(|_| self.write_register(AW2023::Led0T0, 0x66))
            // LED0T1.T3 = 6 (1.04 s) (fall time)
            // LED0T1.T4 = 6 (1.04 s) (off time)
            .and_then(|_| self.write_register(AW2023::Led0T1, 0x66))
            // LED0T2.T0 = 0 (0s)
            // LED0T2.REPEAT = 0 (unlimited)
            .and_then(|_| self.write_register(AW2023::Led0T2, 0x00))
            // Enable all leds
            // LCTR.LE2 = 1
            // LCTR.LE1 = 1
            // LCTR.LE0 = 1
            .and_then(|_| self.write_register(AW2023::LCtr, 0x07))
            .or_else(|_| self.turn_led_off())
    }
}
