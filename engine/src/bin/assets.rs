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
