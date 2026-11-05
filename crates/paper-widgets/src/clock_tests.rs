use paper_control::Palette;

use paper_control::Labels;

use super::*;
use crate::lettering::Lettering;
use crate::scene::Moment;
use crate::tones::Tones;

use crate::FONT;
const PALETTE: Palette =
    Palette { text: [236, 235, 231], accent: [90, 110, 200], shade: [18, 19, 22] };
const STYLES: [ClockStyle; 5] = [
    ClockStyle::Aperture,
    ClockStyle::ApertureRing,
    ClockStyle::Greeter,
    ClockStyle::ControlCenter,
    ClockStyle::DayDial,
];

fn labels() -> Labels {
    let names = |list: &str| list.split(' ').map(str::to_string).collect::<Vec<_>>();
    Labels {
        days: names("MON TUE WED THU FRI SAT SUN").try_into().unwrap(),
        day_names: names("MONDAY TUESDAY WEDNESDAY THURSDAY FRIDAY SATURDAY SUNDAY")
            .try_into()
            .unwrap(),
        months: names("JAN FEB MAR APR MAY JUN JUL AUG SEP OCT NOV DEC").try_into().unwrap(),
        month_names: names(
            "JANUARY FEBRUARY MARCH APRIL MAY JUNE JULY AUGUST SEPTEMBER OCTOBER NOVEMBER DECEMBER",
        )
        .try_into()
        .unwrap(),
    }
}

fn render(style: ClockStyle, moment: Moment) -> Canvas {
    let lettering = Lettering::open(FONT).unwrap();
    let labels = labels();
    let scene = Scene {
        lettering: &lettering,
        tones: Tones::over(&PALETTE, 0.02),
        moment,
        labels: &labels,
    };
    let mut canvas = Canvas::scaled(frame(style), 1.0);
    draw(style, &mut canvas, &scene);
    canvas
}

const EVENING: Moment = Moment { hour: 21, minute: 47, weekday: 1, day: 27, month: 9 };

fn inked_rows(canvas: &Canvas) -> Vec<usize> {
    (0..canvas.height)
        .filter(|&y| (0..canvas.width).any(|x| canvas.pixels[y * canvas.width + x][3] > 128))
        .collect()
}

#[test]
fn every_style_draws() {
    for style in STYLES {
        let canvas = render(style, EVENING);
        assert!(canvas.pixels.iter().any(|pixel| pixel[3] > 0), "{style:?}");
    }
}

#[test]
fn aperture_rows_spaced() {
    let rows = inked_rows(&render(ClockStyle::Aperture, EVENING));
    let gap = rows.windows(2).map(|pair| pair[1] - pair[0]).max().unwrap();
    assert!((8..=20).contains(&gap), "gap {gap}");
}

#[test]
fn day_share() {
    assert_eq!(EVENING.share(), 1307.0 / 1440.0);
    assert_eq!(Moment { hour: 0, minute: 0, ..EVENING }.share(), 0.0);
}

#[test]
fn hatch_follows_time() {
    let shade = Tones::over(&PALETTE, 0.02).shade.rgb;
    let hatched = |canvas: &Canvas| {
        canvas.pixels.iter().filter(|pixel| pixel[3] == u8::MAX && pixel[..3] != shade).count()
    };
    let morning = render(ClockStyle::DayDial, Moment { hour: 6, minute: 0, ..EVENING });
    assert!(hatched(&render(ClockStyle::DayDial, EVENING)) > hatched(&morning));
}

#[test]
fn greeter_splits_colour() {
    let canvas = render(ClockStyle::Greeter, EVENING);
    let accent = Tones::over(&PALETTE, 0.02).accent.rgb;
    let right = canvas.width - 1;
    let accented = (0..canvas.height * canvas.width)
        .filter(|index| index % canvas.width < right && canvas.pixels[*index][3] == u8::MAX)
        .any(|index| canvas.pixels[index][..3] == accent);
    assert!(accented);
}

#[test]
fn scale_doubles_pixels() {
    let lettering = Lettering::open(FONT).unwrap();
    let labels = labels();
    let scene = Scene {
        lettering: &lettering,
        tones: Tones::over(&PALETTE, 0.02),
        moment: EVENING,
        labels: &labels,
    };
    let mut canvas = Canvas::scaled(frame(ClockStyle::Aperture), 2.0);
    draw(ClockStyle::Aperture, &mut canvas, &scene);
    let single = inked_rows(&render(ClockStyle::Aperture, EVENING));
    let double = inked_rows(&canvas);
    let span = |rows: &[usize]| rows[rows.len() - 1] - rows[0];
    assert!((span(&double) as f32 / span(&single) as f32 - 2.0).abs() < 0.05);
}
