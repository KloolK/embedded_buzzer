#![no_std]
#![feature(type_alias_impl_trait)]

use core::sync::atomic::AtomicU32;

use common::LedUpdate;
use embassy_stm32::gpio::{AnyPin, Output};
use embassy_stm32::peripherals::{
    self, ETH, PA1, PA2, PA7, PB0, PB1, PC1, PC2, PC3, PC4, PC5, PE2, PG11, PG12, PG13, RNG,
};
use embassy_stm32::rng::Rng;
use embassy_stm32::{bind_interrupts, eth, rng};
use embassy_time::Duration;
use heapless::mpmc::{Q16, Q64};
use heapless::Vec;
use rand_core::RngCore;

pub mod button_task;
pub mod leds;
pub mod net;

pub type LedOutputs = &'static mut Vec<Output<'static, AnyPin>, NUM_LEDS>;

pub static BUTTON_PRESS_Q: Q64<(u8, u64)> = Q64::new();
pub static LED_CHANGE_Q: Q16<LedUpdate> = Q16::new();

pub const THROTTLE_TIME: Duration = Duration::from_millis(10);
pub static INIT_TIME: AtomicU32 = AtomicU32::new(0);

pub const NUM_LEDS: usize = 6;

bind_interrupts!(pub struct Irqs {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

pub fn gen_random_seed(rng: RNG) -> u64 {
    let mut rng = Rng::new(rng, Irqs);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    u64::from_le_bytes(seed)
}

pub struct NetPeripherals {
    pub eth: ETH,
    pub pa1: PA1,
    pub pa2: PA2,
    pub pa7: PA7,
    pub pb0: PB0,
    pub pb1: PB1,
    pub pc1: PC1,
    pub pc2: PC2,
    pub pc3: PC3,
    pub pc4: PC4,
    pub pc5: PC5,
    pub pe2: PE2,
    pub pg11: PG11,
    pub pg12: PG12,
    pub pg13: PG13,
}

#[macro_export]
macro_rules! create_net_peripherals {
    ($peripherals:expr) => {
        buzzer_board::NetPeripherals {
            eth: $peripherals.ETH,
            pa1: $peripherals.PA1,
            pa2: $peripherals.PA2,
            pa7: $peripherals.PA7,
            pb0: $peripherals.PB0,
            pb1: $peripherals.PB1,
            pc1: $peripherals.PC1,
            pc2: $peripherals.PC2,
            pc3: $peripherals.PC3,
            pc4: $peripherals.PC4,
            pc5: $peripherals.PC5,
            pe2: $peripherals.PE2,
            pg11: $peripherals.PG11,
            pg12: $peripherals.PG12,
            pg13: $peripherals.PG13,
        }
    };
}
