//! Axial hex coordinates (pointy-top layout, matching UHP orientation).
//!
//! `q` grows to the east, `r` grows to the south-east. The six directions are
//! stored as a ring (each entry is adjacent to the previous and next one), so
//! the two cells shared by `a` and `a + DIRS[i]` are `a + DIRS[i-1]` and
//! `a + DIRS[i+1]`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
}

pub const fn hex(q: i32, r: i32) -> Hex {
    Hex { q, r }
}

/// Direction indices, in ring order.
pub const E: usize = 0;
pub const NE: usize = 1;
pub const NW: usize = 2;
pub const W: usize = 3;
pub const SW: usize = 4;
pub const SE: usize = 5;

pub const DIRS: [Hex; 6] = [
    hex(1, 0),  // E
    hex(1, -1), // NE
    hex(0, -1), // NW
    hex(-1, 0), // W
    hex(-1, 1), // SW
    hex(0, 1),  // SE
];

impl Hex {
    pub const ORIGIN: Hex = hex(0, 0);

    #[inline]
    pub fn step(self, dir: usize) -> Hex {
        let d = DIRS[dir];
        hex(self.q + d.q, self.r + d.r)
    }

    #[inline]
    pub fn neighbors(self) -> [Hex; 6] {
        [
            self.step(0),
            self.step(1),
            self.step(2),
            self.step(3),
            self.step(4),
            self.step(5),
        ]
    }

    /// Direction index from `self` to an adjacent `other`, if adjacent.
    pub fn dir_to(self, other: Hex) -> Option<usize> {
        let d = hex(other.q - self.q, other.r - self.r);
        DIRS.iter().position(|&x| x == d)
    }

    /// The two cells adjacent to both `self` and `self.step(dir)`.
    #[inline]
    pub fn common_neighbors(self, dir: usize) -> (Hex, Hex) {
        (self.step((dir + 5) % 6), self.step((dir + 1) % 6))
    }
}
