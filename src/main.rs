#![no_std]
#![no_main]

use esp32c3_hal::{
    clock::ClockControl,
    pac,
    prelude::*,
    pulse_control::ClockSource,
    timer::TimerGroup,
    utils::{smart_leds_adapter::LedAdapterError, SmartLedsAdapter},
    Delay, PulseControl, Rtc, IO,
};
#[allow(unused_imports)]
use panic_halt;
use riscv_rt::entry;
use smart_leds::{SmartLedsWrite, RGB};

// powerbank max output is 5V * 2.1A = 10.5W
// Power consumption per LED: 0.3W for full white

type Color = RGB<u8>;

const NUM_LEDS: usize = 58;
const MAX_MILLIAMPS: usize = 2100; // that's the maximum the powerbank can provide
const MAX_MILLIWATTS: usize = 5 * MAX_MILLIAMPS;
const MICROCONTROLLER_CONSUMPTION_MW: usize = 1500; // just guessing...

const X_START: f32 = -3.0;
const X_END: f32 = (NUM_LEDS / 2 + 3) as f32;
const STEP_WIDTH: f32 = 0.13;
const HB: f32 = 3.0;
const VAL_0: f32 = 0.0;
const VAL_1: f32 = 10.0;
const VAL_2: f32 = 30.0;
const VAL_3: f32 = 60.0;
const HIGHLIGHT_1: f32 = 30.0;
const HIGHLIGHT_2: f32 = 60.0;
const HIGHLIGHT_3: f32 = 150.0;

struct AnimationContext {
    current_pos: f32,
    end_pos: f32,
    step_width: f32,
    asc: bool,
    bb: f32, // base brightness
    tb: f32, // target brightness
    hb: f32, // highlight brightness
    hw: f32, // highlight width
}

impl AnimationContext {
    fn new(
        start_pos: f32,
        end_pos: f32,
        step_width: f32,
        bb: f32,
        tb: f32,
        hb: f32,
        hw: f32,
    ) -> AnimationContext {
        assert!(bb <= tb && tb <= hb);
        assert!(step_width > 0.0);
        AnimationContext {
            current_pos: start_pos,
            end_pos,
            step_width,
            asc: start_pos < end_pos,
            bb,
            tb,
            hb,
            hw,
        }
    }

    fn next(&mut self, v: &mut [u8]) -> bool {
        if self.asc && self.current_pos > self.end_pos
            || !self.asc && self.current_pos < self.end_pos
        {
            return false;
        }

        self.calc_values(v);

        if self.asc {
            self.current_pos += self.step_width;
        } else {
            self.current_pos -= self.step_width;
        };

        return true;
    }

    pub fn calc_values(&self, v: &mut [u8]) {
        let hpos = self.current_pos;
        for i in 0..v.len() {
            v[i] = self.calc_value(hpos, i) as u8;
        }
    }

    pub fn calc_value(
        &self,
        hpos: f32,
        pos: usize, // led position (index)
    ) -> f32 {
        let pos = pos as f32;
        let use_tb = self.asc && hpos >= pos || !self.asc && hpos <= pos;
        let ambient = self.bb + (self.tb - self.bb) * use_tb as u8 as f32;

        let pos_diff = hpos - pos;
        let pos_diff = if pos_diff < 0.0 { -pos_diff } else { pos_diff };
        let highlight = if pos_diff < self.hw {
            self.hb * (1.0 - (pos_diff / self.hw))
        } else {
            0.0
        };
        if highlight > ambient {
            highlight
        } else {
            ambient
        }
    }
}

struct TrailerLight<L>
where
    L: SmartLedsWrite<Error = LedAdapterError, Color = RGB<u8>>,
{
    led: L,
    data: [RGB<u8>; NUM_LEDS],
    delay: Delay,
}

impl<L> TrailerLight<L>
where
    L: SmartLedsWrite<Error = LedAdapterError, Color = RGB<u8>>,
{
    pub fn new(led: L, delay: Delay) -> Self {
        TrailerLight {
            led: led,
            data: [RGB::new(0, 0, 0); NUM_LEDS],
            delay: delay,
        }
    }

    pub fn color(&mut self, color: RGB<u8>) {
        self.data = [color; NUM_LEDS];
        self.write_leds();
    }

    pub fn delay_ms(&mut self, ms: u16) {
        self.delay.delay_ms(ms);
    }

    pub fn black(&mut self) {
        self.color(RGB::new(0, 0, 0));
    }

    pub fn blink(&mut self) {
        const NUM_BLINKING: usize = 4;
        const BLINK_DELAY: u16 = 500;
        for _ in 0..2 {
            for i in 0..NUM_BLINKING {
                self.data[i + NUM_LEDS / 2 - NUM_BLINKING / 2] = Color::new(VAL_1 as u8, 0, 0);
            }
            self.write_leds();
            self.delay_ms(BLINK_DELAY);
            for i in 0..NUM_BLINKING {
                self.data[i + NUM_LEDS / 2 - NUM_BLINKING / 2] = Color::new(0, 0, 0);
            }
            self.write_leds();
            self.delay_ms(BLINK_DELAY);
        }
    }

    pub fn turn_on_animation(&mut self) {
        let mut v = [0; NUM_LEDS / 2];

        let ctxs = [
            AnimationContext::new(X_START, X_END, STEP_WIDTH, VAL_0, VAL_1, HIGHLIGHT_1, HB),
            AnimationContext::new(X_END, X_START, STEP_WIDTH, VAL_1, VAL_2, HIGHLIGHT_2, HB),
            AnimationContext::new(X_START, X_END, STEP_WIDTH, VAL_2, VAL_3, HIGHLIGHT_3, HB),
        ];

        for mut ctx in ctxs {
            while ctx.next(&mut v) {
                for i in 0..NUM_LEDS / 2 {
                    self.data[i + NUM_LEDS / 2] = Color::new(v[i], 0, 0);
                    self.data[NUM_LEDS / 2 - i] = Color::new(v[i], 0, 0);
                }
                self.write_leds();
            }
        }
    }

    pub fn emergency_brake(&mut self) {
        for _ in 0..5 {
            self.color(Color::new(255, 0, 0));
            self.delay_ms(100u16);
            self.color(Color::new(0, 0, 0));
            self.delay_ms(100u16);
        }
    }

    fn write_leds(&mut self) {
        // check max consumption:
        let sum: usize = self
            .data
            .iter()
            .map(|c| c.r as usize + c.g as usize + c.b as usize)
            .sum();
        if (sum / 255 * 100) > MAX_MILLIWATTS - MICROCONTROLLER_CONSUMPTION_MW {
            self.led.write([RGB::new(10, 0, 10)].into_iter()).unwrap();
            for _ in 0..NUM_LEDS {
                self.led.write([RGB::new(0, 0, 0)].into_iter()).unwrap();
            }
            panic!("Exceeded power budget");
        }

        self.led.write(self.data.iter().cloned()).unwrap();
        self.delay.delay_us(500u16);
    }
}

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

    // Delay peripheral
    let delay = Delay::new(&clocks);

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

    // Initialize the LED driver
    //   3 colors (RGB) * 8 bits per color = 24
    //   additional + 1 for the end marker
    let led = SmartLedsAdapter::<_, _, { NUM_LEDS * 24 + 1 }>::new(pulse.channel0, io.pins.gpio8);

    let mut tl = TrailerLight::new(led, delay);

    tl.black();
    tl.delay_ms(500);

    tl.blink();
    tl.turn_on_animation();

    // emergency brake light
    // tl.delay_ms(3000u16);
    // tl.emergency_brake();
    // tl.color(Color::new(VAL_3 as u8, 0, 0));

    loop {}
}
