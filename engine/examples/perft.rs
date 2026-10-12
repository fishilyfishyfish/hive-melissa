//! Move-generation benchmark: perft from the opening and random playouts.
//!
//! `cargo run --release --example perft -- [depth] [playouts]`

use std::time::Instant;

use hive_engine::Game;

fn perft(g: &mut Game, depth: u32) -> u64 {
    let moves = g.valid_moves();
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut n = 0;
    for m in moves {
        g.apply_fast(m);
        n += perft(g, depth - 1);
        g.undo();
    }
    n
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let depth: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let playouts: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);

    for d in 1..=depth {
        let mut g = Game::new(true);
        let t = Instant::now();
        let n = perft(&mut g, d);
        let s = t.elapsed().as_secs_f64();
        println!(
            "perft({d}) = {n:>12}  {:>8.3}s  {:>10.0} nodes/s",
            s,
            n as f64 / s
        );
    }

    // Random playouts (xorshift), up to 300 plies each.
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut gens = 0u64;
    let t = Instant::now();
    for _ in 0..playouts {
        let mut g = Game::new(true);
        while g.ply() < 300 {
            let moves = g.valid_moves();
            gens += 1;
            if moves.is_empty() {
                break;
            }
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let m = moves[(x.wrapping_mul(0x2545_F491_4F6C_DD1D) % moves.len() as u64) as usize];
            g.apply_fast(m);
        }
    }
    let s = t.elapsed().as_secs_f64();
    println!(
        "random playouts: {playouts} games, {gens} move generations, {:.1} us/generation",
        s * 1e6 / gens as f64
    );
}
