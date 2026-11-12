//! The disc's assets, read natively, for stage 4's checks (`docs/loop.md`, the twelfth loop).
//!
//! Usage: assets ISO arcs [LIST]  every .arc on the disc: its layout read, its entry count; with
//!                                LIST, each entry ("archive<TAB>name<TAB>offset<TAB>size") into it

use std::io::Write;
use std::process::ExitCode;

use housefire::arc;
use housefire::disc::Disc;

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
        a => {
            println!("unknown command {a}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
