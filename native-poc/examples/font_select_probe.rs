//! Probe: dump every fontdb face matching a family name and show which
//! face the resolver's weight-700 (bold) and weight-400 (base) queries
//! select. Used to debug bold-face selection against families that ship
//! many width variants (Google's Inconsolata installs 81 static faces).
//!
//! Run: CARGO_TARGET_DIR=native-poc/target cargo run --manifest-path \
//!      native-poc/Cargo.toml --example font_select_probe -- Inconsolata

fn face_match_penalty(
    weight: u16,
    target_weight: u16,
    style_normal: bool,
    stretch_normal: bool,
) -> u32 {
    let stretch_penalty: u32 = if stretch_normal { 0 } else { 2000 };
    let style_penalty: u32 = if style_normal { 0 } else { 1000 };
    let weight_dist = (weight as i32 - target_weight as i32).unsigned_abs();
    stretch_penalty + style_penalty + weight_dist
}

fn main() {
    let family = std::env::args().nth(1).unwrap_or("Inconsolata".into());
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut matches: Vec<_> = db
        .faces()
        .filter(|f| {
            f.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(&family))
        })
        .collect();
    matches.sort_by_key(|f| (f.stretch as u8, f.weight.0));
    println!(
        "=== faces matching family {:?} ({}) ===",
        family,
        matches.len()
    );
    for f in &matches {
        let p400 = face_match_penalty(
            f.weight.0,
            400,
            f.style == fontdb::Style::Normal,
            f.stretch == fontdb::Stretch::Normal,
        );
        let p700 = face_match_penalty(
            f.weight.0,
            700,
            f.style == fontdb::Style::Normal,
            f.stretch == fontdb::Stretch::Normal,
        );
        println!(
            "weight={:<4} stretch={:<15} style={:<8} p400={:<5} p700={:<5} families={:?} src={:?}",
            f.weight.0,
            format!("{:?}", f.stretch),
            format!("{:?}", f.style),
            p400,
            p700,
            f.families
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            f.source,
        );
    }
    for (label, target, min) in [
        ("base(400)", 400u16, None),
        ("bold(700)", 700u16, Some(600u16)),
    ] {
        let face = matches.iter().min_by_key(|f| {
            face_match_penalty(
                f.weight.0,
                target,
                f.style == fontdb::Style::Normal,
                f.stretch == fontdb::Stretch::Normal,
            )
        });
        match face {
            Some(f) if min.is_none_or(|m| f.weight.0 >= m) => {
                println!(
                    "\n--> {} selects: weight={} stretch={:?} src={:?}",
                    label, f.weight.0, f.stretch, f.source
                );
            }
            _ => println!("\n--> {} selects: NONE", label),
        }
    }
}
