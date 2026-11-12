//! The disc's assets, read natively, for stage 4's checks (`docs/loop.md`, the twelfth loop).
//!
//! Usage: assets ISO arcs [LIST]  every .arc on the disc: its layout read, its entry count; with
//!                                LIST, each entry ("archive<TAB>name<TAB>offset<TAB>size") into it
//!        assets ISO textures         textures.arc's entries read: by kind and size, any that do not

use std::io::Write;
use std::process::ExitCode;

use housefire::arc;
use housefire::disc::Disc;
use housefire::texture;
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
        a => {
            println!("unknown command {a}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
