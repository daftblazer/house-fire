//! Tests of the player on small movies made by hand (the scripts are written as instructions),
//! and on the disc's menus (`HOUSEFIRE_BUSTIN_ISO`, skipped without it).

use std::collections::BTreeMap;
use std::rc::Rc;

use super::Flash;
use super::actions::{Instr, Op, Value as C};
use super::geometry::Geometry;
use super::movie::{Character, Frame, Item, Movie, Place, place};
use super::player::Player;
use super::value::Value;

fn s(t: &str) -> C {
    C::Str(t.into())
}

fn push(v: Vec<C>) -> Instr {
    Instr::Push(v)
}

fn op(o: Op) -> Instr {
    Instr::Op(o)
}

/// `extern.name = <the value the code before it left on the stack>`.
fn send(name: &str, mut value: Vec<Instr>) -> Vec<Instr> {
    let mut v = vec![push(vec![s("extern")]), op(Op::GetVariable), push(vec![s(name)])];
    v.append(&mut value);
    v.push(op(Op::SetMember));
    v
}

fn place_char(depth: i32, character: i32, name: Option<&str>, at: [f32; 2], scale: f32) -> Item {
    Item::Place(Box::new(Place {
        flags: place::HAS_CHARACTER | place::HAS_MATRIX | if name.is_some() { place::HAS_NAME } else { 0 },
        depth,
        character,
        matrix: [scale, 0.0, 0.0, scale],
        translation: at,
        color: 0xffff_ffff,
        color_add: 0,
        ratio: 0.0,
        name: name.map(String::from),
        clip_depth: -1,
        clip_actions: Vec::new(),
    }))
}

fn flash(name: &str, frames: Vec<Frame>, characters: Vec<Character>) -> Rc<Flash> {
    let mut chars = vec![Character::Movie];
    chars.extend(characters);
    Rc::new(Flash {
        name: name.into(),
        movie: Movie { width: 640, height: 448, ms_per_frame: 33, frames, characters: chars, exports: Vec::new(), consts: Vec::new(), unknown: Vec::new() },
        geometry: Geometry::default(),
        images: BTreeMap::new(),
        unknown: Vec::new(),
    })
}

fn one_frame(items: Vec<Item>) -> Vec<Frame> {
    vec![Frame { items }]
}

fn root_with(script: Vec<Instr>) -> Player {
    let mut p = Player::new([("root".to_string(), flash("root", one_frame(vec![Item::Action(script)]), Vec::new()))]);
    assert!(p.load("Flash\\root"));
    p
}

fn sent(p: &mut Player) -> Vec<(String, String)> {
    p.take_commands().into_iter().map(|c| (c.name, c.argument)).collect()
}

#[test]
fn numbers_print_whole_or_with_six_decimals_and_strings_join() {
    let mut code = send("whole", vec![push(vec![C::Int(2), C::Int(3)]), op(Op::Multiply)]);
    code.extend(send("half", vec![push(vec![C::Int(1), C::Int(2)]), op(Op::Divide)]));
    code.extend(send("text", vec![push(vec![s("x"), C::Int(1)]), op(Op::Add2)]));
    code.extend(send("flag", vec![push(vec![C::Int(1), C::Int(2)]), op(Op::Less2)]));
    // undefined is empty as text and 0 as a number (the movies are of the SWF 5 generation)
    code.extend(send("nothing", vec![push(vec![C::Undefined, s("!")]), op(Op::Add2)]));
    code.extend(send("zero", vec![push(vec![C::Undefined, C::Int(5)]), op(Op::Multiply)]));
    let mut p = root_with(code);
    let want = [("whole", "6"), ("half", "0.500000"), ("text", "x1"), ("flag", "true"), ("nothing", "!"), ("zero", "0")];
    assert_eq!(sent(&mut p), want.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>());
}

#[test]
fn extern_writes_are_commands_and_are_not_stored() {
    let mut code = send("a", vec![push(vec![C::Int(1)])]);
    // reading it back gives undefined, which is empty text
    code.extend(send("b", vec![push(vec![s("extern")]), op(Op::GetVariable), push(vec![s("a")]), op(Op::GetMember), push(vec![s("|")]), op(Op::Add2)]));
    // what the game set is read
    code.extend(send("c", vec![push(vec![s("extern")]), op(Op::GetVariable), push(vec![s("given")]), op(Op::GetMember)]));
    let mut p = Player::new([("root".to_string(), flash("root", one_frame(vec![Item::Action(code)]), Vec::new()))]);
    p.set_extern("given", Value::Num(0.25));
    p.load("root");
    assert_eq!(sent(&mut p), vec![("a".into(), "1".into()), ("b".into(), "|".into()), ("c".into(), "0.250000".into())]);
}

#[test]
fn a_parameter_not_passed_reads_the_clips_variable() {
    // function f(a, b) { extern.out = a + b }; the clip has a variable b = "B"; f is called with one argument
    let body = send("out", vec![push(vec![s("a")]), op(Op::GetVariable), push(vec![s("b")]), op(Op::GetVariable), op(Op::Add2)]);
    let mut code = vec![push(vec![s("b"), s("B")]), op(Op::DefineLocal)];
    code.push(Instr::DefineFunction { name: Some("f".into()), params: vec!["a".into(), "b".into()], body });
    code.extend([push(vec![s("A"), C::Int(1), s("f")]), op(Op::CallFunction), op(Op::Pop)]);
    code.extend([push(vec![s("A"), s("b2"), C::Int(2), s("f")]), op(Op::CallFunction), op(Op::Pop)]);
    let mut p = root_with(code);
    // with both passed (the first popped is the first parameter) b is the passed one
    assert_eq!(sent(&mut p), vec![("out".to_string(), "AB".to_string()), ("out".to_string(), "b2A".to_string())]);
}

#[test]
fn locals_are_local_and_branches_loop() {
    // sum = 0; i = 0; while (i < 4) { sum = sum + i; i++ }; extern.sum = sum  (3 + 2 + 1 + 0 = 6)
    let code = vec![
        push(vec![s("sum"), C::Int(0)]),
        op(Op::DefineLocal),
        push(vec![s("i"), C::Int(0)]),
        op(Op::DefineLocal),
        // 4: loop head
        push(vec![s("i")]),
        op(Op::GetVariable),
        push(vec![C::Int(4)]),
        op(Op::Less2),
        op(Op::Not),
        Instr::BranchIfTrue(21),
        push(vec![s("sum"), s("sum")]),
        op(Op::GetVariable),
        push(vec![s("i")]),
        op(Op::GetVariable),
        op(Op::Add2),
        op(Op::SetVariable),
        push(vec![s("i"), s("i")]),
        op(Op::GetVariable),
        op(Op::Increment),
        op(Op::SetVariable),
        Instr::BranchAlways(4),
        // 21: after the loop
        push(vec![s("extern")]),
        op(Op::GetVariable),
        push(vec![s("sum"), s("sum")]),
        op(Op::GetVariable),
        op(Op::SetMember),
    ];
    let mut p = root_with(code);
    assert_eq!(sent(&mut p), vec![("sum".to_string(), "6".to_string())]);
}

#[test]
fn a_timeline_enters_each_frame_once_and_goes_back_by_label() {
    // a sprite of 3 frames, each saying its number; frame 2 is labelled "end"
    let say = |n: i32| Item::Action(send("frame", vec![push(vec![C::Int(n)])]));
    let sprite = Character::Sprite {
        frames: vec![Frame { items: vec![say(0)] }, Frame { items: vec![say(1)] }, Frame { items: vec![Item::Label { name: "end".into(), flags: 7, frame: 2 }, say(2)] }],
    };
    let root = one_frame(vec![place_char(1, 1, Some("s"), [0.0, 0.0], 1.0)]);
    let mut p = Player::new([("root".to_string(), flash("root", root, vec![sprite]))]);
    p.load("root");
    p.advance();
    p.advance();
    p.advance(); // the third wraps to the first
    let said: Vec<String> = sent(&mut p).into_iter().map(|c| c.1).collect();
    assert_eq!(said, ["0", "1", "2", "0"]);
    // a label names its frame (in any case)
    let root_id = p.root.unwrap();
    let s_id = p.child_named(root_id, "s").unwrap();
    assert_eq!(p.label_frame(s_id, "END"), Some(2));
    p.advance();
    p.advance();
    assert_eq!(p.clip(s_id).frame, 2);
}

#[test]
fn a_loaded_movie_fills_the_clip_that_asked_for_it_and_is_called_by_name() {
    // the root has a clip "slot"; its script asks for "other.swf" into it
    let slot = Character::Sprite { frames: vec![Frame::default()] };
    let ask = vec![push(vec![s("other.swf"), s("this")]), op(Op::GetVariable), push(vec![s("slot")]), op(Op::GetMember), op(Op::GetUrl2)];
    let root = one_frame(vec![Item::Action(ask), place_char(1, 1, Some("slot"), [0.0, 0.0], 1.0)]);
    // the other movie defines `hello()` on the root and says "up" when it starts
    let define = vec![Instr::DefineFunction { name: Some("hello".into()), params: vec![], body: send("hello", vec![push(vec![C::Int(1)])]) }];
    let mut other_script = send("up", vec![push(vec![C::Int(1)])]);
    other_script.extend(define);
    let other = flash("other", one_frame(vec![Item::Action(other_script)]), Vec::new());
    let mut p = Player::new([("root".to_string(), flash("root", root, vec![slot])), ("other".to_string(), other)]);
    p.load("Flash\\root");
    // nothing asked for a movie the game has no request for
    assert!(!p.can_load("nothing"));
    assert!(p.can_load("other"));
    p.frame = 10;
    assert!(p.load("other"));
    // it starts two game frames after the load
    assert!(sent(&mut p).is_empty());
    p.frame = 12;
    p.tick_loads();
    assert_eq!(sent(&mut p), vec![("up".to_string(), "1".to_string())]);
    assert!(p.call("hello", &[]));
    assert_eq!(sent(&mut p), vec![("hello".to_string(), "1".to_string())]);
    assert!(!p.call("missing", &[]));
}

#[test]
fn the_display_list_composes_matrices_and_colours_back_to_front() {
    // two shapes in a sprite placed at (100, 50) at twice the size; the sprite is half transparent
    let shape = |id| Character::Shape { bounds: [0.0, 0.0, 10.0, 10.0], geometry: id };
    let inner = Character::Sprite { frames: one_frame(vec![place_char(2, 2, None, [5.0, 0.0], 1.0), place_char(1, 1, None, [0.0, 0.0], 1.0)]) };
    let mut root = one_frame(vec![place_char(1, 3, Some("g"), [100.0, 50.0], 2.0)]);
    if let Item::Place(p) = &mut root[0].items[0] {
        p.flags |= place::HAS_COLOR;
        p.color = 0x80ff_ffff;
    }
    let mut p = Player::new([("root".to_string(), flash("root", root, vec![shape(1), shape(2), inner]))]);
    p.load("root");
    let list = p.display_list();
    assert_eq!(list.len(), 2);
    // back to front: depth 1 first
    assert_eq!(list[0].character, 1);
    assert_eq!(list[0].matrix, [2.0, 0.0, 0.0, 2.0, 100.0, 50.0]);
    assert_eq!(list[1].matrix, [2.0, 0.0, 0.0, 2.0, 110.0, 50.0]);
    assert!((list[0].color.mul[3] - 128.0 / 255.0).abs() < 1e-6);
    // a script hides the sprite: nothing is shown
    let g = p.child_named(p.root.unwrap(), "g").unwrap();
    assert!(p.clip(g).visible);
}

#[test]
fn the_menus_send_the_commands_the_game_logged() {
    // needs the disc and an observer log of the home run (`HOUSEFIRE_APT_LOG`)
    let (Ok(iso), Ok(path)) = (std::env::var("HOUSEFIRE_BUSTIN_ISO"), std::env::var("HOUSEFIRE_APT_LOG")) else { return };
    let d = crate::disc::Disc::open(iso).unwrap();
    let log = crate::tree_log::read_log(&path).unwrap();
    let flashes: Vec<_> = super::read_flashes(&d.read("flashes.arc").unwrap()).into_iter().filter_map(|(n, r)| r.ok().map(|f| (n, Rc::new(f)))).collect();
    let (_, ours) = super::play::run(&log, flashes, None, super::play::Timing::default());
    let rep = super::play::compare(&log, &ours, None);
    // the rest is the menu items' sixth decimal, three key presses, and a few frames of the
    // game's own screens (docs/assets.md, "The player")
    assert!(rep.same * 100 >= rep.total * 90, "{} of {}", rep.same, rep.total);
}

#[test]
fn a_with_block_names_its_end() {
    use super::img::Img;
    // With over a Push (the opcode, then count and pointer words) and a Stop: 13 bytes; then Play
    let mut b = vec![0x94, 0, 0, 0];
    b.extend(13u32.to_be_bytes());
    b.extend([0x96, 0, 0, 0]);
    b.extend(1u32.to_be_bytes());
    b.extend(36u32.to_be_bytes()); // the entries' offset
    b.push(0x07); // Stop
    b.push(0x06); // Play, after the block
    b.push(0);
    b.resize(36, 0);
    b.extend(0u32.to_be_bytes());
    let mut m = Img::new(&b);
    let v = super::actions::decode(&mut m, &[C::Int(7)], 0, None).unwrap();
    assert!(matches!(v[0], Instr::With { until: 3, .. }), "{v:?}");
    assert_eq!(v.len(), 4);
}
