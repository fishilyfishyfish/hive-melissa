//! Self-play diagnostics: plays t<ms> vs itself and summarizes how games that
//! hit the ply cap look (queen ring sizes, pinned/covered counts).
//! `cargo run --release --example draws -- [games] [ms]`

use hive_engine::ai::{SearchLimits, Searcher};
use hive_engine::game::articulation_points;
use hive_engine::{Bug, Color, Game, Piece};

fn ring(g: &Game, c: Color) -> usize {
    match g.board().position(Piece::new(c, Bug::Queen, 1)) {
        Some(q) => q
            .neighbors()
            .iter()
            .filter(|&&n| g.board().is_occupied(n))
            .count(),
        None => 0,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let ms: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50.0);
    let mut x: u64 = 0x1234_5678_9abc_def1;
    let mut hist = [[0u32; 7]; 7];
    let mut capped = 0;
    for gi in 0..games {
        let mut g = Game::new(true);
        let mut s = Searcher::new(18);
        let mut max_ring = [0usize; 2];
        while !g.state().is_over() && g.ply() < 200 {
            let m = if g.ply() < 4 {
                let ms = g.valid_moves();
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                ms[(x % ms.len() as u64) as usize]
            } else {
                s.search(
                    &g,
                    &SearchLimits {
                        time_ms: ms,
                        ..Default::default()
                    },
                )
                .best
            };
            g.apply(m);
            for c in [Color::White, Color::Black] {
                max_ring[c as usize] = max_ring[c as usize].max(ring(&g, c));
            }
        }
        if !g.state().is_over() {
            capped += 1;
            let (w, b) = (ring(&g, Color::White), ring(&g, Color::Black));
            hist[w.max(b)][w.min(b)] += 1;
            let pinned = articulation_points(g.board()).len();
            println!(
                "game {gi}: final rings W{w} B{b}, max rings W{} B{}, pinned cells {pinned}\n  {}",
                max_ring[0],
                max_ring[1],
                g.history_uhp()[g.history_uhp().len().saturating_sub(16)..].join(" | ")
            );
        }
    }
    println!("{capped}/{games} capped at 200 plies");
}
