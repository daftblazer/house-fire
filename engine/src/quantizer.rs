//! The colour quantizer the original reduces a Sim's picture with (the object at 801B7464, used by
//! the composer at 8005D018): an octree over the colours in a YIQ-like space, grown from the
//! picture's colours (collisions of a small direct-mapped cache of colour counts feed it), pruned
//! by weight to the palette's size, and searched for each colour's nearest palette entry.
//!
//! Every float step is the original's, fused multiply-adds included (`f32::mul_add`): the palette is
//! the leaves' mean colours, truncated to bytes, so a differing last bit would change it.

/// The cache of colour counts: 511 buckets by the colour's remainder (the original's size).
const BUCKETS: usize = 511;

/// An octree node: a cell of the colour space and what fell in it.
#[derive(Clone, Default)]
struct Node {
    /// The cell's centre.
    centre: [f32; 3],
    /// The sum of the colours (weighted by their counts) that ended here.
    sum: [f32; 3],
    /// What the cell's pruning order goes by: the counts that passed through it, each shifted
    /// by the level (counts of the coarse levels weigh more).
    weight: u32,
    /// The colours' count that ended here (nonzero: the node is a palette entry once built).
    count: u32,
    parent: usize,
    /// The children by octant (bit 0 above the centre in the first coordinate, bit 1 the second,
    /// bit 2 the third); 0 for none (the arena's first node is never used).
    children: [usize; 8],
    mask: u8,
    depth: u8,
    slot: u8,
    /// The palette index, once built.
    palette: u8,
}

/// The weight shift of a level: counts of the first levels weigh up to 256 times.
fn level_shift(level: usize) -> u32 {
    8usize.saturating_sub(level) as u32
}

pub struct Quantizer {
    /// The colour transform (rows of four: the three coordinates' columns, then the offset) and
    /// its inverse.
    forward: [f32; 16],
    inverse: [f32; 16],
    nodes: Vec<Node>,
    free: Vec<usize>,
    /// The most nodes the octree keeps, and how many it has.
    capacity: usize,
    in_use: usize,
    max_depth: usize,
    /// Leaves made at the deepest level so far (not reduced by pruning); then the nodes counted
    /// while pruning, then the palette's size.
    leaves: usize,
    threshold: u32,
    next_threshold: u32,
    colours: usize,
    cache: Vec<(u32, u32)>,
    palette: Vec<[f32; 3]>,
}

/// The colour transform's coordinates: YIQ (the NTSC matrix), the second and third scaled down to
/// a third, the offsets keeping all in 0 to 255.
fn transform() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 0.299;
    m[4] = 0.587;
    m[8] = 0.114;
    m[1] = -0.1474;
    m[5] = -0.2895;
    m[9] = 0.4369;
    m[2] = 0.615;
    m[6] = -0.515;
    m[10] = -0.1;
    m[15] = 1.0;
    // the offsets (0, 127.5, 127.5), added to the matrix's translation of 0
    for (i, o) in [0.0f32, 127.5, 127.5].into_iter().enumerate() {
        m[12 + i] += o;
    }
    // the columns scaled by (1, 1/3, 1/3), the offsets with them
    let scale = [1.0f32, 0.333_333_34, 0.333_333_34];
    for row in 0..4 {
        for (c, s) in scale.iter().enumerate() {
            m[row * 4 + c] *= s;
        }
    }
    m
}

/// The 4 x 4 inverse the original computes (801B2D34), step by step.
fn invert(m: &[f32; 16]) -> Option<[f32; 16]> {
    let mut f = [0.0f32; 14];
    let mut o = [0.0f32; 16];
    f[6] = m[9];
    f[13] = m[0];
    f[4] = m[5];
    f[7] = m[6];
    f[11] = f[13] * f[6];
    f[10] = m[4];
    f[13] = f[13] * f[4];
    f[9] = m[1];
    f[11] = f[11] * f[7];
    f[5] = m[10];
    f[0] = f[10] * f[9];
    f[12] = m[8];
    f[8] = m[2];
    f[13] = f[13].mul_add(f[5], -f[11]);
    f[0] = f[0] * f[5];
    f[3] = 0.0;
    f[9] = f[12] * f[9];
    f[13] = f[13] - f[0];
    f[10] = f[10] * f[6];
    f[10] = f[10].mul_add(f[8], f[13]);
    f[12] = f[12] * f[4];
    f[9] = f[9].mul_add(f[7], f[10]);
    f[12] = f[12] * f[8];
    f[9] = f[9] - f[12];
    if f[9] == 0.0 {
        return None;
    }
    f[0] = f[6] * f[7];
    f[2] = 1.0;
    f[0] = f[4].mul_add(f[5], -f[0]);
    f[6] = f[2] / f[9];
    f[0] = f[0] * f[6];
    o[0] = f[0];
    f[11] = m[2];
    f[13] = m[9];
    f[12] = m[10];
    f[0] = m[1];
    f[13] = f[13] * f[11];
    f[0] = -f[0].mul_add(f[12], -f[13]);
    f[0] = f[0] * f[6];
    o[1] = f[0];
    f[0] = m[2];
    f[12] = m[5];
    f[11] = m[6];
    f[13] = m[1];
    f[12] = f[12] * f[0];
    o[3] = f[3];
    f[13] = f[13].mul_add(f[11], -f[12]);
    f[13] = f[13] * f[6];
    o[2] = f[13];
    f[11] = m[6];
    f[0] = m[4];
    f[13] = m[8];
    f[12] = m[10];
    f[0] = -f[0];
    f[13] = f[13] * f[11];
    f[0] = f[0].mul_add(f[12], f[13]);
    f[0] = f[0] * f[6];
    o[4] = f[0];
    f[11] = m[2];
    f[13] = m[8];
    f[12] = m[10];
    f[0] = m[0];
    f[13] = f[13] * f[11];
    f[0] = f[0].mul_add(f[12], -f[13]);
    f[0] = f[0] * f[6];
    o[5] = f[0];
    f[0] = m[2];
    f[12] = m[4];
    f[11] = m[6];
    f[13] = m[0];
    f[12] = f[12] * f[0];
    o[7] = f[3];
    f[13] = -f[13].mul_add(f[11], -f[12]);
    f[13] = f[13] * f[6];
    o[6] = f[13];
    f[11] = m[5];
    f[0] = m[4];
    f[13] = m[8];
    f[12] = m[9];
    f[0] = -f[0];
    f[13] = f[13] * f[11];
    f[0] = -f[0].mul_add(f[12], f[13]);
    f[0] = f[0] * f[6];
    o[8] = f[0];
    f[11] = m[1];
    f[13] = m[0];
    f[0] = m[8];
    f[12] = m[9];
    f[13] = -f[13];
    f[0] = f[0] * f[11];
    f[13] = f[13].mul_add(f[12], f[0]);
    f[13] = f[13] * f[6];
    o[9] = f[13];
    f[12] = m[1];
    f[0] = m[0];
    f[13] = m[4];
    f[11] = m[5];
    f[0] = -f[0];
    f[13] = f[13] * f[12];
    o[11] = f[3];
    f[0] = -f[0].mul_add(f[11], f[13]);
    f[0] = f[0] * f[6];
    o[10] = f[0];
    f[13] = m[4];
    f[5] = m[10];
    f[4] = m[9];
    f[10] = -f[13];
    f[7] = m[13];
    f[13] = f[13] * f[5];
    f[9] = m[14];
    f[10] = f[10] * f[4];
    f[12] = m[8];
    f[13] = f[13] * f[7];
    f[11] = m[5];
    f[10] = f[10].mul_add(f[9], f[13]);
    f[8] = m[6];
    f[13] = m[12];
    f[0] = f[12] * f[11];
    f[12] = f[12] * f[8];
    f[0] = f[0].mul_add(f[9], f[10]);
    f[12] = f[12] * f[7];
    f[11] = f[13] * f[11];
    f[11] = f[11] * f[5];
    f[0] = f[0] - f[12];
    f[0] = f[0] - f[11];
    f[13] = f[13] * f[4];
    f[13] = f[13].mul_add(f[8], f[0]);
    f[13] = f[13] * f[6];
    o[12] = f[13];
    f[3] = m[10];
    f[12] = m[0];
    f[4] = m[13];
    f[5] = m[1];
    f[11] = f[12] * f[3];
    f[9] = m[9];
    f[11] = f[11] * f[4];
    f[10] = m[8];
    f[8] = m[14];
    f[12] = f[12] * f[9];
    f[7] = m[2];
    f[13] = f[10] * f[5];
    f[0] = m[12];
    f[12] = f[12].mul_add(f[8], -f[11]);
    f[13] = f[13] * f[8];
    f[12] = f[12] - f[13];
    f[10] = f[10] * f[7];
    f[9] = f[0] * f[9];
    f[10] = f[10].mul_add(f[4], f[12]);
    f[0] = f[0] * f[5];
    f[0] = f[0].mul_add(f[3], f[10]);
    f[9] = f[9] * f[7];
    f[0] = f[0] - f[9];
    f[0] = f[0] * f[6];
    o[13] = f[0];
    f[12] = m[0];
    f[3] = m[6];
    f[9] = m[5];
    f[4] = m[13];
    f[11] = f[12] * f[3];
    f[10] = m[4];
    f[12] = f[12] * f[9];
    f[5] = m[1];
    f[11] = f[11] * f[4];
    f[8] = m[14];
    f[7] = m[2];
    f[13] = f[10] * f[5];
    f[0] = m[12];
    f[12] = f[12].mul_add(f[8], -f[11]);
    f[13] = f[13] * f[8];
    f[9] = f[0] * f[9];
    f[12] = f[12] - f[13];
    f[10] = f[10] * f[7];
    f[10] = f[10].mul_add(f[4], f[12]);
    f[0] = f[0] * f[5];
    f[0] = f[0].mul_add(f[3], f[10]);
    f[9] = f[9] * f[7];
    f[0] = f[0] - f[9];
    f[0] = -f[0];
    f[0] = f[0] * f[6];
    o[14] = f[0];
    o[15] = f[2];
    Some(o)
}

/// A colour (red, green, blue bytes) in the transform's space, clamped to 0 to 255.
fn to_space(m: &[f32; 16], rgb: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (f32::from(rgb[0]), f32::from(rgb[1]), f32::from(rgb[2]));
    std::array::from_fn(|c| {
        let v = b.mul_add(m[8 + c], r.mul_add(m[c], g * m[4 + c])) + m[12 + c];
        if v < 0.0 {
            0.0
        } else if v > 255.0 {
            255.0
        } else {
            v
        }
    })
}

/// A point of the transform's space back to a colour: clamped and truncated.
fn from_space(m: &[f32; 16], p: [f32; 3]) -> [u8; 3] {
    std::array::from_fn(|c| {
        let v = p[2].mul_add(m[8 + c], p[0].mul_add(m[c], p[1] * m[4 + c])) + m[12 + c];
        (if v < 0.0 {
            0.0
        } else if v > 255.0 {
            255.0
        } else {
            v
        }) as u8
    })
}

/// The squared distance (801B8664's: the squares of the first two fused with the third's).
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[2].mul_add(d[2], d[0] * d[0]) + d[1] * d[1]
}

impl Quantizer {
    /// A quantizer for up to `colours` colours (at most 256) with `memory` as the octree's size
    /// parameter (the original's 0x7F00 for a Sim's picture).
    pub fn new(colours: usize, memory: u32) -> Quantizer {
        let forward = transform();
        let capacity = ((u64::from(memory >> 3) * 0x2492_4925) >> 32) as usize - 1;
        let mut q = Quantizer {
            forward,
            inverse: invert(&forward).unwrap_or([0.0; 16]),
            nodes: vec![Node::default()],
            free: Vec::new(),
            capacity,
            in_use: 0,
            max_depth: 10,
            leaves: 0,
            threshold: 0,
            next_threshold: 0,
            colours: colours.min(256),
            cache: vec![(u32::MAX, 0); BUCKETS],
            palette: Vec::new(),
        };
        // the root: the cell of side 512 about (256, 256, 256), with every weight kept
        let root = q.alloc(0, 0, 0, [256.0; 3]);
        q.nodes[root].weight = u32::MAX;
        q.nodes[root].parent = root;
        q.leaves = 0;
        q
    }

    fn alloc(&mut self, slot: usize, depth: usize, parent: usize, centre: [f32; 3]) -> usize {
        let node = Node { centre, parent, depth: depth as u8, slot: slot as u8, ..Node::default() };
        self.in_use += 1;
        if let Some(i) = self.free.pop() {
            self.nodes[i] = node;
            i
        } else {
            self.nodes.push(node);
            self.nodes.len() - 1
        }
    }

    /// Adds a colour to the cache of counts; a different colour in its bucket is sent on to the
    /// octree with its count.
    pub fn add(&mut self, rgb: [u8; 3]) {
        let key = u32::from(rgb[0]) | u32::from(rgb[1]) << 8 | u32::from(rgb[2]) << 16;
        let bucket = (key % BUCKETS as u32) as usize;
        let (k, n) = self.cache[bucket];
        if k == key {
            self.cache[bucket].1 += 1;
        } else {
            if n != 0 {
                self.flush(k, n);
            }
            self.cache[bucket] = (key, 1);
        }
    }

    fn flush(&mut self, key: u32, count: u32) {
        let rgb = [key as u8, (key >> 8) as u8, (key >> 16) as u8];
        let p = to_space(&self.forward, rgb);
        self.insert(p, count);
    }

    /// A point with a count into the octree, pruning first when it is nearly full.
    fn insert(&mut self, p: [f32; 3], count: u32) {
        if self.in_use >= self.capacity - 10 {
            loop {
                self.reduce(1);
                self.max_depth -= 1;
                if self.in_use < self.capacity - 10 {
                    break;
                }
            }
        }
        let mut at = 1;
        for k in 1..=self.max_depth {
            let n = &self.nodes[at];
            let octant = usize::from(p[0] > n.centre[0]) | usize::from(p[1] > n.centre[1]) << 1 | usize::from(p[2] > n.centre[2]) << 2;
            let mut child = n.children[octant];
            if child == 0 {
                let d = 256.0f32 / (1u32 << k) as f32;
                let c = n.centre;
                let centre = std::array::from_fn(|i| if octant >> i & 1 != 0 { c[i] + d } else { c[i] - d });
                self.nodes[at].mask |= 1 << octant;
                child = self.alloc(octant, k, at, centre);
                self.nodes[at].children[octant] = child;
                if k == self.max_depth {
                    self.leaves += 1;
                }
            }
            at = child;
            self.nodes[at].weight = self.nodes[at].weight.wrapping_add(count << level_shift(k));
        }
        let leaf = &mut self.nodes[at];
        leaf.count += count;
        let n = count as i32 as f32;
        for (s, v) in leaf.sum.iter_mut().zip(p) {
            *s += v * n;
        }
    }

    /// Merges a node into its parent (801B8220).
    fn merge(&mut self, at: usize) {
        let n = self.nodes[at].clone();
        let p = &mut self.nodes[n.parent];
        p.mask &= !(1 << n.slot);
        p.children[usize::from(n.slot)] = 0;
        p.count += n.count;
        for (s, v) in p.sum.iter_mut().zip(n.sum) {
            *s += v;
        }
        self.in_use -= 1;
        self.free.push(at);
    }

    /// Merges the nodes of the deepest level into their parents, children first (801B817C).
    fn reduce(&mut self, at: usize) {
        for c in 0..8 {
            if self.nodes[at].mask >> c & 1 != 0 {
                self.reduce(self.nodes[at].children[c]);
            }
        }
        if usize::from(self.nodes[at].depth) == self.max_depth {
            self.merge(at);
        }
    }

    /// One pruning pass (801B8430): merges the nodes no heavier than the threshold, noting the
    /// next lightest weight, and counts the nodes that hold colours.
    fn trim(&mut self, at: usize) {
        for c in 0..8 {
            if self.nodes[at].mask >> c & 1 != 0 {
                self.trim(self.nodes[at].children[c]);
            }
        }
        if self.nodes[at].count != 0 {
            self.leaves += 1;
        }
        let w = self.nodes[at].weight;
        if w < self.next_threshold && w > self.threshold {
            self.next_threshold = w;
        }
        if w <= self.threshold {
            self.merge(at);
        }
    }

    /// Gives each node that holds colours a palette index, children first (801B850C).
    fn assign(&mut self, at: usize) {
        for c in 0..8 {
            if self.nodes[at].mask >> c & 1 != 0 {
                self.assign(self.nodes[at].children[c]);
            }
        }
        let n = &mut self.nodes[at];
        if n.count != 0 {
            let inv = 1.0f32 / n.count as f32;
            self.palette.push(n.sum.map(|s| s * inv));
            n.palette = (self.palette.len() - 1) as u8;
        }
    }

    /// Ends the collecting: the cached colours go into the octree, which is pruned to the
    /// palette's size, and the palette's entries are made (801B8308).
    pub fn finish(&mut self) {
        for b in 0..BUCKETS {
            let (k, n) = self.cache[b];
            if n != 0 {
                self.flush(k, n);
            }
            self.cache[b].1 = u32::MAX;
        }
        if self.leaves > self.colours {
            self.next_threshold = 1;
            loop {
                self.threshold = self.next_threshold;
                self.next_threshold = u32::MAX;
                self.leaves = 0;
                self.trim(1);
                if self.leaves <= self.colours {
                    break;
                }
            }
        }
        self.leaves = 0;
        self.assign(1);
    }

    /// The palette's size.
    pub fn len(&self) -> usize {
        self.palette.len()
    }

    pub fn is_empty(&self) -> bool {
        self.palette.is_empty()
    }

    /// A palette entry's colour (red, green, blue bytes).
    pub fn colour(&self, i: usize) -> [u8; 3] {
        from_space(&self.inverse, self.palette[i])
    }

    /// The squared distance of a colour to palette entry `i`, in the transform's space.
    pub fn distance_to(&self, rgb: [u8; 3], i: usize) -> f32 {
        distance(self.palette[i], to_space(&self.forward, rgb))
    }

    /// The palette index nearest a colour (801B8664, without its cache of answers): by a linear
    /// search of a small palette, else by the octree's cell the colour falls in and the
    /// palette entries of that cell's parent.
    pub fn nearest(&self, rgb: [u8; 3]) -> u8 {
        let p = to_space(&self.forward, rgb);
        if self.palette.len() <= 16 {
            let (mut best, mut at) = (distance(self.palette[0], p), 0);
            for (i, c) in self.palette.iter().enumerate().skip(1) {
                let d = distance(*c, p);
                if d < best {
                    (best, at) = (d, i);
                }
            }
            return at as u8;
        }
        let mut node = 1;
        loop {
            let n = &self.nodes[node];
            let octant = usize::from(p[0] > n.centre[0]) | usize::from(p[1] > n.centre[1]) << 1 | usize::from(p[2] > n.centre[2]) << 2;
            if n.mask >> octant & 1 != 0 && n.children[octant] != 0 {
                node = n.children[octant];
            } else {
                break;
            }
        }
        let mut best = (f32::MAX, 0u8);
        self.search(self.nodes[node].parent, p, &mut best);
        best.1
    }

    /// The nearest palette entry among the nodes below `at` (801B88E0).
    fn search(&self, at: usize, p: [f32; 3], best: &mut (f32, u8)) {
        let n = &self.nodes[at];
        for c in 0..8 {
            if n.mask >> c & 1 != 0 {
                self.search(n.children[c], p, best);
            }
        }
        if n.count != 0 {
            let d = distance(self.palette[usize::from(n.palette)], p);
            if d < best.0 {
                *best = (d, n.palette);
            }
        }
    }
}
