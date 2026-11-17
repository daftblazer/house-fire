//! The study aids of `render-check` (the switches its header lists): each prints what it finds
//! of the log's and the engine's draws, one line a draw or a figure.

use housefire::shader::Shader;
use housefire_render::engine_scene;
use housefire_render::frame::{self, Lighting};
use housefire_render::scene::{Class, Scene};
use std::collections::BTreeMap;

/// An FNV-1a hash of a texture's texels, to tell two images apart in a line.
fn fnv(v: &[u32]) -> u64 {
    v.iter().fold(1469598103934665603u64, |h, &x| (h ^ u64::from(x)).wrapping_mul(1099511628211))
}

pub fn sphere_log(scene: &Scene) {
    let mut seen = std::collections::BTreeSet::new();
    for d in &scene.frame.draws {
        if let Some(w) = d.material.sphere {
            let i = &scene.drawn[d.id as usize - 1];
            if seen.insert(format!("{} {:?}", i.model, w)) { println!("SPHERE log {} {:?} flags {:?} combine {:?}", i.model, w, i.flags, d.material.combine); }
        }
    }
}

pub fn shears_log(scene: &Scene) {
    // study aid: each logged shadow draw's shear (a, b) of the camera's x and y columns into
    // its view's third column, the model, and its world matrix's translation
    let cam = scene.frame.camera.view;
    for d in &scene.frame.draws {
        let info = &scene.drawn[d.id as usize - 1];
        if info.class != Class::Shadow { continue }
        let Some(v) = d.view else { continue };
        // v[:,2] = a * cam[:,0] + b * cam[:,1] (least squares over the three rows)
        let (mut s00, mut s01, mut s11, mut t0, mut t1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for r in 0..3 { let (x, y, z) = (cam[r * 4], cam[r * 4 + 1], v[r * 4 + 2]); s00 += x * x; s01 += x * y; s11 += y * y; t0 += x * z; t1 += y * z; }
        let det = s00 * s11 - s01 * s01;
        let (a, b) = ((t0 * s11 - t1 * s01) / det, (s00 * t1 - s01 * t0) / det);
        println!("SHEAR {:.4} {:.4} {} world ({:.2}, {:.2}, {:.2}) draw {}", a, b, info.model, d.world[3], d.world[7], d.world[11], d.id);
    }
}

pub fn sphere_engine(made: &[engine_scene::Made]) {
    let mut seen = std::collections::BTreeSet::new();
    for m in made {
        if let Some(w) = m.draw.material.sphere
            && seen.insert(format!("{} {:?}", m.info.model, w))
        {
            println!("SPHERE eng {} {:?} flags {:?}", m.info.model, w, m.info.flags);
        }
    }
}

pub fn light_study(made: &[engine_scene::Made], oracle: &[([f32; 12], Lighting, frame::Material, usize)], plan: &engine_scene::Plan, shaders: &housefire::fasthash::FastMap<u32, Shader>, scene: &Scene, view: frame::Mat3x4) {
    // the log's light sets in world space, once for each object place; the engine's lamps
    let v = view;
    let to_world = |p: [f32; 3]| -> [f32; 3] { let q = [p[0] - v[3], p[1] - v[7], p[2] - v[11]]; [0, 1, 2].map(|c| (0..3).map(|r| v[4 * r + c] * q[r]).sum::<f32>()) };
    let turn_world = |p: [f32; 3]| -> [f32; 3] { [0, 1, 2].map(|c| (0..3).map(|r| v[4 * r + c] * p[r]).sum::<f32>()) };
    let mut seen = std::collections::BTreeSet::new();
    for m in made {
        let Some(o) = oracle.iter().find(|o| (0..12).all(|i| (o.0[i] - m.draw.world[i]).abs() < 1e-4 * if i % 4 == 3 { 1.0 } else { 0.05 })) else { continue };
        let key = format!("{:?}", o.1);
        if !seen.insert(key) { continue; }
        let Lighting::Lit { ambient, directionals, points } = &o.1 else { println!("LS {} vertex colours", m.info.model); continue };
        let org = Some((plan.draws[m.from].origin, plan.draws[m.from].object));
        let dirs: Vec<String> = directionals.iter().map(|d| format!("dir {:.3?} to {:.3?}", d.colour, turn_world(d.to_light))).collect();
        let pts: Vec<String> = points.iter().map(|p| format!("pt {:.2?} col {:.3?} k1 {}", to_world(p.position), p.colour, p.k1)).collect();
        println!("LS {} at {:?} amb {:.3?} {} {}", m.info.model, org, ambient, dirs.join(" "), pts.join(" "));
    }
    // the engine's point lights against the log's (needs HF_ORACLE=1 for the log's), once for
    // each place and edge: `LSUM` counts the same sets by kind, `LD` shows the ones that differ
    let mut tally: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut counted = std::collections::BTreeSet::new();
    for m in made {
        let d = &plan.draws[m.from];
        let Some(o) = oracle.iter().find(|o| o.3 == scene.meshes[m.draw.mesh.0 as usize].vertices.len() && (0..12).all(|i| (o.0[i] - m.draw.world[i]).abs() < 1e-4 * if i % 4 == 3 { 1.0 } else { 0.05 })) else { continue };
        let Lighting::Lit { points: logged, .. } = &o.1 else { continue };
        let Lighting::Lit { points: ours, .. } = engine_scene::lighting_for(&plan.lights.of(d), &view, None) else { continue };
        if std::env::var_os("HF_COLOURS").is_some() {
            let mut pairs: Vec<String> = Vec::new();
            for lp in logged {
                let lw = to_world(lp.position);
                if let Some(ep) = ours.iter().find(|ep| { let w = to_world(ep.position); (0..3).all(|k| (w[k] - lw[k]).abs() < 1e-2) }) {
                    pairs.push(format!("{:.3}", lp.colour[0] / ep.colour[0].max(1e-6)));
                }
            }
            let part = plan.models.parts.parts_of(d.part.0).find(|p| p.index == d.part).unwrap();
            let mat = shaders.get(&d.shader.unwrap_or(part.batch.hash)).and_then(|sh| sh.material());
            pairs.push(format!("material {:?}", mat));
            println!("LC {} {} v{} at {:?} ratios {}", m.info.model, m.info.textures[0].clone().unwrap_or_default(), scene.meshes[m.draw.mesh.0 as usize].vertices.len(), d.origin, pairs.join(" "));
        }
        if !counted.insert((d.centre[0].to_bits(), d.centre[1].to_bits(), d.edge.map(|e| (e[1][0].to_bits(), e[1][1].to_bits())))) { continue; }
        let places = |ps: &[frame::Point]| { let mut k: Vec<String> = ps.iter().map(|p| format!("{:.1?}", to_world(p.position))).collect(); k.sort(); k };
        let (l, e) = (places(logged), places(&ours));
        // the same lights up to a moving lamp's drift (0.15 of a tile), whatever the order
        let close = |a: &[frame::Point], b: &[frame::Point]| a.iter().all(|p| b.iter().any(|q| (0..3).all(|k| (to_world(p.position)[k] - to_world(q.position)[k]).abs() < 0.15))) && b.iter().all(|q| a.iter().any(|p| (0..3).all(|k| (to_world(p.position)[k] - to_world(q.position)[k]).abs() < 0.15)));
        let kind = if d.edge.is_some() { "wall or fence" } else { "object" };
        if std::env::var_os("HF_CELLSTUDY").is_some() && d.edge.is_none() {
            // the grid points (offsets from the place's floor) whose set is the log's
            let (fx, fy) = (d.origin[0].floor() as i32, d.origin[1].floor() as i32);
            let hits: Vec<(i32, i32)> = (-1..=2).flat_map(|dx| (-1..=2).map(move |dy| (dx, dy))).filter(|&(dx, dy)| {
                let Lighting::Lit { points, .. } = engine_scene::lighting_for(&plan.lights.at_in(d.centre, [(fx + dx) as f32, (fy + dy) as f32]), &view, None) else { return false };
                close(&points, logged)
            }).collect();
            let angle = plan.placements.iter().find(|p| p.leader == d.object).map(|p| p.angle);
            println!("CELL {} origin {:?} angle {angle:?} hits {hits:?}", m.info.model, d.origin);
        }
        let t = tally.entry(kind).or_default();
        if l == e {
            t.0 += 1;
        } else {
            t.1 += 1;
            // which whole-tile grid points near the place give the log's set
            let hits: Vec<(i32, i32)> = (-3..=3).flat_map(|dx| (-3..=3).map(move |dy| (dx, dy))).map(|(dx, dy)| (d.origin[0].round() as i32 + dx, d.origin[1].round() as i32 + dy)).filter(|&(x, y)| {
                let Lighting::Lit { points, .. } = engine_scene::lighting_for(&plan.lights.at_in(d.centre, [x as f32, y as f32]), &view, None) else { return false };
                places(&points) == l
            }).collect();
            println!("LD grid points giving the log's set: {hits:?}");
            println!("LD {kind} {} ({} vertices) at {:?} centre {:.3?} edge {:?}\n    log {l:?}\n    eng {e:?}", m.info.model, scene.meshes[m.draw.mesh.0 as usize].vertices.len(), d.origin, d.centre, d.edge);
        }
    }
    // the windows' meshes one by one (the log's set found by vertex count and matrix): `LW`
    for m in made.iter().filter(|m| m.info.model.contains("Window") && !m.info.model.contains("Door")) {
        let n = scene.meshes[m.draw.mesh.0 as usize].vertices.len();
        let w = &m.draw.world;
        let near = |a: &[f32; 12]| (0..12).all(|i| (a[i] - w[i]).abs() <= 1e-4 * if i % 4 == 3 { 1.0 } else { 0.05 });
        let log = oracle.iter().find(|o| o.3 == n && near(&o.0)).map_or("none".to_string(), |o| match &o.1 {
            Lighting::Lit { ambient, directionals, .. } => format!("amb {:.3} sun {}", ambient[0], !directionals.is_empty()),
            _ => "vertex colours".into(),
        });
        let vs = &scene.meshes[m.draw.mesh.0 as usize].vertices;
        let mean: [f32; 3] = [0, 1, 2].map(|c| vs.iter().map(|v| f32::from(v.position[c])).sum::<f32>() / n as f32);
        let nrm: [f32; 3] = [0, 1, 2].map(|c| vs.iter().map(|v| f32::from(v.normal[c])).sum::<f32>() / n as f32);
        let at: [f32; 3] = [0, 1, 2].map(|r| w[4 * r] * mean[0] + w[4 * r + 1] * mean[1] + w[4 * r + 2] * mean[2] + w[4 * r + 3]);
        println!("LW {} {n} at {:?} sub {} rows {:.2?} {:.2?} {:.2?} mean {:.0?} normal {:.0?} centre {:.3?} log {log}", m.info.model, plan.draws[m.from].origin, plan.draws[m.from].part.2, [w[0], w[1], w[2]].map(|x| x * 1000.0), [w[4], w[5], w[6]].map(|x| x * 1000.0), [w[8], w[9], w[10]].map(|x| x * 1000.0), mean, nrm, at);
    }
    for (kind, (same, differ)) in tally {
        println!("LSUM {kind}: same {same}, differ {differ}");
    }
    println!("LS clock {:?}; the engine's sun {:?}", plan.clock, plan.lights.at(1.0, 1.0).sun);
    let mut shown = std::collections::BTreeSet::new();
    for (d, m) in made.iter().map(|m| (&plan.draws[m.from], m)) {
        if !shown.insert((d.origin[0].to_bits(), d.origin[1].to_bits())) { continue; }
        let Lighting::Lit { ambient, directionals, points } = engine_scene::lighting_for(&plan.lights.of(d), &view, None) else { continue };
        let dirs: Vec<String> = directionals.iter().map(|d| format!("dir {:.3?} to {:.3?}", d.colour, turn_world(d.to_light))).collect();
        let pts: Vec<String> = points.iter().map(|p| format!("pt {:.2?} col {:.3?} k1 {}", to_world(p.position), p.colour, p.k1)).collect();
        println!("LE {} at {:?} amb {:.3?} {} {}", m.info.model, d.origin, ambient, dirs.join(" "), pts.join(" "));
    }
    for l in &plan.lamps {
        for x in &l.lights {
            println!("LAMP leader {} at ({}, {}) light node {} at {:.2?} colour {:.3?} reach {}", l.leader, l.place.x, l.place.y, x.node, x.at, x.colour, x.reach);
        }
    }
}

pub fn material_diff(made: &[engine_scene::Made], oracle: &[([f32; 12], Lighting, frame::Material, usize)], scene: &Scene, view: frame::Mat3x4) {
    // the materials against the log's, by world matrix and vertex count: what differs, by model
    let near = |a: &[f32; 12], b: &[f32; 12]| (0..12).all(|i| (a[i] - b[i]).abs() <= 1e-4 * if i % 4 == 3 { 1.0 } else { a[0].abs().max(a[5]).max(a[10]).max(1e-9) });
    let tex = |scene: &housefire_render::scene::Scene, m: &frame::Material, k: usize| m.layers[k].map(|l| (scene.textures.get(l.texture.0 as usize).map(|t| (t[0].0, t[0].1, t.len(), fnv(&t[0].2))), l.wrap, l.linear, l.mipmaps));
    let mut by: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for m in made {
        let n = scene.meshes[m.draw.mesh.0 as usize].vertices.len();
        let Some(o) = oracle.iter().find(|o| o.3 == n && near(&o.0, &m.draw.world)) else {
            let s = housefire::scene::Camera::on_screen(&view, &scene.frame.camera.projection, [m.draw.world[3], m.draw.world[7], m.draw.world[11]]);
            if s.is_some_and(|s| (0.0..=1.04).contains(&s[0]) && (-0.19..=0.63).contains(&s[1])) {
                by.entry(format!("no log draw, on screen, {}", m.info.model)).or_insert((0, format!("{:?}", s))).0 += 1;
            }
            by.entry("no log draw".into()).or_insert((0, m.info.model.to_string())).0 += 1;
            continue;
        };
        let ulps = (0..12).map(|i| (m.draw.world[i].to_bits() as i64 - o.0[i].to_bits() as i64).abs()).max().unwrap_or(0);
        by.entry(format!("WORLD {} ulps", if ulps == 0 { "0".to_string() } else if ulps < 16 { "1..15".into() } else if ulps < 4096 { "16..4095".into() } else { "more".to_string() })).or_insert((0, m.info.model.to_string())).0 += 1;
        let (e, l) = (&m.draw.material, &o.2);
        let mut diffs = Vec::new();
        if std::mem::discriminant(&e.combine) != std::mem::discriminant(&l.combine) || e.combine != l.combine && !matches!(e.combine, frame::Combine::Crossfade { .. }) {
            diffs.push(format!("combine {:?} vs {:?}", e.combine, l.combine));
        }
        if e.blend != l.blend {
            diffs.push(format!("blend {:?} vs {:?}", e.blend, l.blend));
        }
        if e.alpha_test != l.alpha_test {
            diffs.push(format!("alpha {:?} vs {:?}", e.alpha_test.map(|a| a.refs[0]), l.alpha_test.map(|a| a.refs[0])));
        }
        if e.depth_write != l.depth_write || e.depth_test != l.depth_test {
            diffs.push("depth".into());
        }
        if e.dst_alpha != l.dst_alpha || e.write_alpha != l.write_alpha || e.write_colour != l.write_colour {
            diffs.push(format!("dstalpha {:?}/{} vs {:?}/{}", e.dst_alpha, e.write_alpha, l.dst_alpha, l.write_alpha));
        }
        for k in 0..2 {
            let (te, tl) = (tex(scene, e, k), tex(scene, l, k));
            if te.as_ref().map(|t| &t.0) != tl.as_ref().map(|t| &t.0) {
                diffs.push(format!("layer {k} texture {:?} vs {:?}", te.as_ref().map(|t| (t.0.map(|x| (x.0, x.1, x.2)), &m.info.textures[k])), tl.as_ref().map(|t| t.0.map(|x| (x.0, x.1, x.2)))));
            } else if te != tl {
                diffs.push(format!("layer {k} sampler {:?} vs {:?}", te.map(|t| (t.1, t.2, t.3)), tl.map(|t| (t.1, t.2, t.3))));
            }
        }
        // the log's state by what the engine knows of the draw: its kind, blend, the layer 0
        // texture's alpha (none, some pixels clear, all clear)
        let alpha = m.draw.material.layers[0].and_then(|l| scene.textures.get(l.texture.0 as usize)).map_or("no texture", |t| {
            let n = t[0].2.iter().filter(|&&p| p >> 24 != 255).count();
            if n == 0 { "opaque" } else if n == t[0].2.len() { "all clear" } else { "cutout" }
        });
        let key = format!("STATE {:?} {:?} layers {} texture {alpha}: log alpha {:?} dst {:?}/{} depth {:?}/{}", m.info.class, e.blend, e.layers.iter().flatten().count(), l.alpha_test.map(|a| (a.refs[0], a.comp[0].0, a.op)), l.dst_alpha, l.write_alpha, l.depth_test.map(|c| c.0), l.depth_write);
        by.entry(key).or_insert((0, m.info.model.to_string())).0 += 1;
        by.entry(if diffs.is_empty() { "same".into() } else { diffs.join("; ") }).or_insert((0, m.info.model.to_string())).0 += 1;
    }
    for (k, (n, ex)) in by {
        println!("MAT {n:>4} e.g. {ex}: {k}");
    }
}

pub fn floor_cmp(plan: &engine_scene::Plan, log: &housefire::tree_log::Log, tick: u32) {
    // (study aid: each engine strip against the log's floor strip with the same corners: the
    // colours' differences, summed up, and the strips the log does not have)
    let log_strips: Vec<&Vec<housefire::tree_log::StripVertex>> = log.strip_vertices.iter().filter(|(j, _)| log.other_draws[*j].0.tick == tick).map(|(_, v)| v).collect();
    let (mut matched, mut missing, mut same, mut near, mut far, mut corners) = (0, 0, 0, 0, 0, 0);
    let mut worst: Vec<(i32, String)> = Vec::new();
    for st in &plan.floors {
        let hit = log_strips.iter().find(|v| v.len() == st.corners.len() && v.iter().zip(&st.corners).all(|(a, b)| (a.0[0] - b.0[0]).abs() < 1e-3 && (a.0[1] - b.0[1]).abs() < 1e-3));
        let Some(v) = hit else { missing += 1; println!("  no logged strip for room {} shader {:08x} run (x {}, y {}, n {}) value {}", st.room, st.shader, st.run.x, st.run.y, st.run.n, st.run.value); continue };
        matched += 1;
        for (a, b) in v.iter().zip(&st.corners) {
            corners += 1;
            let d = (0..3).map(|k| (i32::from(a.2.to_be_bytes()[k]) - i32::from(b.2[k])).abs()).max().unwrap_or(0);
            if d == 0 { same += 1 } else if d <= 4 { near += 1 } else { far += 1 }
            if d > 4 { worst.push((d, format!("room {} shader {:08x} run {:?} corner ({}, {}) log {:08x} engine {:02x?}", st.room, st.shader, (st.run.x, st.run.y, st.run.n), a.0[0], a.0[1], a.2, b.2))); }
        }
    }
    worst.sort_by_key(|w| -w.0);
    println!("floor strips: engine {}, log {} in the tick; engine strips equal to a logged one {matched}, with no logged one {missing}; corners {corners}: colour exact {same}, within 4 {near}, beyond {far}", plan.floors.len(), log_strips.len());
    for w in worst.iter().take(8) { println!("  {}", w.1); }
}

pub fn places(plan: &engine_scene::Plan, scene: &Scene, view: frame::Mat3x4) {
    let proj = scene.frame.camera.projection;
    let mut seen = std::collections::BTreeSet::new();
    for d in plan.draws.iter().filter(|d| d.object != 0) {
        if !seen.insert(d.object) { continue }
        let twin = scene.frame.draws.iter().any(|l| (0..12).all(|i| (l.world[i] - d.world[i]).abs() < 1e-3));
        let sc = housefire::scene::Camera::on_screen(&view, &proj, [d.origin[0], d.origin[1], 0.0]);
        println!("PLACE object {} origin ({:.2},{:.2}) screen {:.5?} twin {}", d.object, d.origin[0], d.origin[1], sc, twin);
    }
}

pub fn shears_engine(plan: &engine_scene::Plan, scene: &Scene) {
    // study aid: each planned shadow with its logged twin's shear and the engine's light set
    let cam = scene.frame.camera.view;
    for d in &plan.shadows {
        let twin = scene.frame.draws.iter().find(|l| scene.drawn[l.id as usize - 1].class == Class::Shadow && l.view.is_some() && (0..12).all(|i| (l.world[i] - d.world[i]).abs() < 1e-3));
        let Some(l) = twin else { println!("SHEARE {} at {:?} no twin", d.object, d.origin); continue };
        let v = l.view.unwrap();
        let (mut s00, mut s01, mut s11, mut t0, mut t1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for r in 0..3 { let (x, y, z) = (cam[r * 4], cam[r * 4 + 1], v[r * 4 + 2]); s00 += x * x; s01 += x * y; s11 += y * y; t0 += x * z; t1 += y * z; }
        let det = s00 * s11 - s01 * s01;
        let (a, b) = ((t0 * s11 - t1 * s01) / det, (s00 * t1 - s01 * t0) / det);
        let ls = plan.lights.at_in(d.centre, d.origin);
        let pts: Vec<String> = ls.points.iter().map(|p| format!("({:.2},{:.2},{:.2}) r{:.2} c{:.2?}", p.at[0], p.at[1], p.at[2], p.reach, p.colour)).collect();
        let to_world = |p: [f32; 3]| -> [f32; 3] { let q = [p[0] - cam[3], p[1] - cam[7], p[2] - cam[11]]; [0, 1, 2].map(|c| (0..3).map(|r| cam[4 * r + c] * q[r]).sum::<f32>()) };
        let lit = scene.frame.draws.iter().find(|l| scene.drawn[l.id as usize - 1].class != Class::Shadow && (0..12).all(|i| i == 11 || (l.world[i] - d.world[i]).abs() < 1e-3) && matches!(l.lighting, Lighting::Lit { .. }));
        let logpts = match lit.map(|l| &l.lighting) { Some(Lighting::Lit { points, .. }) => points.iter().map(|p| format!("({:.2},{:.2},{:.2}) c{:?} k0 {} k1 {}", to_world(p.position)[0], to_world(p.position)[1], to_world(p.position)[2], p.colour, p.k0, p.k1)).collect::<Vec<_>>().join(" "), _ => "none".into() };
        let dirs = match lit.map(|l| &l.lighting) { Some(Lighting::Lit { directionals, .. }) => directionals.iter().map(|q| { let w = (0..3).map(|c| (0..3).map(|r| cam[4 * r + c] * q.to_light[r]).sum::<f32>()).collect::<Vec<_>>(); format!("DIR ({:.3},{:.3},{:.3}) c{:?}", w[0], w[1], w[2], q.colour) }).collect::<Vec<_>>().join(" "), _ => "none".into() };
        println!("SHEARL {:?} {:?} log lights {} {}", d.origin, (a, b), logpts, dirs);
        println!("SHEARE {} {:?} kind {:?} eng {:.4?} origin ({:.2},{:.2}) shear ({:.4},{:.4}) room {} sun {} lights {}", scene.drawn[l.id as usize - 1].model, d.object, d.kind, engine_scene::light_shear(plan.place_of(d), &ls.points), d.origin[0], d.origin[1], a, b, plan.lights.room_of_place(d.origin[0], d.origin[1]), ls.sun.is_some(), pts.join(" "));
    }
}

pub fn floor_dump(scene: &Scene) {
    for (i, d) in scene.frame.draws.iter().enumerate() {
        let info = &scene.drawn[d.id as usize - 1];
        if matches!(info.class, Class::Floor | Class::FloorMask) {
            let m = &scene.meshes[d.mesh.0 as usize];
            let xs: Vec<f32> = m.vertices.iter().map(|v| v.position[0] as f32 / 64.0).collect();
            let ys: Vec<f32> = m.vertices.iter().map(|v| v.position[1] as f32 / 64.0).collect();
            let f = |v: &[f32], g: fn(f32, f32) -> f32, z: f32| v.iter().copied().fold(z, g);
            if std::env::var_os("HF_FLOORDUMP").is_some_and(|v| v == "mat") { println!("FDM {:?} {:?}", d.material, d.lighting); }
            println!("FD {i} {} v{} x {}..{} y {}..{} tex {:?} blend {:?} comb {:?} c0 {:02x?} c1 {:02x?}", info.model, m.vertices.len(), f(&xs, f32::min, 99.0), f(&xs, f32::max, -99.0), f(&ys, f32::min, 99.0), f(&ys, f32::max, -99.0), info.textures, d.material.blend, d.material.combine, m.vertices[0].colour, m.vertices[1].colour);
        }
    }
}

pub fn shadow_dump(scene: &Scene) {
    for (i, d) in scene.frame.draws.iter().enumerate() {
        let info = &scene.drawn[d.id as usize - 1];
        if matches!(info.class, Class::Shadow) {
            let w = d.world;
            if std::env::var_os("HF_SHADOWDUMP").is_some_and(|v| v == "mat") {
                println!("SDM {:?} {:?}", d.material, d.lighting);
            }
            if std::env::var_os("HF_SHADOWDUMP").is_some_and(|v| v == "view") { println!("SDV {i} {} v{} {:?} at {:.2} {:.2} {:.2}", info.model, scene.meshes[d.mesh.0 as usize].vertices.len(), d.view.map(|v| [v[2], v[6], v[10]]), d.world[3], d.world[7], d.world[11]); }
            println!("SD {i} {} v{} at {:.3} {:.3} {:.3} rot {:.1} scale {:.4} {:.4}", info.model, scene.meshes[d.mesh.0 as usize].vertices.len(), w[3], w[7], w[11], w[4].atan2(w[0]).to_degrees(), w[0].hypot(w[4]), w[10]);
        }
    }
}

pub fn draw_order(scene: &Scene) {
    // (study aid: the draws in order, runs of the same class, model and blend collapsed)
    let mut last = String::new();
    let mut n = 0;
    for d in &scene.frame.draws {
        let info = &scene.drawn[d.id as usize - 1];
        let tag = format!("{:?}/{}/{}", info.class, info.model, if matches!(d.material.blend, frame::Blend::Opaque) { "op" } else { "bl" });
        if tag == last {
            n += 1;
            continue;
        }
        if n > 0 {
            println!("ORD {last} x{n}");
        }
        last = tag;
        n = 1;
    }
    println!("ORD {last} x{n}");
}

pub fn list_model(scene: &Scene, name: &str) {
    // HF_LISTLIGHT=1 adds each draw's logged light set in world space
    let v = scene.frame.camera.view;
    let to_world = |p: [f32; 3]| -> [f32; 3] { let q = [p[0] - v[3], p[1] - v[7], p[2] - v[11]]; [0, 1, 2].map(|c| (0..3).map(|r| v[4 * r + c] * q[r]).sum::<f32>()) };
    if std::env::var_os("HF_LISTLIGHT").is_some() {
        for (i, d) in scene.frame.draws.iter().enumerate().filter(|(_, d)| scene.drawn[d.id as usize - 1].model[..] == *name || (name == "SKINNED" && scene.drawn[d.id as usize - 1].class == Class::Skinned)) {
            if let Lighting::Lit { ambient, points, directionals } = &d.lighting {
                let pts: Vec<String> = points.iter().map(|p| format!("{:.2?} col {:.3?} k {} {}", to_world(p.position), p.colour, p.k0, p.k1)).collect();
                let dirs: Vec<String> = directionals.iter().map(|q| format!("{:.3?}", q.colour)).collect();
                let at = d.bones.as_ref().map(|b| [b[0][3], b[0][7], b[0][11]]);
                println!("  light {i}: amb {ambient:.3?} dirs {} at {at:.1?} pts {}", dirs.join(" "), pts.join(" | "));
            }
        }
    }
    // (study aid: the draws of a model in draw order: its mesh's vertices, textures, flags, state)
    for (i, d) in scene.frame.draws.iter().enumerate().filter(|(_, d)| scene.drawn[d.id as usize - 1].model[..] == *name) {
        let info = &scene.drawn[d.id as usize - 1];
        println!("  draw {i}: {} vertices at ({:.2}, {:.2}, {:.2}) {:?} flags {:?} {:?} blend {:?} alpha {:?} depth {:?}/{} dst {:?}", scene.meshes[d.mesh.0 as usize].vertices.len(), d.world[3], d.world[7], d.world[11], info.textures, info.flags, d.material.combine, d.material.blend, d.material.alpha_test.map(|a| a.refs[0]), d.material.depth_test.is_some(), d.material.depth_write, d.material.dst_alpha);
    }
}
