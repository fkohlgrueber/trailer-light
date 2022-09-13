#![no_std]
#![no_main]

use esp32c3_hal::{
    clock::ClockControl,
    pac,
    prelude::*,
    pulse_control::ClockSource,
    timer::TimerGroup,
    utils::{smartLedAdapter, SmartLedsAdapter},
    Delay,
    PulseControl,
    Rtc,
    IO,
};
#[allow(unused_imports)]
use panic_halt;
use riscv_rt::entry;
use smart_leds::{
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite, RGB,
};

// powerbank max output is 5V * 2.1A = 10.5W
// Power consumption per LED: 0.3W for full white

type Color = RGB<u8>;

const NUM_LEDS: usize = 58;
const MAX_MILLIAMPS: usize = 2100; // that's the maximum the powerbank can provide
const MAX_MILLIWATTS: usize = 5 * MAX_MILLIAMPS;
const MICROCONTROLLER_CONSUMPTION_MW: usize = 1500; // just guessing...


#[entry]
fn main() -> ! {
    let peripherals = pac::Peripherals::take().unwrap();
    let mut system = peripherals.SYSTEM.split();
    let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    let mut rtc = Rtc::new(peripherals.RTC_CNTL);
    let timer_group0 = TimerGroup::new(peripherals.TIMG0, &clocks);
    let mut wdt0 = timer_group0.wdt;
    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);

    // Disable watchdogs
    rtc.swd.disable();
    rtc.rwdt.disable();
    wdt0.disable();

    // Configure RMT peripheral globally
    let pulse = PulseControl::new(
        peripherals.RMT,
        &mut system.peripheral_clock_control,
        ClockSource::APB,
        0,
        0,
        0,
    )
    .unwrap();

    // We use one of the RMT channels to instantiate a `SmartLedsAdapter` which can
    // be used directly with all `smart_led` implementations
    let mut led = <smartLedAdapter!(1)>::new(pulse.channel0, io.pins.gpio8);

    // Initialize the Delay peripheral, and use it to toggle the LED state in a
    // loop.
    let mut delay = Delay::new(&clocks);

    let mut color = Hsv {
        hue: 0,
        sat: 255,
        val: 60,
    };

    let mut write_leds = |colors: &[Color; NUM_LEDS]| {
        // check max consumption:
        let sum: usize = colors.iter().map(|c| c.r as usize + c.g as usize + c.b as usize).sum();
        if (sum / 255 * 100) > MAX_MILLIWATTS - MICROCONTROLLER_CONSUMPTION_MW {
            led.write([RGB{r: 10, g: 0, b: 10}].into_iter())
                .unwrap();
            for _ in 0..57 {
                led.write([RGB{r: 0, g: 0, b: 0}].into_iter())
                    .unwrap();
            }
            panic!("Exceeded power budget");
        }

        for c in colors {
            led.write([c.clone()].into_iter())
                .unwrap();
        }
        delay.delay_ms(20u8);
    };

    let mut data = [RGB{..Default::default()}; NUM_LEDS];

    loop {
        // Iterate over the rainbow!
        for hue in 0..=255 {
            
            for x in 0..58 {
                color.hue = (hue + x) & 0xff;
                data[x as usize] = hsv2rgb(color);
            }

            write_leds(&data);
        }
    }
}
