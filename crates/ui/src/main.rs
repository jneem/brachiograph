// TODO: save/load
// TODO: feedback and error messages

//#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    cell::RefCell,
    io::{BufRead, BufReader},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use brachiograph::Angle;
use brachiologo::Program;
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

struct Serial {
    write: Box<dyn SerialPort>,
    read: BufReader<Box<dyn SerialPort>>,
}

// Send a single op element to brachiograph, blocking if necessary.
fn send(serial: &mut Serial, op: Op) -> anyhow::Result<()> {
    log::debug!("{:?}", op);
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
        log::debug!("read {resp:?}");
        match resp.trim() {
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
        let serial = serial.map(|s| {
            log::info!(
                "opened serial port with flow control {:?}",
                s.flow_control()
            );
            Serial {
                read: BufReader::with_capacity(128, s.try_clone().unwrap()),
                write: s,
            }
        });
        Inner { port: serial }
    }
}

#[derive(Clone, Default)]
struct State {
    inner: Arc<RefCell<Inner>>,
}

impl State {
    fn do_exec(&self, code: &str) -> anyhow::Result<()> {
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

    fn exec(&self, code: &str) -> anyhow::Result<()> {
        if let Err(e) = self.do_exec(code) {
            self.inner.borrow_mut().port = None;
            Err(e)
        } else {
            Ok(())
        }
    }

    fn has_brachiograph(&self) -> bool {
        self.inner.borrow().port.is_some()
    }

    fn try_connect(&self) {
        *self.inner.borrow_mut() = Inner::default();
    }
}

#[derive(Debug)]
enum Op {
    PenUp,
    PenDown,
    MoveTo { x: i32, y: i32 },
}

fn interpret<'input>(code: &'input str) -> anyhow::Result<Vec<Op>> {
    let program: Program<'input> = Program::parse(code).map_err(|e| anyhow!("parse error: {e}"))?;
    let steps = program.exec().map_err(|e| anyhow!("interp error: {e}"))?;

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
            brachiologo::BuiltIn::Arc { degrees, radius } => {
                // Arc does not move the turtle or change the heading.
                let start = pos + Vec2::from_angle(angle.radians().to_num()) * radius;
                let (x, y) = clamp(start);
                ret.push(Op::PenUp);
                ret.push(Op::MoveTo { x, y });
                ret.push(Op::PenDown);
                for i in (0..=(degrees as i32)).step_by(10) {
                    // Arc goes clockwise
                    let angle = angle - Angle::from_degrees(i);
                    let p = pos + Vec2::from_angle(angle.radians().to_num()) * radius;
                    let (x, y) = clamp(p);
                    ret.push(Op::MoveTo { x, y });
                }
                let (x, y) = clamp(pos);
                ret.push(Op::PenUp);
                ret.push(Op::MoveTo { x, y });
                ret.push(Op::PenDown);
            }
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
    pretty_env_logger::init();

    let state = State::default();
    let mut file_menu = MenuBar::new();
    file_menu.add_native_item(MenuItem::Quit);

    /*
    let save = MenuItemAttributes::new("Save")
        .with_accelerators(&Accelerator::new(ModifiersState::CONTROL, KeyCode::KeyS));

    let open = MenuItemAttributes::new("Open")
        .with_accelerators(&Accelerator::new(ModifiersState::CONTROL, KeyCode::KeyO));
    file_menu.add_item(save);
    file_menu.add_item(open);
    */

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
    let has_brachiograph = cx.props.has_brachiograph();
    let button_text = if has_brachiograph {
        "Run!"
    } else {
        "Connect..."
    };
    let status = if has_brachiograph {
        String::from("")
    } else {
        String::from("(No brachiograph found)")
    };

    let flash = use_state(&cx, || true);
    let spn = if *flash.get() {
        rsx!(span {
            class: "bold-flash1",
            status
        })
    } else {
        rsx!(span {
            class: "bold-flash2",
            status
        })
    };

    cx.render(rsx! (
        style { include_str!("./style.css") }
        textarea {
            rows: 20,
            cols: 80,
            value: "{text}",
            spellcheck: false,
            autocomplete: false,
            oninput: move |ev| text.set(ev.value.clone()),
        }
        div {
            button {
                onclick: move |_| {
                    if has_brachiograph {
                        if let Err(e) = cx.props.exec(&text.get()) {
                            log::error!("error {e}");
                            flash.set(!*flash.get());
                        }
                    } else {
                        cx.props.try_connect();
                        cx.needs_update();
                        if !cx.props.has_brachiograph() {
                            flash.set(!*flash.get());
                        }

                        cx.needs_update();
                    }
                },
                button_text
            }

            spn
        }
    ))
}
