//! Skeletons: `characte.arc`'s entries (`docs/assets.md`, "Skeletons"). An entry is its name,
//! a bone count, then the bones (the first named by the entry's name, the others each by its
//! own): the parent (-1 for the root), the children (a count, then their indices), a translation
//! (three floats), a rotation (a quaternion, four), a flag byte and two 4x4 matrices; then a
//! plane's name and five floats.

fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

fn f32s<const N: usize>(b: &[u8], p: usize) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (k, v) in out.iter_mut().enumerate() {
        *v = f32::from_bits(be32(b, p + 4 * k)?);
    }
    Some(out)
}

fn cstr(b: &[u8], p: usize) -> Option<(String, usize)> {
    let len = b.get(p..)?.iter().position(|&c| c == 0)?;
    Some((b[p..p + len].iter().map(|&c| c as char).collect(), p + len + 1))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub children: Vec<u32>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub flag: u8,
    pub matrices: [[f32; 16]; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub name: String,
    pub bones: Vec<Bone>,
    pub plane: (String, [f32; 5]),
}

/// A skeleton, read to its end; `None` unless every byte is accounted for.
pub fn read(b: &[u8]) -> Option<Skeleton> {
    let (name, mut p) = cstr(b, 0)?;
    let count = be32(b, p)? as usize;
    p += 4;
    if count > b.len() / 150 + 1 {
        return None;
    }
    let mut bones = Vec::with_capacity(count);
    for k in 0..count {
        let bone_name = if k == 0 {
            name.clone()
        } else {
            let (n, q) = cstr(b, p)?;
            p = q;
            n
        };
        let parent = be32(b, p)? as i32;
        let n = be32(b, p + 4)? as usize;
        p += 8;
        if n > count {
            return None;
        }
        let children = (0..n).map(|j| be32(b, p + 4 * j)).collect::<Option<Vec<u32>>>()?;
        p += 4 * n;
        let translation = f32s::<3>(b, p)?;
        let rotation = f32s::<4>(b, p + 12)?;
        let flag = *b.get(p + 28)?;
        p += 29;
        let matrices = [f32s::<16>(b, p)?, f32s::<16>(b, p + 64)?];
        p += 128;
        bones.push(Bone { name: bone_name, parent, children, translation, rotation, flag, matrices });
    }
    let (plane_name, q) = cstr(b, p)?;
    let plane = (plane_name, f32s::<5>(b, q)?);
    (q + 20 == b.len()).then_some(Skeleton { name, bones, plane })
}
