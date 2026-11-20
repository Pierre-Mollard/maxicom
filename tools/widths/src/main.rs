//! Writes `src/glyphs/unsure_width.rs`: the chars a host terminal may draw wider or narrower than
//! ratatui counts them (MAX-271). See the README.

use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use unicode_width::UnicodeWidthChar;

/// The Unicode version hosts are taken to know: glibc 2.39's (Ubuntu 24.04), where tmux was
/// measured. An older host may still drift on the chars added since, the redraw key fixes it
const OLD: &str = "15.1.0";
const NEW: &str = "18.0.0";
/// Same as the pin in Cargo.toml
const UW: &str = "0.2.3";

/// Chars a known host draws with another width than its Unicode data says: glibc (and tmux)
/// make the circled numbers on black squares wide, East Asian Width says ambiguous
const QUIRKS: &[(u32, u32)] = &[(0x3248, 0x324F)];

/// One Unicode version's data: general category, East Asian Width, prepended concatenation marks
struct Ucd {
    category: HashMap<u32, String>,
    wide: HashMap<u32, bool>,
    prepended: HashMap<u32, bool>,
}

/// `start..=end ; value # comment` lines, `value` being the first word after `;`
fn ranges(path: &str) -> Vec<(u32, u32, String)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}, run fetch.sh"));
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((cps, value)) = line.split_once(';') else {
            continue;
        };
        let (start, end) = cps
            .trim()
            .split_once("..")
            .unwrap_or((cps.trim(), cps.trim()));
        let start = u32::from_str_radix(start, 16).unwrap();
        let end = u32::from_str_radix(end, 16).unwrap();
        out.push((start, end, value.trim().to_string()));
    }
    out
}

fn load(dir: &str) -> Ucd {
    let mut ucd = Ucd {
        category: HashMap::new(),
        wide: HashMap::new(),
        prepended: HashMap::new(),
    };
    for (start, end, gc) in ranges(&format!("{dir}/DerivedGeneralCategory.txt")) {
        for cp in start..=end {
            ucd.category.insert(cp, gc.clone());
        }
    }
    for (start, end, eaw) in ranges(&format!("{dir}/EastAsianWidth.txt")) {
        for cp in start..=end {
            ucd.wide.insert(cp, eaw == "W" || eaw == "F");
        }
    }
    for (start, end, prop) in ranges(&format!("{dir}/PropList.txt")) {
        if prop == "Prepended_Concatenation_Mark" {
            for cp in start..=end {
                ucd.prepended.insert(cp, true);
            }
        }
    }
    ucd
}

/// The width a wcwidth-style host gives `cp` with this version's data (glibc's rules, which
/// tmux follows): marks and format chars 0 but the prepended marks and the soft hyphen,
/// Hangul medial and final jamo 0, East Asian Wide and Fullwidth 2, the rest and the code
/// points this version doesn't know 1
fn host(ucd: &Ucd, cp: u32) -> Option<u8> {
    let gc = ucd.category.get(&cp).map_or("Cn", String::as_str);
    if gc == "Cn" {
        return None;
    }
    let zero = matches!(gc, "Mn" | "Me")
        || (gc == "Cf" && cp != 0xAD && !ucd.prepended.contains_key(&cp))
        || (0x1160..=0x11FF).contains(&cp)
        || (0xD7B0..=0xD7FF).contains(&cp)
        || cp == 0x200B;
    Some(if zero {
        0
    } else if ucd.wide.get(&cp) == Some(&true) {
        2
    } else {
        1
    })
}

/// Why a host may draw `c` with another width than ratatui, `None` when every host agrees: its
/// width under today's data differs from `unicode-width`'s or from the width under `OLD`'s
/// data (hosts lag behind and give 1 column to every code point they don't know, ideographs
/// included), or it isn't assigned and `unicode-width` doesn't give it the 1 column hosts give
/// unknown chars, or it's one of the `QUIRKS`
fn unsure(old: &Ucd, new: &Ucd, c: char) -> Option<String> {
    let ours = c.width().unwrap_or(0) as u8;
    let cp = c as u32;
    if QUIRKS
        .iter()
        .any(|&(start, end)| (start..=end).contains(&cp))
    {
        return Some(format!("quirk: ratatui {ours}, glibc 2"));
    }
    match (host(old, cp), host(new, cp)) {
        (_, None) if ours != 1 => Some(format!("unassigned: ratatui {ours}, hosts 1")),
        (None, Some(now)) if ours != now || now != 1 => Some(format!(
            "new since {OLD}: ratatui {ours}, host {now}, older hosts 1"
        )),
        (_, Some(now)) if ours != now => Some(format!("today: ratatui {ours}, host {now}")),
        (Some(then), Some(now)) if then != now => Some(format!(
            "changed since {OLD}: ratatui {ours}, host {now}, older hosts {then}"
        )),
        _ => None,
    }
}

/// Assigned chars' names from UnicodeData.txt, a `<..., First>` / `<..., Last>` pair naming
/// every char between them
fn names(dir: &str) -> HashMap<u32, String> {
    let path = format!("{dir}/UnicodeData.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}, run fetch.sh"));
    let mut out = HashMap::new();
    let mut first = None;
    for line in text.lines() {
        let mut fields = line.split(';');
        let cp = u32::from_str_radix(fields.next().unwrap(), 16).unwrap();
        let name = fields.next().unwrap();
        if let Some(range) = name.strip_suffix(", First>") {
            first = Some((cp, range.trim_start_matches('<').to_string()));
        } else if name.ends_with(", Last>") {
            let (start, range) = first.take().unwrap();
            for c in start..=cp {
                out.insert(c, format!("{range} {c:04X}"));
            }
        } else {
            out.insert(cp, name.to_string());
        }
    }
    out
}

/// `--list`: each assigned unsure char with its name and the rule that caught it, then how
/// many unassigned code points are in the table
fn list(old: &Ucd, new: &Ucd) {
    let names = names(&format!("target/ucd/{NEW}"));
    let mut unassigned = 0;
    for c in (0xA0..=0x10FFFF).filter_map(char::from_u32) {
        let Some(why) = unsure(old, new, c) else {
            continue;
        };
        let Some(name) = names.get(&(c as u32)) else {
            unassigned += 1;
            continue;
        };
        // a zero width char is shown on a dotted circle, as Unicode charts do
        let shown = if c.width() == Some(0) {
            format!("\u{25CC}{c}")
        } else {
            c.to_string()
        };
        println!("U+{:05X}  {shown}\t{name}  ({why})", c as u32);
    }
    println!("and {unassigned} unassigned code points");
}

fn main() {
    let old = load(&format!("target/ucd/{OLD}"));
    let new = load(&format!("target/ucd/{NEW}"));
    if std::env::args().any(|a| a == "--list") {
        list(&old, &new);
        return;
    }

    let mut runs: Vec<(u32, u32)> = Vec::new();
    for c in (0xA0..=0x10FFFF).filter_map(char::from_u32) {
        if unsure(&old, &new, c).is_none() {
            continue;
        }
        let cp = c as u32;
        match runs.last_mut() {
            // the surrogates between D7FF and E000 aren't chars: a run can step over them
            Some((_, end)) if *end + 1 == cp || (*end == 0xD7FF && cp == 0xE000) => *end = cp,
            _ => runs.push((cp, cp)),
        }
    }
    let count: u32 = runs.iter().map(|(s, e)| e - s + 1).sum();

    let mut out = String::new();
    writeln!(
        out,
        "//! Chars a host terminal may draw wider or narrower than ratatui counts them (MAX-271)"
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(out, "//! Generated by `tools/widths` (Unicode {OLD} and {NEW} data, unicode-width {}), don't edit:", UW).unwrap();
    writeln!(out, "//! {count} code points in {} ranges", runs.len()).unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// Whether a host may give `c` another width than ratatui"
    )
    .unwrap();
    writeln!(out, "pub fn is_unsure(c: char) -> bool {{").unwrap();
    writeln!(out, "    let cp = u32::from(c);").unwrap();
    writeln!(
        out,
        "    // below the first range (ASCII, Latin-1 but the soft hyphen): no search"
    )
    .unwrap();
    writeln!(out, "    if cp < RANGES[0].0 {{").unwrap();
    writeln!(out, "        return false;").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    RANGES").unwrap();
    writeln!(out, "        .binary_search_by(|&(start, end)| {{").unwrap();
    writeln!(out, "            if end < cp {{").unwrap();
    writeln!(out, "                std::cmp::Ordering::Less").unwrap();
    writeln!(out, "            }} else if start > cp {{").unwrap();
    writeln!(out, "                std::cmp::Ordering::Greater").unwrap();
    writeln!(out, "            }} else {{").unwrap();
    writeln!(out, "                std::cmp::Ordering::Equal").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }})").unwrap();
    writeln!(out, "        .is_ok()").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "const RANGES: &[(u32, u32)] = &[").unwrap();
    for chunk in runs.chunks(4) {
        let row: Vec<String> = chunk
            .iter()
            .map(|(s, e)| format!("(0x{s:05X}, 0x{e:05X})"))
            .collect();
        writeln!(out, "    {},", row.join(", ")).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "#[cfg(test)]").unwrap();
    writeln!(out, "#[path = \"../tests/glyphs/unsure_width.rs\"]").unwrap();
    writeln!(out, "mod tests;").unwrap();
    fs::write("src/glyphs/unsure_width.rs", out).unwrap();
    eprintln!(
        "src/glyphs/unsure_width.rs: {count} code points in {} ranges",
        runs.len()
    );
}
