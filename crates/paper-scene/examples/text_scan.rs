use paper_scene::dynamic_text::{LocalTime, resolve};
use paper_scene::pkg::Package;
use serde_json::Value;

fn main() {
    let now =
        LocalTime { hour: 14, minute: 7, second: 9, day: 8, month: 9, year: 2026, weekday: 2 };
    let (mut total, mut live, mut blank) = (0usize, 0usize, 0usize);
    let mut misses: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for dir in std::env::args().skip(1) {
        let Ok(pkg) = Package::open(&std::path::Path::new(&dir).join("scene.pkg")) else {
            continue;
        };
        let Ok(Some(scene)) = pkg.find_json("scene.json") else { continue };
        let Some(objects) = scene.get("objects").and_then(Value::as_array) else { continue };
        for object in objects {
            let Some(text @ Value::Object(map)) = object.get("text") else { continue };
            if !map.contains_key("script") {
                continue;
            }
            total += 1;
            match resolve(Some(text), now) {
                Some(value) if value.is_empty() => blank += 1,
                Some(_) => live += 1,
                None => {
                    let placeholder = paper_scene::text::text_value(map.get("value"))
                        .unwrap_or_default()
                        .replace('\n', "\\n");
                    *misses.entry(placeholder.chars().take(24).collect()).or_default() += 1;
                }
            }
        }
    }
    println!(
        "scripted text {total}: live {live} ({:.0}%), blank by design {blank}, frozen {}",
        100.0 * live as f64 / total.max(1) as f64,
        total - live - blank
    );
    let mut ranked: Vec<_> = misses.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (placeholder, count) in ranked.iter().take(12) {
        println!("  still frozen x{count:<4} {placeholder:?}");
    }
}
