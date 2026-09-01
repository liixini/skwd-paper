use paper_scene::audit::{audit_workshop, render_json, render_markdown};
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: scene_audit <workshop-root> [--markdown]");
        std::process::exit(2);
    };
    let markdown = args.next().as_deref() == Some("--markdown");
    let totals = audit_workshop(Path::new(&root));
    if markdown {
        print!("{}", render_markdown(&totals));
    } else {
        println!("{}", serde_json::to_string_pretty(&render_json(&totals)).unwrap());
    }
}
