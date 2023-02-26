// TODO: draw a diagram

use crate::{Angle, Angles, Fixed};

use cordic::{asin, atan, cos, sin, sqrt};
use fixed::traits::{FromFixed, ToFixed};

#[derive(Debug, Clone)]
pub struct Config {
    // Length of the arms. We assume they're the same length: it cuts down
    // on the required trig operations.
    pub arm_len: Fixed,

    pub shoulder_range: (Angle, Angle),
    pub elbow_range: (Angle, Angle),
    pub x_range: (Fixed, Fixed),
    pub y_range: (Fixed, Fixed),
}

impl Default for Config {
    fn default() -> Config {
        Config {
            arm_len: 8.to_fixed(),
            shoulder_range: (Angle::from_degrees(-45), Angle::from_degrees(120)),
            elbow_range: (Angle::from_degrees(-60), Angle::from_degrees(75)),
            x_range: ((-8).to_fixed(), 8.to_fixed()),
            y_range: (5.to_fixed(), 13.to_fixed()),
        }
    }
}

impl Config {
    // The configuration is valid if every point in the configured x/y-range can be reached by the arms.
    pub fn is_valid(&self) -> bool {
        let (y0, y1) = self.y_range;
        let (x0, x1) = self.x_range;
        let ell = self.arm_len;

        if y0 <= 0 || y1 <= y0 || x1 <= x0 {
            return false;
        }

        let x_max = x0.abs().max(x1.abs());
        if x_max * x_max + y1 * y1 >= 4 * ell * ell {
            return false;
        }

        // Next, we check the angle constraints. To check them for the whole rectangle it's enough to
        // check them on the boundary. Constrained to a single boundary line segment, the shoulder angle
        // has at most one critical point (which occurs when the ulna is orthogonal to the boundary segment).
        // So it's enough to check the four corners and the (at most) four critical points.
        let check = |x: Fixed, y: Fixed| -> bool {
            let Ok(angles) = self.at_coord(x, y) else {
                return false;
            };
            self.shoulder_is_valid(angles.shoulder) && self.elbow_is_valid(angles.elbow)
        };

        // Four corners.
        if !check(x0, y0) || !check(x0, y1) || !check(x1, y0) || !check(x1, y1) {
            return false;
        }

        // Critical points on horizontal boundaries.
        //
        // When constraining to the horizontal line {y = a}, if the ulna is pointing straight up then the
        // elbow has y-coordinate a-ell, and so the shoulder angle is asin((a-ell)/ell). Since we already checked
        // the radial constraints, a is between 0 and 2 ell, so (a-ell)/ell is between -1 and 1.
        for a in [y0, y1] {
            let shoulder_angle = Angle::from_radians(asin((a - ell) / ell));
            let elbow_angle = -shoulder_angle;
            let x = -sqrt(ell * ell - (a - ell) * (a - ell));
            if x0 <= x && x <= x1 {
                if !self.shoulder_is_valid(shoulder_angle) || !self.elbow_is_valid(elbow_angle) {
                    return false;
                }
            }
        }

        // Don't let the elbow bend "backwards"
        if self.elbow_range.0.degrees() < -90 {
            return false;
        }
        // Critical points on vertical boundaries.
        // When constraining to the vertical line {x = b}, if the ulna is pointing horizontally then
        // (because y > 0 and the elbow can't bend "backwards") the hand is on the right and the
        // elbow is on the left. In this case the elbow angle is -asin((b-ell)/ell), but it only makes
        // sense if b > 0.
        for b in [x0, x1] {
            if b > 0 {
                let elbow_rads = -asin((b - ell) / ell);
                let elbow_angle = Angle::from_radians(elbow_rads);
                let shoulder_angle = Angle::from_radians(Fixed::FRAC_PI_2 + elbow_rads);
                let y = sqrt(ell * ell - (b - ell) * (b - ell));
                if y0 <= y && y <= y1 {
                    if !self.shoulder_is_valid(shoulder_angle) || !self.elbow_is_valid(elbow_angle)
                    {
                        return false;
                    }
                }
            }
        }

        true
    }

    pub fn shoulder_is_valid(&self, shoulder: Angle) -> bool {
        self.shoulder_range.0.degrees() <= shoulder.degrees()
            && shoulder.degrees() <= self.shoulder_range.1.degrees()
    }

    pub fn elbow_is_valid(&self, elbow: Angle) -> bool {
        self.elbow_range.0.degrees() <= elbow.degrees()
            && elbow.degrees() <= self.elbow_range.1.degrees()
    }

    pub fn coord_is_valid(&self, x: Fixed, y: Fixed) -> bool {
        self.x_range.0 <= x && x <= self.x_range.1 && self.y_range.0 <= y && y <= self.y_range.1
    }

    // TODO: error type
    pub fn at_coord(&self, x: impl ToFixed, y: impl ToFixed) -> Result<Angles, ()> {
        let x: Fixed = x.to_fixed();
        let y: Fixed = y.to_fixed();
        self.at_coord_impl(x, y)
    }

    fn at_coord_impl(&self, x: Fixed, y: Fixed) -> Result<Angles, ()> {
        if x < self.x_range.0 || x > self.x_range.1 || y < self.y_range.0 || y > self.y_range.1 {
            return Err(());
        }

        let r2 = x * x + y * y;
        // cordic's atan2 implementation is not great: it naively does y/x and can overflow.
        let theta = {
            if x.abs() > y.abs() {
                let t = atan(y / x);
                if t < 0 {
                    // atan returns something between -pi/2 and pi/2, so this will give something between 0 and pi.
                    // Since we've assumed y \ge 0, that's everything.
                    t + Fixed::PI
                } else {
                    t
                }
            } else {
                // atan returns something between -pi/2 and pi/2, so this will give something between 0 and pi.
                // Since we've assumed y \ge 0, that's everything.
                Fixed::FRAC_PI_2 - atan(x / y)
            }
        };

        // TODO: can precompute the quotient
        let sin_elbow = Fixed::from_num(1i32) - r2 / (2 * self.arm_len * self.arm_len);
        // The clamp shouldn't be necessary if this config passed `is_valid`, but just in case of any numerical errors...
        let sin_elbow = sin_elbow.clamp(Fixed::from_num(-1), Fixed::from_num(1));
        let elbow_rads = -asin(sin_elbow);
        let elbow = Angle::from_radians(elbow_rads);
        let shoulder_rads = Fixed::FRAC_PI_2 + Fixed::FRAC_PI_4 - theta + elbow_rads / 2;
        let shoulder = Angle::from_radians(shoulder_rads);

        Ok(Angles { elbow, shoulder })
    }

    pub fn coord_at_angle<T: FromFixed>(&self, angles: Angles) -> (T, T) {
        let r = Fixed::SQRT_2
            * self.arm_len
            * sqrt(Fixed::from_num(1i32) + sin(angles.elbow.radians()));
        let theta = Fixed::FRAC_PI_2 + Fixed::FRAC_PI_4 + angles.elbow.radians() / 2
            - angles.shoulder.radians();

        (Fixed::to_num(r * cos(theta)), Fixed::to_num(r * sin(theta)))
    }
}

#[cfg(test)]
mod tests {
    use fixed::traits::ToFixed;

    use super::*;
    fn assert_approx(geom: &crate::Angles, shoulder: impl ToFixed, elbow: impl ToFixed) {
        let shoulder: Fixed = shoulder.to_fixed();
        let elbow: Fixed = elbow.to_fixed();
        assert!((geom.shoulder.degrees() - shoulder).abs() < 0.1);
        assert!((geom.elbow.degrees() - elbow).abs() < 0.1);
    }

    fn assert_approx_f64(x: f64, y: f64) {
        assert!((x - y).abs() < 0.01);
    }

    #[test]
    fn precalculated_coords() {
        let b = Config::default();
        assert_approx(&b.at_coord(-8, 8).unwrap(), 0, 0);
        assert_approx(&b.at_coord(0, 11.313).unwrap(), 45, 0);
        assert_approx(&b.at_coord(0, 8).unwrap(), 30, -30);
        assert_approx(&b.at_coord(8, 8).unwrap(), 90, 0);
    }

    #[test]
    fn precalculated_inverse() {
        let b = Config::default();
        let (x, y) = b.coord_at_angle(Angles {
            shoulder: Angle::from_degrees(0),
            elbow: Angle::from_degrees(0),
        });
        assert_approx_f64(x, -8.0);
        assert_approx_f64(y, 8.0);

        let (x, y) = b.coord_at_angle(Angles {
            shoulder: Angle::from_degrees(45),
            elbow: Angle::from_degrees(0),
        });
        assert_approx_f64(x, 0.0);
        assert_approx_f64(y, 11.313);
    }

    #[test]
    fn default_config() {
        //assert!(Config::default().is_valid());
    }

    #[test]
    fn bad_configs() {
        fn check(good: bool, x0: i32, x1: i32, y0: i32, y1: i32) {
            let mut conf = Config::default();
            conf.x_range = (Fixed::from_num(x0), Fixed::from_num(x1));
            conf.y_range = (Fixed::from_num(y0), Fixed::from_num(y1));
            assert_eq!(good, conf.is_valid());
        }

        // Shoulder doesn't go back far enough to reach all of y=2...
        check(false, -8, 8, 2, 13);
        // ...but if we chop off part of the x axis, it's ok.
        check(true, 4, 8, 2, 13);

        // Can't reach the corners of y=14.
        check(false, -8, 8, 5, 14);
        // The right-hand edge is too far for the shoulder.
        check(false, 4, 14, 3, 4);
    }
}
