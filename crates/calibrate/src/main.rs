use std::{io::Write, path::PathBuf};

use anyhow::{anyhow, bail};
use brachiograph::{Direction, Joint, Op, Resp, ServoPositionDelta};
use brachiograph_host::Serial;
use clap::Parser;
use termion::{event::Key, input::TermRead, raw::IntoRawMode};

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    tty: Option<String>,

    #[clap(short)]
    output: PathBuf,
}

fn duty_delta(c: char) -> Option<ServoPositionDelta> {
    let mag = if c.is_ascii_uppercase() { 10 } else { 1 };
    let (shoulder, elbow) = match c.to_ascii_lowercase() {
        'k' => (1, 0),
        'j' => (-1, 0),
        'f' => (0, 1),
        'd' => (0, -1),
        _ => return None,
    };
    Some(ServoPositionDelta {
        shoulder: shoulder * mag,
        elbow: elbow * mag,
    })
}

static SHOULDER_ANGLES: &[(i16, &str)] = &[
    (-45, "0"),
    (-30, "1"),
    (0, "2"),
    (15, "3"),
    (30, "4"),
    (45, "5"),
    (60, "6"),
    (75, "7"),
    (90, "8"),
    (105, "9"),
    (120, "10"),
];

static ELBOW_ANGLES: &[(i16, &str)] = &[
    (-60, "a"),
    (-45, "b"),
    (-30, "c"),
    (-15, "d"),
    (0, "e"),
    (15, "f"),
    (30, "g"),
    (45, "h"),
    (60, "i"),
    (75, "j"),
];

struct Instruction {
    joint: Joint,
    direction: Direction,
    target_angle: i16,
    target_name: &'static str,
}

fn calibration_instructions() -> impl Iterator<Item = Instruction> {
    fn one(
        angles: &'static [(i16, &'static str)],
        joint: Joint,
        direction: Direction,
    ) -> impl Iterator<Item = Instruction> {
        let angles = if direction == Direction::Increasing {
            Box::new(angles.iter()) as Box<dyn Iterator<Item = _>>
        } else {
            Box::new(angles.iter().rev()) as Box<dyn Iterator<Item = _>>
        };
        angles.map(move |&(target_angle, target_name)| Instruction {
            joint,
            direction,
            target_angle,
            target_name,
        })
    }
    one(SHOULDER_ANGLES, Joint::Shoulder, Direction::Increasing)
        .chain(one(SHOULDER_ANGLES, Joint::Shoulder, Direction::Decreasing))
        .chain(one(ELBOW_ANGLES, Joint::Elbow, Direction::Increasing))
        .chain(one(ELBOW_ANGLES, Joint::Elbow, Direction::Decreasing))
}

impl std::fmt::Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let direction = if self.direction == Direction::Increasing {
            "increase"
        } else {
            "decrease"
        };
        let joint = if self.joint == Joint::Shoulder {
            "shoulder"
        } else {
            "elbow"
        };
        let name = self.target_name;
        f.write_fmt(format_args!("{direction} {joint} to \"{name}\""))
    }
}

// TODO: make this shared, for deserializing in the feeder.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Calib {
    shoulder_inc: Vec<(i16, u16)>,
    shoulder_dec: Vec<(i16, u16)>,
    elbow_inc: Vec<(i16, u16)>,
    elbow_dec: Vec<(i16, u16)>,
}

impl Calib {
    fn push(&mut self, joint: Joint, dir: Direction, angle: i16, duty: u16) {
        let list = match (joint, dir) {
            (Joint::Shoulder, Direction::Increasing) => &mut self.shoulder_inc,
            (Joint::Shoulder, Direction::Decreasing) => &mut self.shoulder_dec,
            (Joint::Elbow, Direction::Increasing) => &mut self.elbow_inc,
            (Joint::Elbow, Direction::Decreasing) => &mut self.elbow_dec,
        };
        list.push((angle, duty));
    }

    fn sort(&mut self) {
        self.shoulder_inc.sort();
        self.shoulder_dec.sort();
        self.elbow_inc.sort();
        self.elbow_dec.sort();
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut serial = Serial::detect()
        .ok_or_else(|| anyhow!("failed to detect brachiograph! Is it on and plugged in?"))?;

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let stdin = std::io::stdin();
    let stdin = stdin.lock();
    let mut raw = stdout.into_raw_mode()?;
    let mut keys = stdin.keys();
    let mut calib = Calib::default();

    for inst in calibration_instructions() {
        write!(&mut raw, "{}\r[{}] ", termion::clear::CurrentLine, inst)?;
        raw.flush()?;
        while let Some(key) = keys.next().transpose()? {
            match key {
                Key::Char('q') => {
                    write!(&mut raw, "{}\rGoodbye!\r\n", termion::clear::CurrentLine)?;
                    return Ok(());
                }
                Key::Char('\n') => {
                    let duties = serial.send(Op::GetPosition)?;
                    let Resp::CurPosition(duties) = duties else {
                        bail!("unexpected response {:?} to GetPosition", duties);
                    };
                    // TODO: we could keep track of duties ourselves instead of querying...
                    let duty = if inst.joint == Joint::Shoulder {
                        duties.shoulder
                    } else {
                        duties.elbow
                    };
                    calib.push(inst.joint, inst.direction, inst.target_angle, duty);
                    break;
                }
                Key::Char(c) => {
                    if let Some(delta) = duty_delta(c) {
                        serial.send(Op::ChangePosition(delta))?;
                    }
                }
                _ => {}
            }
        }
    }

    calib.sort();

    let data = postcard::to_allocvec(&calib)?;
    std::fs::write(args.output, data)?;

    Ok(())
}
