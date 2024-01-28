#![no_std]
#![no_main]

extern crate panic_halt;

const IR_MINIMUM_DISTANCE: u16 = 500;
const SERIAL_BAUD_RATE: u32 = 57600;

#[arduino_hal::entry]
fn main() -> ! {
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);
    let mut adc = arduino_hal::Adc::new(dp.ADC, Default::default());

    let mut serial = arduino_hal::default_serial!(dp, pins, SERIAL_BAUD_RATE);
    let mut led = pins.d9.into_output();

    let ir_analog = pins.a0.into_analog_input(&mut adc);

    loop {
        let analog = ir_analog.analog_read(&mut adc);
        ufmt::uwriteln!(&mut serial, "ir sensor digital out = {}", analog);

        if analog <= IR_MINIMUM_DISTANCE {
            led.set_high()
        } else {
            led.set_low();
        }
    }
}
