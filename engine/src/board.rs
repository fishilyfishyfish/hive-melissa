//! Board storage: a fixed 64x64 toroidal grid of stacks plus a per-piece
//! position table.
//!
//! The base game has at most 22 pieces, so every coordinate the game can touch
//! lies within a span far smaller than 64 in each axis; indexing by
//! `(q & 63, r & 63)` therefore never aliases two live cells while letting the
//! hive drift freely (ants walking around can translate it indefinitely).

use crate::hex::Hex;
use crate::piece::{Color, Piece, NUM_PIECES};

const GRID_BITS: u32 = 6;
const GRID: usize = 1 << GRID_BITS;
const MASK: i32 = GRID as i32 - 1;
/// Base game: one ground piece plus up to four beetles.
pub const MAX_HEIGHT: usize = 7;

#[inline]
fn idx(h: Hex) -> usize {
    (((h.q & MASK) as usize) << GRID_BITS) | (h.r & MASK) as usize
}

#[derive(Clone)]
pub struct Board {
    stacks: Vec<[Piece; MAX_HEIGHT]>,
    heights: Vec<u8>,
    /// Position of each piece, `None` while still in hand.
    pos: [Option<Hex>; NUM_PIECES],
    /// Number of pieces on the board.
    count: usize,
}

impl PartialEq for Board {
    fn eq(&self, other: &Self) -> bool {
        if self.pos != other.pos {
            return false;
        }
        self.occupied_cells()
            .iter()
            .all(|&c| self.stack(c) == other.stack(c))
    }
}

impl std::fmt::Debug for Board {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut cells = self.occupied_cells();
        cells.sort();
        let mut m = f.debug_map();
        for c in cells {
            let names: Vec<String> = self.stack(c).iter().map(|p| p.name()).collect();
            m.entry(&(c.q, c.r), &names);
        }
        m.finish()
    }
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

impl Board {
    pub fn new() -> Board {
        Board {
            stacks: vec![[Piece(0); MAX_HEIGHT]; GRID * GRID],
            heights: vec![0; GRID * GRID],
            pos: [None; NUM_PIECES],
            count: 0,
        }
    }

    #[inline]
    pub fn height(&self, h: Hex) -> usize {
        self.heights[idx(h)] as usize
    }

    #[inline]
    pub fn is_occupied(&self, h: Hex) -> bool {
        self.heights[idx(h)] != 0
    }

    #[inline]
    pub fn top(&self, h: Hex) -> Option<Piece> {
        let i = idx(h);
        let ht = self.heights[i] as usize;
        if ht == 0 {
            None
        } else {
            Some(self.stacks[i][ht - 1])
        }
    }

    /// Color controlling a cell (color of the top piece).
    #[inline]
    pub fn top_color(&self, h: Hex) -> Option<Color> {
        self.top(h).map(|p| p.color())
    }

    /// Stack at `h`, bottom first.
    pub fn stack(&self, h: Hex) -> &[Piece] {
        let i = idx(h);
        &self.stacks[i][..self.heights[i] as usize]
    }

    #[inline]
    pub fn position(&self, p: Piece) -> Option<Hex> {
        self.pos[p.0 as usize]
    }

    pub fn piece_count(&self) -> usize {
        self.count
    }

    pub fn push(&mut self, p: Piece, h: Hex) {
        let i = idx(h);
        let ht = self.heights[i] as usize;
        assert!(ht < MAX_HEIGHT, "stack overflow at {:?}", h);
        assert!(
            self.pos[p.0 as usize].is_none(),
            "{} already on board",
            p.name()
        );
        self.stacks[i][ht] = p;
        self.heights[i] += 1;
        self.pos[p.0 as usize] = Some(h);
        self.count += 1;
    }

    pub fn pop(&mut self, h: Hex) -> Piece {
        let i = idx(h);
        let ht = self.heights[i] as usize;
        assert!(ht > 0, "pop from empty cell {:?}", h);
        let p = self.stacks[i][ht - 1];
        self.heights[i] -= 1;
        self.pos[p.0 as usize] = None;
        self.count -= 1;
        p
    }

    /// Distinct occupied cells, in piece-id order of first appearance.
    pub fn occupied_cells(&self) -> Vec<Hex> {
        let mut out: Vec<Hex> = Vec::with_capacity(self.count);
        for p in self.pos.iter().flatten() {
            if !out.contains(p) {
                out.push(*p);
            }
        }
        out
    }

    /// Whether `p` is the top piece of its stack.
    pub fn is_on_top(&self, p: Piece) -> bool {
        match self.position(p) {
            Some(h) => self.top(h) == Some(p),
            None => false,
        }
    }

    /// Whether all six neighbors of `h` are occupied.
    pub fn is_surrounded(&self, h: Hex) -> bool {
        h.neighbors().iter().all(|&n| self.is_occupied(n))
    }
}
