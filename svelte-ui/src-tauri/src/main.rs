#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use serde::Serialize;
use std::sync::{
    mpsc::{channel, Receiver, Sender},
    Arc, Mutex,
};
use tauri::{App, AppHandle, Manager};

use brachiograph_host::{Op, Serial};
use brachiologo::Program;

struct State {
    tx: Mutex<Sender<Cmd>>,
}

#[derive(Clone, Debug, Serialize)]
enum RunError {
    Connection,
    Code {
        start_line: u32,
        start_col: u32,
        len: u32,
    },
}

impl<'a> From<brachiologo::ParseError<'a>> for RunError {
    fn from(e: brachiologo::ParseError<'a>) -> Self {
        RunError::Code {
            start_line: e.input.location_line(),
            start_col: e.input.get_column() as u32,
            len: e.input.len() as u32,
        }
    }
}

impl<'a> From<brachiologo::Error<'a>> for RunError {
    fn from(e: brachiologo::Error<'a>) -> Self {
        RunError::Code {
            start_line: e.span().location_line(),
            start_col: e.span().get_column() as u32,
            len: e.span().len() as u32,
        }
    }
}

fn main() {
    pretty_env_logger::init();
    let (tx, rx) = channel();
    let state = State { tx: Mutex::new(tx) };

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle();
            std::thread::spawn(move || brachio_thread(handle, rx));
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![run, check_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

enum Cmd {
    Ping,
    Run(String),
}

#[derive(Clone, Debug, Serialize)]
enum Response {
    Ready,
    Missing,
}

fn brachio_thread(app: AppHandle, rx: Receiver<Cmd>) {
    let mut port = Serial::detect();

    while let Ok(msg) = rx.recv() {
        match msg {
            Cmd::Ping => {
                // TODO: actually send a ping along the connection
                if port.is_none() {
                    port = Serial::detect();
                }
                if port.is_some() {
                    app.emit_all("brachio-msg", Response::Ready).unwrap();
                } else {
                    app.emit_all("brachio-msg", Response::Missing).unwrap();
                }
            }
            Cmd::Run(s) => {
                if port.is_none() {
                    port = Serial::detect();
                }
                if let Some(p) = port.as_mut() {
                    if let Err(e) = try_run(&s, p) {
                        match e {
                            RunError::Connection => {
                                port = None;
                                app.emit_all("brachio-msg", Response::Missing).unwrap();
                            }
                            RunError::Code {
                                start_line,
                                start_col,
                                len,
                            } => {
                                println!("code error {e:?}");
                            }
                        }
                    }
                } else {
                    app.emit_all("brachio-msg", Response::Missing).unwrap();
                }
            }
        }
    }
}

fn try_run(code: &str, serial: &mut Serial) -> Result<(), RunError> {
    let prog = Program::parse(code)?;
    println!("got prog");
    let primitives = prog.exec()?;
    println!("got prims");
    let ops = brachiograph_host::interpret(&primitives);
    let rect = kurbo::Rect::new(-80.0, 50.0, 80.0, 130.0);
    let ops = ops.into_iter().map(|p| p.center_and_clamp(&rect));
    // TODO: add "init" and "finish" ops
    serial
        .send(Op::MoveTo { x: 0.0, y: 90.0 })
        .map_err(|_| RunError::Connection)?;
    serial.send(Op::PenDown).map_err(|_| RunError::Connection)?;
    for op in ops {
        serial.send(op).map_err(|_| RunError::Connection)?;
    }
    serial.send(Op::PenUp).map_err(|_| RunError::Connection)?;
    serial
        .send(Op::MoveTo { x: -80.0, y: 80.0 })
        .map_err(|_| RunError::Connection)?;

    Ok(())
}

#[tauri::command]
fn run(code: String, state: tauri::State<State>) {
    println!("running {code:?}");
    state.tx.lock().unwrap().send(Cmd::Run(code)).unwrap();
}

#[tauri::command]
fn check_status(state: tauri::State<State>) {
    println!("check status");
    state.tx.lock().unwrap().send(Cmd::Ping).unwrap();
}
