use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::bail;
use clap::Parser;
use kurbo::{Affine, BezPath, ParamCurve, PathEl, Rect, Shape};
use serialport::SerialPort;

#[derive(Parser, Debug)]
struct Args {
    tty: String,
    input: PathBuf,
}

struct Serial {
    write: Box<dyn SerialPort>,
    read: BufReader<Box<dyn SerialPort>>,
}

fn load_svg(path: &Path) -> anyhow::Result<Vec<BezPath>> {
    // TODO: apparently git master usvg supports text-to-path?
    let data = std::fs::read(path)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt)?;
    let mut ret = Vec::new();

    for node in tree.root.descendants() {
        let mut bez = BezPath::new();
        if let usvg::NodeKind::Path(p) = &*node.borrow() {
            // TODO: do we need to apply the transform in p.transform or has that been done
            // already? FIXME: yes, I think we do need it
            for seg in p.data.segments() {
                match seg {
                    usvg::PathSegment::MoveTo { x, y } => bez.move_to((x, y)),
                    usvg::PathSegment::LineTo { x, y } => bez.line_to((x, y)),
                    usvg::PathSegment::CurveTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => bez.curve_to((x1, y1), (x2, y2), (x, y)),
                    usvg::PathSegment::ClosePath => bez.close_path(),
                }
            }
        }
        ret.push(bez);
    }
    Ok(ret)
}

// Transform each of the paths by a common scaling and translation,
// so that the resulting paths all lie in `rect`.
//
// Also flips the y coordinate, because svg is y-down and brachiograph is y-up.
fn transform(paths: &mut [BezPath], rect: Rect) {
    if paths.is_empty() {
        return;
    }
    let mut bbox = paths[0].bounding_box();
    for p in &paths[1..] {
        bbox = bbox.union(p.bounding_box());
    }
    let transform = Affine::FLIP_Y * Affine::translate(-bbox.center().to_vec2());
    let scale = (rect.height() / bbox.height()).min(rect.width() / bbox.width());
    let transform = Affine::scale(scale) * transform;
    let transform = Affine::translate(rect.center().to_vec2()) * transform;
    for path in paths {
        path.apply_affine(transform);
    }
}

// TODO: in the case of short (in terms of arc-length) sequences of segments, it might be
// worth just converting them to a single line.
fn flatten(path: &BezPath) -> BezPath {
    if path.elements().len() <= 1 {
        return BezPath::new();
    }
    let mut start = (0., 0.).into();
    let mut ret = BezPath::new();
    path.flatten(0.05, |el| match el {
        PathEl::LineTo(_) => ret.push(el),
        PathEl::MoveTo(p) => {
            start = p;
            ret.push(el);
        }
        PathEl::ClosePath => ret.push(PathEl::LineTo(start)),
        _ => unreachable!(),
    });
    ret
}

#[derive(Debug)]
enum Op {
    PenUp,
    PenDown,
    MoveTo { x: i32, y: i32 },
}

fn to_ops(path: &BezPath) -> Vec<Op> {
    let mut ret = Vec::new();
    fn round_point(p: &kurbo::Point) -> Op {
        Op::MoveTo {
            x: (p.x * 10.0).round() as i32,
            y: (p.y * 10.0).round() as i32,
        }
    }

    for el in path {
        match el {
            PathEl::MoveTo(p) => {
                ret.push(Op::PenUp);
                ret.push(round_point(&p));
                ret.push(Op::PenDown);
            }
            PathEl::LineTo(p) => ret.push(round_point(&p)),
            _ => unreachable!(),
        }
    }
    ret
}

// Send a single op element to brachiograph, blocking if necessary.
fn send(serial: &mut Serial, op: Op) -> anyhow::Result<()> {
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
        match resp.trim() {
            "ack" => break,
            "queue full" => continue,
            resp => bail!("Unexpected response: {resp:?}"),
        }
    }

    Ok(())
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

    let mut paths = load_svg(&args.input)?;
    // TODO: make the rect configurable
    transform(&mut paths, Rect::new(-8.0, 5.0, 8.0, 13.0));
    let ops: Vec<_> = paths
        .iter()
        .map(flatten)
        .flat_map(|bez| to_ops(&bez).into_iter())
        .collect();
    for op in ops {
        println!("{:?}", op);
        send(&mut serial, op)?;
    }
    send(&mut serial, Op::PenUp)?;
    send(&mut serial, Op::MoveTo { x: -80, y: 80 })?;

    Ok(())
}
