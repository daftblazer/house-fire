use housefire::tree_log::read_log;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let log = read_log(&a[1]).unwrap();
    let mut c: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for l in &log.tex_loads {
        let m0 = u32::from_be_bytes(l.obj[0..4].try_into().unwrap());
        let m1 = u32::from_be_bytes(l.obj[4..8].try_into().unwrap());
        *c.entry((m0, m1)).or_default() += 1;
    }
    for ((m0, m1), n) in &c {
        println!("mode0 {m0:08x} (wrap {} {} mag {} min {} bias {} edge {} maxaniso {} ) mode1 {m1:08x}: {n}", m0 & 3, (m0 >> 2) & 3, (m0 >> 4) & 1, (m0 >> 5) & 7, (m0 >> 9) & 0xff, (m0 >> 8) & 1, (m0 >> 19) & 3, );
    }
    println!("{:02x?}", log.tex_loads[0].obj);
}
