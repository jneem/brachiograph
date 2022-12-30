#![cfg_attr(not(feature = "std"), no_std)]

use core::str::FromStr;
use defmt::Format;
use fixed::traits::ToFixed;

pub mod geom;
pub mod pwm;

pub type Fixed = fixed::types::I20F12;
pub type Duration = fugit::Duration<u64, 1, 1_000_000>;
pub type Instant = fugit::Instant<u64, 1, 1_000_000>;

#[derive(Clone, Format)]
pub struct Movement {
    init: Point,
    target: Point,
    start: Instant,
    dur: Duration,
}

impl Movement {
    pub fn interpolate(&self, now: Instant) -> Point {
        let dur = now.checked_duration_since(self.start).unwrap();
        let total_dur: Fixed = self.dur.to_millis().to_fixed();
        let dur: Fixed = dur.to_millis().to_fixed();
        let ratio = if total_dur > 0 {
            (dur / total_dur).clamp(0.to_fixed(), 1.to_fixed())
        } else {
            1.to_fixed()
        };
        let ret = Point {
            x: self.init.x + ratio * (self.target.x - self.init.x),
            y: self.init.y + ratio * (self.target.y - self.init.y),
        };
        ret
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        now >= self.start + self.dur
    }
}

// TODO: doing pen-up/pen-down state? It takes some time...
#[derive(Clone, defmt::Format)]
pub enum State {
    Resting(Point),
    Moving(Movement),
}

impl State {
    pub fn update(&mut self, now: Instant) -> Point {
        match self {
            State::Resting(pos) => *pos,
            State::Moving(movement) => {
                if movement.is_finished(now) {
                    let ret = movement.target;
                    *self = State::Resting(ret);
                    ret
                } else {
                    movement.interpolate(now)
                }
            }
        }
    }

    pub fn is_moving(&self) -> bool {
        matches!(self, State::Moving(_))
    }
}

#[derive(Clone)]
pub struct Brachiograph {
    config: geom::Config,
    pos: Point,
    state: State,
    pen_down: bool,
    // Target speed, in units per second.
    speed: Fixed,
}

pub struct RestingBrachiograph<'a> {
    inner: &'a mut Brachiograph,
}

impl<'a> RestingBrachiograph<'a> {
    // TODO: error type
    pub fn move_to(&mut self, now: Instant, x: impl ToFixed, y: impl ToFixed) -> Result<(), ()> {
        let init = self.inner.pos;
        let x: Fixed = x.to_fixed();
        let y: Fixed = y.to_fixed();
        if !self.inner.config.coord_is_valid(x, y) {
            return Err(());
        };

        let dx = x - init.x;
        let dy = y - init.y;
        let dist = cordic::sqrt(dx * dx + dy * dy);
        let seconds = dist / self.inner.speed;
        let mov = Movement {
            init,
            target: Point { x, y },
            start: now,
            dur: Duration::millis((seconds * 1000).to_num()),
        };
        /*
        defmt::println!(
            "interpolating dist {} units in {} ms",
            dist.to_num::<i32>(),
            (seconds * 1000).to_num::<i32>()
        );
        */
        self.inner.state = State::Moving(mov);
        Ok(())
    }

    pub fn pen_up(&mut self) {
        self.inner.pen_down = false;
    }

    pub fn pen_down(&mut self) {
        self.inner.pen_down = true;
    }
}

impl Brachiograph {
    pub fn new(x: impl ToFixed, y: impl ToFixed) -> Brachiograph {
        let pos = Point {
            x: x.to_fixed(),
            y: y.to_fixed(),
        };
        Brachiograph {
            config: Default::default(),
            pos,
            state: State::Resting(pos),
            pen_down: false,
            speed: Fixed::from_num(4),
        }
    }

    pub fn config(&self) -> &geom::Config {
        &self.config
    }

    pub fn angles(&self) -> geom::State {
        // FIXME: unwrap. Should we store both position and angles?
        self.config.at_coord(self.pos.x, self.pos.y).unwrap()
    }

    pub fn update(&mut self, now: Instant) -> geom::State {
        self.pos = self.state.update(now);
        // FIXME: unwrap. Should we store both position and angles?
        self.config.at_coord(self.pos.x, self.pos.y).unwrap()
    }

    pub fn resting(&mut self) -> Option<RestingBrachiograph<'_>> {
        if self.state.is_moving() {
            None
        } else {
            Some(RestingBrachiograph { inner: self })
        }
    }
}

/// We represent angles between 0 and 180 degrees (the theoretical range of the servos)
/// as minutes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Angle(Fixed);

impl Format for Angle {
    fn format(&self, f: defmt::Formatter) {
        let degs: i32 = self.0.to_num();
        let mins: i32 = (self.0.frac() * 60).to_num();
        defmt::write!(f, "Angle({}°{}')", degs, mins);
    }
}

impl Angle {
    pub fn from_degrees<N: ToFixed>(deg: N) -> Angle {
        // TODO: reinstante the angle-clamping
        Angle(deg.to_fixed())
    }

    pub fn from_radians<N: ToFixed>(rad: N) -> Angle {
        let rad: Fixed = rad.to_fixed();
        Angle::from_degrees(rad * 180 / Fixed::PI)
    }

    pub fn interpolate(&self, other: Angle, ratio: Fixed) -> Angle {
        let lambda = ratio.clamp(0u8.into(), 1u8.into());
        let mu = Fixed::from(1u8) - lambda;
        Angle(self.0 * mu + other.0 * lambda)
    }

    pub fn degrees(self) -> Fixed {
        self.0
    }
}

/// Represented as milliseconds, between 0 and 1000.
#[derive(Debug, Format)]
pub struct Delay(u16);

impl Delay {
    pub fn from_millis(ms: u16) -> Delay {
        Delay(ms.clamp(0, 1000))
    }

    pub fn to_millis(&self) -> u16 {
        self.0
    }
}

impl From<core::time::Duration> for Delay {
    fn from(dur: core::time::Duration) -> Self {
        Delay(dur.as_millis().clamp(0, 1000) as u16)
    }
}

#[derive(Debug, Format)]
pub struct Angles {
    pub shoulder: Angle,
    pub elbow: Angle,
}

#[derive(Copy, Clone, Debug, Format)]
pub struct Point {
    #[defmt(Display2Format)]
    pub x: Fixed,
    #[defmt(Display2Format)]
    pub y: Fixed,
}

#[derive(Debug, Format)]
pub enum Op {
    Cancel,
    MoveTo(Point),
    PenUp,
    PenDown,
}

#[derive(Debug, Format)]
pub enum OpParseErr {
    UnknownOp,
    BadAngles,
    BadPoint,
}

impl FromStr for Op {
    type Err = OpParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.trim().split_ascii_whitespace();
        match words.next() {
            Some("moveto") => {
                let x: i16 = words
                    .next()
                    .ok_or(OpParseErr::BadPoint)?
                    .parse()
                    .map_err(|_| OpParseErr::BadPoint)?;
                let y: i16 = words
                    .next()
                    .ok_or(OpParseErr::BadPoint)?
                    .parse()
                    .map_err(|_| OpParseErr::BadPoint)?;
                Ok(Op::MoveTo(Point {
                    x: Fixed::from_num(x) / 10,
                    y: Fixed::from_num(y) / 10,
                }))
            }
            Some("penup") => Ok(Op::PenUp),
            Some("pendown") => Ok(Op::PenDown),
            _ => Err(OpParseErr::UnknownOp),
        }
    }
}

#[derive(Debug, Format)]
pub enum Resp {
    Angles(Angles),
    Busy,
    PenUp,
    PenDown,
}
