use arrayvec::ArrayVec;

use crate::{Angle, Angles, Direction, Fixed, Joint, PenState, ServoCalibration, ServoPosition};

#[derive(Debug, Clone)]
pub struct Calibration {
    pub shoulder: Pwm,
    pub elbow: Pwm,
    pub pen: TogglePwm,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            shoulder: Pwm::shoulder(),
            elbow: Pwm::elbow(),
            pen: TogglePwm::pen(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CalibratedPosition {
    pub calib: Calibration,
    // For the hysteresis correction, we need to store the previous angles.
    pub last_angles: Angles,
}

impl CalibratedPosition {
    pub fn update(&mut self, angles: Angles, pen: PenState) -> ServoPosition {
        let shoulder = self
            .calib
            .shoulder
            .duty(self.last_angles.shoulder, angles.shoulder);
        let elbow = self.calib.elbow.duty(self.last_angles.elbow, angles.elbow);
        let pen = self.calib.pen.duty(pen);
        self.last_angles = angles;
        ServoPosition {
            shoulder,
            elbow,
            pen,
        }
    }

    pub fn change_calibration(&mut self, joint: Joint, dir: Direction, calib: ServoCalibration) {
        let list = match (joint, dir) {
            (Joint::Shoulder, Direction::Increasing) => &mut self.calib.shoulder.inc,
            (Joint::Shoulder, Direction::Decreasing) => &mut self.calib.shoulder.dec,
            (Joint::Elbow, Direction::Increasing) => &mut self.calib.elbow.inc,
            (Joint::Elbow, Direction::Decreasing) => &mut self.calib.elbow.dec,
        };
        *list = calib.data;
    }
}

// A pair of (degrees, pulse-width-modulation-in-microseconds)
pub type CalibrationEntry = (i16, u16);

#[derive(Debug, Clone)]
pub struct Pwm {
    // Calibrations to use when the angle is increasing.
    pub inc: ArrayVec<CalibrationEntry, 16>,
    // Calibrations to use when the angle is decreasing.
    pub dec: ArrayVec<CalibrationEntry, 16>,
}

#[derive(Debug, Clone)]
pub struct TogglePwm {
    pub on: u16,
    pub off: u16,
}

impl Pwm {
    pub fn shoulder() -> Pwm {
        Pwm {
            inc: [(-45, 2333), (120, 500)].into_iter().collect(),
            dec: [(-45, 2333), (120, 500)].into_iter().collect(),
        }
    }

    pub fn elbow() -> Pwm {
        Pwm {
            inc: [(-60, 2167), (75, 833)].into_iter().collect(),
            dec: [(-60, 2167), (75, 833)].into_iter().collect(),
        }
    }

    pub fn duty(&self, last_angle: Angle, angle: Angle) -> u16 {
        let deg = angle.degrees();
        let slices = if angle.degrees() > last_angle.degrees() {
            self.inc.windows(2)
        } else {
            self.dec.windows(2)
        };
        for slice in slices {
            let before = Fixed::from_num(slice[0].0);
            let after = Fixed::from_num(slice[1].0);
            if deg < before {
                // We cannot represent an angle so small, so return the smallest angle we have.
                return slice[0].1;
            } else if deg <= after {
                let lambda = (deg - before) / (after - before);
                let mu = Fixed::from_num(1i32) - lambda;
                let before: Fixed = Fixed::from(slice[0].1);
                let after: Fixed = Fixed::from(slice[1].1);
                return (before * mu + after * lambda).round().to_num();
            }
        }
        // We cannot represent an angle so large, so return the largest angle we have.
        return self.inc.last().unwrap().1;
    }
}

impl TogglePwm {
    pub fn pen() -> TogglePwm {
        TogglePwm { off: 750, on: 1250 }
    }

    pub fn duty(&self, state: PenState) -> u16 {
        match state {
            PenState::Up => self.off,
            PenState::Down => self.on,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_approx(a: u16, b: u16) {
        assert!((a as i32 - b as i32).abs() < 10);
    }

    #[test]
    fn precomputed_duties() {
        let sh = Pwm::shoulder();
        assert_approx(916, sh.duty(Angle::from_degrees(0), Angle::from_degrees(0)));
    }
}
