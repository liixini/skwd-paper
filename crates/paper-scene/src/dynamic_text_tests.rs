use super::{LocalTime, media_bound, media_scripted, resolve, substitute, weekday_of};
use serde_json::json;

fn now() -> LocalTime {
    LocalTime { hour: 14, minute: 7, second: 9, day: 8, month: 9, year: 2026, weekday: 2 }
}

#[test]
fn zeller_agrees_with_the_dates_the_corpus_placeholders_encode() {
    assert_eq!(weekday_of(2021, 2, 1), 1);
    assert_eq!(weekday_of(2026, 9, 8), 2);
    assert_eq!(weekday_of(2000, 1, 1), 6);
}

#[test]
fn a_clock_placeholder_keeps_its_separator_padding_and_meridiem() {
    assert_eq!(substitute("12:34", now()).as_deref(), Some("14:07"));
    assert_eq!(substitute("12:34:56", now()).as_deref(), Some("14:07:09"));
    assert_eq!(substitute("12.34", now()).as_deref(), Some("14.07"));
    assert_eq!(substitute("10:45 pm", now()).as_deref(), Some("02:07 pm"));
    assert_eq!(substitute("10:45 PM", now()).as_deref(), Some("02:07 PM"));
    let morning = LocalTime { hour: 9, ..now() };
    assert_eq!(substitute("10:45 pm", morning).as_deref(), Some("09:07 am"));
}

#[test]
fn a_date_placeholder_keeps_its_layout_and_resolves_day_month_order_by_its_own_weekday() {
    assert_eq!(
        substitute("2021 | 02 | 01 | Monday", now()).as_deref(),
        Some("2026 | 09 | 08 | Tuesday")
    );
    assert_eq!(substitute("DAY", now()).as_deref(), Some("TUESDAY"));
    assert_eq!(substitute("| DAY |", now()).as_deref(), Some("| TUESDAY |"));
    assert_eq!(substitute("Mon", now()).as_deref(), Some("Tue"));
    assert_eq!(substitute("28 Jan 2024", now()).as_deref(), Some("08 Sep 2026"));
}

#[test]
fn text_without_a_recognisable_date_or_clock_is_left_alone() {
    for inert in ["Text Layer", "Song Titel", "Artist", "", "1\n5\n\nN\nO\nV"] {
        assert_eq!(substitute(inert, now()), None, "{inert} must not be rewritten");
    }
}

#[test]
fn generic_placeholder_tokens_and_stacked_letters_become_live() {
    assert_eq!(substitute("<Clock>", now()).as_deref(), Some("14:07"));
    assert_eq!(substitute("<Date>", now()).as_deref(), Some("2026-09-08"));
    assert_eq!(substitute("S\nU\nN", now()).as_deref(), Some("T\nU\nE"));
    assert_eq!(substitute("s\nu\nn", now()).as_deref(), Some("t\nu\ne"));
    assert_eq!(substitute("X\nY\nZ", now()), None, "only real day and month names stack");
    assert_eq!(
        substitute("1\n5\n\nN\nO\nV", now()),
        None,
        "a stacked date with digits would leave a stale day beside a fresh month"
    );
}

#[test]
fn the_update_cadence_follows_the_finest_unit_the_placeholder_shows() {
    use super::{Cadence, cadence, seconds_until_next};
    assert_eq!(cadence("12:34:56"), Cadence::Second);
    assert_eq!(cadence("12:34"), Cadence::Minute);
    assert_eq!(cadence("<Clock>"), Cadence::Minute);
    assert_eq!(cadence("2021 | 02 | 01 | Monday"), Cadence::Day);
    assert_eq!(cadence("DAY"), Cadence::Day);
    assert_eq!(cadence("<Date>"), Cadence::Day);
    assert!(Cadence::Second < Cadence::Minute && Cadence::Minute < Cadence::Day);

    let at = LocalTime { hour: 18, minute: 2, second: 47, ..now() };
    assert_eq!(seconds_until_next(Cadence::Second, at), 1.0);
    assert_eq!(seconds_until_next(Cadence::Minute, at), 13.0);
    assert_eq!(seconds_until_next(Cadence::Day, at), 21433.0);
    let midnight = LocalTime { hour: 0, minute: 0, second: 0, ..now() };
    assert_eq!(seconds_until_next(Cadence::Day, midnight), 86_400.0);
    assert!(seconds_until_next(Cadence::Minute, LocalTime { second: 59, ..at }) >= 1.0);
}

#[test]
fn media_bound_text_goes_blank_because_the_engine_shows_nothing_without_a_player() {
    let song = json!({
        "script": "export function mediaPropertiesChanged(event) { mediaData = event.title; }",
        "value": " \u{266a} Nobuo Uematsu"
    });
    assert_eq!(resolve(Some(&song), now()).as_deref(), Some(""));
    assert!(media_bound("applyUserProperties(); mediaPlaybackChanged(e)"));
    assert!(!media_bound("export function update() { return new Date(); }"));

    let clock = json!({"script": "getHours()", "value": "12:34"});
    assert_eq!(resolve(Some(&clock), now()).as_deref(), Some("14:07"), "clocks are untouched");

    let faded = json!({"script": "export function mediaPlaybackChanged(e) {}", "value": 1.0});
    assert!(media_scripted(Some(&faded)), "a media-bound alpha hides its layer");
    assert!(!media_scripted(Some(&json!({"script": "return 1.0;", "value": 1.0}))));
    assert!(!media_scripted(Some(&json!(1.0))), "a plain number is never media bound");
    assert!(!media_scripted(None));
}

#[test]
fn the_scripts_own_weekday_table_wins_because_it_carries_the_authors_spacing() {
    use super::{substitute_with, weekday_table};
    let script = "const day = ['S U N D A Y','M O N D A Y','T U E S D A Y','W E D N E S D A Y','T H U R S D A Y','F R I D A Y','S A T U R D A Y']; getDay()";
    let table = weekday_table(script).expect("table");
    assert_eq!(table[2], "T U E S D A Y");
    assert_eq!(substitute_with("DAY", now(), Some(&table)).as_deref(), Some("T U E S D A Y"));
    assert_eq!(
        substitute_with("| DAY |", now(), Some(&table)).as_deref(),
        Some("T U E S D A Y"),
        "the table replaces the whole run, decoration included"
    );

    let monday_first = "['Lunes','Martes','Miercoles','Jueves','Viernes','Sabado','Domingo']";
    assert!(weekday_table(monday_first).is_none(), "only English names are recognised");

    let rotated = "['Monday','Tuesday','Wednesday','Thursday','Friday','Saturday','Sunday']";
    assert_eq!(weekday_table(rotated).expect("rotated")[2], "Tuesday");

    assert!(weekday_table("no arrays here").is_none());
    assert_eq!(
        substitute_with("2021 | 02 | 01 | Monday", now(), Some(&table)).as_deref(),
        Some("2026 | 09 | 08 | Tuesday"),
        "a full date keeps our own formatting"
    );
}

#[test]
fn resolve_only_touches_scripted_text_and_only_when_the_value_actually_changes() {
    let scripted = json!({"script": "export function update() {}", "value": "12:34"});
    assert_eq!(resolve(Some(&scripted), now()).as_deref(), Some("14:07"));

    let plain = json!("12:34");
    assert_eq!(resolve(Some(&plain), now()), None, "a static string is never a clock");

    let unscripted = json!({"value": "12:34"});
    assert_eq!(resolve(Some(&unscripted), now()), None, "no script means no substitution");

    let already = json!({"script": "x", "value": "14:07"});
    assert_eq!(resolve(Some(&already), now()), None, "an unchanged value stays the placeholder");
}

fn clock_script() -> &'static str {
    "let delimiter = ':';\nlet showSeconds = true;\nlet use24hFormat = true;\n\
     export function update(value) { let time = new Date();\n\
     let hours = time.getHours(); let minutes = time.getMinutes();\n\
     value = hours + delimiter + minutes; value += ' ' + suffix; return value; }"
}

#[test]
fn scripted_clock_uses_its_format_instead_of_the_editor_placeholder() {
    for placeholder in ["<3D Clock>", "12:34"] {
        let text = json!({"script": clock_script(), "value": placeholder});
        let clock = super::clock::Clock::from_text(&text).unwrap();
        assert_eq!(clock.cadence(), super::Cadence::Second);
        assert_eq!(resolve(Some(&text), now()).as_deref(), Some("14:07:09"));
        let boundary = LocalTime { hour: 20, minute: 44, second: 59, ..now() };
        assert_eq!(clock.value(boundary), "20:44:59");
        assert_eq!(clock.value(LocalTime { minute: 45, second: 0, ..boundary }), "20:45:00");
    }
}

#[test]
fn scripted_clock_honors_saved_settings_and_twelve_hour_boundaries() {
    let text = json!({"script": clock_script(), "value": "<3D Clock>",
        "scriptproperties": {"delimiter": " / ", "showSeconds": false, "use24hFormat": false}});
    let clock = super::clock::Clock::from_text(&text).unwrap();
    assert_eq!(clock.cadence(), super::Cadence::Minute);
    assert_eq!(clock.value(LocalTime { hour: 0, minute: 0, ..now() }), "12 / 00 AM");
    assert_eq!(clock.value(LocalTime { hour: 12, minute: 0, ..now() }), "12 / 00 PM");
    assert_eq!(clock.value(now()), "2 / 07 PM");
}

#[test]
fn clock_property_builder_defaults_apply_without_saved_overrides() {
    let script = "export var scriptProperties = createScriptProperties()\n\
        .addCheckbox({name: 'use24hFormat', value: true})\n\
        .addCheckbox({name: 'showSeconds', value: false})\n\
        .addText({name: 'delimiter', value: ':'}).finish();\n\
        export function update(value) { let time = new Date();\n\
        let hours = time.getHours(); let minutes = time.getMinutes();\n\
        return hours + scriptProperties.delimiter + minutes; }";
    let text = json!({"script": script, "value": "12:34"});
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("14:07"));
    let text = json!({"script": script, "value": "12:34", "scriptproperties": {
        "use24hFormat": false, "showSeconds": true, "delimiter": "."}});
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("02.07.09"));
}

#[test]
fn naming_static_text_clock_does_not_make_it_a_clock() {
    for text in [
        json!("<3D Clock>"),
        json!({"value": "<3D Clock>",
        "script": "export function update(value) { return value; }"}),
    ] {
        assert!(super::clock::Clock::from_text(&text).is_none());
        assert!(resolve(Some(&text), now()).is_none());
    }
}

fn date_text() -> serde_json::Value {
    json!({"value": "- Date -", "script": "let currentLocale = 'fr-FR';\n\
        function getLocaleData(locale) { let lang = String(locale).toLowerCase();\n\
        if (lang.indexOf('fr') === 0) { return {\n\
        monthsAbbr: ['janv','févr','mars','avr','mai','juin','juil','août','sept','oct','nov','déc'],\n\
        monthsFull: ['janvier','février','mars','avril','mai','juin','juillet','août','septembre','octobre','novembre','décembre'],\n\
        daysAbbr: ['dim','lun','mar','mer','jeu','ven','sam'],\n\
        daysFull: ['dimanche','lundi','mardi','mercredi','jeudi','vendredi','samedi'] }; } }\n\
        export function update(value) { let localeData = getLocaleData(currentLocale);\n\
        let dayText = formatSpaced(dayText); return date.getDate() + date.getFullYear(); }",
        "scriptproperties": {"monthFormat": "3", "dayFormat": "2", "showDay": false,
            "alignVertical": false, "useDelimiter": true, "addDelimiter": "."}})
}

#[test]
fn localized_date_uses_the_scripts_own_month_names_and_saved_delimiter() {
    let text = date_text();
    let date = super::date::Date::from_text(&text).unwrap();
    assert_eq!(date.value(LocalTime { day: 11, ..now() }), "11.septembre.2026");
    assert_eq!(date.value(LocalTime { month: 2, day: 1, ..now() }), "1.février.2026");
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("8.septembre.2026"));
    assert_eq!(date.value(LocalTime { day: 1, month: 1, year: 2027, ..now() }), "1.janvier.2027");
}

#[test]
fn localized_date_preserves_numeric_abbreviated_and_weekday_formats() {
    let mut text = date_text();
    text["scriptproperties"]["monthFormat"] = json!("1");
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("8.9.2026"));
    text["scriptproperties"]["monthFormat"] = json!("2");
    text["scriptproperties"]["useDelimiter"] = json!(false);
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("8 sept 2026"));
    text["scriptproperties"]["showDay"] = json!(true);
    text["scriptproperties"]["alignVertical"] = json!(true);
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("| M A R D I |\n"));
    text["scriptproperties"]["dayFormat"] = json!("1");
    text["scriptproperties"]["alignVertical"] = json!(false);
    assert_eq!(resolve(Some(&text), now()).as_deref(), Some("M A R"));
}
