#![no_main]
#![no_std]

use brachiograph::{Angle, Op, OpParseErr};
use fixed_macro::fixed;
use ringbuffer::{ConstGenericRingBuffer as RingBuffer, RingBuffer as _, RingBufferWrite};
use stm32f1xx_hal::{device::TIM3, timer::PwmChannel};
use usb_device::prelude::*;
use usbd_serial::SerialPort; // global logger + panicking-behavior + memory layout

type Fixed = fixed::types::I20F12;
type Duration = fugit::TimerDurationU64<100>;

#[derive(Default)]
pub struct OpQueue {
    queue: RingBuffer<Op, 4>,
}

// TODO: invent a data format for this
fn shoulder_config() -> brachiograph::pwm::Pwm {
    use brachiograph::pwm::CalibrationEntry;
    brachiograph::pwm::Pwm {
        calib: [
            CalibrationEntry {
                degrees: -37,
                duty_ratio: fixed!(0.11871: U0F16),
            },
            CalibrationEntry {
                degrees: -30,
                duty_ratio: fixed!(0.113969: U0F16),
            },
            CalibrationEntry {
                degrees: -15,
                duty_ratio: fixed!(0.104492: U0F16),
            },
            CalibrationEntry {
                degrees: 0,
                duty_ratio: fixed!(0.095458: U0F16),
            },
            CalibrationEntry {
                degrees: 15,
                duty_ratio: fixed!(0.087402: U0F16),
            },
            CalibrationEntry {
                degrees: 30,
                duty_ratio: fixed!(0.078857: U0F16),
            },
            CalibrationEntry {
                degrees: 45,
                duty_ratio: fixed!(0.071289: U0F16),
            },
            CalibrationEntry {
                degrees: 60,
                duty_ratio: fixed!(0.063964: U0F16),
            },
            CalibrationEntry {
                degrees: 75,
                duty_ratio: fixed!(0.056884: U0F16),
            },
            CalibrationEntry {
                degrees: 90,
                duty_ratio: fixed!(0.049804: U0F16),
            },
            CalibrationEntry {
                degrees: 105,
                duty_ratio: fixed!(0.041992: U0F16),
            },
            CalibrationEntry {
                degrees: 120,
                duty_ratio: fixed!(0.033935: U0F16),
            },
        ]
        .into_iter()
        .collect(),
    }
}

fn elbow_config() -> brachiograph::pwm::Pwm {
    use brachiograph::pwm::CalibrationEntry;
    brachiograph::pwm::Pwm {
        calib: [
            CalibrationEntry {
                degrees: -90,
                duty_ratio: fixed!(0.114014: U0F16),
            },
            CalibrationEntry {
                degrees: -75,
                duty_ratio: fixed!(0.105957: U0F16),
            },
            CalibrationEntry {
                degrees: -60,
                duty_ratio: fixed!(0.105957: U0F16),
            },
            CalibrationEntry {
                degrees: -15,
                duty_ratio: fixed!(0.081787: U0F16),
            },
            CalibrationEntry {
                degrees: -45,
                duty_ratio: fixed!(0.097900: U0F16),
            },
            CalibrationEntry {
                degrees: -30,
                duty_ratio: fixed!(0.089843: U0F16),
            },
            CalibrationEntry {
                degrees: 0,
                duty_ratio: fixed!(0.073974: U0F16),
            },
            CalibrationEntry {
                degrees: 15,
                duty_ratio: fixed!(0.065917: U0F16),
            },
            CalibrationEntry {
                degrees: 30,
                duty_ratio: fixed!(0.058349: U0F16),
            },
            CalibrationEntry {
                degrees: 45,
                duty_ratio: fixed!(0.051269: U0F16),
            },
            CalibrationEntry {
                degrees: 60,
                duty_ratio: fixed!(0.043457: U0F16),
            },
            CalibrationEntry {
                degrees: 75,
                duty_ratio: fixed!(0.035400: U0F16),
            },
        ]
        .into_iter()
        .collect(),
    }
}

impl OpQueue {
    fn enqueue(&mut self, op: Op) -> Result<(), ()> {
        if self.queue.is_full() {
            Err(())
        } else {
            self.queue.push(op);
            app::tick::spawn().unwrap();
            Ok(())
        }
    }
}

pub struct CmdBuf {
    // TODO: use FixedVec or something.
    buf: [u8; 128],
    end: usize,
}

impl Default for CmdBuf {
    fn default() -> CmdBuf {
        CmdBuf {
            buf: [0; 128],
            end: 0,
        }
    }
}

impl CmdBuf {
    fn parse(&mut self) -> Option<Result<Op, OpParseErr>> {
        defmt::println!("parsing {:?}", self.buf[..self.end]);
        if let Some(idx) = self.buf[..self.end].iter().position(|&c| c == b'\n') {
            // FIXME: unwrap
            let buf = core::str::from_utf8(&self.buf[..idx]).unwrap();
            let res = buf.parse();
            defmt::println!("shifting back by {}", idx);
            for i in (idx + 1)..self.end {
                self.buf[i - idx - 1] = self.buf[i];
            }
            self.end -= idx + 1;
            Some(res)
        } else {
            None
        }
    }

    fn buf(&mut self) -> &mut [u8] {
        &mut self.buf[self.end..]
    }

    fn extend_by(&mut self, count: usize) {
        assert!(count.saturating_add(self.end) <= self.buf.len());
        self.end += count;
    }

    fn clear(&mut self) {
        self.end = 0;
    }
}

fn get_max_duty<const C: u8>(pwm: &PwmChannel<TIM3, C>) -> Fixed {
    let max = pwm.get_max_duty();
    // 2.5% duty means 0 degrees, 12.5% means 180 degrees, and everything else is linearly interpolated.
    // max duty of zero means max duty of 2^16.
    if max == 0 {
        Fixed::from_num(1i32 << 16)
    } else {
        Fixed::from_num(max)
    }
}

fn set_angle<const C: u8>(
    pwm: &mut PwmChannel<TIM3, C>,
    cfg: &brachiograph::pwm::Pwm,
    angle: Angle,
) {
    let duty_ratio = Fixed::from_num(cfg.duty(angle).unwrap()); // FIXME
    let max = get_max_duty(pwm);
    let duty = max * duty_ratio;
    defmt::println!(
        "setting duty {} (of {}) for angle {}",
        duty.to_num::<u32>(),
        pwm.get_max_duty(),
        angle.degrees().to_num::<i32>()
    );
    pwm.set_duty(duty.to_num());
}

pub struct Pwms {
    shoulder: PwmChannel<TIM3, 0>,
    elbow: PwmChannel<TIM3, 1>,
    pen: PwmChannel<TIM3, 2>,
    shoulder_cfg: brachiograph::pwm::Pwm,
    elbow_cfg: brachiograph::pwm::Pwm,
    pen_cfg: brachiograph::pwm::TogglePwm,
}

impl Pwms {
    pub fn set_shoulder(&mut self, angle: Angle) {
        set_angle(&mut self.shoulder, &self.shoulder_cfg, angle)
    }

    pub fn set_elbow(&mut self, angle: Angle) {
        set_angle(&mut self.elbow, &self.elbow_cfg, angle)
    }

    pub fn pen_down(&mut self, down: bool) {
        let duty_ratio = Fixed::from_num(if down {
            self.pen_cfg.on
        } else {
            self.pen_cfg.off
        });
        let duty = get_max_duty(&self.pen) * duty_ratio;
        self.pen.set_duty(duty.to_num());
    }
}

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [SPI1])]
mod app {
    use super::{CmdBuf, Duration, OpQueue, Pwms};
    use brachiograph::{Brachiograph, Op};
    use cortex_m::asm;
    use ringbuffer::RingBufferRead;
    use stm32f1xx_hal::{
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use systick_monotonic::Systick;
    use usb_device::prelude::*;
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    // TODO: what is the SYST frequency?
    #[monotonic(binds = SysTick, default = true)]
    type Mono = Systick<100>;

    #[shared]
    struct Shared {
        usb_dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
        op_queue: OpQueue,
        state: Brachiograph,
        led: stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
    }

    #[local]
    struct Local {
        cmd_buf: CmdBuf,
        pwms: Pwms,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Hello, world!");

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
        defmt::println!("hclk rate: {:?}", clocks.hclk().to_Hz());
        let mono = Systick::new(cx.core.SYST, clocks.hclk().to_Hz());

        let mut gpioa = cx.device.GPIOA.split();
        let mut gpiob = cx.device.GPIOB.split();

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
        let mut timer = cx.device.TIM1.counter_ms(&clocks);
        timer.start(1.secs()).unwrap();
        timer.listen(stm32f1xx_hal::timer::Event::Update);

        let shoulder_pin = gpioa.pa6.into_alternate_push_pull(&mut gpioa.crl);
        let elbow_pin = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
        let pen_pin = gpiob.pb0.into_alternate_push_pull(&mut gpiob.crl);
        let (shoulder, elbow, pen) = cx
            .device
            .TIM3
            .pwm_us::<stm32f1xx_hal::timer::Tim3NoRemap, _, _>(
                (shoulder_pin, elbow_pin, pen_pin),
                &mut afio.mapr,
                fugit::Duration::<u32, 1, 1_000_000>::millis(20),
                &clocks,
            )
            .split();
        let shoulder_cfg = super::shoulder_config();
        let elbow_cfg = super::elbow_config();
        let pen_cfg = brachiograph::pwm::TogglePwm::pen();
        let mut pwms = super::Pwms {
            shoulder,
            elbow,
            pen,
            shoulder_cfg,
            elbow_cfg,
            pen_cfg,
        };
        let state = Brachiograph::new(0, 8);
        let init_angles = state.angles();
        pwms.set_shoulder(init_angles.shoulder);
        pwms.set_elbow(init_angles.elbow);
        pwms.pen_down(false);
        pwms.shoulder.enable();
        pwms.elbow.enable();
        pwms.pen.enable();

        (
            Shared {
                usb_dev,
                serial,
                led,
                state,
                op_queue: OpQueue::default(),
            },
            Local {
                cmd_buf: CmdBuf::default(),
                pwms,
            },
            init::Monotonics(mono),
        )
    }

    #[task(binds = USB_HP_CAN_TX, shared = [usb_dev, serial, led])]
    fn usb_tx(cx: usb_tx::Context) {
        let mut usb_dev = cx.shared.usb_dev;
        let mut serial = cx.shared.serial;
        let mut led = cx.shared.led;
        (&mut usb_dev, &mut serial, &mut led)
            .lock(|usb_dev, serial, led| super::usb_poll(usb_dev, serial, led))
    }

    #[task(binds = USB_LP_CAN_RX0, shared = [usb_dev, serial, op_queue, led], local = [cmd_buf])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let mut usb_dev = cx.shared.usb_dev;
        let mut serial = cx.shared.serial;
        let mut op_queue = cx.shared.op_queue;
        let mut led = cx.shared.led;
        let cmd_buf = cx.local.cmd_buf;
        (&mut usb_dev, &mut serial, &mut op_queue, &mut led).lock(
            |usb_dev, serial, op_queue, led| {
                super::usb_read(usb_dev, serial, cmd_buf, op_queue, led)
            },
        )
    }

    #[task(shared = [op_queue, state], local = [pwms])]
    fn tick(cx: tick::Context) {
        let mut op_queue = cx.shared.op_queue;
        let mut state = cx.shared.state;
        let pwms = cx.local.pwms;
        (&mut op_queue, &mut state).lock(|op_queue, state| {
            let now = monotonics::now();
            // TODO: no better way to convert instants??
            let geom_now = fugit::Instant::<u64, 1, 1_000_000>::from_ticks(0)
                + now.duration_since_epoch().convert();
            let geom = state.update(geom_now);
            pwms.set_shoulder(geom.shoulder);
            pwms.set_elbow(geom.elbow);

            if let Some(mut resting) = state.resting() {
                if let Some(op) = op_queue.queue.dequeue() {
                    match op {
                        Op::PenUp => {
                            resting.pen_up();
                            pwms.pen_down(false);
                        }
                        Op::PenDown => {
                            resting.pen_down();
                            pwms.pen_down(true);
                        }
                        Op::MoveTo(point) => {
                            // TODO: error handling
                            if resting.move_to(geom_now, point.x, point.y).is_err() {
                                defmt::println!("failed to move");
                            }
                        }
                    }
                }
            }
            if state.resting().is_none() {
                tick::spawn_after(Duration::millis(10)).unwrap();
            }
        })
    }
}

fn usb_read<B: usb_device::bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    serial: &mut SerialPort<'static, B>,
    cmd_buf: &mut CmdBuf,
    op_queue: &mut OpQueue,
    led: &mut stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
) {
    if !usb_dev.poll(&mut [serial]) {
        return;
    }
    if cmd_buf.buf().is_empty() {
        defmt::println!("ran out of buffer, clearing it");
        cmd_buf.clear();
    }
    let buf = cmd_buf.buf();

    led.set_low();
    match serial.read(buf) {
        Ok(count) if count > 0 => {
            defmt::println!("{}", &buf[0..count]);
            cmd_buf.extend_by(count);

            if let Some(cmd) = cmd_buf.parse() {
                match cmd {
                    Ok(cmd) => {
                        defmt::println!("{:?}", cmd);
                        if op_queue.enqueue(cmd).is_err() {
                            // FIXME: unwrap
                            serial.write(b"busy\n").unwrap();
                        }
                        // TODO: write back
                    }
                    Err(e) => {
                        defmt::println!("Error: {:?}", e);
                        // TODO: write back
                    }
                }
            }
        }
        _ => {}
    }
    led.set_high();
}

fn usb_poll<B: usb_device::bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    serial: &mut SerialPort<'static, B>,
    led: &mut stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
) {
    if !usb_dev.poll(&mut [serial]) {
        return;
    }
    let mut buf = [0u8; 64];
    led.set_low();
    match serial.read(&mut buf) {
        Ok(count) if count > 0 => {
            for c in buf[0..count].iter_mut() {
                *c = c.to_ascii_uppercase();
            }
            defmt::println!("{}", &buf[0..count]);
            serial.write(&buf[0..count]).ok();
        }
        _ => {}
    }
    led.set_high();
}

use defmt_rtt as _; // global logger

// TODO(5) adjust HAL import
use stm32f1xx_hal as _; // memory layout

use panic_probe as _;

// same panicking *behavior* as `panic-probe` but doesn't print a panic message
// this prevents the panic message being printed *twice* when `defmt::panic` is invoked
#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}

// defmt-test 0.3.0 has the limitation that this `#[tests]` attribute can only be used
// once within a crate. the module can be in any file but there can only be at most
// one `#[tests]` module in this library crate
#[cfg(test)]
#[defmt_test::tests]
mod unit_tests {
    use defmt::assert;

    #[test]
    fn it_works() {
        assert!(true)
    }
}
