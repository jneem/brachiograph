#![no_main]
#![no_std]

use brachiograph_runner as _;

use brachiograph::{ServoPosition, SlowOp};
use ringbuffer::{
    ConstGenericRingBuffer as RingBuffer, RingBuffer as _, RingBufferExt, RingBufferWrite,
};
use stm32f1xx_hal::{device::TIM3, timer::PwmChannel};

const TICK_HZ: u32 = 100;

type Duration = fugit::TimerDurationU64<TICK_HZ>;

#[derive(Default)]
pub struct OpQueue {
    // TODO: would be sort of nice if we can make this big, but it overflows the stack. We can
    // probably shrink `Op` by a factor of 2 or more. It isn't a huge deal, though: we're unlikely
    // to process more than a handful of ops per second, so there's no need to queue up too many.
    queue: RingBuffer<SlowOp, 32>,
}

include!("../calibration_data.rs");

fn default_shoulder_config() -> brachiograph::pwm::Pwm {
    brachiograph::pwm::Pwm {
        inc: SHOULDER_INC.iter().copied().collect(),
        dec: SHOULDER_DEC.iter().copied().collect(),
    }
}

fn default_elbow_config() -> brachiograph::pwm::Pwm {
    brachiograph::pwm::Pwm {
        inc: ELBOW_INC.iter().copied().collect(),
        dec: ELBOW_DEC.iter().copied().collect(),
    }
}

impl OpQueue {
    fn enqueue(&mut self, op: SlowOp) -> Result<(), ()> {
        if self.queue.is_full() {
            Err(())
        } else {
            self.queue.push(op);
            Ok(())
        }
    }

    fn clear(&mut self) {
        self.queue.clear();
    }
}

pub struct Pwms {
    shoulder: PwmChannel<TIM3, 0>,
    elbow: PwmChannel<TIM3, 1>,
    pen: PwmChannel<TIM3, 2>,
}

impl Pwms {
    pub fn init(
        shoulder: PwmChannel<TIM3, 0>,
        elbow: PwmChannel<TIM3, 1>,
        pen: PwmChannel<TIM3, 2>,
        pos: ServoPosition,
    ) -> Pwms {
        let mut pwms = Pwms {
            shoulder,
            elbow,
            pen,
        };
        pwms.set_shoulder(pos.shoulder);
        pwms.set_elbow(pos.elbow);
        pwms.set_pen(pos.pen);
        pwms.shoulder.enable();
        pwms.elbow.enable();
        pwms.pen.enable();
        pwms
    }

    pub fn set_shoulder(&mut self, duty: u16) {
        self.shoulder.set_duty(duty);
    }

    pub fn set_elbow(&mut self, duty: u16) {
        self.elbow.set_duty(duty);
    }

    pub fn set_pen(&mut self, duty: u16) {
        self.pen.set_duty(duty);
    }
}

#[rtic::app(device = stm32f1xx_hal::pac, dispatchers = [SPI1])]
mod app {
    use super::{Duration, OpQueue, Pwms};
    use brachiograph::{geom, Brachiograph, FastOp, Op, Resp, SlowOp};
    use brachiograph_runner::serial::UsbSerial;
    use cortex_m::asm;
    use ringbuffer::{RingBufferExt, RingBufferRead};
    use stm32f1xx_hal::{
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use systick_monotonic::Systick;
    use usb_device::prelude::*;
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    #[monotonic(binds = SysTick, default = true)]
    type Mono = Systick<{ crate::TICK_HZ }>;

    #[shared]
    struct Shared {
        serial: UsbSerial,
        op_queue: OpQueue,
        state: Brachiograph,
        pwms: Pwms,
        _led: stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
    }

    #[local]
    struct Local {
        geom_config: geom::Config,
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
        let usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0xca6d, 0xba6d))
            .manufacturer("jneem")
            .product("Brachiograph Serial Interface")
            .serial_number("brachio-001")
            .device_class(USB_CLASS_CDC)
            .build();
        let serial = UsbSerial::new(usb_dev, serial);

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

        let mut state = Brachiograph::new(-8, 8);
        let geom_config = state.config().clone();
        let now = fugit::Instant::<u64, 1, 1_000_000>::from_ticks(0);
        let pwms = Pwms::init(shoulder, elbow, pen, state.update(now));
        tick::spawn_after(Duration::millis(20)).unwrap();

        (
            Shared {
                serial,
                _led: led,
                state,
                op_queue: OpQueue::default(),
                pwms,
            },
            Local { geom_config },
            init::Monotonics(mono),
        )
    }

    #[task(binds = USB_HP_CAN_TX, shared = [serial])]
    fn usb_tx(_cx: usb_tx::Context) {
        defmt::println!("can tx");
        // TODO: I haven't ever seen this get called...
        // Doc says "USB High Priority or CAN TX"
    }

    fn validate_slow_op(geom_config: &geom::Config, op: &SlowOp) -> bool {
        if let SlowOp::MoveTo(p) = &op {
            geom_config.coord_is_valid(p.x, p.y)
        } else {
            true
        }
    }

    fn handle_fast_op(
        op_queue: &mut OpQueue,
        state: &mut Brachiograph,
        serial: &mut UsbSerial,
        op: FastOp,
    ) {
        match op {
            FastOp::Cancel => {
                op_queue.clear();
                let _ = serial.send(Resp::Ack);
            }
            FastOp::Calibrate(joint, dir, calib) => state.change_calibration(joint, dir, calib),
        }
    }

    #[task(priority = 2, binds = USB_LP_CAN_RX0, shared = [serial, op_queue, state], local = [geom_config])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let mut serial = cx.shared.serial;
        let mut op_queue = cx.shared.op_queue;
        let mut state = cx.shared.state;
        let geom_config = cx.local.geom_config;
        (&mut serial, &mut op_queue, &mut state).lock(|serial, op_queue, state| {
            if !serial.poll() {
                return;
            }
            while let Some(op) = serial.read() {
                match op {
                    Op::Fast(op) => handle_fast_op(op_queue, state, serial, op),
                    Op::Slow(op) => {
                        if validate_slow_op(geom_config, &op) {
                            if op_queue.enqueue(op).is_err() {
                                let _ = serial.send(Resp::QueueFull);
                            } else {
                                let _ = serial.send(Resp::Ack);
                            }
                        } else {
                            // TODO: specify the error in the response
                            let _ = serial.send(Resp::Nack);
                            continue;
                        }
                    }
                }
            }
            serial.write();
        })
    }

    #[task(priority = 1, shared = [op_queue, state, pwms])]
    fn tick(cx: tick::Context) {
        let mut op_queue = cx.shared.op_queue;
        let mut state = cx.shared.state;
        let mut pwms = cx.shared.pwms;
        (&mut op_queue, &mut state, &mut pwms).lock(|op_queue, state, pwms| {
            let now = monotonics::now();
            // TODO: no better way to convert instants??
            let geom_now = fugit::Instant::<u64, 1, 1_000_000>::from_ticks(0)
                + now.duration_since_epoch().convert();
            let servos = state.update(geom_now);

            pwms.set_shoulder(servos.shoulder);
            pwms.set_elbow(servos.elbow);
            pwms.set_pen(servos.pen);

            if let Some(resting) = state.resting() {
                if let Some(op) = op_queue.queue.peek() {
                    match op {
                        SlowOp::PenUp => {
                            resting.pen_up(geom_now);
                            op_queue.queue.dequeue();
                        }
                        SlowOp::PenDown => {
                            resting.pen_down(geom_now);
                            op_queue.queue.dequeue();
                        }
                        SlowOp::MoveTo(point) => {
                            // TODO: error handling
                            if resting.move_to(geom_now, point.x, point.y).is_err() {
                                defmt::println!("failed to move");
                            }
                            op_queue.queue.dequeue();
                        }
                        SlowOp::ChangePosition(_delta) => {
                            // TODO: error handling
                            defmt::println!("must be in calibration mode");
                        }
                    }
                }
            } else if let Some(calibrating) = state.calibrating() {
                if let Some(SlowOp::ChangePosition(delta)) = op_queue.queue.peek() {
                    calibrating.delta(*delta);
                    op_queue.queue.dequeue();
                }
            }

            // TODO: can we have it idle if there's nothing to do? I haven't figured out how to
            // re-wake it if necessary, since `tick::spawn` panics if `tick` is already running
            // and I don't know how to *check* if it's running.
            tick::spawn_after(Duration::millis(20)).unwrap();
        })
    }
}
