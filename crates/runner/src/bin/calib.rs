#![no_main]
#![no_std]

use brachiograph_runner as _;

use stm32f1xx_hal::{device::TIM3, timer::PwmChannel};
use usb_device::prelude::*;
use usbd_serial::SerialPort;

fn set_duty<const C: u8>(pwm: &mut PwmChannel<TIM3, C>, inc: i16) {
    let max = pwm.get_max_duty();
    let old = pwm.get_duty();
    let new = (old as i16 + inc) as u16;
    defmt::println!("duty {}/{}", new, max);
    pwm.set_duty(new);
}

pub struct Pwms {
    shoulder: PwmChannel<TIM3, 0>,
    elbow: PwmChannel<TIM3, 1>,
}

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [SPI1])]
mod app {
    use super::Pwms;
    use cortex_m::asm;
    use stm32f1xx_hal::{
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use systick_monotonic::Systick;
    use usb_device::prelude::*;
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    #[monotonic(binds = SysTick, default = true)]
    type Mono = Systick<100>;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        pwms: Pwms,
        usb_dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
        led: stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        static mut USB_BUS: Option<usb_device::bus::UsbBusAllocator<UsbBusType>> = None;

        let mut flash = cx.device.FLASH.constrain();
        let mut afio = cx.device.AFIO.constrain();
        let rcc = cx.device.RCC.constrain();
        let clocks = rcc
            .cfgr
            .use_hse(8.MHz())
            .sysclk(48.MHz())
            .pclk1(24.MHz())
            .freeze(&mut flash.acr);

        assert!(clocks.usbclk_valid());
        let mono = Systick::new(cx.core.SYST, clocks.hclk().to_Hz());

        let mut gpioa = cx.device.GPIOA.split();

        let mut usb_dp = gpioa.pa12.into_push_pull_output(&mut gpioa.crh);
        usb_dp.set_low();
        asm::delay(clocks.sysclk().raw() / 100);

        let usb = Peripheral {
            usb: cx.device.USB,
            pin_dm: gpioa.pa11,
            pin_dp: usb_dp.into_floating_input(&mut gpioa.crh),
        };
        unsafe {
            USB_BUS.replace(UsbBus::new(usb));
        }
        let usb_bus = unsafe { USB_BUS.as_ref().unwrap() };
        let serial = SerialPort::new(usb_bus);
        let usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x16c0, 0x27dd))
            .manufacturer("Cam Bam")
            .product("Bam")
            .serial_number("TEST")
            .device_class(USB_CLASS_CDC)
            .build();

        let led = gpioa.pa1.into_push_pull_output(&mut gpioa.crl);

        let shoulder_pin = gpioa.pa6.into_alternate_push_pull(&mut gpioa.crl);
        let elbow_pin = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
        let (mut shoulder, mut elbow) = cx
            .device
            .TIM3
            .pwm_us::<stm32f1xx_hal::timer::Tim3NoRemap, _, _>(
                (shoulder_pin, elbow_pin),
                &mut afio.mapr,
                fugit::Duration::<u32, 1, 1_000_000>::millis(20),
                &clocks,
            )
            .split();
        shoulder.set_duty(shoulder.get_max_duty() * 2 / 20);
        elbow.set_duty(elbow.get_max_duty() / 10);
        shoulder.enable();
        elbow.enable();
        let pwms = super::Pwms { shoulder, elbow };

        (
            Shared {},
            Local {
                usb_dev,
                serial,
                led,
                pwms,
            },
            init::Monotonics(mono),
        )
    }

    #[task(binds = USB_LP_CAN_RX0, local = [usb_dev, serial, led, pwms])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let usb_dev = cx.local.usb_dev;
        let serial = cx.local.serial;
        let led = cx.local.led;
        let pwms = cx.local.pwms;
        super::usb_read(usb_dev, serial, pwms, led);
    }
}

fn usb_read<B: usb_device::bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    serial: &mut SerialPort<'static, B>,
    pwms: &mut Pwms,
    led: &mut stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
) {
    if !usb_dev.poll(&mut [serial]) {
        return;
    }

    led.set_low();
    let mut buf = [0; 64];
    match serial.read(&mut buf) {
        Ok(count) if count > 0 => {
            for &ch in &buf[0..count] {
                match ch {
                    b'k' => {
                        set_duty(&mut pwms.shoulder, 1);
                    }
                    b'K' => {
                        set_duty(&mut pwms.shoulder, 10);
                    }
                    b'j' => {
                        set_duty(&mut pwms.shoulder, -1);
                    }
                    b'J' => {
                        set_duty(&mut pwms.shoulder, -10);
                    }
                    b'd' => {
                        set_duty(&mut pwms.elbow, -1);
                    }
                    b'D' => {
                        set_duty(&mut pwms.elbow, -10);
                    }
                    b'f' => {
                        set_duty(&mut pwms.elbow, 1);
                    }
                    b'F' => {
                        set_duty(&mut pwms.elbow, 10);
                    }
                    b'p' => {
                        let mut buf = itoa::Buffer::new();
                        let sh = buf.format(pwms.shoulder.get_duty());
                        if serial.write(sh.as_bytes()).ok() != Some(sh.len()) {
                            defmt::println!("failed to write it all");
                        }
                        if serial.write(b" ").ok() != Some(1) {
                            defmt::println!("failed to write it all");
                        }
                        let el = buf.format(pwms.elbow.get_duty());
                        if serial.write(el.as_bytes()).ok() != Some(el.len()) {
                            defmt::println!("failed to write it all");
                        }
                        if serial.write(b"\n").ok() != Some(1) {
                            defmt::println!("failed to write it all");
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    led.set_high();
}
