#![cfg_attr(not(feature = "std"), no_std)]

use fixed::traits::ToFixed;

pub mod geom;
pub mod pwm;
pub use fixed;
pub use fugit;
use pwm::Calibration;
use serde::{Deserialize, Serialize};

/// The type that we use for most of our numerical computations.
///
/// (Because this library is intended to be usable on embedded processors
/// without floating point units.)
pub type Fixed = fixed::types::I20F12;

/// The duration type that we use for most of our time calculations.
pub type Duration = fugit::Duration<u64, 1, 1_000_000>;

/// The instant type that we use for most of our time calculations.
pub type Instant = fugit::Instant<u64, 1, 1_000_000>;

/// Represents a brachiograph in transition from one point to another.
#[derive(Clone)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub struct Movement {
    init: Point,
    target: Point,
    start: Instant,
    dur: Duration,
}

impl Movement {
    /// At time `now`, where is this movement?
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

    /// Has the movement finished moving?
    pub fn is_finished(&self, now: Instant) -> bool {
        now >= self.start + self.dur
    }
}

/// The action that a brachiograph is carrying out.
#[derive(Clone)]
pub enum State {
    /// Resting (either pen up or pen down) at a point.
    Resting(Point, PenState),
    /// Moving (either pen up or pen down) from one point to another.
    Moving(Movement, PenState),
    /// Putting the pen either up or down (at a given point, and finishing at a given time).
    Lifting(Point, PenState, Instant),
}

impl State {
    /// Update this state to the new `now`.
    ///
    /// Returns the current position of the hand.
    pub fn update(&mut self, now: Instant) -> Point {
        match self {
            State::Resting(pos, ..) => *pos,
            State::Moving(movement, pen) => {
                if movement.is_finished(now) {
                    let ret = movement.target;
                    *self = State::Resting(ret, *pen);
                    ret
                } else {
                    movement.interpolate(now)
                }
            }
            State::Lifting(pos, pen, until) => {
                let ret = *pos;
                if now >= *until {
                    *self = State::Resting(ret, *pen);
                }
                ret
            }
        }
    }

    /// Is the state resting?
    pub fn is_resting(&self) -> bool {
        matches!(self, State::Resting(..))
    }
}

/// The state of a brachiograph.
// TODO: maybe it makes sense to separate the "geometric" brachiograph and have an extra layer that
// adds calibration
#[derive(Clone)]
pub struct Brachiograph {
    config: geom::Config,
    // Target speed, in units per second.
    speed: Fixed,
    state: State,
}

/// A brachiograph that is resting, ready to undertake another action.
pub struct RestingBrachiograph<'a> {
    inner: &'a mut Brachiograph,
    pos: Point,
    pen: PenState,
}

/// A brachiograph that is resting, ready to undertake another action.
/*
pub struct CalibratingBrachiograph<'a> {
    inner: &'a mut Brachiograph,
    pos: ServoPosition,
    pen: PenState,
}
*/

impl<'a> RestingBrachiograph<'a> {
    // TODO: error type
    pub fn move_to(mut self, now: Instant, x: impl ToFixed, y: impl ToFixed) -> Result<(), ()> {
        let init = self.pos;
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
        self.inner.state = State::Moving(mov, self.pen);
        Ok(())
    }

    /// Lift the pen to stop drawing.
    ///
    /// `now` is the current time.
    pub fn pen_up(mut self, now: Instant) {
        if self.pen == PenState::Down {
            self.pen = PenState::Up;
            self.inner.state = State::Lifting(self.pos, PenState::Up, now + Duration::millis(800));
        }
    }

    /// Lower the pen to start drawing.
    ///
    /// `now` is the current time.
    pub fn pen_down(mut self, now: Instant) {
        if self.pen == PenState::Up {
            self.pen = PenState::Down;
            self.inner.state =
                State::Lifting(self.pos, PenState::Down, now + Duration::millis(800));
        }
    }
}

/*
impl<'a> CalibratingBrachiograph<'a> {
    pub fn delta(mut self, delta: ServoPositionDelta) {
        self.pos.shoulder =
            (self.pos.shoulder as i32 + delta.shoulder as i32).clamp(0, u16::MAX as i32) as u16;
        self.pos.elbow =
            (self.pos.elbow as i32 + delta.elbow as i32).clamp(0, u16::MAX as i32) as u16;
        self.inner.state = State::Calibrating(self.pos, self.pen);
    }
}
*/

impl Brachiograph {
    pub fn new(x: impl ToFixed, y: impl ToFixed) -> Brachiograph {
        let pos = Point {
            x: x.to_fixed(),
            y: y.to_fixed(),
        };
        Brachiograph {
            // Note that we only ever use the default config, whose validity is checked in the tests.
            // If we ever use a non-default config, make sure to check validity at runtime.
            config: Default::default(),
            state: State::Resting(pos, PenState::Up),
            speed: Fixed::from_num(4),
        }
    }

    pub fn config(&self) -> &geom::Config {
        &self.config
    }

    pub fn pen(&self, now: Instant) -> PenState {
        match self.state {
            State::Resting(_, pen) | State::Moving(_, pen) => pen,
            State::Lifting(_, pen, finished) => {
                if now >= (finished - Duration::millis(400)) {
                    pen
                } else {
                    !pen
                }
            }
        }
    }

    pub fn resting(&mut self) -> Option<RestingBrachiograph<'_>> {
        if let State::Resting(pos, pen) = &self.state {
            Some(RestingBrachiograph {
                pos: *pos,
                pen: *pen,
                inner: self,
            })
        } else {
            None
        }
    }

    /*
    pub fn calibrating(&mut self) -> Option<CalibratingBrachiograph<'_>> {
        if let State::Calibrating(pos, pen) = &self.state {
            Some(CalibratingBrachiograph {
                pos: *pos,
                pen: *pen,
                inner: self,
            })
        } else {
            None
        }
    }

    pub fn change_calibration(&mut self, joint: Joint, dir: Direction, calib: ServoCalibration) {
        match (joint, dir) {
            (Joint::Shoulder, Direction::Increasing) => self.calib.shoulder.inc = calib.data,
            (Joint::Shoulder, Direction::Decreasing) => self.calib.shoulder.dec = calib.data,
            (Joint::Elbow, Direction::Increasing) => self.calib.elbow.inc = calib.data,
            (Joint::Elbow, Direction::Decreasing) => self.calib.elbow.dec = calib.data,
        }
    }
    */

    pub fn update(&mut self, now: Instant) -> Angles {
        let pos = self.state.update(now);
        // FIXME: unwrap. Should we store both position and angles?
        self.config.at_coord(pos.x, pos.y).unwrap()
    }
}

/// We represent angles between 0 and 180 degrees (the theoretical range of the servos)
/// as minutes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Angle(Fixed);

#[cfg(target_os = "none")]
impl defmt::Format for Angle {
    fn format(&self, f: defmt::Formatter) {
        let degs: i32 = self.0.to_num();
        let mins: i32 = (self.0.frac() * 60).to_num();
        defmt::write!(f, "Angle({}°{}')", degs, mins);
    }
}

impl Angle {
    pub fn clamp(self, lower: Angle, upper: Angle) -> Angle {
        Angle::from_degrees(self.degrees().clamp(lower.degrees(), upper.degrees()))
    }

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

    pub fn radians(self) -> Fixed {
        self.degrees() * Fixed::PI / 180
    }
}

impl core::ops::Neg for Angle {
    type Output = Angle;

    fn neg(self) -> Self::Output {
        Angle(-self.0)
    }
}

impl core::ops::Add<Angle> for Angle {
    type Output = Angle;

    fn add(self, rhs: Angle) -> Self::Output {
        Angle::from_degrees(self.degrees() + rhs.degrees())
    }
}

impl core::ops::AddAssign<Angle> for Angle {
    fn add_assign(&mut self, rhs: Angle) {
        self.0 += rhs.degrees()
    }
}

impl core::ops::Sub<Angle> for Angle {
    type Output = Angle;

    fn sub(self, rhs: Angle) -> Self::Output {
        self + (-rhs)
    }
}

/// Represented as milliseconds, between 0 and 1000.
#[derive(Debug)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
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

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub struct Angles {
    pub shoulder: Angle,
    pub elbow: Angle,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub struct Point {
    #[cfg_attr(target_os = "none", defmt(Display2Format))]
    pub x: Fixed,
    #[cfg_attr(target_os = "none", defmt(Display2Format))]
    pub y: Fixed,
}

/// The "raw" position of the shoulder and elbow servos.
///
/// This differs from [`Angles`] in that `Angles` have been calibrated.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub struct ServoPosition {
    pub shoulder: u16,
    pub elbow: u16,
    pub pen: u16,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Position {
    Raw(ServoPosition),
    Cooked(Point),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub struct ServoPositionDelta {
    pub shoulder: i16,
    pub elbow: i16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServoCalibration {
    pub data: arrayvec::ArrayVec<(i16, u16), 16>,
}

#[cfg(target_os = "none")]
impl defmt::Format for ServoCalibration {
    fn format(&self, _fmt: defmt::Formatter) {
        todo!()
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Joint {
    Shoulder,
    Elbow,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum PenState {
    Up,
    Down,
}

impl core::ops::Not for PenState {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            PenState::Up => PenState::Down,
            PenState::Down => PenState::Up,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Direction {
    Increasing,
    Decreasing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Op {
    // Slow ops
    ChangePosition(ServoPositionDelta),
    MoveTo(Point),
    PenUp,
    PenDown,

    // Fast ops
    Cancel,
    Calibrate(Joint, Direction, ServoCalibration),
    GetPosition,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Resp {
    Ack,
    Nack,
    QueueFull,
    Angles(Angles),
    CurPosition(ServoPosition),
}
