use arrayvec::ArrayVec;
use fixed_macro::fixed;

use crate::{Angle, Fixed};

type Frac = fixed::types::U0F16;

// A pair of (degrees, pulse-width-modulation-in-microseconds)
pub type CalibrationEntry = (i16, u16);

pub struct Pwm {
    // Calibrations to use when the angle is increasing.
    pub inc: ArrayVec<CalibrationEntry, 16>,
    // Calibrations to use when the angle is decreasing.
    pub dec: ArrayVec<CalibrationEntry, 16>,
}

pub struct TogglePwm {
    pub on: Frac,
    pub off: Frac,
}

// TODO: non-trivial calibration, including hysterisis
impl Pwm {
    pub fn shoulder() -> Pwm {
        Pwm {
            inc: [(-50, 1194), (110, 306)].into_iter().collect(),
            dec: [(-50, 1194), (110, 306)].into_iter().collect(),
        }
    }

    pub fn elbow() -> Pwm {
        Pwm {
            inc: [(-80, 1194), (80, 306)].into_iter().collect(),
            dec: [(-80, 1194), (80, 306)].into_iter().collect(),
        }
    }

    pub fn duty(&self, angle: Angle) -> Option<u16> {
        let deg = angle.degrees();
        // FIXME: use dec if appropriate
        for slice in self.inc.windows(2) {
            let before = Fixed::from_num(slice[0].0);
            let after = Fixed::from_num(slice[1].0);
            if before <= deg && deg <= after {
                let lambda = (deg - before) / (after - before);
                let mu = Fixed::from_num(1i32) - lambda;
                let before: Fixed = Fixed::from(slice[0].1);
                let after: Fixed = Fixed::from(slice[1].1);
                return Some((before * mu + after * lambda).round().to_num());
            }
        }
        return None;
    }
}

impl TogglePwm {
    pub fn pen() -> TogglePwm {
        TogglePwm {
            off: fixed!(0.075: U0F16),
            on: fixed!(0.125: U0F16),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_approx(a: Frac, b: Frac) {
        assert!(a.dist(b) < 0.0001);
    }

    #[test]
    fn precomputed_duties() {
        let sh = Pwm::shoulder();
        assert_approx(
            fixed!(0.09166666: U0F16),
            sh.duty(Angle::from_degrees(0)).unwrap(),
        );
    }
}
