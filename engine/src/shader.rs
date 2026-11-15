//! Shaders as Bustin' Out keeps them (`shaders.arc`'s entries, `docs/assets.md`, "Shaders"): a
//! material by name, its layers each naming its texture. A sub-model names its shader, and a
//! layer its texture, by the CRC-32 of the name in capitals (`name_hash`).

/// A layer as the shader keeps it: its texture (by name hash), two words, seven bytes (what the
/// device's SetMaterialStages reads at +0x10: mode, blend, blend-by, enabled, factor, unlit, uv
/// source) and a float.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub texture: u32,
    pub flags: u32,
    pub word: u32,
    pub bytes: [u8; 7],
    pub value: f32,
}

/// A shader: its name, the 78 bytes after it (its layer count, a byte, first; the material's
/// colours, `Shader::material`), its layers and the 28 bytes after them (not read yet).
#[derive(Clone, Debug, PartialEq)]
pub struct Shader {
    pub name: String,
    pub head: Vec<u8>,
    pub layers: Vec<Layer>,
    pub tail: Vec<u8>,
}

fn be32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

impl Shader {
    /// The material's lighting colours, from the head: (diffuse, ambient), its words 7-9 and
    /// 4-6 after the layer count's two bytes (what Light_GroupScale reads at the material's
    /// +0x44 and +0x54; the floors' and terrains' measured against the 'b' records).
    pub fn material(&self) -> Option<([f32; 3], [f32; 3])> {
        let w = |i: usize| Some(f32::from_bits(be32(&self.head, 2 + 4 * i)?));
        Some(([w(7)?, w(8)?, w(9)?], [w(4)?, w(5)?, w(6)?]))
    }
}

/// Reads an entry: `None` unless it is a shader (the word 13, 12 zero bytes, the name) whose
/// parts fill it exactly.
pub fn read(e: &[u8]) -> Option<Shader> {
    if be32(e, 0)? != 13 {
        return None;
    }
    let len = e.get(16..)?.iter().position(|&c| c == 0)?;
    let name: String = e[16..16 + len].iter().map(|&c| c as char).collect();
    let q = 16 + len + 1;
    let head = e.get(q..q + 78)?.to_vec();
    let n = head[0] as usize;
    let mut p = q + 78;
    let mut layers = Vec::with_capacity(n);
    for _ in 0..n {
        let l = e.get(p..p + 23)?;
        layers.push(Layer {
            texture: be32(l, 0)?,
            flags: be32(l, 4)?,
            word: be32(l, 8)?,
            bytes: l[12..19].try_into().ok()?,
            value: f32::from_bits(be32(l, 19)?),
        });
        p += 23;
    }
    (e.len() == p + 28).then(|| Shader { name, head, layers, tail: e[p..].to_vec() })
}

/// The CRC-32 (IEEE, reflected) of a name in capitals: how a sub-model names its shader and a
/// layer its texture.
pub fn name_hash(name: &str) -> u32 {
    let mut crc = !0u32;
    for c in name.bytes().map(|c| c.to_ascii_uppercase()) {
        crc ^= c as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_hash_as_the_disc_has_them() {
        assert_eq!(name_hash("cas_sky_blue"), 0x51a6_fa33);
        assert_eq!(name_hash("Rubber_Tree_Plant"), 0x4072_e590);
    }

    #[test]
    fn a_one_layer_shader_fills_its_entry() {
        let mut e = 13u32.to_be_bytes().to_vec();
        e.extend([0u8; 12]);
        e.extend(b"ab\0");
        let mut head = vec![0u8; 78];
        head[0] = 1;
        e.extend(head);
        e.extend(0x1234_5678u32.to_be_bytes());
        e.extend([0, 0, 0, 8, 0, 0, 0, 0x18, 0, 1, 0, 1, 0xff, 0, 0, 0x3f, 0, 0, 0]);
        e.extend([0u8; 28]);
        let s = read(&e).unwrap();
        assert_eq!((s.name.as_str(), s.layers[0].texture, s.layers[0].value), ("ab", 0x1234_5678, 0.5));
    }
}
