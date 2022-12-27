// TODO: draw a diagram

use crate::{Angle, Fixed};

use cordic::{acos, asin, sqrt};
use fixed::traits::ToFixed;

#[derive(Debug, Clone)]
pub struct Config {
    // Length of the bit from the shoulder to the elbow.
    // The units are arbitrary (all our calculations use the same
    // arbitrary units).
    pub humerus: Fixed,
    // Length of the bit from the elbow to the wrist.
    pub ulna: Fixed,

    pub shoulder_range: (Angle, Angle),
    pub elbow_range: (Angle, Angle),
    pub x_range: (Fixed, Fixed),
    pub y_range: (Fixed, Fixed),
}

impl Default for Config {
    fn default() -> Config {
        Config {
            humerus: 8.to_fixed(),
            ulna: 8.to_fixed(),
            shoulder_range: (Angle::from_degrees(-50), Angle::from_degrees(110)),
            elbow_range: (Angle::from_degrees(-80), Angle::from_degrees(80)),
            x_range: ((-8).to_fixed(), 8.to_fixed()),
            y_range: (4.to_fixed(), 14.to_fixed()),
        }
    }
}

#[derive(Debug, Copy, Clone, defmt::Format)]
pub struct State {
    pub shoulder: Angle,
    pub elbow: Angle,
}

impl Config {
    pub fn shoulder_is_valid(&self, shoulder: Angle) -> bool {
        self.shoulder_range.0.degrees() <= shoulder.degrees()
            && shoulder.degrees() <= self.shoulder_range.1.degrees()
    }

    pub fn elbow_is_valid(&self, elbow: Angle) -> bool {
        self.elbow_range.0.degrees() <= elbow.degrees()
            && elbow.degrees() <= self.elbow_range.1.degrees()
    }

    pub fn coord_is_valid(&self, x: Fixed, y: Fixed) -> bool {
        self.at_coord_impl(x, y).is_ok()
    }

    // TODO: error type
    pub fn at_coord(&self, x: impl ToFixed, y: impl ToFixed) -> Result<State, ()> {
        let x: Fixed = x.to_fixed();
        let y: Fixed = y.to_fixed();
        self.at_coord_impl(x, y)
    }

    fn at_coord_impl(&self, x: Fixed, y: Fixed) -> Result<State, ()> {
        if x < self.x_range.0 || x > self.x_range.1 || y < self.y_range.0 || y > self.y_range.1 {
            return Err(());
        }

        let r2 = x * x + y * y;
        // cordic's atan2 implementation is not great: it does a division and can overflow.
        let r = sqrt(r2);
        let mut theta = acos(x / r);
        if y < 0 {
            theta = Fixed::PI * 2 - theta;
        }

        // TODO: can precompute some of this
        let sin_elbow = (self.humerus * self.humerus + self.ulna * self.ulna - r2)
            / (2 * self.humerus * self.ulna);
        if sin_elbow > 1 || sin_elbow < -1 {
            return Err(());
        }
        // FIXME: double-check the sign
        let elbow_rads = -asin(sin_elbow);
        let elbow = Angle::from_radians(elbow_rads);
        let shoulder_rads = Fixed::FRAC_PI_2 + Fixed::FRAC_PI_4 - theta + elbow_rads / 2;
        let shoulder = Angle::from_radians(shoulder_rads);

        if self.elbow_is_valid(elbow) && self.shoulder_is_valid(shoulder) {
            Ok(State { elbow, shoulder })
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use fixed::traits::ToFixed;

    use super::*;
    fn assert_approx(geom: &State, shoulder: impl ToFixed, elbow: impl ToFixed) {
        let shoulder: Fixed = shoulder.to_fixed();
        let elbow: Fixed = elbow.to_fixed();
        assert!((geom.shoulder.degrees() - shoulder).abs() < 0.5);
        assert!((geom.elbow.degrees() - elbow).abs() < 0.5);
    }

    #[test]
    fn precalculated_coords() {
        let b = Config::default();
        assert_approx(&b.at_coord(-8, 8).unwrap(), 0, 0);
        assert_approx(&b.at_coord(0, 11.313).unwrap(), 45, 0);
        assert_approx(&b.at_coord(0, 8).unwrap(), 30, -30);
        assert_approx(&b.at_coord(8, 8).unwrap(), 90, 0);
    }
}
