use anyhow::bail;
use brachiograph::{Angle, Fixed, Op, Resp};
use brachiologo::TurtleCmd;
use kurbo::{Point, Rect, Vec2};
use std::io::{BufRead, BufReader};

use serialport::{SerialPort, SerialPortType};

const VENDOR_ID: u16 = 0xca6d;
const PRODUCT_ID: u16 = 0xba6d;

fn detect_port() -> Option<Box<dyn SerialPort>> {
    let ports = serialport::available_ports().ok()?;
    for port in ports {
        let SerialPortType::UsbPort(usb_info) = port.port_type else {
            continue;
        };
        log::debug!("found usbserial port {usb_info:?}");

        if usb_info.vid == VENDOR_ID && usb_info.pid == PRODUCT_ID {
            match serialport::new(&port.port_name, 9600)
                // I'm not completely sure what the implications of this timeout value are,
                // but on linux read_line returns immediately, while on windows it doesn't
                // return until the timeout is up. So keep the timeout small.
                .timeout(std::time::Duration::from_millis(50))
                .open()
            {
                Ok(port) => {
                    return Some(port);
                }
                Err(e) => {
                    log::warn!("failed to open port '{}': {}", port.port_name, e);
                }
            }
        } else {
            log::info!("skipping usb-serial {:?}", usb_info);
        }
    }

    None
}

/*
// TODO: this should go in the brachiograph crate and be used in the runner
#[derive(Debug)]
pub enum Op {
    PenUp,
    PenDown,
    MoveTo { x: f64, y: f64 },
}

impl Op {
    /// Change coordinates of this op so that the original origin is in the center of the rect,
    /// and clamp it to remain inside the rect.
    pub fn center_and_clamp(self, rect: &Rect) -> Self {
        let clamp = |x: f64, y: f64| Op::MoveTo {
            x: (x + rect.center().x).clamp(rect.min_x(), rect.max_x()),
            y: (y + rect.center().y).clamp(rect.min_y(), rect.max_y()),
        };
        match self {
            Op::MoveTo { x, y } => clamp(x, y),
            other => other,
        }
    }
}
*/

pub struct Serial {
    write: Box<dyn SerialPort>,
    read: BufReader<Box<dyn SerialPort>>,
}

impl Serial {
    pub fn detect() -> Option<Self> {
        detect_port().map(|s| Serial {
            read: BufReader::with_capacity(128, s.try_clone().unwrap()),
            write: s,
        })
    }

    pub fn name(&self) -> Option<String> {
        self.write.name()
    }

    pub fn send(&mut self, op: Op) -> anyhow::Result<Resp> {
        println!("{:?}", op);
        loop {
            let msg = postcard::to_stdvec_cobs(&op)?;
            self.write.write_all(&msg)?;

            let mut read = self.read.fill_buf()?.to_vec();
            let (msg, remaining) = postcard::take_from_bytes_cobs(&mut read)?;
            let remaining_len = remaining.len();
            drop(remaining);
            self.read.consume(read.len() - remaining_len);
            match dbg!(msg) {
                Resp::QueueFull => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
                Resp::Nack => bail!("error (TODO: better message)"),
                other => return Ok(other),
            }
        }
    }
}

pub fn interpret<'input>(steps: &[TurtleCmd]) -> Vec<Op> {
    let mut pos = Point::ORIGIN;
    let mut angle = Angle::from_degrees(90);
    let mut ret = Vec::new();

    let mv = |pt: Point| {
        Op::MoveTo(brachiograph::Point {
            x: Fixed::from_num(pt.x),
            y: Fixed::from_num(pt.y),
        })
    };

    for step in steps.iter().copied() {
        match step {
            brachiologo::TurtleCmd::Arc { degrees, radius } => {
                // Arc does not move the turtle or change the heading.
                let start = pos + Vec2::from_angle(angle.radians().to_num()) * radius;
                ret.push(Op::PenUp);
                ret.push(mv(start));
                ret.push(Op::PenDown);
                for i in (0..=(degrees as i32)).step_by(10) {
                    // Arc goes clockwise
                    let angle = angle - Angle::from_degrees(i);
                    let p = pos + Vec2::from_angle(angle.radians().to_num()) * radius;
                    ret.push(mv(p));
                }
                ret.push(Op::PenUp);
                ret.push(mv(pos));
                ret.push(Op::PenDown);
            }
            brachiologo::TurtleCmd::Forward(dist) => {
                pos += Vec2::from_angle(angle.radians().to_num()) * dist;
                ret.push(mv(pos));
            }
            brachiologo::TurtleCmd::Back(dist) => {
                pos -= Vec2::from_angle(angle.radians().to_num()) * dist;
                ret.push(mv(pos));
            }
            brachiologo::TurtleCmd::Left(ang) => {
                angle += Angle::from_degrees(ang);
            }
            brachiologo::TurtleCmd::Right(ang) => {
                angle += Angle::from_degrees(ang);
            }
            brachiologo::TurtleCmd::PenUp => {
                ret.push(Op::PenUp);
            }
            brachiologo::TurtleCmd::PenDown => {
                ret.push(Op::PenDown);
            }
        }
    }

    ret
}
