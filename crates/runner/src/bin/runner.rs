#![no_main]
#![no_std]

use brachiograph_runner as _;

use brachiograph::{Angle, Op};
use fixed_macro::fixed;
use ringbuffer::{ConstGenericRingBuffer as RingBuffer, RingBuffer as _, RingBufferWrite};
use stm32f1xx_hal::{device::TIM3, timer::PwmChannel};

type Fixed = fixed::types::I20F12;
type Duration = fugit::TimerDurationU64<50>;

#[derive(Default)]
pub struct OpQueue {
    // TODO: would be nice if we can make this big, but it overflows the stack. How to store it elsewhere?
    queue: RingBuffer<Op, 32>,
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
            //defmt::println!("push op {}", op);
            self.queue.push(op);
            if self.queue.len() == 1 {
                app::tick::spawn().unwrap();
            }
            Ok(())
        }
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
    use super::{Duration, OpQueue, Pwms};
    use brachiograph::{geom, Brachiograph, Op};
    use brachiograph_runner::serial::{Status, UsbSerial};
    use cortex_m::asm;
    use ringbuffer::{RingBuffer, RingBufferRead};
    use stm32f1xx_hal::{
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use systick_monotonic::Systick;
    use usb_device::prelude::*;
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    // TODO: what is the SYST frequency?
    #[monotonic(binds = SysTick, default = true)]
    type Mono = Systick<50>;

    #[shared]
    struct Shared {
        serial: UsbSerial<Op, Status>,
        op_queue: OpQueue,
        state: Brachiograph,
        _led: stm32f1xx_hal::gpio::Pin<'A', 1, stm32f1xx_hal::gpio::Output>,
    }

    #[local]
    struct Local {
        pwms: Pwms,
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
        let usb_dev = UsbDeviceBuilder::new(usb_bus, UsbVidPid(0x16c0, 0x27dd))
            .manufacturer("Cam Bam")
            .product("Bam")
            .serial_number("TEST")
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

        let geom_config = state.config().clone();

        (
            Shared {
                serial,
                _led: led,
                state,
                op_queue: OpQueue::default(),
            },
            Local { pwms, geom_config },
            init::Monotonics(mono),
        )
    }

    #[task(binds = USB_HP_CAN_TX, shared = [serial])]
    fn usb_tx(_cx: usb_tx::Context) {
        // TODO: I haven't ever seen this get called...
        // Doc says "USB High Priority or CAN TX"
    }

    #[task(binds = USB_LP_CAN_RX0, shared = [serial, op_queue], local = [geom_config])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let mut serial = cx.shared.serial;
        let mut op_queue = cx.shared.op_queue;
        let geom_config = cx.local.geom_config;
        (&mut serial, &mut op_queue).lock(|serial, op_queue| {
            if !serial.poll() {
                return;
            }
            while let Some(op) = serial.read() {
                if let Op::MoveTo(p) = &op {
                    if !geom_config.coord_is_valid(p.x, p.y) {
                        let _ = serial.send(Status::Nack);
                        continue;
                    }
                }

                if op_queue.enqueue(op).is_err() {
                    let _ = serial.send(Status::QueueFull);
                } else {
                    let _ = serial.send(Status::Ack);
                }
            }
            serial.write();
        })
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
                    //defmt::println!("popped op {}", op);
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
            if state.resting().is_none() || !op_queue.queue.is_empty() {
                tick::spawn_after(Duration::millis(10)).unwrap();
            }
        })
    }
}
