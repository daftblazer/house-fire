//! The values the movies' scripts work with: numbers (single precision, as the game's
//! player has them), strings, booleans, objects, arrays, functions and movie clips.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::actions::{Instr, Value as Const};

/// A movie clip of the player's tree (an index into it).
pub type ClipId = usize;

/// A script object, shared by reference.
pub type ObjRef = Rc<RefCell<Object>>;

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Num(f32),
    Str(Rc<str>),
    Obj(ObjRef),
    Clip(ClipId),
}

/// A script function: its parameters, its body, the constant pool of the stream that defined
/// it, and the clip whose variables its code sees (functions here do not close over the locals
/// of the function that defined them).
#[derive(Debug)]
pub struct Func {
    pub name: Option<String>,
    pub params: Vec<Rc<str>>,
    pub body: Rc<Vec<Instr>>,
    pub pool: Rc<Vec<Const>>,
    pub scope: ClipId,
}

/// The objects the player itself provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Native {
    /// The game's side of the movie: writing a member sends the game a command.
    Extern,
    Math,
    Key,
    /// The class objects (`Array`, `Object`, `String`, `Number`).
    Class(&'static str),
}

#[derive(Debug)]
pub enum Kind {
    Plain,
    Array(Vec<Value>),
    Func(Rc<Func>),
    Native(Native),
}

#[derive(Debug)]
pub struct Object {
    pub props: Props,
    pub kind: Kind,
}

impl Object {
    pub fn new(kind: Kind) -> ObjRef {
        Rc::new(RefCell::new(Object { props: Props::default(), kind }))
    }
}

/// Named values in the order they were first set (an enumeration walks them in that order).
#[derive(Clone, Debug, Default)]
pub struct Props {
    list: Vec<(Rc<str>, Value)>,
    index: HashMap<Rc<str>, usize>,
}

impl Props {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.index.get(k).map(|&i| &self.list[i].1)
    }

    pub fn set(&mut self, k: &str, v: Value) {
        if let Some(&i) = self.index.get(k) {
            self.list[i].1 = v;
        } else {
            let key: Rc<str> = k.into();
            self.index.insert(key.clone(), self.list.len());
            self.list.push((key, v));
        }
    }

    pub fn remove(&mut self, k: &str) -> bool {
        let Some(&i) = self.index.get(k) else { return false };
        self.list.remove(i);
        self.index.clear();
        for (j, (key, _)) in self.list.iter().enumerate() {
            self.index.insert(key.clone(), j);
        }
        true
    }

    pub fn contains(&self, k: &str) -> bool {
        self.index.contains_key(k)
    }

    pub fn keys(&self) -> impl Iterator<Item = &Rc<str>> {
        self.list.iter().map(|(k, _)| k)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// The truth of a condition: numbers other than 0, non-empty strings (a string is turned
    /// into a number first, as the older players do), objects and clips.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => parse_num(s).is_some_and(|n| n != 0.0 && !n.is_nan()),
            Value::Obj(_) | Value::Clip(_) => true,
        }
    }

    pub fn to_num(&self) -> f32 {
        match self {
            // the movies are of the SWF 5 generation: undefined is 0 as a number, empty as text
            Value::Undefined => 0.0,
            Value::Null => 0.0,
            Value::Bool(b) => f32::from(u8::from(*b)),
            Value::Num(n) => *n,
            Value::Str(s) => parse_num(s).unwrap_or(f32::NAN),
            Value::Obj(_) | Value::Clip(_) => f32::NAN,
        }
    }

    /// The text of a value: whole numbers without a fraction, others with six decimals (how the
    /// game's player prints them in the commands it sends).
    pub fn to_text(&self) -> Rc<str> {
        match self {
            Value::Undefined => "".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => if *b { "true" } else { "false" }.into(),
            Value::Num(n) => num_text(*n).into(),
            Value::Str(s) => s.clone(),
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Array(a) => a.iter().map(|v| v.to_text().to_string()).collect::<Vec<_>>().join(",").into(),
                Kind::Func(_) => "[type Function]".into(),
                _ => "[object Object]".into(),
            },
            Value::Clip(c) => format!("_level0.clip{c}").into(),
        }
    }
}

pub fn num_text(n: f32) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{:.6}", f64::from(n))
    }
}

/// A string as a number: blanks around it are skipped; an empty string is not a number.
pub fn parse_num(s: &str) -> Option<f32> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).ok().map(|v| v as f32);
    }
    t.parse::<f32>().ok().filter(|_| t.bytes().all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c)))
}

/// Equality as `==` of the scripts: numbers and strings compare as numbers when either side is
/// a number, objects by identity, undefined with null.
pub fn loose_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Undefined | Null, Undefined | Null) => true,
        (Undefined | Null, _) | (_, Undefined | Null) => false,
        (Str(x), Str(y)) => x == y,
        (Obj(x), Obj(y)) => Rc::ptr_eq(x, y),
        (Clip(x), Clip(y)) => x == y,
        (Obj(_) | Clip(_), _) | (_, Obj(_) | Clip(_)) => false,
        _ => a.to_num() == b.to_num(),
    }
}

/// Equality of `===`: the same kind and the same value.
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Undefined, Undefined) | (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Num(x), Num(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Obj(x), Obj(y)) => Rc::ptr_eq(x, y),
        (Clip(x), Clip(y)) => x == y,
        _ => false,
    }
}
