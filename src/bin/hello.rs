#![no_main]
#![no_std]

use brachiograph as _; // global logger + panicking-behavior + memory layout

use cortex_m::asm;
use nb::block;
use stm32f1xx_hal::{
    pac,
    prelude::*,
    time::ms,
    timer::{Channel, Tim2NoRemap, Timer},
    usb::{Peripheral, UsbBus},
};
use usb_device::prelude::*;
use usbd_serial::{SerialPort, USB_CLASS_CDC};

#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::println!("Hello, world!");
    let cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();
    let mut flash = dp.FLASH.constrain();
    let mut afio = dp.AFIO.constrain();
    let rcc = dp.RCC.constrain();
    let clocks = rcc
        .cfgr
        .use_hse(8.MHz())
        .sysclk(48.MHz())
        .pclk1(24.MHz())
        .freeze(&mut flash.acr);

    assert!(clocks.usbclk_valid());

    let mut gpioa = dp.GPIOA.split();

    let mut usb_dp = gpioa.pa12.into_push_pull_output(&mut gpioa.crh);
    usb_dp.set_low();
    asm::delay(clocks.sysclk().raw() / 100);

    let usb = Peripheral {
        usb: dp.USB,
        pin_dm: gpioa.pa11,
        pin_dp: usb_dp.into_floating_input(&mut gpioa.crh),
    };
    let usb_bus = UsbBus::new(usb);
    let mut serial = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .manufacturer("Cam Bam")
        .product("Bam")
        .serial_number("TEST")
        .device_class(USB_CLASS_CDC)
        .build();

    let mut led = gpioa.pa1.into_push_pull_output(&mut gpioa.crl);
    let mut timer = Timer::syst(cp.SYST, &clocks).counter_hz();
    timer.start(5.Hz()).unwrap();

    let pwm_pin = gpioa.pa0.into_alternate_push_pull(&mut gpioa.crl);
    let mut pwm = dp
        .TIM2
        .pwm_hz::<Tim2NoRemap, _, _>(pwm_pin, &mut afio.mapr, 50.Hz(), &clocks);
    defmt::println!("period {}", pwm.get_period());
    pwm.set_period(ms(500).into_rate());
    let max = pwm.get_max_duty();
    defmt::println!("max duty {}", max);
    pwm.set_duty(Channel::C1, max / 20);
    pwm.enable(Channel::C1);

    loop {
        /*
        if usb_dev.poll(&mut [&mut serial]) {
            defmt::println!("polled");
            led.set_low();
            let mut buf = [0u8; 64];
            if let Ok(len) = serial.read(&mut buf) {
                defmt::println!("{}", core::str::from_utf8(&buf[..len]).unwrap());
            }
            led.set_high();
        }
        */
    }
}
