// TODO: factor out code that's duplicated from feeder
// TODO: save/load
// TODO: feedback and error messages

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    cell::RefCell,
    io::{BufRead, BufReader},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use brachiograph::Angle;
use dioxus::prelude::*;
use dioxus_desktop::{
    tao::menu::{MenuBar, MenuItem},
    Config, WindowBuilder,
};
use kurbo::{Point, Rect, Vec2};
use serialport::{SerialPort, SerialPortType};

const VENDOR_ID: u16 = 0xca6d;
const PRODUCT_ID: u16 = 0xba6d;

fn detect_port() -> Option<Box<dyn SerialPort>> {
    let ports = serialport::available_ports().ok()?;
    for port in ports {
        let SerialPortType::UsbPort(usb_info) = port.port_type else {
            continue;
        };

        if usb_info.vid == VENDOR_ID && usb_info.pid == PRODUCT_ID {
            match serialport::new(&port.port_name, 9600)
                .timeout(std::time::Duration::from_secs(60))
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

struct Serial {
    write: Box<dyn SerialPort>,
    read: BufReader<Box<dyn SerialPort>>,
}

// Send a single op element to brachiograph, blocking if necessary.
fn send(serial: &mut Serial, op: Op) -> anyhow::Result<()> {
    println!("{:?}", op);
    let mut resp = String::new();
    loop {
        match op {
            Op::PenDown => {
                writeln!(&mut serial.write, "pendown")?;
            }
            Op::PenUp => {
                writeln!(&mut serial.write, "penup")?;
            }
            Op::MoveTo { x, y } => {
                writeln!(&mut serial.write, "moveto {x} {y}")?;
            }
        }

        resp.clear();
        serial.read.read_line(&mut resp)?;
        match dbg!(resp.trim()) {
            "ack" => break,
            "queue full" => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            resp => bail!("Unexpected response: {resp:?}"),
        }
    }

    Ok(())
}

struct Inner {
    port: Option<Serial>,
}

impl Default for Inner {
    fn default() -> Inner {
        let serial = detect_port();
        let serial = serial.map(|s| Serial {
            read: BufReader::with_capacity(128, s.try_clone().unwrap()),
            write: s,
        });
        Inner { port: serial }
    }
}

#[derive(Clone, Default)]
struct State {
    inner: Arc<RefCell<Inner>>,
}

impl State {
    fn exec(&self, code: &str) -> anyhow::Result<()> {
        let ops = interpret(code)?;
        let mut serial = self.inner.borrow_mut();
        if let Some(serial) = &mut serial.port {
            send(serial, Op::MoveTo { x: 0, y: 90 })?;
            send(serial, Op::PenDown)?;
            for op in ops {
                send(serial, op)?;
            }
            send(serial, Op::PenUp)?;
            send(serial, Op::MoveTo { x: -80, y: 80 })?;
        }

        Ok(())
    }
}

#[derive(Debug)]
enum Op {
    PenUp,
    PenDown,
    MoveTo { x: i32, y: i32 },
}

fn interpret(code: &str) -> anyhow::Result<Vec<Op>> {
    let program = brachiologo::program(code)
        .map_err(|e| anyhow!("parse error: {e}"))?
        .1;
    let mut scope = brachiologo::Scope::default();
    let mut steps = Vec::new();
    scope.exec_block(&mut steps, &program)?;

    let rect = Rect::new(-80., 50., 80., 130.);
    let mut pos = rect.center();
    let mut angle = Angle::from_degrees(90);
    let mut ret = Vec::new();

    let clamp = |pt: Point| {
        (
            pt.x.clamp(rect.min_x(), rect.max_x()).round() as i32,
            pt.y.clamp(rect.min_y(), rect.max_y()).round() as i32,
        )
    };

    for step in steps {
        match step {
            brachiologo::BuiltIn::Forward(dist) => {
                pos += Vec2::from_angle(angle.radians().to_num()) * dist;
                let (x, y) = clamp(pos);
                ret.push(Op::MoveTo { x, y });
            }
            brachiologo::BuiltIn::Back(dist) => {
                pos -= Vec2::from_angle(angle.radians().to_num()) * dist;
                let (x, y) = clamp(pos);
                ret.push(Op::MoveTo { x, y });
            }
            brachiologo::BuiltIn::Left(ang) => {
                angle += Angle::from_degrees(ang);
            }
            brachiologo::BuiltIn::Right(ang) => {
                angle += Angle::from_degrees(ang);
            }
            brachiologo::BuiltIn::ClearScreen => {}
            brachiologo::BuiltIn::PenUp => {
                ret.push(Op::PenUp);
            }
            brachiologo::BuiltIn::PenDown => {
                ret.push(Op::PenDown);
            }
        }
    }

    Ok(ret)
}

fn main() {
    let state = State::default();
    let mut file_menu = MenuBar::new();
    file_menu.add_native_item(MenuItem::Quit);
    let mut menu = MenuBar::new();
    menu.add_submenu("File", true, file_menu);
    let config = Config::new().with_window(
        WindowBuilder::new()
            .with_title("Brachiologo")
            .with_decorations(true)
            .with_closable(true)
            .with_menu(menu),
    );

    dioxus_desktop::launch_with_props(app, state, config);
}

fn app(cx: Scope<State>) -> Element {
    let text = use_state(&cx, || String::from(""));
    let name = cx
        .props
        .inner
        .borrow()
        .port
        .as_ref()
        .and_then(|p| p.write.name());
    let port_msg = if let Some(name) = name {
        format!("Brachiograph on port {}", name)
    } else {
        String::from("No brachiograph detected")
    };

    cx.render(rsx! (
        h3 { port_msg }
        textarea {
            rows: 20,
            cols: 80,
            value: "{text}",
            oninput: move |ev| text.set(ev.value.clone()),
        }
        div {
            button {
                onclick: move |_| {
                    println!("click {:?}", text.get());
                    if let Err(e) = cx.props.exec(&text.get()) {
                        log::error!("error {e}");
                    }
                },
                "Run!"
            }
        }
    ))
}
