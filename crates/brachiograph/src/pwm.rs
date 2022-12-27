use arrayvec::ArrayVec;
use fixed::traits::ToFixed;
use fixed_macro::fixed;

use crate::{Angle, Fixed};

type Frac = fixed::types::U0F16;

pub struct CalibrationEntry {
    pub degrees: i16,
    pub duty_ratio: Frac,
}

// TODO: provide simple manipulations (e.g. shifting the zero)
pub struct Pwm {
    pub calib: ArrayVec<CalibrationEntry, 16>,
}

pub struct TogglePwm {
    pub on: Frac,
    pub off: Frac,
}

// TODO: non-trivial calibration, including hysterisis
impl Pwm {
    pub fn shoulder() -> Pwm {
        Pwm {
            calib: [
                CalibrationEntry {
                    degrees: -50,
                    duty_ratio: fixed!(0.119444444: U0F16),
                },
                CalibrationEntry {
                    degrees: 110,
                    duty_ratio: fixed!(0.03055555: U0F16),
                },
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn elbow() -> Pwm {
        Pwm {
            calib: [
                CalibrationEntry {
                    degrees: -80,
                    duty_ratio: fixed!(0.119444444: U0F16),
                },
                CalibrationEntry {
                    degrees: 80,
                    duty_ratio: fixed!(0.03055555: U0F16),
                },
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn duty(&self, angle: Angle) -> Option<Frac> {
        let deg = angle.degrees();
        for slice in self.calib.windows(2) {
            let before: Fixed = slice[0].degrees.to_fixed();
            let after: Fixed = slice[1].degrees.to_fixed();
            if before <= deg && deg <= after {
                let lambda = (deg - before) / (after - before);
                let lambda = Frac::saturating_from_num(lambda);
                let mu = Frac::saturating_from_num(1) - lambda;
                let before_ratio = slice[0].duty_ratio;
                let after_ratio = slice[1].duty_ratio;
                return Some(before_ratio * mu + after_ratio * lambda);
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
