use brachiograph::geom::Config;
use brachiograph::{Angle, Angles};
use svg::node::element::{path::Data, Path, Text};
use svg::Document;

fn line() -> Path {
    Path::new()
        .set("fill", "none")
        .set("stroke", "black")
        .set("stroke-width", 0.05)
}

fn tick(center: (f64, f64), p: (f64, f64), r: f64, s: &'static str) -> (Path, Text) {
    let v0 = (p.0 - center.0) / r;
    let v1 = (p.1 - center.1) / r;
    let d = Data::new()
        .move_to(p)
        .line_to((p.0 + v0 * 0.4, p.1 + v1 * 0.4));
    let path = line().set("d", d);
    let text = Text::new()
        .set("x", p.0 + v0 * 0.7)
        .set("y", p.1 + v1 * 0.7)
        .set("font-size", 0.5)
        .add(svg::node::Text::new(s));
    (path, text)
}

fn main() {
    let config = Config::default();
    let zero = Angle::from_degrees(0);

    let shoulder_calib_start = Angles {
        shoulder: config.shoulder_range.0,
        elbow: zero,
    };
    let shoulder_calib_end = Angles {
        shoulder: config.shoulder_range.1,
        elbow: zero,
    };
    let (x, y): (f64, f64) = config.coord_at_angle(shoulder_calib_start);
    let (x_to, y_to): (f64, f64) = config.coord_at_angle(shoulder_calib_end);
    let r = 128.0f64.sqrt();

    let data = Data::new()
        .move_to((-12.0, -0.0))
        .line_to((12.0, -0.0))
        .move_to((0.0, 1.0))
        .line_to((0.0, -15.0));

    let axes = line().set("d", data).set("stroke-width", "0.01");

    let data = Data::new()
        .move_to((x, -y))
        .elliptical_arc_to((r, r, 0.0, 0.0, 1.0, x_to, -y_to));
    let shoulder_path = line().set("d", data);

    let fortyfive = Angle::from_degrees(45);
    let elbow_calib_start = Angles {
        shoulder: fortyfive,
        elbow: config.elbow_range.0,
    };
    let elbow_calib_end = Angles {
        shoulder: fortyfive,
        elbow: config.elbow_range.1,
    };

    let (x, y): (f64, f64) = config.coord_at_angle(elbow_calib_start);
    let (x_to, y_to): (f64, f64) = config.coord_at_angle(elbow_calib_end);
    let r = 8.0;
    let data = Data::new()
        .move_to((x, -y))
        .elliptical_arc_to((r, r, 0.0, 0.0, 0.0, x_to, -y_to));
    let elbow_path = line().set("d", data);

    let mut document = Document::new()
        .set("viewBox", (-12, -15, 24, 16))
        .set("width", "24cm")
        .set("height", "16cm")
        .add(axes)
        .add(shoulder_path)
        .add(elbow_path);

    let names = &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
    for (angle, name) in ((config.shoulder_range.0.degrees().to_num::<i32>()
        ..=config.shoulder_range.1.degrees().to_num())
        .step_by(15))
    .zip(names)
    {
        let r = 128.0f64.sqrt();
        let angle = Angles {
            shoulder: Angle::from_degrees(angle),
            elbow: zero,
        };
        let (x, y) = config.coord_at_angle(angle);
        let (path, text) = tick((0.0, 0.0), (x, -y), r, name);
        document = document.add(path).add(text);
    }

    let names = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
    for (angle, name) in ((config.elbow_range.0.degrees().to_num::<i32>()
        ..=config.elbow_range.1.degrees().to_num())
        .step_by(15))
    .zip(names)
    {
        let r = 8.0;
        let c = (-8.0 / 2.0f64.sqrt(), -8.0 / 2.0f64.sqrt());
        let angle = Angles {
            shoulder: fortyfive,
            elbow: Angle::from_degrees(angle),
        };
        let (x, y) = config.coord_at_angle(angle);
        let (path, text) = tick(c, (x, -y), r, name);
        document = document.add(path).add(text);
    }

    svg::save("image.svg", &document).unwrap();
}
