//! The disc's assets, read natively, for stage 4's checks (`docs/loop.md`, the twelfth loop).
//!
//! Usage: assets ISO arcs [LIST]  every .arc on the disc: its layout read, its entry count; with
//!                                LIST, each entry ("archive<TAB>name<TAB>offset<TAB>size") into it
//!        assets ISO textures         textures.arc's entries read: by kind and size, any that do not
//!        assets ISO rle              rletextu.arc's entries decoded: their texel counts
//!        assets ISO texbatch IN OUT  every texture (textures.arc's and
//!                                    datasets.arc's), its first level: IN the GX format, width,
//!                                    height, size, palette size, texels and palette (IA8) of each (little-endian words), OUT
//!                                    its texels decoded natively (RGBA words), for
//!                                    tools/texdecode_check.sh
//!        assets ISO tlutcheck LOG    the palettes the original set up (the log's 'u') against
//!                                    the textures' palettes (study for now)
//!        assets ISO stripcheck LOG   the strips of short arrays the original played (the log's
//!                                    'w') against models.arc: where each array's bytes are
//!        assets ISO modelcheck LOG   models.arc's records read (their first sub-model) and
//!                                    checked against the strips the original plays from them
//!        assets ISO texcheck LOG     the textures the original set up (the log's 'x' records)
//!                                    against textures.arc's: each by its first level's hash

use std::io::Write;
use std::process::ExitCode;

use housefire::arc;
use housefire::disc::Disc;
use housefire::model;
use housefire::texture;
use housefire::rle_texture;
use housefire::quickdata;
use housefire::scene;
use housefire::shader;
use housefire::skeleton;
use housefire::tree_log::read_log;
use std::collections::{BTreeMap, BTreeSet};

type TexByHash = BTreeMap<u64, Vec<(String, texture::Kind, u32, u32)>>;

/// Every texture of textures.arc and datasets.arc by its first level's FNV-1a hash (as the
/// observer's 'x' records hash a texture's image): its name, kind and size; and how many were
/// in datasets.arc.
fn texture_hashes(d: &Disc) -> (TexByHash, usize) {
    let data = d.read("textures.arc").unwrap_or_default();
    let entries = arc::entries(&data).unwrap_or_default();
    // each entry's first level, hashed as the observer does
    let fnv = |b: &[u8]| b.iter().fold(1469598103934665603u64, |h, &x| (h ^ x as u64).wrapping_mul(1099511628211));
    let mut by_hash: BTreeMap<u64, Vec<(String, texture::Kind, u32, u32)>> = BTreeMap::new();
    for e in &entries {
        let b = arc::bytes(&data, e);
        if let Some(t) = texture::read(b) {
            let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
            by_hash.entry(fnv(&b[t.texels.0..t.texels.0 + first])).or_default().push((t.name.clone(), t.kind, t.width, t.height));
        }
    }
    // and those kept in datasets.arc's entries (the lots' and objects' own)
    let ds = d.read("datasets.arc").unwrap_or_default();
    let mut embedded = 0;
    for e in arc::entries(&ds).unwrap_or_default() {
        let b = arc::bytes(&ds, &e);
        let mut p = 0;
        while let Some(q) = b[p..].windows(4).position(|w| w == b"LFXT") {
            let at = p + q;
            if let Some((t, end)) = texture::read_prefix(&b[at..]) {
                let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
                let tex = &b[at + t.texels.0..at + t.texels.0 + first];
                by_hash.entry(fnv(tex)).or_default().push((format!("{}/{}", e.name, t.name), t.kind, t.width, t.height));
                embedded += 1;
                p = at + end;
            } else {
                p = at + 4;
            }
        }
    }
    (by_hash, embedded)
}

/// Every model record read whole: those kept inside datasets.arc's entries (an object's `RD_`
/// dataset), then models.arc's; each with the archive entry it is in.
fn all_records(d: &Disc) -> Vec<(String, model::Record)> {
    let mut out = Vec::new();
    let ds = d.read("datasets.arc").unwrap_or_default();
    for e in arc::entries(&ds).unwrap_or_default() {
        let b = arc::bytes(&ds, &e);
        let mut p = 0;
        while let Some(q) = b[p..].windows(6).position(|w| w == [0, 1, 0, 0, 0, 0]) {
            let at = p + q;
            match model::read_record_prefix(&b[at..]) {
                Some((r, end)) => {
                    out.push((e.name.clone(), r));
                    p = at + end;
                }
                None => p = at + 1,
            }
        }
    }
    let data = d.read("models.arc").unwrap_or_default();
    for e in arc::entries(&data).unwrap_or_default() {
        if let Some(r) = model::read_record(arc::bytes(&data, &e)) {
            out.push((e.name.clone(), r));
        }
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: assets ISO arcs [LIST]");
        return ExitCode::from(2);
    }
    let Ok(d) = Disc::open(&args[1]) else {
        println!("cannot read {}", args[1]);
        return ExitCode::from(1);
    };
    match args[2].as_str() {
        "arcs" => {
            let mut list = args.get(3).and_then(|p| std::fs::File::create(p).ok()).map(std::io::BufWriter::new);
            let (mut read, mut not) = (0, 0);
            for path in d.files().keys().filter(|p| p.to_ascii_lowercase().ends_with(".arc")) {
                let data = d.read(path).unwrap_or_default();
                match arc::entries(&data) {
                    Some(e) => {
                        read += 1;
                        println!("{path}: {} entries", e.len());
                        if let Some(l) = list.as_mut() {
                            for x in &e {
                                let _ = writeln!(l, "{path}\t{}\t{}\t{}", x.name, x.offset, x.size);
                            }
                        }
                    }
                    None => {
                        not += 1;
                        println!("{path}: NOT an archive");
                    }
                }
            }
            println!("archives read {read}, not {not}");
        }
        "textures" => {
            let data = d.read("textures.arc").unwrap_or_default();
            let Some(entries) = arc::entries(&data) else {
                println!("textures.arc: not an archive");
                return ExitCode::from(1);
            };
            let mut by: BTreeMap<String, usize> = BTreeMap::new();
            let mut bad = 0;
            for e in &entries {
                match texture::read(arc::bytes(&data, e)) {
                    Some(t) => *by.entry(format!("{:?} {}x{} levels {} palette {:?}", t.kind, t.width, t.height, t.levels, t.palette.map(|p| p.1))).or_default() += 1,
                    None => {
                        bad += 1;
                        println!("NOT read: {}", e.name);
                    }
                }
            }
            for (k, n) in &by {
                println!("{n:5} {k}");
            }
            println!("textures read {}, not {bad}", entries.len() - bad);
        }
        "rle" => {
            let data = d.read("rletextu.arc").unwrap_or_default();
            let entries = arc::entries(&data).unwrap_or_default();
            let (mut bad, mut counts) = (0, BTreeMap::<String, usize>::new());
            for e in &entries {
                match rle_texture::read(arc::bytes(&data, e)) {
                    Some(t) => {
                        let n = t.indices().len();
                        let shape = if n.is_power_of_two() { format!("{n} (a power of two)") } else { format!("{n}") };
                        *counts.entry(format!("{} colours, {shape}", t.palette.len())).or_default() += 1;
                    }
                    None => bad += 1,
                }
            }
            for (k, n) in &counts {
                println!("{n:5} {k}");
            }
            println!("run-length textures read {}, not {bad}", entries.len() - bad);
        }
        "texbatch" => {
            let (Some(inp), Some(outp)) = (args.get(3), args.get(4)) else {
                println!("usage: assets ISO texbatch IN OUT");
                return ExitCode::from(2);
            };
            let (mut inb, mut outb) = (Vec::new(), Vec::new());
            let mut n = 0;
            let mut add = |t: &texture::Texture, b: &[u8]| {
                let fmt: u32 = match t.kind {
                    texture::Kind::Bits16 => 5,
                    texture::Kind::Blocks => 14,
                    texture::Kind::Bits32 => 6,
                    texture::Kind::Palette4 => 8,
                    texture::Kind::Palette8 => 9,
                };
                let first = (t.width as usize * t.height as usize * t.kind.bits() as usize) / 8;
                let src = &b[t.texels.0..t.texels.0 + first];
                // a palette's first half, as the game loads it (IA8)
                let pal: &[u8] = t.palette.map_or(&[], |(p, n)| &b[p..p + n * 2]);
                let px = match t.palette {
                    Some(_) => texture::decode_indexed(t.kind, t.width, t.height, src, pal, 0),
                    None => texture::decode_level(t.kind, t.width, t.height, src),
                };
                let Some(px) = px else { return };
                for v in [fmt, t.width, t.height, first as u32, pal.len() as u32] {
                    inb.extend(v.to_le_bytes());
                }
                inb.extend(src);
                inb.extend(pal);
                for p in px {
                    outb.extend(p.to_le_bytes());
                }
                n += 1;
            };
            let data = d.read("textures.arc").unwrap_or_default();
            for e in arc::entries(&data).unwrap_or_default() {
                let b = arc::bytes(&data, &e);
                if let Some(t) = texture::read(b) {
                    add(&t, b);
                }
            }
            let ds = d.read("datasets.arc").unwrap_or_default();
            for e in arc::entries(&ds).unwrap_or_default() {
                let b = arc::bytes(&ds, &e);
                let mut p = 0;
                while let Some(q) = b[p..].windows(4).position(|w| w == b"LFXT") {
                    let at = p + q;
                    match texture::read_prefix(&b[at..]) {
                        Some((t, end)) => {
                            add(&t, &b[at..]);
                            p = at + end;
                        }
                        None => p = at + 4,
                    }
                }
            }
            if std::fs::write(inp, &inb).is_err() || std::fs::write(outp, &outb).is_err() {
                println!("cannot write the batch");
                return ExitCode::from(1);
            }
            println!("textures in the batch: {n}");
        }
        "tlutcheck" => {
            let Some(log) = args.get(3).and_then(|p| read_log(p)) else {
                println!("usage: assets ISO tlutcheck LOG");
                return ExitCode::from(2);
            };
            let mut seen: BTreeMap<(u8, Vec<u16>), usize> = BTreeMap::new();
            for t in &log.tluts {
                *seen.entry((t.format, t.entries.clone())).or_default() += 1;
            }
            println!("palette setups {}, distinct {}", log.tluts.len(), seen.len());
            for ((f, e), n) in seen.iter().take(6) {
                println!("  format {f} x{n}: {} entries, first {:04x?}", e.len(), &e[..e.len().min(6)]);
            }
            // each palette texture's palette bytes against the logged palettes' bytes (big-endian)
            let logged: Vec<(u8, Vec<u8>)> = seen.keys().map(|(f, e)| (*f, e.iter().flat_map(|v| v.to_be_bytes()).collect())).collect();
            let mut how: BTreeMap<String, usize> = BTreeMap::new();
            let dt = d.read("textures.arc").unwrap_or_default();
            for e in arc::entries(&dt).unwrap_or_default() {
                let b = arc::bytes(&dt, &e);
                let Some(t) = texture::read(b) else { continue };
                let Some((p, n)) = t.palette else { continue };
                let pal = &b[p..p + n * 4];
                let (first, second) = (&pal[..n * 2], &pal[n * 2..]);
                let mut k = "none".to_string();
                for (f, l) in &logged {
                    if l.len() >= first.len() && &l[..first.len()] == first {
                        k = format!("first half, format {f}");
                    } else if l.len() >= second.len() && &l[..second.len()] == second {
                        k = format!("second half, format {f}");
                    }
                }
                *how.entry(format!("{:?}: {k}", t.kind)).or_default() += 1;
            }
            for (k, n) in &how {
                println!("  palette textures matching a logged palette by {k}: {n}");
            }
            let data = d.read("textures.arc").unwrap_or_default();
            let mut shown = 0;
            for e in arc::entries(&data).unwrap_or_default() {
                let b = arc::bytes(&data, &e);
                if let Some(t) = texture::read(b)
                    && let Some((p, n)) = t.palette
                    && shown < 6
                {
                    shown += 1;
                    println!("  {} ({:?}, {n} colours): first {:02x?}", t.name, t.kind, &b[p..p + 24.min(n * 4)]);
                }
            }
        }
        "stripcheck" => {
            let Some(log) = args.get(3).and_then(|p| read_log(p)) else {
                println!("usage: assets ISO stripcheck LOG");
                return ExitCode::from(2);
            };
            // the strips' arrays' first 32 bytes, each a needle; every archive scanned for them
            // (4-aligned), each needle's archives noted
            let mut needles: std::collections::HashMap<[u8; 8], Vec<[u8; 32]>> = std::collections::HashMap::new();
            for s in &log.strips {
                for (i, w) in s.words.iter().enumerate() {
                    if (0x8000_0000..0x817F_FFE0).contains(w) && s.at[i] != [0u8; 32] {
                        let v = needles.entry(s.at[i][..8].try_into().unwrap()).or_default();
                        if !v.contains(&s.at[i]) {
                            v.push(s.at[i]);
                        }
                    }
                }
            }
            let mut found: std::collections::HashMap<[u8; 32], BTreeSet<String>> = std::collections::HashMap::new();
            for path in d.files().keys().filter(|p| p.to_ascii_lowercase().ends_with(".arc")) {
                let data = d.read(path).unwrap_or_default();
                for o in (0..data.len().saturating_sub(32)).step_by(4) {
                    if let Some(v) = needles.get(&<[u8; 8]>::try_from(&data[o..o + 8]).unwrap()) {
                        for n in v {
                            if &data[o..o + 32] == n {
                                found.entry(*n).or_default().insert(path.clone());
                            }
                        }
                    }
                }
            }
            let mut by_word: BTreeMap<usize, BTreeMap<String, usize>> = BTreeMap::new();
            for s in &log.strips {
                for (i, w) in s.words.iter().enumerate() {
                    if !(0x8000_0000..0x817F_FFE0).contains(w) {
                        continue;
                    }
                    let k = match found.get(&s.at[i]) {
                        Some(a) => a.iter().cloned().collect::<Vec<_>>().join("+"),
                        None => "nowhere".to_string(),
                    };
                    *by_word.entry(i).or_default().entry(k).or_default() += 1;
                }
            }
            for (i, m) in &by_word {
                println!("  word {i}: {m:?}");
            }
            if std::env::var_os("ASSETS_SHOW_STRIP_PLACES").is_some() {
                // study: for strips whose word 2 is in models.arc, each array's entry and offset
                let data = d.read("models.arc").unwrap_or_default();
                let entries = arc::entries(&data).unwrap_or_default();
                let place = |n: &[u8; 32]| -> Option<(String, usize, usize)> {
                    entries.iter().find_map(|e| {
                        let b = arc::bytes(&data, e);
                        (0..b.len().saturating_sub(32)).step_by(4).find(|&o| &b[o..o + 32] == n).map(|o| (e.name.clone(), o, b.len()))
                    })
                };
                // the rule found: the 16 bits before a strip's positions are its vertex count
                let (mut same, mut other, mut tested) = (0, 0, 0);
                for s in log.strips.iter() {
                    if tested >= 600 {
                        break;
                    }
                    let Some((n, off, _)) = place(&s.at[2]) else { continue };
                    tested += 1;
                    let e = entries.iter().find(|e| e.name == n).unwrap();
                    let b = arc::bytes(&data, e);
                    let c = u16::from_be_bytes([b[off - 2], b[off - 1]]) as u32;
                    if c == s.words[1] { same += 1 } else { other += 1 }
                }
                println!("strips whose positions are in models.arc: {tested} tested, the count before them the strip's for {same}, not for {other}");
                // the arrays after the positions: which of the strip's other words' bytes are at
                // positions + 8n (and + 12n, + 11n, + 16n), by the attribute byte 7 before them
                let mut after: BTreeMap<String, usize> = BTreeMap::new();
                let mut tested = 0;
                for s in log.strips.iter() {
                    if tested >= 600 {
                        break;
                    }
                    let Some((n, off, _)) = place(&s.at[2]) else { continue };
                    let e = entries.iter().find(|e| e.name == n).unwrap();
                    let b = arc::bytes(&data, e);
                    let c = u16::from_be_bytes([b[off - 2], b[off - 1]]) as usize;
                    if c as u32 != s.words[1] {
                        continue;
                    }
                    tested += 1;
                    let attr = b[off - 7];
                    for (name, at) in [("8n", off + 8 * c), ("12n", off + 12 * c), ("11n", off + 11 * c), ("16n", off + 16 * c)] {
                        for k in 3..16 {
                            if (0x8000_0000..0x817F_FFE0).contains(&s.words[k]) && b.get(at..at + 32) == Some(&s.at[k][..]) {
                                *after.entry(format!("attribute {attr:#04x}: word {k} at positions + {name}")).or_default() += 1;
                            }
                        }
                    }
                    *after.entry(format!("attribute {attr:#04x}: strips")).or_default() += 1;
                }
                for (k, n) in &after {
                    println!("  {n:5} {k}");
                }
                let mut shown = 0;
                let mut names = BTreeSet::new();
                for s in &log.strips {
                    if shown >= 10 {
                        break;
                    }
                    let Some(p2) = place(&s.at[2]) else { continue };
                    let n = &p2.0;
                    if n.starts_with("a") || n.starts_with("c") || n.starts_with("m") || n.starts_with("shadow") || n.starts_with("u") || !names.insert(n.clone()) {
                        continue;
                    }
                    if let Some(e) = entries.iter().find(|e| &e.name == n) {
                        let b = arc::bytes(&data, e);
                        println!("  before w2 in {n}: {:02x?}", &b[p2.1.saturating_sub(24)..p2.1]);
                    }
                    shown += 1;
                    let others: Vec<String> = [3usize, 4, 6, 14, 15]
                        .iter()
                        .filter(|&&i| (0x8000_0000..0x817F_FFE0).contains(&s.words[i]))
                        .map(|&i| format!("w{i} {:?} (+{:x} from w2)", place(&s.at[i]).map(|p| p.1), s.words[i].wrapping_sub(s.words[2])))
                        .collect();
                    println!("strip count {} words0 {:08x}: w2 in {} at {:#x} (of {:#x}); {}", s.words[1], s.words[0], p2.0, p2.1, p2.2, others.join(", "));
                }
            }
        }
        "modelcheck" => {
            let Some(log) = args.get(3).and_then(|p| read_log(p)) else {
                println!("usage: assets ISO modelcheck LOG");
                return ExitCode::from(2);
            };
            let data = d.read("models.arc").unwrap_or_default();
            let entries = arc::entries(&data).unwrap_or_default();
            let models: Vec<model::Model> = entries.iter().filter_map(|e| model::read(arc::bytes(&data, e))).collect();
            let kinds = models.iter().filter(|m| m.first.kind == 1).count();
            println!("records {}, first sub-model read {} (kind 1: {kinds}, kind 0: {})", entries.len(), models.len(), models.len() - kinds);
            // the kind-1 records whose first sub-model is then followed by just the 73-byte tail
            let mut ends: BTreeMap<String, usize> = BTreeMap::new();
            for e in &entries {
                let b = arc::bytes(&data, e);
                if let Some(m) = model::read(b) {
                    let k = if b.len() == m.tail + 73 { "the 73-byte tail and the end".to_string() } else { format!("{} bytes more", b.len() as i64 - m.tail as i64) };
                    if m.first.kind == 1 {
                        *ends.entry(format!("kind 1, format {:#x}: {}", m.format, if k.starts_with("the") { k } else { "more".into() })).or_default() += 1;
                    } else {
                        *ends.entry(format!("kind 0, format {:#x}", m.format)).or_default() += 1;
                    }
                }
            }
            for (k, n) in &ends {
                println!("  {n:5} {k}");
            }
            let mut whole: Vec<model::Record> = entries.iter().filter_map(|e| model::read_record(arc::bytes(&data, e))).collect();
            if !log.matrices.is_empty() {
                let ticks: BTreeSet<u32> = log.matrices.iter().map(|m| m.tick).collect();
                let mut by_id: BTreeMap<(bool, u8), usize> = BTreeMap::new();
                for m in &log.matrices {
                    *by_id.entry((m.projection, m.id)).or_default() += 1;
                }
                println!("matrices {} over ticks {:?}..{:?} ({} ticks): by (projection, id) {by_id:?}", log.matrices.len(), ticks.first(), ticks.last(), ticks.len());
                if std::env::var_os("ASSETS_SHOW_MATRICES").is_some() {
                    for m in log.matrices.iter().filter(|m| m.id >= 0xF0).take(12) {
                        let v = &m.values;
                        println!("  tick {} id {:x} rows [{:.3} {:.3} {:.3} | {:.3}] [{:.3} {:.3} {:.3} | {:.3}] [{:.3} {:.3} {:.3} | {:.3}]", m.tick, m.id, v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11]);
                    }
                }
            }
            if std::env::var_os("ASSETS_SHOW_FAR_WORDS").is_some() {
                let mut seen = std::collections::HashSet::new();
                for st in log.strips.iter().filter(|s| seen.insert((s.words[0] >> 24, s.words[10]))).take(12) {
                    println!("strip op {:02x} count {} w8 {:08x} w9 {:08x}", st.words[0] >> 24, st.words[1], st.words[8], st.words[9]);
                    for k in 10..16 {
                        println!("   w{k} {:08x}: {:02x?}", st.words[k], &st.at[k][..24]);
                    }
                }
            }
            if std::env::var_os("ASSETS_SHOW_UNREAD").is_some() {
                for e in entries.iter().filter(|e| model::read_record(arc::bytes(&data, e)).is_none()) {
                    let b = arc::bytes(&data, e);
                    println!("  not read whole: {} ({} bytes): {:02x?}", e.name, b.len(), &b[..b.len().min(12)]);
                }
            }
            // and the records kept inside datasets.arc's entries (each at a "00 01 00 00 00 00"
            // and a name whose head reads)
            let ds = d.read("datasets.arc").unwrap_or_default();
            let (mut found, mut whole_ds) = (0, 0);
            for e in arc::entries(&ds).unwrap_or_default() {
                let b = arc::bytes(&ds, &e);
                let mut p = 0;
                while let Some(q) = b[p..].windows(6).position(|w| w == [0, 1, 0, 0, 0, 0]) {
                    let at = p + q;
                    if model::read(&b[at..]).is_some() {
                        found += 1;
                    }
                    match model::read_record_prefix(&b[at..]) {
                        Some((r, end)) => {
                            whole_ds += 1;
                            whole.push(r);
                            p = at + end;
                        }
                        None => p = at + 1,
                    }
                }
            }
            println!("datasets.arc: model heads {found}, read whole {whole_ds}");
            let subs: usize = whole.iter().map(|r| r.nodes.iter().map(|n| n.subs.len()).sum::<usize>()).sum();
            println!("records read whole to their end: {} ({} nodes, {subs} sub-models)", whole.len(), whole.iter().map(|r| r.nodes.len()).sum::<usize>());
            // the strips from any of their sub-models: count and texture coordinates
            // (several sub-models can share their positions' first bytes: all kept)
            let mut all: std::collections::HashMap<[u8; 32], Vec<&model::SubModel>> = std::collections::HashMap::new();
            for r in &whole {
                for n in &r.nodes {
                    for (_, sm) in &n.subs {
                        if let Some(p) = sm.positions.get(..32) {
                            all.entry(p.try_into().unwrap()).or_default().push(sm);
                        }
                    }
                }
            }
            let (mut st, mut ok) = (0, 0);
            for s in &log.strips {
                if let Some(v) = all.get(&s.at[2]) {
                    st += 1;
                    let sm = v[0];
                    let good = v.iter().any(|sm| sm.count as u32 == s.words[1] && sm.texcoords.get(..32).is_none_or(|t| t == &s.at[3][..]));
                    ok += usize::from(good);
                    if !good && std::env::var_os("ASSETS_SHOW_BAD_STRIPS").is_some() {
                        println!("  differs: {} candidates; count {} strip {}; uv {:02x?} strip w3 {:02x?}; word0 {:08x}", v.len(), sm.count, s.words[1], sm.texcoords.get(..8), &s.at[3][..8], s.words[0]);
                    }
                }
            }
            println!("strips from a sub-model of a whole record {st}: count and texture coordinates the same {ok}");
            // the command after a strip: 0x22 with four bytes, then 0x28, then the next strip
            // (0x38, its count, positions, texture coordinates): the four bytes against the next
            // strip's batch's matrices (their slots, in order)
            {
                let (mut n, mut slots, mut idx, mut other) = (0, 0, 0, 0);
                let mut shown = 0;
                for st in log.strips.iter().filter(|s| s.words[8] >> 24 == 0x22 && s.words[12] >> 24 == 0x38) {
                    let Some(v) = all.get(&st.at[14]) else { continue };
                    let Some(sm) = v.iter().find(|sm| sm.count as u32 == st.words[13]) else { continue };
                    n += 1;
                    let w = st.words[9].to_be_bytes();
                    let s: Vec<u8> = sm.matrices.iter().map(|m| m.0 as u8).collect();
                    let k = s.len().min(4);
                    if s[..k] == w[..k] {
                        slots += 1;
                    } else if sm.matrices.iter().take(4).map(|m| m.1).eq(w.iter().copied().take(k)) {
                        idx += 1;
                    } else {
                        other += 1;
                        if shown < 6 {
                            shown += 1;
                            println!("  0x22 bytes {:02x?} batch matrices {:?} weights {}", w, sm.matrices, sm.weights);
                        }
                    }
                }
                println!("strips followed by 0x22 and a strip of a read batch {n}: its four bytes the batch's slots {slots}, its indices {idx}, other {other}");
            }
            // opcode 66 (the command's first byte 0x42): a model drawn from a display list; which
            // of its words are a sub-model's display list and positions
            let mut lists: std::collections::HashMap<[u8; 32], &model::SubModel> = std::collections::HashMap::new();
            for r in &whole {
                for n in &r.nodes {
                    for (_, sm) in &n.subs {
                        if let Some((_, _, l)) = &sm.list
                            && let Some(h) = l.get(..32)
                        {
                            lists.insert(h.try_into().unwrap(), sm);
                        }
                    }
                }
            }
            let mut ops66 = 0;
            let mut words: BTreeMap<String, usize> = BTreeMap::new();
            for s in log.strips.iter().filter(|s| s.words[0] >> 24 == 0x42) {
                ops66 += 1;
                let mut any = false;
                for k in 1..16 {
                    if !(0x8000_0000..0x817F_FFE0).contains(&s.words[k]) {
                        continue;
                    }
                    if let Some(sm) = lists.get(&s.at[k]) {
                        any = true;
                        *words.entry(format!("word {k} a sub-model's display list")).or_default() += 1;
                        for j in 1..16 {
                            if sm.positions.get(..32) == Some(&s.at[j][..]) {
                                *words.entry(format!("word {j} that sub-model's positions")).or_default() += 1;
                            }
                        }
                    }
                }
                if !any {
                    *words.entry("no word a known display list".into()).or_default() += 1;
                }
            }
            println!("opcode 66 commands {ops66}:");
            for (k, n) in &words {
                println!("  {n:5} {k}");
            }
            // each draw's material (the last 'm' before its strip record) by its sub-model's hash:
            // does a hash always come with the same material (layers, their bytes, the textures'
            // first bytes)?
            if !log.materials.is_empty() {
                type Mat = (u32, [[u8; 0x18]; 2], [[u8; 128]; 2]);
                let mut by_hash: BTreeMap<u32, std::collections::BTreeSet<Mat>> = BTreeMap::new();
                let mut mi = 0;
                let mut drawn = 0;
                for (i, st) in log.strips.iter().enumerate() {
                    while mi + 1 < log.materials.len() && log.materials[mi + 1].strips_before <= i {
                        mi += 1;
                    }
                    let m = &log.materials[mi];
                    if m.strips_before > i {
                        continue;
                    }
                    let sm = if st.words[0] >> 24 == 0x42 {
                        lists.get(&st.at[6]).copied()
                    } else {
                        all.get(&st.at[2]).and_then(|v| v.iter().find(|sm| sm.count as u32 == st.words[1]).copied())
                    };
                    let Some(sm) = sm else { continue };
                    drawn += 1;
                    // a second layer's bytes and texture are stale when there is one layer
                    let (l1, t1) = if m.layers >= 2 { (m.layer[1], m.textures[1].1) } else { ([0; 0x18], [0; 128]) };
                    by_hash.entry(sm.hash).or_default().insert((m.layers, [m.layer[0], l1], [m.textures[0].1, t1]));
                }
                let one = by_hash.values().filter(|v| v.len() == 1).count();
                let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
                for v in by_hash.values() {
                    *sizes.entry(v.len()).or_default() += 1;
                }
                // a material seen with every hash (a pass over everything drawn, not a mesh's own)
                let mut common: Option<std::collections::BTreeSet<Mat>> = None;
                for v in by_hash.values() {
                    common = Some(match common {
                        None => v.clone(),
                        Some(c) => c.intersection(v).cloned().collect(),
                    });
                }
                let common = common.unwrap_or_default();
                let own = by_hash.values().filter(|v| v.difference(&common).count() == 1).count();
                println!("draws with a material and a read sub-model {drawn}: hashes {}, with one material {one}; materials a hash (count: hashes) {sizes:?}; seen with every hash {}, one material besides them {own}", by_hash.len(), common.len());
                // each material's first texture slot (an in-memory texture: +0x10 width, +0x12
                // height, +0x18 the LFXT kind byte, +0x1c the texels' address): its address an 'x'
                // record's image, whose hash names an LFXT of that kind and size
                {
                    let (tex_hash, _) = texture_hashes(&d);
                    let images: BTreeMap<u32, u64> = log.textures.iter().map(|x| (x.image, x.hash)).collect();
                    // the slot's 128 bytes searched for an 'x' image's address, as itself or as GX
                    // keeps it in a GXTexObj (the physical address >> 5): at which offset
                    let mut at: BTreeMap<(usize, &str), usize> = BTreeMap::new();
                    let (mut slots_n, mut named) = (0, 0);
                    let mut seen = BTreeSet::new();
                    for m in &log.materials {
                        let (addr, b) = (m.textures[0].0, &m.textures[0].1);
                        if addr < 0x8000_0000 || !seen.insert(addr) {
                            continue;
                        }
                        slots_n += 1;
                        let mut hit = false;
                        for k in (0..124).step_by(4) {
                            let w = u32::from_be_bytes([b[k], b[k + 1], b[k + 2], b[k + 3]]);
                            for (form, img) in [("address", w), ("GX (phys >> 5)", (w & 0x00FF_FFFF) << 5 | 0x8000_0000)] {
                                if let Some(h) = images.get(&img) {
                                    *at.entry((k, form)).or_default() += 1;
                                    if !hit && tex_hash.contains_key(h) {
                                        hit = true;
                                        named += 1;
                                    }
                                }
                            }
                        }
                    }
                    println!("materials' first slots {slots_n}: an 'x' image found in them for {named} (named by its hash); at {at:?}");
                }
                // each texture loaded for a draw: its image an 'x' record's, named by its hash
                if !log.tex_loads.is_empty() {
                    let (tex_hash, _) = texture_hashes(&d);
                    let images: BTreeMap<u32, u64> = log.textures.iter().map(|x| (x.image, x.hash)).collect();
                    let (mut seen_x, mut named) = (0, 0);
                    for t in &log.tex_loads {
                        if let Some(h) = images.get(&t.image()) {
                            seen_x += 1;
                            named += usize::from(tex_hash.contains_key(h));
                        }
                    }
                    println!("textures loaded for draws {}: the image an 'x' record's {seen_x}, named (an LFXT by its hash) {named}", log.tex_loads.len());
                    // the draws of read sub-models: the map-0 texture loaded last before each, by
                    // the sub-model's hash
                    let mut by: BTreeMap<u32, BTreeMap<String, usize>> = BTreeMap::new();
                    let mut ti = 0;
                    for (i, st) in log.strips.iter().enumerate() {
                        let sm = if st.words[0] >> 24 == 0x42 {
                            lists.get(&st.at[6]).copied()
                        } else {
                            all.get(&st.at[2]).and_then(|v| v.iter().find(|sm| sm.count as u32 == st.words[1]).copied())
                        };
                        while ti < log.tex_loads.len() && log.tex_loads[ti].strips_before <= i {
                            ti += 1;
                        }
                        let Some(sm) = sm else { continue };
                        let Some(t) = log.tex_loads[..ti].iter().rev().find(|t| t.map == 0) else { continue };
                        let name = images
                            .get(&t.image())
                            .and_then(|h| tex_hash.get(h))
                            .map_or_else(|| format!("{:08x}", t.image()), |v| v[0].0.clone());
                        *by.entry(sm.hash).or_default().entry(name).or_default() += 1;
                    }
                    // against shaders.arc: each draw's sub-model hash a shader's name hash; that
                    // shader's first layer names the texture loaded (an LFXT by its name hash);
                    // the device's first layer (+4..+0x18) the shader's layer
                    let sh = d.read("shaders.arc").unwrap_or_default();
                    let sh_entries = arc::entries(&sh).unwrap_or_default();
                    let shaders: Vec<shader::Shader> = sh_entries.iter().filter_map(|e| shader::read(arc::bytes(&sh, e))).collect();
                    println!("shaders.arc: {} entries, read whole {}", sh_entries.len(), shaders.len());
                    let by_name: BTreeMap<u32, &shader::Shader> = shaders.iter().map(|s| (shader::name_hash(&s.name), s)).collect();
                    let (mut draws, mut found, mut tex_same, mut tex_named, mut layer_same) = (0, 0, 0, 0, 0);
                    let (mut ti, mut mi) = (0, 0);
                    for (i, st) in log.strips.iter().enumerate() {
                        while ti < log.tex_loads.len() && log.tex_loads[ti].strips_before <= i {
                            ti += 1;
                        }
                        while mi < log.materials.len() && log.materials[mi].strips_before <= i {
                            mi += 1;
                        }
                        let sm = if st.words[0] >> 24 == 0x42 {
                            lists.get(&st.at[6]).copied()
                        } else {
                            all.get(&st.at[2]).and_then(|v| v.iter().find(|sm| sm.count as u32 == st.words[1]).copied())
                        };
                        // (the material set after the strip pairs worse: 225 of 1,351 draws)
                        let m = mi.checked_sub(1).map(|k| &log.materials[k]);
                        let t = log.tex_loads[..ti].iter().rev().find(|t| t.map == 0);
                        let (Some(sm), Some(t), Some(m)) = (sm, t, m) else { continue };
                        // a two-layer draw's device layers are the common pass's
                        if m.layers != 1 {
                            continue;
                        }
                        draws += 1;
                        let Some(s) = by_name.get(&sm.hash) else { continue };
                        found += 1;
                        let Some(l) = s.layers.first() else { continue };
                        if let Some(names) = images.get(&t.image()).and_then(|h| tex_hash.get(h)) {
                            tex_named += 1;
                            let base = |n: &str| n.rsplit('/').next().unwrap_or(n).to_string();
                            tex_same += usize::from(names.iter().any(|n| shader::name_hash(&base(&n.0)) == l.texture));
                        }
                        let dl = &m.layer[0];
                        let w = |k: usize| u32::from_be_bytes([dl[k], dl[k + 1], dl[k + 2], dl[k + 3]]);
                        // the factor (+0x14) is the game's at run time (a fade: 0x80 against the shader's 0xff)
                        let same = w(4) == l.flags && w(8) == l.word && w(12) == l.value.to_bits() && dl[16..20] == l.bytes[..4] && dl[21..23] == l.bytes[5..];
                        layer_same += usize::from(same);
                        if !same && std::env::var_os("ASSETS_SHOW_LAYER_DIFFS").is_some() {
                            println!("  {} layers {}: device {:02x?} shader {:08x} {:08x} {:08x} {:02x?}", s.name, m.layers, &dl[4..23], l.flags, l.word, l.value.to_bits(), l.bytes);
                        }
                    }
                    println!("one-layer draws of read sub-models {draws}: their hash a shader's {found}; of those with a named texture {tex_named}, the shader's first layer naming it {tex_same}; the device's first layer the shader's {layer_same}");
                    if std::env::var_os("ASSETS_SHOW_DRAW_TEXTURES").is_some() {
                        for (h, v) in &by {
                            println!("  {h:08x}: {v:?}");
                        }
                    }
                }
                if std::env::var_os("ASSETS_SHOW_SLOTS").is_some() {
                    let mut slots: BTreeMap<[u8; 128], usize> = BTreeMap::new();
                    for m in &log.materials {
                        *slots.entry(m.textures[0].1).or_default() += 1;
                    }
                    let tx = d.read("textures.arc").unwrap_or_default();
                    println!("distinct first-slot bytes {}", slots.len());
                    for (b, n) in slots.iter().take(10) {
                        let at = tx.windows(32).position(|w| w == &b[..32]);
                        let lfxt = at.and_then(|a| tx[..a].windows(4).rposition(|w| w == b"LFXT").map(|l| a - l));
                        println!("  {n:5} {:02x?}: in textures.arc {at:?}, past its LFXT {lfxt:?}", &b[..64]);
                    }
                }
                if std::env::var_os("ASSETS_SHOW_MATERIALS").is_some() {
                    for (h, v) in by_hash.iter().take(12) {
                        for (n, l, t) in v.iter().take(3) {
                            println!("  {h:08x}: layers {n} {:02x?} {:02x?} tex {:02x?}", l[0], l[1], &t[0][..16]);
                        }
                    }
                }
            }
            // the unmatched draws' display lists (word 6's first 32 bytes, distinct), searched for
            // in models.arc and datasets.arc: which entry keeps each and whether a head precedes it
            if std::env::var_os("ASSETS_SHOW_UNMATCHED_66").is_some() {
                let mut heads: BTreeMap<[u8; 32], usize> = BTreeMap::new();
                for s in log.strips.iter().filter(|s| s.words[0] >> 24 == 0x42) {
                    if !(1..16).any(|k| lists.contains_key(&s.at[k])) {
                        *heads.entry(s.at[6]).or_default() += 1;
                    }
                }
                println!("unmatched display lists, distinct {}", heads.len());
                let ds = d.read("datasets.arc").unwrap_or_default();
                let arcs = [("models", &data, entries.clone()), ("datasets", &ds, arc::entries(&ds).unwrap_or_default())];
                for (h, n) in heads.iter().take(40) {
                    let mut at = String::from("not found");
                    'f: for (an, a, es) in &arcs {
                        for e in es {
                            let b = arc::bytes(a, e);
                            if let Some(q) = b.windows(32).position(|w| w == h) {
                                let head = b[..q].windows(6).rposition(|w| w == [0, 1, 0, 0, 0, 0]);
                                let name = head.map(|x| b[x + 6..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>());
                                at = format!("{an}:{} +{q} (entry {} bytes), head {:?} {:?}", e.name, b.len(), head, name);
                                break 'f;
                            }
                        }
                    }
                    println!("  {n:4} {:02x?}: {at}", &h[..6]);
                }
            }
            if std::env::var_os("ASSETS_SHOW_TAILS").is_some() {
                let mut shown = BTreeMap::<u8, usize>::new();
                for e in &entries {
                    let b = arc::bytes(&data, e);
                    if let Some(m) = model::read(b) {
                        let n = shown.entry(m.first.kind).or_default();
                        if *n < 8 {
                            *n += 1;
                            println!("  kind {} n {} {} ({} bytes, tail {}): {:02x?}", m.first.kind, m.first.count, m.name, b.len(), b.len() - m.tail, &b[m.tail..(m.tail + 40).min(b.len())]);
                        }
                    }
                }
            }
            // each strip whose positions' first 32 bytes are a read sub-model's: its word 3's
            // bytes against that sub-model's texture coordinates
            let mut by_pos: std::collections::HashMap<[u8; 32], &model::SubModel> = std::collections::HashMap::new();
            for m in &models {
                if let Some(p) = m.first.positions.get(..32) {
                    by_pos.insert(p.try_into().unwrap(), &m.first);
                }
            }
            let (mut strips, mut same_count, mut same_uv, mut other_uv) = (0, 0, 0, 0);
            for s in &log.strips {
                let Some(sm) = by_pos.get(&s.at[2]) else { continue };
                strips += 1;
                same_count += usize::from(sm.count as u32 == s.words[1]);
                match sm.texcoords.get(..32) {
                    Some(t) if t == &s.at[3][..] => same_uv += 1,
                    _ => other_uv += 1,
                }
            }
            println!("strips from a read sub-model {strips}: the count the same {same_count}; word 3 the texture coordinates {same_uv}, not {other_uv}");
            // kind 0: word 4 its colours, word 5 its normals widened to 4 bytes (a zero after each)
            let (mut k0, mut col, mut nrm) = (0, 0, 0);
            for s in &log.strips {
                let Some(sm) = by_pos.get(&s.at[2]) else { continue };
                let Some(c) = sm.colours.as_ref() else { continue };
                k0 += 1;
                col += usize::from(c.get(..32) == Some(&s.at[4][..]));
                let wide: Vec<u8> = sm.normals.chunks(3).take(8).flat_map(|n| [n[0], n[1], n[2], 0]).collect();
                nrm += usize::from(wide[..] == s.at[5][..wide.len().min(32)]);
            }
            println!("kind-0 strips {k0}: word 4 the colours {col}, word 5 the normals widened {nrm}");
            // which of a strip's words are a read sub-model's other parts: its colours, normals,
            // and the 32 bytes after its arrays
            let mut by_tail: std::collections::HashMap<[u8; 32], (u8, &str)> = std::collections::HashMap::new();
            for (e, m) in entries.iter().filter_map(|e| model::read(arc::bytes(&data, e)).map(|m| (e, m))) {
                let b = arc::bytes(&data, e);
                if let Some(t) = b.get(m.tail..m.tail + 32) {
                    by_tail.insert(t.try_into().unwrap(), (m.first.kind, "the bytes after the arrays"));
                }
                if let Some(c) = m.first.colours.as_ref().and_then(|c| c.get(..32)) {
                    by_tail.insert(c.try_into().unwrap(), (m.first.kind, "the colours"));
                }
                if let Some(n) = m.first.normals.get(..32) {
                    by_tail.insert(n.try_into().unwrap(), (m.first.kind, "the normals"));
                }
            }
            let mut what: BTreeMap<String, usize> = BTreeMap::new();
            for s in &log.strips {
                if !by_pos.contains_key(&s.at[2]) {
                    continue;
                }
                for k in 4..16 {
                    // words that point into memory only (the others' bytes are zeros)
                    if !(0x8000_0000..0x817F_FFE0).contains(&s.words[k]) {
                        continue;
                    }
                    if let Some((kind, w)) = by_tail.get(&s.at[k]) {
                        *what.entry(format!("kind {kind}: word {k} is {w}")).or_default() += 1;
                    }
                }
            }
            for (k, n) in &what {
                println!("  {n:4} {k}");
            }
            if std::env::var_os("ASSETS_SHOW_KIND0").is_some() {
                // study: kind-0 strips' words 4 and 5 beside the sub-model's 4-byte array and normals
                let mut shown = 0;
                for s in &log.strips {
                    let Some(sm) = by_pos.get(&s.at[2]) else { continue };
                    if sm.kind != 0 || shown >= 3 {
                        continue;
                    }
                    shown += 1;
                    println!("  kind-0 strip n {}: w4 {:02x?}\n     w5 {:02x?}\n     4-byte {:02x?}\n     normals {:02x?}", sm.count, &s.at[4][..16], &s.at[5][..16], &sm.colours.as_ref().unwrap()[..16], &sm.normals[..16]);
                }
            }
            if std::env::var_os("ASSETS_SHOW_DELTAS").is_some() {
                // study: a strip's words as offsets from its positions, beside where the record
                // keeps the sub-model's arrays (from its positions)
                let mut where_pos: std::collections::HashMap<[u8; 32], (String, usize, usize, usize)> = std::collections::HashMap::new();
                for e in &entries {
                    let b = arc::bytes(&data, e);
                    if let Some(m) = model::read(b)
                        && let Some(p) = m.first.positions.get(..32)
                    {
                        let n = m.first.count;
                        let pos = m.tail - m.first.positions.len() - m.first.texcoords.len() - m.first.colours.as_ref().map_or(0, |c| c.len()) - m.first.normals.len() - m.first.values.as_ref().map_or(0, |v| v.len());
                        where_pos.insert(p.try_into().unwrap(), (e.name.clone(), n, pos, m.tail));
                    }
                }
                let mut shown = 0;
                for s in &log.strips {
                    let Some((name, n, pos, tail)) = where_pos.get(&s.at[2]) else { continue };
                    if shown >= 6 {
                        break;
                    }
                    shown += 1;
                    let d: Vec<String> = s.words.iter().map(|&w| if (0x8000_0000..0x817F_FFE0).contains(&w) { format!("{:+}", w as i64 - s.words[2] as i64) } else { format!("{w:#x}") }).collect();
                    println!("  {name} n {n}: arrays at +0 (positions), +{} (uv), +{} (4-byte), +{} (normals), tail +{}; words {}", 8 * n, 12 * n, 16 * n, tail - pos, d.join(" "));
                }
            }
        }
        "selcheck" => {
            // the objects whose resources the original collected ('s' records): their resource
            // ids named by the archives' index words, and a lot object's GUID in their definition
            let (Some(log), Some(places)) = (args.get(3).and_then(|p| read_log(p)), args.get(4).and_then(|p| std::fs::read_to_string(p).ok())) else {
                println!("usage: assets ISO selcheck LOG PLACES");
                return ExitCode::from(2);
            };
            let (mut by_id, mut hashed, mut all) = (BTreeMap::new(), 0, 0);
            for a in d.files().keys().filter(|n| n.ends_with(".arc")).cloned().collect::<Vec<_>>() {
                if a == "audiostr.arc" {
                    continue;
                }
                for e in arc::entries(&d.read(&a).unwrap_or_default()).unwrap_or_default() {
                    all += 1;
                    hashed += usize::from(e.id == shader::name_hash(&e.name));
                    by_id.entry(e.id).or_insert(format!("{a}:{}", e.name));
                }
            }
            println!("archive entries {all}: id the upper-case name's CRC {hashed}");
            let guids: BTreeMap<u32, String> = places
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split(' ').collect();
                    Some((u32::from_str_radix(f.get(1)?, 16).ok()?, f.get(8).unwrap_or(&"").to_string()))
                })
                .collect();
            let name = |id: u32| by_id.get(&id).cloned();
            let (mut named, mut with_guid) = (0, 0);
            for s in &log.selectors {
                let g = (0..s.definition.len().saturating_sub(3)).find_map(|i| {
                    let w = u32::from_be_bytes(s.definition[i..i + 4].try_into().unwrap());
                    guids.get(&w).map(|p| (i, p.clone()))
                });
                named += usize::from(name(s.material_id).is_some());
                with_guid += usize::from(g.is_some());
                if std::env::var_os("ASSETS_SHOW_SELECTOR_BYTES").is_some() {
                    let hex = |b: &[u8]| b.chunks(4).map(|w| w.iter().map(|x| format!("{x:02x}")).collect::<String>()).collect::<Vec<_>>().join(" ");
                    println!("  sel {:08x}\n    {}\n    def {}", s.selector, hex(&s.bytes), hex(&s.definition));
                }
                if std::env::var_os("ASSETS_SHOW_SELECTOR_WORDS").is_some() {
                    for (what, b) in [("sel", &s.bytes), ("def", &s.definition)] {
                        for o in (0..b.len() - 3).step_by(2) {
                            let w = u32::from_be_bytes(b[o..o + 4].try_into().unwrap());
                            if let Some(n) = name(w) {
                                println!("  {what} {:08x} +{o:02x} {w:08x} {n}", s.selector);
                            }
                        }
                    }
                }
                if std::env::var_os("ASSETS_SHOW_SELECTORS").is_some() {
                    println!("  sel {:08x} tick {} kind {} material {:08x} {:?} extra {:08x} {:?} guid {g:?}", s.selector, s.tick, s.kind, s.material_id, name(s.material_id), s.extra_id, name(s.extra_id));
                }
            }
            println!("selectors {}: material id named by an index word {named}; a lot object's GUID in the definition {with_guid}", log.selectors.len());
            // the link: an object's GUID (its definition's word +0x1c) to the dataset its
            // definition's extra names
            let mut link: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
            for s in &log.selectors {
                let guid = u32::from_be_bytes(s.definition[0x1c..0x20].try_into().unwrap());
                if guid != 0 {
                    link.entry(guid).or_default().insert(s.extra_id);
                }
            }
            let one = link.values().filter(|v| v.len() == 1).count();
            let to_dataset = link.values().filter(|v| v.iter().any(|&i| name(i).is_some_and(|n| n.contains("datasets.arc")))).count();
            let placed = guids.keys().filter(|g| link.contains_key(g)).count();
            println!("GUIDs {}: one extra id each {one}; naming a dataset {to_dataset}; lot objects' GUIDs among them {placed} of {}", link.len(), guids.len());
            // the same link read from the disc: quickdat.arc's SimsObjects
            let q = d.read("quickdat.arc").unwrap_or_default();
            let img = arc::entries(&q)
                .unwrap_or_default()
                .into_iter()
                .find(|e| e.name == "SimsObjects")
                .and_then(|e| quickdata::Image::read(arc::bytes(&q, &e)));
            let res = img.as_ref().map(quickdata::object_resources).unwrap_or_default();
            let (mut same, mut differ, mut none_both) = (0, 0, 0);
            for (g, ids) in &link {
                let want = *ids.iter().next().unwrap();
                match res.get(g).map(|r| r.dataset) {
                    Some(id) if id == want => same += 1,
                    None if want == 0 => none_both += 1,
                    got => {
                        differ += 1;
                        println!("  DIFFERENT {g:08x}: the original's {want:08x}, the disc's {got:08x?}");
                    }
                }
            }
            let named = res.values().filter(|r| name(r.dataset).is_some()).count();
            println!("disc: {} definitions with resources ({named} naming an archive entry); against the original's {}: the same {same}, none in both {none_both}, different {differ}", res.len(), link.len());
            if std::env::var_os("ASSETS_SHOW_LINK").is_some() {
                for (g, ids) in &link {
                    println!("  {g:08x} {:?} {:?}", guids.get(g), ids.iter().map(|&i| (format!("{i:08x}"), name(i))).collect::<Vec<_>>());
                }
            }
        }
        "orientcheck" => {
            // the orientations the original gave its static models ('o', SetOrient), the last of
            // each model by a tick, against this world's objects then: each object's place and
            // facing, by the rule that fits
            let (Some(log), Some(places), Some(tick)) = (
                args.get(3).and_then(|p| read_log(p)),
                args.get(4).and_then(|p| std::fs::read_to_string(p).ok()),
                args.get(5).and_then(|t| t.parse::<u32>().ok()),
            ) else {
                println!("usage: assets ISO orientcheck LOG PLACES TICK");
                return ExitCode::from(2);
            };
            let mut last: BTreeMap<u32, &housefire::tree_log::OrientRec> = BTreeMap::new();
            for o in log.orients.iter().filter(|o| o.tick <= tick) {
                last.insert(o.model, o);
            }
            println!("orientations {} ({} by tick {tick}), models {}", log.orients.len(), log.orients.iter().filter(|o| o.tick <= tick).count(), last.len());
            let objects: Vec<(String, f32, f32, f32, i32, String)> = places
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split(' ').collect();
                    let (x, y): (i32, i32) = (f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?);
                    (x > 0 && y > 0 && f.get(6)? == &"0" && f.get(7)? == &"0").then(|| (f[0].to_string(), x as f32 / 16.0, y as f32 / 16.0, f[4].parse().unwrap_or(0.0), f[5].parse().unwrap_or(0), f.get(8).unwrap_or(&"").to_string()))
                })
                .collect();
            type Rule = (&'static str, fn(f32) -> f32);
            let rules: [Rule; 3] = [("its place", |x| x), ("its tile's centre", |x| x.floor() + 0.5), ("corner to the lower tile's centre", |x| ((x * 16.0 - 1.0) / 16.0).floor() + 0.5)];
            for (rule, r) in rules {
                let (mut found, mut turned) = (0, 0);
                let mut angles: BTreeMap<(i32, i32), usize> = BTreeMap::new();
                let angle = |m: &housefire::tree_log::OrientRec| m.m[1].atan2(m.m[0]).to_degrees().round() as i32;
                for o in &objects {
                    let here: Vec<&&housefire::tree_log::OrientRec> = last.values().filter(|m| (m.m[12] - r(o.1)).abs() < 0.01 && (m.m[13] - r(o.2)).abs() < 0.01).collect();
                    if here.is_empty() {
                        continue;
                    }
                    found += 1;
                    match here.iter().find(|m| (angle(m) - (90 - 45 * o.4)).rem_euclid(360) == 0) {
                        Some(_) => turned += 1,
                        None => {
                            *angles.entry((o.4, angle(here[0]))).or_default() += 1;
                            if std::env::var_os("ASSETS_SHOW_UNTURNED").is_some_and(|v| v == rule) {
                                println!("    {} {} at ({}, {}) facing {}: angles there {:?}", o.0, o.5, o.1, o.2, o.4, here.iter().map(|m| angle(m)).collect::<Vec<_>>());
                            }
                        }
                    }
                }
                println!("  objects ({}) with an orientation at {rule}: {found}; one there turned 90 - 45 x facing {turned}; the others' (facing, an angle there) {angles:?}", objects.len());
            }
            // by group, as the engine places them (scene::placements: a multi-tile object drawn
            // once, at its cells' box centre, turned by its leader's facing)
            let q = d.read("quickdat.arc").unwrap_or_default();
            let res = arc::entries(&q)
                .unwrap_or_default()
                .into_iter()
                .find(|e| e.name == "SimsObjects")
                .and_then(|e| quickdata::Image::read(arc::bytes(&q, &e)))
                .map(|img| quickdata::object_resources(&img))
                .unwrap_or_default();
            let parts: Vec<scene::Part> = places
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split(' ').collect();
                    let (x16, y16): (i32, i32) = (f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?);
                    let id: i16 = f[0].parse().ok()?;
                    let leader = f.get(9).and_then(|v| v.parse::<i16>().ok()).filter(|&v| v != 0).unwrap_or(id);
                    (x16 > 0 && y16 > 0 && f.get(6)? == &"0" && f.get(7)? == &"0").then(|| scene::Part { id, leader, x16, y16, facing: f[5].parse().unwrap_or(0), wall: u32::from_str_radix(f[1], 16).ok().and_then(|g| res.get(&g)).is_some_and(|r| r.on_wall()) })
                })
                .collect();
            let angle = |m: &housefire::tree_log::OrientRec| m.m[1].atan2(m.m[0]).to_degrees().round() as i32;
            let (mut placed, mut turned, mut missing) = (0, 0, 0);
            let groups = scene::placements(&parts);
            // (only objects with a dataset of their own are drawn: markers and controllers not)
            let has_res: BTreeSet<i16> = places
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split(' ').collect();
                    let id: i16 = f[0].parse().ok()?;
                    let leader = f.get(9).and_then(|v| v.parse::<i16>().ok()).filter(|&v| v != 0).unwrap_or(id);
                    res.get(&u32::from_str_radix(f.get(1)?, 16).ok()?).is_some_and(|r| r.dataset != 0).then_some(leader)
                })
                .collect();
            let groups: Vec<scene::Placement> = groups.into_iter().filter(|g| has_res.contains(&g.leader)).collect();
            for g in &groups {
                let here: Vec<&&housefire::tree_log::OrientRec> = last.values().filter(|m| (m.m[12] - g.x).abs() < 0.01 && (m.m[13] - g.y).abs() < 0.01).collect();
                if here.is_empty() {
                    missing += 1;
                    if std::env::var_os("ASSETS_SHOW_GROUPLESS").is_some() {
                        println!("    no orientation at ({}, {}): leader {}", g.x, g.y, g.leader);
                    }
                    continue;
                }
                placed += 1;
                if here.iter().any(|m| (angle(m) - g.angle as i32).rem_euclid(360) == 0) {
                    turned += 1;
                } else if std::env::var_os("ASSETS_SHOW_GROUPLESS").is_some() {
                    println!("    not turned so: leader {} at ({}, {}) {}: {:?}", g.leader, g.x, g.y, g.angle, here.iter().map(|m| angle(m)).collect::<Vec<_>>());
                }
            }
            println!("  by group ({} objects with a dataset, drawn, of the {} parts): an orientation at the engine's place {placed}, one turned as the engine turns it {turned}; none there {missing}", groups.len(), parts.len());
            if std::env::var_os("ASSETS_SHOW_ORIENTS").is_some() {
                for m in last.values() {
                    println!("    model {:08x} tick {}: at ({:.4}, {:.4}, {:.4}) angle {:.1}", m.model, m.tick, m.m[12], m.m[13], m.m[14], m.m[1].atan2(m.m[0]).to_degrees());
                }
            }
        }
        "wallcheck" => {
            // the lot's walls (lockstep's LOCKSTEP_DUMP_WALLS: each tile side with a wall) against
            // the original's static models' orientations: a wall part on each side, at the tile's
            // centre, turned so its back is to the wall (90 for the wall on +x, -90 on -x, 180 on
            // +y, 0 on -y)
            let (Some(log), Some(walls), Some(tick)) = (
                args.get(3).and_then(|p| read_log(p)),
                args.get(4).and_then(|p| std::fs::read_to_string(p).ok()),
                args.get(5).and_then(|t| t.parse::<u32>().ok()),
            ) else {
                println!("usage: assets ISO wallcheck LOG PIECES TICK");
                return ExitCode::from(2);
            };
            let mut last: BTreeMap<u32, &housefire::tree_log::OrientRec> = BTreeMap::new();
            for o in log.orients.iter().filter(|o| o.tick <= tick) {
                last.insert(o.model, o);
            }
            let mut at: BTreeMap<(i32, i32, i32), usize> = BTreeMap::new();
            for m in last.values() {
                let angle = m.m[1].atan2(m.m[0]).to_degrees().round() as i32;
                *at.entry(((m.m[12] * 2.0).round() as i32, (m.m[13] * 2.0).round() as i32, angle.rem_euclid(360))).or_default() += 1;
            }
            // each piece the engine draws (lockstep's dump: x y angle fence) against the original's
            // orientations: one at that place, turned so (a fence's either way along its edge)
            let (mut pieces, mut found, mut fences, mut fences_found) = (0, 0, 0, 0);
            let mut missing = Vec::new();
            for l in walls.lines() {
                let f: Vec<f32> = l.split(' ').take(4).filter_map(|v| v.parse().ok()).collect();
                let [x, y, angle, fence] = f[..] else { continue };
                let key = |a: f32| ((x * 2.0).round() as i32, (y * 2.0).round() as i32, (a.round() as i32).rem_euclid(360));
                let hit = at.contains_key(&key(angle)) || (fence == 1.0 && at.contains_key(&key(angle + 180.0)));
                pieces += 1;
                found += usize::from(hit);
                if fence == 1.0 {
                    fences += 1;
                    fences_found += usize::from(hit);
                }
                if !hit && missing.len() < 12 {
                    missing.push((x, y, angle, fence));
                }
            }
            // which record each piece draws: the world matrices at the tick at the piece's place
            // (the draw after each: its batch or display list names its record)
            if std::env::var_os("ASSETS_SHOW_WALL_MODELS").is_some() {
                let recs = all_records(&d);
                let mut owner: std::collections::HashMap<[u8; 32], (&str, &str)> = std::collections::HashMap::new();
                for (entry, r) in &recs {
                    for n in &r.nodes {
                        for (_, sm) in &n.subs {
                            for k in [sm.positions.get(..32), sm.list.as_ref().and_then(|l| l.2.get(..32))].into_iter().flatten() {
                                owner.entry(k.try_into().unwrap()).or_insert((entry, &r.name));
                            }
                        }
                    }
                }
                let mut seen: BTreeMap<(String, bool), usize> = BTreeMap::new();
                for l in walls.lines() {
                    let f: Vec<f32> = l.split(' ').filter_map(|v| v.parse().ok()).collect();
                    let [x, y, _, fence] = f[..] else { continue };
                    for m in log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick && (m.values[3] - x).abs() < 0.01 && (m.values[7] - y).abs() < 0.01) {
                        let Some(st) = log.strips.get(m.strips_before) else { continue };
                        let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                        let name = owner.get(&key).map_or("?".to_string(), |(e, n)| format!("{e}/{n}"));
                        *seen.entry((name, fence == 1.0)).or_default() += 1;
                    }
                }
                for ((n, fence), c) in &seen {
                    println!("  {} draws at wall pieces' places: {n} {c}", if *fence { "fence" } else { "wall" });
                }
            }
            // each wall part's pattern (the dump's shader hash): its shader's first layer's texture
            // among those the original loads (map 0) for the piece's draw: the last before its
            // world matrix's strips up to the next world matrix's
            let (tex_hash, _) = texture_hashes(&d);
            // (an image's address is reused for other textures: the 'x' record last made by the load's tick)
            let mut by_image: BTreeMap<u32, Vec<(u32, u64)>> = BTreeMap::new();
            for x in &log.textures {
                by_image.entry(x.image).or_default().push((x.tick, x.hash));
            }
            let image_at = |t: &housefire::tree_log::TexLoadRec| by_image.get(&t.image()).and_then(|v| v.iter().rev().find(|x| x.0 <= t.tick).or(v.first())).map(|x| x.1);
            let sh = d.read("shaders.arc").unwrap_or_default();
            let shaders: BTreeMap<u32, shader::Shader> = arc::entries(&sh)
                .unwrap_or_default()
                .iter()
                .filter_map(|e| Some((e.id, shader::read(arc::bytes(&sh, e))?)))
                .collect();
            // (a layer names its texture by the texture's archive id, not always its name's hash)
            let tx = d.read("textures.arc").unwrap_or_default();
            let tex_names: BTreeMap<u32, String> = arc::entries(&tx).unwrap_or_default().into_iter().map(|e| (e.id, e.name.rsplit('/').next().unwrap_or(&e.name).to_string())).collect();
            let world: Vec<usize> = (0..log.matrices.len()).filter(|&i| log.matrices[i].id == 0xF1 && log.matrices[i].tick == tick).collect();
            // the view and projection the lot is drawn with (those in force at the first world
            // matrix with a wall part's place: ASSETS_SHOW_CAMERA prints them whole)
            if std::env::var_os("ASSETS_SHOW_CAMERA").is_some()
                && let Some(&i) = world.first()
            {
                let v = log.matrices[..i].iter().rev().find(|p| p.id == 0xF0);
                let p = log.matrices.iter().filter(|p| p.projection && p.tick == tick).collect::<Vec<_>>();
                println!("view {:?}", v.map(|v| &v.values));
                for p in p { println!("projection at {} {:?}", p.strips_before, p.values); }
                println!("first world matrix at strips {}", log.matrices[i].strips_before);
            }
            // the eye: the view in force at the first world matrix of a wall part's place
            let eye = world.first().and_then(|&i| log.matrices[..i].iter().rev().find(|p| p.id == 0xF0)).map(|v| {
                let r = &v.values;
                [0, 1].map(|c| -(r[c] * r[3] + r[4 + c] * r[7] + r[8 + c] * r[11]))
            });
            let mut by_facing: BTreeMap<(bool, bool), usize> = BTreeMap::new();
            let mut by_cap: BTreeMap<(bool, bool), usize> = BTreeMap::new();
            // (ASSETS_SHOW_WALL_RECORDS: the records whose display list starts as each wall part's
            // face or cap draw does, all of them: one list may be kept by several records)
            let list_owners: BTreeMap<[u8; 32], BTreeSet<String>> = if std::env::var_os("ASSETS_SHOW_WALL_RECORDS").is_some() {
                let mut m: BTreeMap<[u8; 32], BTreeSet<String>> = BTreeMap::new();
                for (entry, r) in all_records(&d) {
                    for n in &r.nodes {
                        for (_, sm) in &n.subs {
                            if let Some(k) = sm.list.as_ref().and_then(|l| l.2.get(..32)) {
                                m.entry(k.try_into().unwrap()).or_default().insert(format!("{entry}/{} ({} vertices)", r.name, sm.count));
                            }
                            // (and by its positions' first bytes, kept apart by a leading byte)
                            if let Some(k) = sm.positions.get(..31)
                            {
                                let mut key = [0xffu8; 32];
                                key[1..].copy_from_slice(k);
                                m.entry(key).or_default().insert(format!("{entry}/{}", r.name));
                            }
                        }
                    }
                }
                m
            } else {
                BTreeMap::new()
            };
            let mut record_tally: BTreeMap<(String, String), usize> = BTreeMap::new();
            let mut by_opening: BTreeMap<(String, String), usize> = BTreeMap::new();
            let mut by_form: BTreeMap<(String, String), usize> = BTreeMap::new();
            let (mut parts, mut same, mut undrawn, mut down) = (0, 0, 0, 0);
            let mut wrong: BTreeMap<String, usize> = BTreeMap::new();
            for l in walls.lines() {
                let f: Vec<&str> = l.split(' ').collect();
                let v: Vec<f32> = f.iter().take(4).filter_map(|v| v.parse().ok()).collect();
                let [x, y, angle, fence] = v[..] else { continue };
                if fence == 1.0 {
                    continue;
                }
                parts += 1;
                let want = f.get(4).and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(|h| shaders.get(&h)).and_then(|s| s.layers.first()).and_then(|l| tex_names.get(&l.texture));
                let mut drawn = Vec::new();
                // (a cap: a draw after a matrix at the part, a display list of 6 vertices with the plain image)
                let mut caps = Vec::new();
                // (the wall records consistent with every draw at the part: ASSETS_SHOW_PART_RECORDS)
                let mut consistent: Option<BTreeSet<String>> = None;
                let mut counts = Vec::new();
                for &i in &world {
                    let m = &log.matrices[i];
                    let a = m.values[4].atan2(m.values[0]).to_degrees();
                    if (m.values[3] - x).abs() > 0.01 || (m.values[7] - y).abs() > 0.01 || (a - angle).rem_euclid(360.0).min((angle - a).rem_euclid(360.0)) > 0.5 {
                        continue;
                    }
                    if !list_owners.is_empty()
                        && let Some(st) = log.strips.get(m.strips_before)
                        && st.words[0] >> 24 == 0x42
                    {
                        let mut key = [0xffu8; 32];
                        key[1..].copy_from_slice(&st.at[2][..31]);
                        if let Some(o) = list_owners.get(&key) {
                            let walls_only: BTreeSet<String> = o.iter().filter_map(|n| n.rsplit('/').next()).filter(|n| n.starts_with("wall_")).map(String::from).collect();
                            if !walls_only.is_empty() {
                                counts.push(st.words[1]);
                                consistent = Some(match consistent.take() {
                                    None => walls_only,
                                    Some(c) => c.intersection(&walls_only).cloned().collect(),
                                });
                            }
                        }
                    }
                    // its draw: the strip after the matrix, with the map-0 texture loaded last before it
                    if let Some(t) = log.tex_loads.iter().rev().find(|t| t.strips_before <= m.strips_before && t.map == 0)
                        && let Some(names) = image_at(t).and_then(|h| tex_hash.get(&h))
                    {
                        // (for study: the pass it is in: the projection and view in force, the material's layers)
                        if std::env::var_os("ASSETS_SHOW_PASS").is_some() {
                            let proj = log.matrices[..i].iter().rposition(|p| p.projection);
                            let view = log.matrices[..i].iter().rposition(|p| p.id == 0xF0);
                            let mat = log.materials.iter().rposition(|q| q.strips_before <= m.strips_before);
                            let pv = proj.map(|j| format!("{j}:{}", log.matrices[j].values.iter().take(6).map(|v| format!("{v:.2}")).collect::<Vec<_>>().join("/")));
                            let vv = view.map(|j| format!("{j}:{:.1}/{:.1}/{:.1}", log.matrices[j].values[3], log.matrices[j].values[7], log.matrices[j].values[11]));
                            let ml = mat.map(|q| (log.materials[q].layers, log.materials[q].layer[0][4..12].iter().map(|b| format!("{b:02x}")).collect::<String>()));
                            let st = log.strips.get(m.strips_before).map(|st| format!("{:08x}/{}", st.words[0], st.words[1]));
                            println!("      pass {x} {y} {angle} tex {} proj {pv:?} view {vv:?} material {ml:?} strip {st:?} col2 {:.4}/{:.4}/{:.4} z {:.2}", names[0].0, m.values[2], m.values[6], m.values[10], m.values[11]);
                        }
                        drawn.extend(names.iter().map(|n| n.0.rsplit('/').next().unwrap_or(&n.0).to_string()));
                        if !list_owners.is_empty()
                            && let Some(st) = log.strips.get(m.strips_before)
                            && st.words[0] >> 24 == 0x42
                            && (st.words[1] == 5 || st.words[1] == 6 || st.words[1] == 18 || st.words[1] == 23)
                        {
                            let kind = format!("{} vertices, {}", st.words[1], names[0].0.rsplit('/').next().unwrap_or(""));
                            let owners = list_owners.get(&st.at[6]).map_or("?".to_string(), |o| o.iter().cloned().collect::<Vec<_>>().join(" | "));
                            // the form the draw shows: its positions' records (`_right_extends`
                            // has its own; a 5-vertex cap is `_left_extends`'s; the rest straight's)
                            {
                                let mut key = [0xffu8; 32];
                                key[1..].copy_from_slice(&st.at[2][..31]);
                                let pos = list_owners.get(&key);
                                let has = |n: &str| pos.is_some_and(|o| o.contains(n));
                                let seen = if st.words[1] == 5 {
                                    Some("left_extends")
                                } else if has("House1Preload/wall_straight") || has("House1Preload/wall_straight_door") || has("housepre01/wall_straight_standard_window") {
                                    Some("straight")
                                } else if has("House1Preload/wall_straight_right_extends") {
                                    Some("right_extends")
                                } else {
                                    None
                                };
                                if let Some(seen) = seen {
                                    *by_form.entry((f.get(7).copied().unwrap_or("-").to_string(), seen.to_string())).or_default() += 1;
                                }
                            }
                            if std::env::var_os("ASSETS_WALL_BY_POSITIONS").is_some() {
                                for (k, a) in st.at.iter().enumerate() {
                                    let mut key = [0xffu8; 32];
                                    key[1..].copy_from_slice(&a[..31]);
                                    if let Some(o) = list_owners.get(&key) {
                                        println!("    bypos {x} {y} {angle} {} at[{k}]: {}", st.words[1], o.iter().cloned().collect::<Vec<_>>().join(" | "));
                                    }
                                }
                            }
                            *record_tally.entry((kind, owners.clone())).or_default() += 1;
                            if std::env::var_os("ASSETS_SHOW_WALL_FAMILY").is_some() && (st.words[1] == 23 || st.words[1] == 18) {
                                let fam = if st.words[1] == 18 { "plain" } else if owners.contains("_door") { "door" } else if owners.contains("window") { "window" } else { "?" };
                                println!("    family {x} {y} {angle} {fam}");
                            }
                            if st.words[1] == 23 || st.words[1] == 18 {
                                let fam = if st.words[1] == 18 { "plain" } else if owners.contains("_door") { "door" } else if owners.contains("window") { "window" } else { "?" };
                                *by_opening.entry((f.get(6).copied().unwrap_or("-").to_string(), fam.to_string())).or_default() += 1;
                            }
                        }
                        if let Some(st) = log.strips.get(m.strips_before)
                            && st.words[0] >> 24 == 0x42
                            && names.iter().any(|n| n.0.ends_with("walls_down"))
                        {
                            caps.push((st.words[1], m.values[11]));
                        }
                    }
                }
                if std::env::var_os("ASSETS_SHOW_PART_RECORDS").is_some() {
                    counts.sort();
                    println!("    records {x} {y} {angle} {counts:?} {:?}", consistent.unwrap_or_default());
                }
                // the cap the engine gives the part (the dump's sixth field) against the original's
                *by_cap.entry((f.get(5) == Some(&"1"), !caps.iter().filter(|c| (c.0 == 5 || c.0 == 6) && c.1 == 0.0).collect::<Vec<_>>().is_empty())).or_default() += 1;
                if std::env::var_os("ASSETS_SHOW_PARTS").is_some() {
                    let patterned = want.is_some_and(|w| drawn.iter().any(|n| n.eq_ignore_ascii_case(w)));
                    let state = if patterned { "patterned" } else if drawn.is_empty() { "none" } else { "plain" };
                    println!("    part {x} {y} {angle} {state} caps {caps:?}");
                }
                if let Some(eye) = eye {
                    let piece = housefire::scene::WallPiece { x, y, angle, fence: false, pattern: 0, cap: false, opening: Default::default(), form: housefire::scene::Form::Straight };
                    *by_facing.entry((piece.faces(eye), want.is_some_and(|w| drawn.iter().any(|n| n.eq_ignore_ascii_case(w))))).or_default() += 1;
                }
                if want.is_some_and(|w| drawn.iter().any(|n| n.eq_ignore_ascii_case(w))) {
                    same += 1;
                } else if drawn.is_empty() {
                    undrawn += 1;
                } else if drawn.iter().any(|n| n == "walls_down") {
                    down += 1;
                } else {
                    *wrong.entry(format!("{} drawn {:?}", f.get(4).unwrap_or(&""), drawn.iter().collect::<BTreeSet<_>>())).or_default() += 1;
                }
            }
            if std::env::var_os("ASSETS_SHOW_PATTERN_SHADERS").is_some() {
                for h in [0x7a39_7517u32, 0x402c_df3f, 0x0d3e_4581, 0x30e8_87b4, 0x16cf_77ec] {
                    if let Some(s) = shaders.get(&h) {
                        println!("  shader {h:08x} {}: layers {:?}", s.name, s.layers.iter().map(|l| format!("{:08x}", l.texture)).collect::<Vec<_>>());
                    }
                }
            }
            // the event stream after the world matrix at a place (ASSETS_SEQ_AT=x,y): matrices,
            // strips and texture loads, in the order the original made them
            if let Some((sx, sy)) = std::env::var("ASSETS_SEQ_AT").ok().and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))) {
                for &i in world.iter().filter(|&&i| (log.matrices[i].values[3] - sx).abs() < 0.01 && (log.matrices[i].values[7] - sy).abs() < 0.01) {
                    let m = &log.matrices[i];
                    println!("  matrix {i} at strips {} angle {:.0}", m.strips_before, m.values[4].atan2(m.values[0]).to_degrees());
                    let s0 = m.strips_before;
                    let mut ev: Vec<(usize, u8, String)> = Vec::new();
                    for (j, mm) in log.matrices.iter().enumerate().skip(i + 1).take_while(|(_, mm)| mm.strips_before < s0 + 12) {
                        ev.push((mm.strips_before, 0, format!("matrix {j} id {:02x} t ({:.2}, {:.2}, {:.2})", mm.id, mm.values.get(3).unwrap_or(&0.0), mm.values.get(7).unwrap_or(&0.0), mm.values.get(11).unwrap_or(&0.0))));
                    }
                    for t in log.tex_loads.iter().filter(|t| t.strips_before >= s0 && t.strips_before < s0 + 12) {
                        let n = image_at(t).and_then(|h| tex_hash.get(&h)).map_or("?".to_string(), |v| v[0].0.clone());
                        ev.push((t.strips_before, 1, format!("tex map {} {n}", t.map)));
                    }
                    for (k, st) in log.strips.iter().enumerate().skip(s0).take(12) {
                        ev.push((k, 2, format!("strip {k} {:08x} {:08x}", st.words[0], st.words[1])));
                    }
                    ev.sort();
                    for e in ev {
                        println!("    {}", e.2);
                    }
                }
            }
            if std::env::var_os("ASSETS_SHOW_IMAGE_REUSE").is_some() {
                let mut per: BTreeMap<u32, BTreeSet<u64>> = BTreeMap::new();
                for x in &log.textures {
                    per.entry(x.image).or_default().insert(x.hash);
                }
                let reused = per.values().filter(|v| v.len() > 1).count();
                let at_tick = log.textures.iter().filter(|x| x.tick <= tick).count();
                println!("  'x' records {} ({at_tick} by tick {tick}), images {}, reused with another texture {reused}", log.textures.len(), per.len());
            }
            println!(
                "wall parts by WallPiece::faces and face drawn: facing drawn {}, facing not drawn (out of view) {}, facing away drawn {}, facing away not drawn {}",
                by_facing.get(&(true, true)).unwrap_or(&0),
                by_facing.get(&(true, false)).unwrap_or(&0),
                by_facing.get(&(false, true)).unwrap_or(&0),
                by_facing.get(&(false, false)).unwrap_or(&0)
            );
            // (every display-list draw of the tick whose positions are a wall record's, by vertex
            // count: whether any free-end piece, 17 or 4 vertices, is drawn at all)
            if !list_owners.is_empty() {
                let mut by_count: BTreeMap<u32, usize> = BTreeMap::new();
                for &i in &world {
                    let m = &log.matrices[i];
                    if let Some(st) = log.strips.get(m.strips_before).filter(|st| st.words[0] >> 24 == 0x42) {
                        let mut key = [0xffu8; 32];
                        key[1..].copy_from_slice(&st.at[2][..31]);
                        if list_owners.get(&key).is_some_and(|o| o.iter().any(|n| n.rsplit('/').next().is_some_and(|b| b.starts_with("wall_")))) {
                            *by_count.entry(st.words[1]).or_default() += 1;
                            if st.words[1] == 4 || st.words[1] == 17 || st.words[1] == 8 {
                                let o = &list_owners[&key];
                                println!("  {} vertices at ({:.2}, {:.2}, {:.2}) turned {:.0}: {}", st.words[1], m.values[3], m.values[7], m.values[11], m.values[4].atan2(m.values[0]).to_degrees(), o.iter().take(4).cloned().collect::<Vec<_>>().join(" | "));
                            }
                        }
                    }
                }
                println!("wall records' draws in the frame by vertex count: {by_count:?}");
            }
            if !by_form.is_empty() {
                println!("wall draws by (WallPiece::form, the form their positions show): {by_form:?}");
            }
            if !by_opening.is_empty() {
                println!("wall faces by (WallPiece::opening, the original's model family): {by_opening:?}");
            }
            for ((k, o), c) in record_tally.iter().filter(|_| std::env::var_os("ASSETS_SHOW_RECORD_TALLY").is_some()) {
                println!("  draw {k}: {c}: {o}");
            }
            println!(
                "wall parts' caps (WallPiece::cap / the original's): both {}, the engine's only {}, the original's only {}, neither {}",
                by_cap.get(&(true, true)).unwrap_or(&0),
                by_cap.get(&(true, false)).unwrap_or(&0),
                by_cap.get(&(false, true)).unwrap_or(&0),
                by_cap.get(&(false, false)).unwrap_or(&0)
            );
            // (`walls_down`, `wall_black`, `polyshadow` and `1st_final` are one image: a plain one)
            println!("wall parts {parts}: drawn with the pattern's texture {same}; not drawn this frame {undrawn}; drawn plain only (the walls_down / wall_black image) {down}; other {}", parts - same - undrawn - down);
            for (k, c) in wrong.iter().take(12) {
                println!("  not: {k} ({c})");
            }
            println!("wall pieces (the engine's) {pieces}: the original's orientation there {found}; of them fences {fences}, found {fences_found}");
            if !missing.is_empty() {
                println!("  not found: {missing:?}");
            }
        }
        "recdiff" => {
            // two or more records (DATASET/NAME) side by side: per node and sub-model, where their
            // arrays first differ (positions, texture coordinates, normals, the display list)
            let want: Vec<&str> = args.iter().skip(3).map(|s| s.as_str()).collect();
            let recs = all_records(&d);
            let found: Vec<(&String, &model::Record)> = want.iter().filter_map(|w| recs.iter().find(|(e, r)| format!("{e}/{}", r.name) == *w).map(|(e, r)| (e, r))).collect();
            let Some((_, first)) = found.first() else {
                println!("none found");
                return ExitCode::FAILURE;
            };
            for (e, r) in &found {
                println!("{e}/{}: nodes {}, subs {:?}", r.name, r.nodes.len(), r.nodes.iter().map(|n| n.subs.iter().map(|(_, s)| s.count).collect::<Vec<_>>()).collect::<Vec<_>>());
            }
            let diff = |a: &[u8], b: &[u8]| if a == b { "same".to_string() } else { format!("differ at {} (sizes {} {})", a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len())), a.len(), b.len()) };
            for (e, r) in found.iter().skip(1) {
                for (ni, (na, nb)) in first.nodes.iter().zip(&r.nodes).enumerate() {
                    for (si, ((_, a), (_, b))) in na.subs.iter().zip(&nb.subs).enumerate() {
                        let la = a.list.as_ref().map(|l| l.2.clone()).unwrap_or_default();
                        let lb = b.list.as_ref().map(|l| l.2.clone()).unwrap_or_default();
                        println!(
                            "  {e}/{} node {ni} sub {si}: positions {}, texcoords {}, normals {}, list {}",
                            r.name,
                            diff(&a.positions, &b.positions),
                            diff(&a.texcoords, &b.texcoords),
                            diff(&a.normals, &b.normals),
                            diff(&la, &lb)
                        );
                    }
                }
            }
        }
        "skelcheck" => {
            // characte.arc's skeletons read to their ends, and each against the model record of
            // its name: its node tree's parents the bones', its batches' matrix slots in range
            let c = d.read("characte.arc").unwrap_or_default();
            let entries = arc::entries(&c).unwrap_or_default();
            let mut skels = BTreeMap::new();
            let (mut read, mut hashed) = (0, 0);
            for e in &entries {
                if let Some(s) = skeleton::read(arc::bytes(&c, e)) {
                    read += 1;
                    hashed += usize::from(e.id == shader::name_hash(&e.name));
                    skels.insert(e.id, s);
                } else if std::env::var_os("ASSETS_SHOW_UNREAD").is_some() {
                    println!("  not read: {}", e.name);
                }
            }
            println!("skeletons {}: read to their ends {read}; id the name's hash {hashed}", entries.len());
            let (mut with_record, mut same_tree, mut slots_in) = (0, 0, 0);
            let mut seen = BTreeSet::new();
            for (_, r) in all_records(&d) {
                let Some(s) = skels.get(&shader::name_hash(&r.name)) else { continue };
                if !seen.insert(r.name.clone()) {
                    continue;
                }
                with_record += 1;
                // a node's word: the bone its mesh hangs from (-1: none, a skinned mesh's)
                let nodes: Vec<i32> = r.nodes.iter().map(|n| n.bone).collect();
                if nodes.iter().all(|&b| b == -1 || (b as usize) < s.bones.len()) {
                    same_tree += 1;
                } else if std::env::var_os("ASSETS_SHOW_TREES").is_some() {
                    println!("  {}: nodes' bones {nodes:?}, bones {}", r.name, s.bones.len());
                }
                let max = r.nodes.iter().flat_map(|n| n.subs.iter().flat_map(|(_, sm)| sm.matrices.iter().map(|m| m.0 as usize))).max();
                slots_in += usize::from(max.is_none_or(|m| m < s.bones.len()));
            }
            println!("records with a skeleton of their name {with_record}: every node's bone a bone of it {same_tree}; matrix slots within the bones {slots_in}");
        }
        "scenecheck" => {
            // the original's world matrices (the observer's 0xF1 'p' records) at a tick against
            // this world's objects then (lockstep's LOCKSTEP_DUMP_PLACES file): each object's place
            // (sixteenths of a tile) a matrix's translation, by the offset that fits most
            let (Some(log), Some(places), Some(tick)) = (
                args.get(3).and_then(|p| read_log(p)),
                args.get(4).and_then(|p| std::fs::read_to_string(p).ok()),
                args.get(5).and_then(|t| t.parse::<u32>().ok()),
            ) else {
                println!("usage: assets ISO scenecheck LOG PLACES TICK");
                return ExitCode::from(2);
            };
            let worlds: Vec<[f32; 3]> = log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick).map(|m| [m.values[3], m.values[7], m.values[11]]).collect();
            let objects: Vec<(String, f32, f32, i32, String)> = places
                .lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split(' ').collect();
                    let (x, y): (i32, i32) = (f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?);
                    // on the lot, not held, not a person
                    (x > 0 && y > 0 && f.get(6)? == &"0" && f.get(7)? == &"0").then(|| (f[0].to_string(), x as f32 / 16.0, y as f32 / 16.0, f[5].parse().unwrap_or(0), f.get(8).unwrap_or(&"").to_string()))
                })
                .collect();
            println!("tick {tick}: world matrices {}, objects on the lot (not held, not persons) {}", worlds.len(), objects.len());
            // the tile's centre a place is drawn at: the tile by floor, or by floor of (x16 - 1)
            // (a place on a tile's corner counts to the tile below)
            type Rule = (&'static str, fn(f32) -> f32);
            let rules: [Rule; 2] = [("floor", |x| x.floor() + 0.5), ("corner to the lower", |x| ((x * 16.0 - 1.0) / 16.0).floor() + 0.5)];
            for (name, r) in rules {
                let found = objects.iter().filter(|o| worlds.iter().any(|w| (w[0] - r(o.1)).abs() < 0.01 && (w[1] - r(o.2)).abs() < 0.01)).count();
                println!("  the tile's centre ({name}): objects with a world matrix there {found}");
            }
            // the matrix at an object's tile centre: its rotation about the vertical (the 3x3's
            // first column, in degrees) against the object's facing, and its scale
            let full: Vec<&Vec<f32>> = log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick).map(|m| &m.values).collect();
            let mut by_facing: BTreeMap<i32, BTreeMap<i32, usize>> = BTreeMap::new();
            for o in &objects {
                let (cx, cy) = (o.1.floor() + 0.5, o.2.floor() + 0.5);
                for v in full.iter().filter(|v| (v[3] - cx).abs() < 0.01 && (v[7] - cy).abs() < 0.01) {
                    let scale = (v[0] * v[0] + v[4] * v[4] + v[8] * v[8]).sqrt();
                    let angle = v[4].atan2(v[0]).to_degrees().round() as i32;
                    // (the scale is the model's: its 16-bit positions' scale folded in)
                    let key = if scale > 1e-7 { angle } else { 9999 };
                    *by_facing.entry(o.3).or_default().entry(key).or_default() += 1;
                }
            }
            println!("  facing -> the matrix's angle in degrees (9999: a zero matrix): {by_facing:?}");
            // the matrices that draw (their 3x3 not zero): their scale, and the nearest object
            let drawn: Vec<&&Vec<f32>> = full.iter().filter(|v| v[0].abs() + v[1].abs() + v[4].abs() + v[5].abs() + v[10].abs() > 1e-3).collect();
            let mut scales: BTreeMap<i32, usize> = BTreeMap::new();
            let mut near: BTreeMap<i32, usize> = BTreeMap::new();
            for v in &drawn {
                let scale = (v[0] * v[0] + v[4] * v[4] + v[8] * v[8]).sqrt();
                *scales.entry((scale * 100.0).round() as i32).or_default() += 1;
                let d = objects.iter().map(|o| ((v[3] - o.1).powi(2) + (v[7] - o.2).powi(2)).sqrt()).fold(f32::MAX, f32::min);
                *near.entry((d * 10.0).ceil() as i32).or_default() += 1;
            }
            println!("  drawing matrices {}: scales (x100) {scales:?}; distance to the nearest object's place (tenths of a tile, rounded up) {near:?}", drawn.len());
            if std::env::var_os("ASSETS_SHOW_DRAWING").is_some() {
                for v in drawn.iter().take(30) {
                    println!("    [{:.3} {:.3} {:.3} | {:.3}] [{:.3} {:.3} {:.3} | {:.3}] [{:.3} {:.3} {:.3} | {:.3}]", v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11]);
                }
            }
            // each world matrix with the model drawn after it (the next strip's batch or display
            // list, by its first bytes): the records' entries and names against the objects'
            // files, and the matrix's place and angle against the object's
            let recs = all_records(&d);
            let mut owner: std::collections::HashMap<[u8; 32], (&str, &str)> = std::collections::HashMap::new();
            for (entry, r) in &recs {
                for n in &r.nodes {
                    for (_, sm) in &n.subs {
                        if let Some(p) = sm.positions.get(..32) {
                            owner.entry(p.try_into().unwrap()).or_insert((entry, &r.name));
                        }
                        if let Some(l) = sm.list.as_ref().and_then(|l| l.2.get(..32)) {
                            owner.entry(l.try_into().unwrap()).or_insert((entry, &r.name));
                        }
                    }
                }
            }
            let stem = |path: &str| path.rsplit('/').next().unwrap_or(path).trim_end_matches(".iff").to_ascii_lowercase();
            let (mut with_model, mut named, mut at_place, mut angle_ok) = (0, 0, 0, 0);
            let mut unnamed: BTreeMap<String, usize> = BTreeMap::new();
            for m in log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick) {
                let Some(st) = log.strips.get(m.strips_before) else { continue };
                let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                let Some(&(entry, name)) = owner.get(&key) else { continue };
                with_model += 1;
                let v = &m.values;
                let mine: Vec<&(String, f32, f32, i32, String)> = objects.iter().filter(|o| stem(&o.4) == entry.to_ascii_lowercase() || stem(&o.4) == name.to_ascii_lowercase()).collect();
                if mine.is_empty() {
                    *unnamed.entry(format!("{entry}/{name}")).or_default() += 1;
                    continue;
                }
                named += 1;
                if let Some(o) = mine.iter().find(|o| (v[3] - (o.1.floor() + 0.5)).abs() < 0.01 && (v[7] - (o.2.floor() + 0.5)).abs() < 0.01) {
                    at_place += 1;
                    let angle = v[4].atan2(v[0]).to_degrees().round() as i32;
                    angle_ok += usize::from((angle - (90 - 45 * o.3)).rem_euclid(360) == 0);
                }
            }
            println!("  world matrices with a read model drawn after {with_model}: its record an object's file {named}, at that object's tile centre {at_place}, at the angle 90 - 45 x facing {angle_ok}");
            // an object's dataset: "RD_" and its interaction table's label without " tree table"
            let rd_of = |path: &str| -> Option<String> {
                let iff = housefire::iff::IffFile::parse(d.read(path)?);
                let label = iff.of(b"TTAB").next()?.label.clone();
                Some(format!("RD_{}", label.strip_suffix(" tree table").unwrap_or(&label)))
            };
            let rds: Vec<Option<String>> = objects.iter().map(|o| rd_of(&o.4)).collect();
            let ds_names: BTreeSet<String> = arc::entries(&d.read("datasets.arc").unwrap_or_default()).unwrap_or_default().into_iter().map(|e| e.name).collect();
            let with_rd = rds.iter().filter(|r| r.as_ref().is_some_and(|r| ds_names.contains(r))).count();
            if std::env::var_os("ASSETS_SHOW_NO_RD").is_some() {
                let mut seen = BTreeSet::new();
                for (o, r) in objects.iter().zip(&rds) {
                    if r.as_ref().is_none_or(|r| !ds_names.contains(r)) && seen.insert(o.4.clone()) {
                        println!("    no dataset: {} -> {r:?}", o.4);
                    }
                }
            }
            // the models an RD_ dataset keeps, by name
            let mut rd_models: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
            for (entry, r) in &recs {
                if entry.starts_with("RD_") {
                    rd_models.entry(entry).or_default().insert(&r.name);
                }
            }
            let (mut from_rd, mut placed, mut turned) = (0, 0, 0);
            for m in log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick) {
                let Some(st) = log.strips.get(m.strips_before) else { continue };
                let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                let Some(&(_, name)) = owner.get(&key) else { continue };
                let has = |r: &Option<String>| r.as_deref().and_then(|r| rd_models.get(r)).is_some_and(|ms| ms.contains(name));
                if !rds.iter().any(has) {
                    continue;
                }
                from_rd += 1;
                let v = &m.values;
                if let Some((o, _)) = objects.iter().zip(&rds).find(|(o, r)| has(r) && (v[3] - (o.1.floor() + 0.5)).abs() < 0.01 && (v[7] - (o.2.floor() + 0.5)).abs() < 0.01) {
                    placed += 1;
                    let angle = v[4].atan2(v[0]).to_degrees().round() as i32;
                    turned += usize::from((angle - (90 - 45 * o.3)).rem_euclid(360) == 0);
                }
            }
            println!("  objects whose RD_ dataset (TTAB label) is on the disc {with_rd} of {}; world matrices of a model one of the lot's objects' datasets keeps {from_rd}: at the tile centre of such an object {placed}, turned 90 - 45 x facing {turned}", objects.len());
            // the link the original uses: an object's GUID to its model (quick data's
            // ObjDefinition::ResData); each drawn record by its name's hash
            let q = d.read("quickdat.arc").unwrap_or_default();
            let res = arc::entries(&q)
                .unwrap_or_default()
                .into_iter()
                .find(|e| e.name == "SimsObjects")
                .and_then(|e| quickdata::Image::read(arc::bytes(&q, &e)))
                .map(|img| quickdata::object_resources(&img))
                .unwrap_or_default();
            let guid_of: BTreeMap<&str, u32> = places
                .lines()
                .filter_map(|l| {
                    let mut f = l.split(' ');
                    Some((f.next()?, u32::from_str_radix(f.next()?, 16).ok()?))
                })
                .collect();
            let model_of = |o: &(String, f32, f32, i32, String)| guid_of.get(o.0.as_str()).and_then(|g| res.get(g)).map(|r| r.model).filter(|&m| m != 0);
            let with_model_hash = objects.iter().filter(|o| model_of(o).is_some()).count();
            let (mut of_lot, mut placed, mut turned) = (0, 0, 0);
            let mut drawn_objects: BTreeSet<&str> = BTreeSet::new();
            let mut misplaced: BTreeMap<String, usize> = BTreeMap::new();
            for m in log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick) {
                let Some(st) = log.strips.get(m.strips_before) else { continue };
                let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                let Some(&(_, name)) = owner.get(&key) else { continue };
                let h = shader::name_hash(name);
                let mine: Vec<&(String, f32, f32, i32, String)> = objects.iter().filter(|o| model_of(o) == Some(h)).collect();
                if mine.is_empty() {
                    continue;
                }
                of_lot += 1;
                let v = &m.values;
                match mine.iter().find(|o| (v[3] - (o.1.floor() + 0.5)).abs() < 0.01 && (v[7] - (o.2.floor() + 0.5)).abs() < 0.01) {
                    Some(o) => {
                        placed += 1;
                        drawn_objects.insert(&o.0);
                        let angle = v[4].atan2(v[0]).to_degrees().round() as i32;
                        turned += usize::from((angle - (90 - 45 * o.3)).rem_euclid(360) == 0);
                    }
                    None => {
                        *misplaced.entry(name.to_string()).or_default() += 1;
                        if std::env::var_os("ASSETS_SHOW_MISPLACED").is_some_and(|n| n == name) {
                            let s = (v[0] * v[0] + v[4] * v[4] + v[8] * v[8]).sqrt();
                            println!("    {name} drawn at ({:.4}, {:.4}, {:.4}) angle {:.1} scale {s:.5}", v[3], v[7], v[11], v[4].atan2(v[0]).to_degrees());
                        }
                    }
                }
            }
            println!("  by the quick data link: lot objects with a model {with_model_hash} of {}; world matrices drawing one of their models {of_lot}: at its object's tile centre {placed} ({} objects), turned 90 - 45 x facing {turned}; elsewhere {misplaced:?}", objects.len(), drawn_objects.len());
            // whether each object's model is drawn at all near its place (a node's matrix is the
            // object's times the node's transform, so within a tile or two)
            let worlds_of = |h: u32| -> Vec<[f32; 2]> {
                log.matrices
                    .iter()
                    .filter(|m| m.id == 0xF1 && m.tick == tick)
                    .filter(|m| {
                        let Some(st) = log.strips.get(m.strips_before) else { return false };
                        let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                        owner.get(&key).is_some_and(|&(_, n)| shader::name_hash(n) == h)
                    })
                    .map(|m| [m.values[3], m.values[7]])
                    .collect()
            };
            let mut near_hist: BTreeMap<i32, usize> = BTreeMap::new();
            for o in objects.iter().filter(|o| model_of(o).is_some()) {
                let d = worlds_of(model_of(o).unwrap()).iter().map(|w| ((w[0] - o.1).powi(2) + (w[1] - o.2).powi(2)).sqrt()).fold(f32::MAX, f32::min);
                *near_hist.entry(if d == f32::MAX { -1 } else { (d * 2.0).ceil() as i32 }).or_default() += 1;
                if std::env::var_os("ASSETS_SHOW_FAR").is_some() && d > 1.5 {
                    println!("    far: {} {} at ({}, {}) nearest draw of its model {d:.2}", o.0, o.4, o.1, o.2);
                }
            }
            println!("  objects with a model, by the nearest draw of it (half tiles, rounded up; -1 none drawn): {near_hist:?}");
            // the object's own matrix: a draw of a node hanging from the root bone (0) of one of
            // its model's records; where it is against the object's place
            let mut bone_of: std::collections::HashMap<[u8; 32], i32> = std::collections::HashMap::new();
            for (_, r) in &recs {
                for n in &r.nodes {
                    for (_, sm) in &n.subs {
                        for k in [sm.positions.get(..32), sm.list.as_ref().and_then(|l| l.2.get(..32))].into_iter().flatten() {
                            bone_of.entry(k.try_into().unwrap()).or_insert(n.bone);
                        }
                    }
                }
            }
            let mut root_offsets: BTreeMap<(i32, i32), usize> = BTreeMap::new();
            let (mut roots, mut at_place, mut at_centre, mut turned_root) = (0, 0, 0, 0);
            let mut by_obj: BTreeSet<&str> = BTreeSet::new();
            for m in log.matrices.iter().filter(|m| m.id == 0xF1 && m.tick == tick) {
                let Some(st) = log.strips.get(m.strips_before) else { continue };
                let key = if st.words[0] >> 24 == 0x42 { st.at[6] } else { st.at[2] };
                let (Some(&(_, name)), Some(&bone)) = (owner.get(&key), bone_of.get(&key)) else { continue };
                if bone != 0 {
                    continue;
                }
                let h = shader::name_hash(name);
                let v = &m.values;
                let Some(o) = objects.iter().filter(|o| model_of(o) == Some(h)).min_by(|a, b| ((v[3] - a.1).powi(2) + (v[7] - a.2).powi(2)).total_cmp(&((v[3] - b.1).powi(2) + (v[7] - b.2).powi(2)))) else { continue };
                roots += 1;
                by_obj.insert(&o.0);
                *root_offsets.entry((((v[3] - o.1) * 16.0).round() as i32, ((v[7] - o.2) * 16.0).round() as i32)).or_default() += 1;
                at_place += usize::from((v[3] - o.1).abs() < 0.01 && (v[7] - o.2).abs() < 0.01);
                at_centre += usize::from((v[3] - (o.1.floor() + 0.5)).abs() < 0.01 && (v[7] - (o.2.floor() + 0.5)).abs() < 0.01);
                let angle = v[4].atan2(v[0]).to_degrees().round() as i32;
                turned_root += usize::from((angle - (90 - 45 * o.3)).rem_euclid(360) == 0);
                if std::env::var_os("ASSETS_SHOW_ROOTS").is_some() {
                    println!("    root of {name} for {} at ({}, {}) facing {}: drawn at ({:.4}, {:.4}, {:.4}) angle {angle}", o.4, o.1, o.2, o.3, v[3], v[7], v[11]);
                }
            }
            println!("  root-bone draws of a lot object's model {roots} ({} objects): at its place {at_place}, at its tile centre {at_centre}, turned 90 - 45 x facing {turned_root}; offsets from the place (sixteenths) {root_offsets:?}", by_obj.len());
            if std::env::var_os("ASSETS_SHOW_UNDRAWN").is_some() {
                for o in objects.iter().filter(|o| model_of(o).is_some() && !drawn_objects.contains(o.0.as_str())) {
                    println!("    not drawn at its place: {} {} at ({}, {}) facing {} model {:08x}", o.0, o.4, o.1, o.2, o.3, model_of(o).unwrap());
                }
            }
            if std::env::var_os("ASSETS_SHOW_UNNAMED").is_some() {
                println!("  records drawn with no object of their name: {unnamed:?}");
            }
            if std::env::var_os("ASSETS_SHOW_UNPLACED").is_some() {
                let r = |x: f32| ((x * 16.0 - 1.0) / 16.0).floor() + 0.5;
                for o in objects.iter().filter(|o| !worlds.iter().any(|w| (w[0] - r(o.1)).abs() < 0.01 && (w[1] - r(o.2)).abs() < 0.01)).take(25) {
                    let near = worlds.iter().min_by(|a, b| ((a[0] - o.1).powi(2) + (a[1] - o.2).powi(2)).total_cmp(&((b[0] - o.1).powi(2) + (b[1] - o.2).powi(2))));
                    println!("  {} at ({}, {}) facing {}: nearest {near:?} ({})", o.0, o.1, o.2, o.3, o.4);
                }
            }
        }
        "texcheck" => {
            let Some(log) = args.get(3).and_then(|p| read_log(p)) else {
                println!("usage: assets ISO texcheck LOG");
                return ExitCode::from(2);
            };
            let (by_hash, embedded) = texture_hashes(&d);
            println!("textures in datasets.arc: {embedded}");
            let mut images: BTreeMap<u32, &housefire::tree_log::TexObjRec> = BTreeMap::new();
            for x in &log.textures {
                images.entry(x.image).or_insert(x);
            }
            let mut found: BTreeMap<String, usize> = BTreeMap::new();
            let mut not: BTreeMap<String, usize> = BTreeMap::new();
            for x in images.values() {
                match by_hash.get(&x.hash) {
                    Some(v) => {
                        let (_, kind, w, h) = &v[0];
                        let same = *w == x.width as u32 && *h == x.height as u32;
                        *found.entry(format!("{kind:?} as GX format {}{}", x.format, if same { "" } else { ", size NOT the same" })).or_default() += 1;
                    }
                    None => {
                        *not.entry(format!("GX format {} {}x{}", x.format, x.width, x.height)).or_default() += 1;
                        if std::env::var_os("ASSETS_SHOW_NOT_FOUND").is_some() {
                            let uses: Vec<u32> = log.textures.iter().filter(|y| y.image == x.image).map(|y| y.tick).collect();
                            println!("not found: image {:08x} format {} {}x{} wrap {:?} mip {} hash {:016x}; set up at ticks {:?}", x.image, x.format, x.width, x.height, x.wrap, x.mip, x.hash, &uses[..uses.len().min(6)]);
                        }
                    }
                }
            }
            println!("setups {}, images {}", log.textures.len(), images.len());
            for (k, n) in &found {
                println!("  found in textures.arc: {n:5} {k}");
            }
            for (k, n) in &not {
                println!("  not found:             {n:5} {k}");
            }
        }
        a => {
            println!("unknown command {a}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
