//! The disc's assets, read natively, for stage 4's checks (`docs/loop.md`, the twelfth loop).
//!
//! Usage: assets ISO arcs [LIST]  every .arc on the disc: its layout read, its entry count; with
//!                                LIST, each entry ("archive<TAB>name<TAB>offset<TAB>size") into it
//!        assets ISO textures         textures.arc's entries read: by kind and size, any that do not
//!        assets ISO rle              rletextu.arc's entries decoded: their texel counts
//!        assets ISO texcheck LOG     the textures the original set up (the log's 'x' records)
//!                                    against textures.arc's: each by its first level's hash

use std::io::Write;
use std::process::ExitCode;

use housefire::arc;
use housefire::disc::Disc;
use housefire::texture;
use housefire::rle_texture;
use housefire::tree_log::read_log;
use std::collections::BTreeMap;

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
        "texcheck" => {
            let Some(log) = args.get(3).and_then(|p| read_log(p)) else {
                println!("usage: assets ISO texcheck LOG");
                return ExitCode::from(2);
            };
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
