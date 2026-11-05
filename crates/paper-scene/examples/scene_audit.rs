use paper_scene::audit::{audit_workshop, evaluate_scene, render_json, render_markdown};
use std::path::Path;

const EVALUATE_FLAG: &str = "--evaluate";

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: scene_audit <workshop-root> [--markdown] | --evaluate <scene-dir>");
        std::process::exit(2);
    };
    if root == EVALUATE_FLAG {
        let Some(dir) = args.next() else {
            eprintln!("usage: scene_audit --evaluate <scene-dir>");
            std::process::exit(2);
        };
        match evaluate_scene(Path::new(&dir)) {
            Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
            Err(error) => {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    let markdown = args.next().as_deref() == Some("--markdown");
    let totals = audit_workshop(Path::new(&root));
    if markdown {
        print!("{}", render_markdown(&totals));
    } else {
        println!("{}", serde_json::to_string_pretty(&render_json(&totals)).unwrap());
    }
}
