//! Bounded 2D CPU LBVH for immutable display-list bounds. Morton-ordered leaves,
//! highest-differing-bit splits, flat nodes. No external code/runtime dependency.
//! Algorithm background: T. Karras, Thinking Parallel Part III (NVIDIA, 2012).
const MAX_COMMANDS: usize = 16_384;
const NONE: u32 = u32::MAX;

struct Node {
    bounds: [u32; 4],
    left: u32,
    right: u32,
    command: u32,
}

pub(crate) struct Lbvh {
    nodes: Vec<Node>,
}

fn spread(mut x: u32) -> u32 {
    x &= 0xffff;
    x = (x | (x << 8)) & 0x00ff00ff;
    x = (x | (x << 4)) & 0x0f0f0f0f;
    x = (x | (x << 2)) & 0x33333333;
    (x | (x << 1)) & 0x55555555
}

fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

impl Lbvh {
    pub(crate) fn build(bounds: &[[u32; 4]], width: u32, height: u32) -> Result<Self, String> {
        if bounds.len() > MAX_COMMANDS || width == 0 || height == 0 || width > 4096 || height > 4096
        {
            return Err("LBVH bounds limit".into());
        }
        let mut keys = Vec::with_capacity(bounds.len());
        for (index, r) in bounds.iter().enumerate() {
            if r[0] >= r[2] || r[1] >= r[3] || r[2] > width || r[3] > height {
                return Err("invalid LBVH rectangle".into());
            }
            let x = ((u64::from(r[0]) + u64::from(r[2])) * 65535 / (2 * u64::from(width))) as u32;
            let y = ((u64::from(r[1]) + u64::from(r[3])) * 65535 / (2 * u64::from(height))) as u32;
            keys.push((spread(x) | (spread(y) << 1), index as u32));
        }
        // Index tie-break makes equal Morton codes deterministic.
        keys.sort_unstable();
        let mut tree = Self {
            nodes: Vec::with_capacity(bounds.len().saturating_mul(2).saturating_sub(1)),
        };
        if !keys.is_empty() {
            tree.append(&keys, bounds);
        }
        Ok(tree)
    }

    fn append(&mut self, keys: &[(u32, u32)], bounds: &[[u32; 4]]) -> u32 {
        let index = self.nodes.len() as u32;
        self.nodes.push(Node {
            bounds: [0; 4],
            left: NONE,
            right: NONE,
            command: NONE,
        });
        if keys.len() == 1 {
            self.nodes[index as usize].bounds = bounds[keys[0].1 as usize];
            self.nodes[index as usize].command = keys[0].1;
        } else {
            let different = keys[0].0 ^ keys[keys.len() - 1].0;
            let split = if different == 0 {
                keys.len() / 2
            } else {
                let bit = 1u32 << (31 - different.leading_zeros());
                keys.partition_point(|key| key.0 & bit == 0)
            };
            // Different prefixes reduce the remaining 32 bits; equal keys are
            // halved. Depth is bounded by 32 + ceil(log2(MAX_COMMANDS)).
            let left = self.append(&keys[..split], bounds);
            let right = self.append(&keys[split..], bounds);
            self.nodes[index as usize] = Node {
                bounds: union(
                    self.nodes[left as usize].bounds,
                    self.nodes[right as usize].bounds,
                ),
                left,
                right,
                command: NONE,
            };
        }
        index
    }

    /// Query in output pixels. Caller validates a finite positive scale and
    /// viewport. Sorting hits restores PDF painter order, never Morton order.
    pub(crate) fn query(&self, view: [u32; 4], scale: f32) -> Vec<usize> {
        let mut result = Vec::new();
        if self.nodes.is_empty()
            || !scale.is_finite()
            || scale <= 0.0
            || view[0] >= view[2]
            || view[1] >= view[3]
        {
            return result;
        }
        let root = self.nodes[0].bounds;
        let s = f64::from(scale);
        if f64::from(view[0]) <= f64::from(root[0]) * s
            && f64::from(view[1]) <= f64::from(root[1]) * s
            && f64::from(view[2]) >= f64::from(root[2]) * s
            && f64::from(view[3]) >= f64::from(root[3]) * s
        {
            // Whole-scene view: no traversal or Morton-to-painter sort needed.
            return (0..self.nodes.len().div_ceil(2)).collect();
        }
        let mut stack = [0u32; 64];
        let mut length = 1;
        while length > 0 {
            length -= 1;
            let node = &self.nodes[stack[length] as usize];
            let r = node.bounds;
            let s = f64::from(scale);
            if f64::from(r[2]) * s <= f64::from(view[0])
                || f64::from(r[3]) * s <= f64::from(view[1])
                || f64::from(r[0]) * s >= f64::from(view[2])
                || f64::from(r[1]) * s >= f64::from(view[3])
            {
                continue;
            }
            if node.command != NONE {
                result.push(node.command as usize);
            } else {
                stack[length] = node.right;
                stack[length + 1] = node.left;
                length += 2;
            }
        }
        result.sort_unstable();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_duplicates_order_and_limits() {
        let boxes = [[0, 0, 4, 4], [4, 0, 8, 4], [0, 0, 4, 4], [0, 4, 8, 8]];
        let tree = Lbvh::build(&boxes, 8, 8).unwrap();
        assert_eq!(tree.query([0, 0, 4, 4], 1.0), vec![0, 2]);
        assert_eq!(tree.query([8, 0, 16, 8], 2.0), vec![1]);
        assert_eq!(tree.query([0, 0, 8, 8], 1.0), vec![0, 1, 2, 3]);
        assert!(tree.query([0, 0, 8, 8], f32::NAN).is_empty());
        assert!(Lbvh::build(&[], 8, 8)
            .unwrap()
            .query([0, 0, 8, 8], 1.0)
            .is_empty());
        for r in [[0, 0, 0, 1], [2, 0, 1, 1], [0, 0, 9, 8]] {
            assert!(Lbvh::build(&[r], 8, 8).is_err());
        }
        assert!(Lbvh::build(&vec![[0, 0, 1, 1]; MAX_COMMANDS + 1], 8, 8).is_err());
        let same = Lbvh::build(&vec![[0, 0, 8, 8]; MAX_COMMANDS], 8, 8).unwrap();
        assert_eq!(same.nodes.len(), 2 * MAX_COMMANDS - 1);
        assert_eq!(
            same.query([1, 1, 2, 2], 1.0),
            (0..MAX_COMMANDS).collect::<Vec<_>>()
        );
    }
    #[test]
    fn deterministic_queries_equal_exhaustive_intersection() {
        let mut state = 9u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u32
        };
        let boxes: Vec<_> = (0..6000)
            .map(|_| {
                let x = next() % 1000;
                let y = next() % 1000;
                [
                    x,
                    y,
                    (x + 1 + next() % 50).min(1024),
                    (y + 1 + next() % 50).min(1024),
                ]
            })
            .collect();
        let tree = Lbvh::build(&boxes, 1024, 1024).unwrap();
        for _ in 0..2000 {
            let x = next() % 1024;
            let y = next() % 1024;
            let q = [
                x,
                y,
                (x + 1 + next() % 128).min(1024),
                (y + 1 + next() % 128).min(1024),
            ];
            for scale in [0.25, 1.0, 1.25, 2.0] {
                let s = f64::from(scale);
                let expected: Vec<_> = boxes
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        f64::from(r[2]) * s > q[0] as f64
                            && f64::from(r[3]) * s > q[1] as f64
                            && f64::from(r[0]) * s < (q[2] as f64)
                            && f64::from(r[1]) * s < (q[3] as f64)
                    })
                    .map(|(i, _)| i)
                    .collect();
                assert_eq!(tree.query(q, scale), expected);
            }
        }
    }
}
