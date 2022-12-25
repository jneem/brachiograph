#![no_main]
#![no_std]

use brachiograph as _;
use brachiograph_protocol::{Angle, Op, OpParseErr};
use ringbuffer::{ConstGenericRingBuffer as RingBuffer, RingBuffer as _, RingBufferWrite};
use stm32f1xx_hal::{
    device::{TIM2, TIM3},
    gpio::{Alternate, Pin, PA6, PA7, PB0},
    timer::{Ch, PwmChannel, PwmHz, Tim3NoRemap},
};
use usb_device::prelude::*;
use usbd_serial::SerialPort; // global logger + panicking-behavior + memory layout

type Fixed = fixed::types::I20F12;
type Instant = fugit::TimerInstantU64<100>;
type Duration = fugit::TimerDurationU64<100>;

#[derive(Default, Clone)]
pub struct BrachiographState {
    shoulder: Angle,
    elbow: Angle,
    pen_down: bool,
}

pub struct OpInProgress {
    start: Instant,
    start_state: BrachiographState,
    op: Op,
}

#[derive(Default)]
pub struct OpQueue {
    queue: RingBuffer<Op, 4>,
    in_progress: Option<OpInProgress>,
}

impl OpQueue {
    fn enqueue(&mut self, op: Op) -> Result<(), ()> {
        if self.queue.is_full() {
            Err(())
        } else {
            self.queue.push(op);
            if self.in_progress.is_none() {
                app::tick::spawn().unwrap();
            }
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

fn set_angle<const C: u8>(pwm: &mut PwmChannel<TIM3, C>, angle: Fixed) {
    let max = pwm.get_max_duty();
    // 10% duty means 0 degrees, 20% means 180 degrees, and everything else is linearly interpolated.
    // max duty of zero means max duty of 2^16.
    let max: Fixed = if max == 0 {
        Fixed::from_num(1i32 << 16)
    } else {
        max.into()
    };
    // This should be ensured elsewhere, but just in case.
    let angle = angle.clamp(20u8.into(), 160u8.into());
    let ratio = angle / 180;
    let duty = (ratio + Fixed::from_num(1)) * max / 10;
    defmt::println!(
        "setting duty {} (of {}) for angle {}",
        duty.to_num::<u32>(),
        pwm.get_max_duty(),
        angle.to_num::<u32>()
    );
    pwm.set_duty(duty.to_num());
}

pub struct Pwms {
    shoulder: PwmChannel<TIM3, 0>,
    elbow: PwmChannel<TIM3, 1>,
    pen: PwmChannel<TIM3, 2>,
    /*
    pwms: PwmHz<
        TIM3,
        Tim3NoRemap,
        (Ch<0>, Ch<1>, Ch<2>),
        (PA6<Alternate>, PA7<Alternate>, PB0<Alternate>),
    >,
    */
}

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [SPI1])]
mod app {
    use super::{BrachiographState, CmdBuf, Duration, Fixed, OpInProgress, OpQueue, Pwms};
    use brachiograph_protocol::Op;
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
        state: BrachiographState,
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
            .pwm_hz::<stm32f1xx_hal::timer::Tim3NoRemap, _, _>(
                (shoulder_pin, elbow_pin, pen_pin),
                &mut afio.mapr,
                50.Hz(),
                &clocks,
            )
            .split();
        let pwms = super::Pwms {
            shoulder,
            elbow,
            pen,
        };

        (
            Shared {
                usb_dev,
                serial,
                led,
                state: BrachiographState::default(),
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
            if let Some(in_progress) = &op_queue.in_progress {
                let done = match &in_progress.op {
                    // TODO: should add implicit delays to penup and pendown
                    Op::PenUp => {
                        // TODO: set angle
                        state.pen_down = false;
                        true
                    }
                    Op::PenDown => {
                        // TODO: set angle
                        state.pen_down = true;
                        true
                    }
                    Op::SetAngles(set_angles) => {
                        let duration = now.checked_duration_since(in_progress.start).unwrap();
                        let total: Fixed = set_angles.delay.to_millis().into();
                        let ratio = if total > 0 {
                            let actual: Fixed = (duration.to_millis() as u16).into();
                            (actual / total).min(1u8.into())
                        } else {
                            Fixed::from_num(1)
                        };
                        let shoulder = in_progress
                            .start_state
                            .shoulder
                            .interpolate(set_angles.shoulder, ratio);
                        let elbow = in_progress
                            .start_state
                            .shoulder
                            .interpolate(set_angles.elbow, ratio);
                        state.shoulder = shoulder;
                        state.elbow = elbow;
                        super::set_angle(&mut pwms.shoulder, shoulder.degrees());
                        super::set_angle(&mut pwms.elbow, elbow.degrees());
                        defmt::println!("shoulder angle: {:?}", shoulder);
                        ratio >= 1
                    }
                };
                if done {
                    defmt::println!("done with op {:?}", in_progress.op);
                    op_queue.in_progress = None;
                }
            }
            if op_queue.in_progress.is_none() {
                if let Some(op) = op_queue.queue.dequeue() {
                    op_queue.in_progress = Some(OpInProgress {
                        start: now,
                        op,
                        start_state: state.clone(),
                    });
                }
            }
            if op_queue.in_progress.is_some() {
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
