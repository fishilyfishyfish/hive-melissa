#![allow(dead_code)]

pub mod reference;

use hive_engine::{Game, Move};
use reference::RefMove;

/// xorshift64* — tiny deterministic RNG so tests need no dependencies.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

pub fn to_ref(m: Move) -> RefMove {
    match m {
        Move::Pass => (None, None, None),
        Move::Place { piece, to } => (Some(piece.name()), None, Some((to.q, to.r))),
        Move::Move { piece, from, to } => (
            Some(piece.name()),
            Some((from.q, from.r)),
            Some((to.q, to.r)),
        ),
    }
}

pub fn ref_cells(g: &Game) -> std::collections::HashMap<(i32, i32), Vec<String>> {
    let b = g.board();
    b.occupied_cells()
        .into_iter()
        .map(|c| ((c.q, c.r), b.stack(c).iter().map(|p| p.name()).collect()))
        .collect()
}
