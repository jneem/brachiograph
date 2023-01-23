use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail};
use brachiograph::{Angle, Fixed, Op, Resp, SlowOp};
use clap::Parser;
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape, Vec2};
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
                    usvg::PathSegment::MoveTo { x, y } => {
                        let (x, y) = p.transform.apply(x, y);
                        bez.move_to((x, y));
                    }
                    usvg::PathSegment::LineTo { x, y } => {
                        let (x, y) = p.transform.apply(x, y);
                        bez.line_to((x, y));
                    }
                    usvg::PathSegment::CurveTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => {
                        let (x, y) = p.transform.apply(x, y);
                        let (x1, y1) = p.transform.apply(x1, y1);
                        let (x2, y2) = p.transform.apply(x2, y2);
                        bez.curve_to((x1, y1), (x2, y2), (x, y));
                    }
                    usvg::PathSegment::ClosePath => bez.close_path(),
                }
            }
        }
        if !bez.is_empty() {
            ret.push(bez);
        }
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

fn load_logo(path: &Path) -> anyhow::Result<Vec<brachiologo::BuiltIn>> {
    let data = std::fs::read_to_string(path)?;
    let program = brachiologo::program(&data)
        .map_err(|e| anyhow!("parse error: {e}"))?
        .1;
    let mut scope = brachiologo::Scope::default();
    let mut output = Vec::new();
    scope.exec_block(&mut output, &program)?;
    Ok(output)
}

fn run_turtle(steps: &[brachiologo::BuiltIn], rect: Rect) -> Vec<SlowOp> {
    let mut pos = rect.center();
    let mut angle = Angle::from_degrees(90);
    let mut ret = Vec::new();

    let clamp = |pt: Point| {
        Point::new(
            pt.x.clamp(rect.min_x(), rect.max_x()),
            pt.y.clamp(rect.min_y(), rect.max_y()),
        )
    };

    for step in steps {
        match step {
            brachiologo::BuiltIn::Forward(dist) => {
                pos += Vec2::from_angle(angle.radians().to_num()) * *dist;
                ret.push(p_to_op(clamp(pos)));
            }
            brachiologo::BuiltIn::Back(dist) => {
                pos -= Vec2::from_angle(angle.radians().to_num()) * *dist;
                ret.push(p_to_op(clamp(pos)));
            }
            brachiologo::BuiltIn::Left(ang) => {
                angle += Angle::from_degrees(*ang);
            }
            brachiologo::BuiltIn::Right(ang) => {
                angle += Angle::from_degrees(*ang);
            }
            brachiologo::BuiltIn::ClearScreen => {}
            brachiologo::BuiltIn::PenUp => {
                ret.push(SlowOp::PenUp);
            }
            brachiologo::BuiltIn::PenDown => {
                ret.push(SlowOp::PenDown);
            }
        }
    }

    ret
}

fn to_ops(path: &BezPath) -> Vec<SlowOp> {
    let mut ret = Vec::new();

    for el in path {
        match el {
            PathEl::MoveTo(p) => {
                ret.push(SlowOp::PenUp);
                ret.push(p_to_op(p));
                ret.push(SlowOp::PenDown);
            }
            PathEl::LineTo(p) => ret.push(p_to_op(p)),
            _ => unreachable!(),
        }
    }
    ret
}

// Send a single op element to brachiograph, blocking if necessary.
fn send(serial: &mut Serial, op: SlowOp) -> anyhow::Result<()> {
    println!("{:?}", op);
    loop {
        let msg = postcard::to_stdvec_cobs(&Op::Slow(op.clone()))?;
        serial.write.write_all(&msg)?;

        let mut read = serial.read.fill_buf()?.to_vec();
        let (msg, remaining) = postcard::take_from_bytes_cobs(&mut read)?;
        let remaining_len = remaining.len();
        drop(remaining);
        serial.read.consume(read.len() - remaining_len);
        match dbg!(msg) {
            Resp::Ack => break,
            Resp::QueueFull => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            resp => bail!("Unexpected response: {resp:?}"),
        }
    }

    Ok(())
}

fn p_to_op(p: impl Into<Point>) -> SlowOp {
    let p = p.into();
    SlowOp::MoveTo(brachiograph::Point {
        x: Fixed::from_num(p.x),
        y: Fixed::from_num(p.y),
    })
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

    let ext = args.input.extension().and_then(|s| s.to_str());
    let ops = if ext == Some("svg") {
        let mut paths = load_svg(&args.input)?;
        // TODO: make the rect configurable
        transform(&mut paths, Rect::new(-8.0, 5.0, 8.0, 13.0));
        paths
            .iter()
            .map(flatten)
            .flat_map(|bez| to_ops(&bez).into_iter())
            .collect()
    } else if ext == Some("logo") {
        let turtle = load_logo(&args.input)?;
        send(&mut serial, p_to_op((0., 9.)))?;
        send(&mut serial, SlowOp::PenDown)?;
        run_turtle(&turtle, Rect::new(-8.0, 5.0, 8.0, 13.0))
    } else {
        bail!("didn't recognize input file type");
    };
    for op in ops {
        send(&mut serial, op)?;
    }
    send(&mut serial, SlowOp::PenUp)?;
    send(&mut serial, p_to_op((-8., 8.)))?;

    Ok(())
}
