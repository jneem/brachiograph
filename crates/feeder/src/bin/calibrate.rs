use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

use anyhow::anyhow;
use clap::Parser;
use serialport::SerialPort;
use termion::{
    event::Key,
    input::{Keys, TermRead},
    raw::{IntoRawMode, RawTerminal},
};

struct Serial {
    write: Box<dyn SerialPort>,
    read: BufReader<Box<dyn SerialPort>>,
}

impl Serial {
    fn get_duties(&mut self) -> anyhow::Result<(u16, u16)> {
        while self.write.write(b"p")? != 1 {}
        self.write.flush()?;
        let mut buf = String::new();
        BufRead::read_line(&mut self.read, &mut buf)?;
        let mut nums = buf
            .trim()
            .split_ascii_whitespace()
            .map(|s| s.parse::<u16>());
        let sh = nums.next().ok_or_else(|| anyhow!("missing shoulder"))??;
        let el = nums.next().ok_or_else(|| anyhow!("missing elbow"))??;
        Ok((sh, el))
    }
}

fn read_num<R: std::io::Read, W: std::io::Write>(
    keys: &mut Keys<R>,
    raw: &mut RawTerminal<W>,
) -> anyhow::Result<Option<i16>> {
    let mut buf = String::new();

    write!(raw, "\r\nDegrees? ")?;
    raw.flush()?;
    while let Some(key) = keys.next().transpose()? {
        match key {
            Key::Char('\n') => {
                write!(raw, "\n")?;
                return Ok(buf.parse().ok());
            }
            Key::Backspace => {
                buf.pop();
                write!(raw, "{}\r", termion::clear::CurrentLine)?;
                write!(raw, "Degrees? {}", buf)?;
                raw.flush()?;
            }
            Key::Char(c) => {
                buf.push(c);
                write!(raw, "{}", c)?;
                raw.flush()?;
            }
            _ => {}
        }
    }

    Ok(None)
}

#[derive(Parser, Debug)]
struct Args {
    tty: String,

    #[clap(short)]
    output: PathBuf,
}

#[derive(Copy, Clone)]
struct CalibKind {
    shoulder: bool,
    increasing: bool,
}

impl std::fmt::Display for CalibKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let which = if self.shoulder { "shoulder" } else { "elbow" };
        let dir = if self.increasing { "inc" } else { "dec" };
        write!(f, "{}/{}", which, dir)
    }
}

#[derive(Debug, Default)]
struct Calib {
    shoulder_inc: BTreeMap<i16, u16>,
    shoulder_dec: BTreeMap<i16, u16>,
    elbow_inc: BTreeMap<i16, u16>,
    elbow_dec: BTreeMap<i16, u16>,
}

impl Calib {
    fn table(&mut self, kind: CalibKind) -> &mut BTreeMap<i16, u16> {
        match (kind.shoulder, kind.increasing) {
            (true, true) => &mut self.shoulder_inc,
            (true, false) => &mut self.shoulder_dec,
            (false, true) => &mut self.elbow_inc,
            (false, false) => &mut self.elbow_dec,
        }
    }

    fn print_one<W: Write>(
        &self,
        w: &mut W,
        name: &str,
        table: &BTreeMap<i16, u16>,
    ) -> anyhow::Result<()> {
        write!(w, "static {name}: &'static [(i16, u16)] = &[\n")?;
        for (deg, us) in table {
            write!(w, "\t({deg}, {us}),\n")?;
        }
        write!(w, "];\n")?;

        Ok(())
    }

    fn print<W: Write>(&self, mut w: W) -> anyhow::Result<()> {
        self.print_one(&mut w, "SHOULDER_INC", &self.shoulder_inc)?;
        self.print_one(&mut w, "SHOULDER_DEC", &self.shoulder_dec)?;
        self.print_one(&mut w, "ELBOW_INC", &self.elbow_inc)?;
        self.print_one(&mut w, "ELBOW_DEC", &self.elbow_dec)?;
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let serial = serialport::new(&args.tty, 9600)
        .timeout(std::time::Duration::from_secs(60))
        .open()?;
    let mut serial = Serial {
        read: BufReader::with_capacity(128, serial.try_clone().unwrap()),
        write: serial,
    };

    let mut kind = CalibKind {
        shoulder: true,
        increasing: true,
    };
    let mut calib = Calib::default();
    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let stdin = std::io::stdin();
    let stdin = stdin.lock();
    let mut raw = stdout.into_raw_mode()?;
    let mut keys = stdin.keys();

    write!(&mut raw, "{}\r[{}] ", termion::clear::CurrentLine, kind)?;
    raw.flush()?;

    while let Some(key) = keys.next().transpose()? {
        match key {
            Key::Char('q') => {
                write!(&mut raw, "{}\rGoodbye!\r\n", termion::clear::CurrentLine)?;
                break;
            }
            Key::Char('m') => {
                let deg = read_num(&mut keys, &mut raw)?;
                if let Some(deg) = deg {
                    let (sh, el) = serial.get_duties()?;
                    let duty = if kind.shoulder { sh } else { el };
                    calib.table(kind).insert(deg, sh);
                    write!(&mut raw, "added {} entry {} for {}°\r\n", kind, duty, deg)?;
                }
            }
            Key::Char('x') => {
                let deg = read_num(&mut keys, &mut raw)?;
                if let Some(deg) = deg {
                    calib.table(kind).remove(&deg);
                    write!(&mut raw, "deleted {} entry for {}°\r\n", kind, deg)?;
                }
            }
            Key::Char('p') => {
                // Print the current calibration to the screen.
                write!(&mut raw, "{}\r", termion::clear::CurrentLine)?;
                calib.print(&mut raw)?;
            }
            Key::Char('w') => {
                // Write the current calibration to a file.
                let out = std::fs::File::create(&args.output)?;
                calib.print(out)?;
            }
            Key::Char(c) => {
                if "dDfFjJkK".find(c).is_some() {
                    kind.shoulder = "jJkK".find(c).is_some();
                    kind.increasing = "kKfF".find(c).is_some();
                    serial.write.write(&[c as u8])?;
                    serial.write.flush()?;
                }
            }
            _ => {}
        }
        write!(&mut raw, "{}\r[{}] ", termion::clear::CurrentLine, kind)?;
        raw.flush()?;
    }

    Ok(())
}
