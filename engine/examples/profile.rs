//! Fixed workload for profiling: searches a few midgame positions.
use hive_engine::ai::{SearchLimits, Searcher};
use hive_engine::Game;

fn main() {
    let mut x: u64 = 88172645463325252;
    let mut total = 0;
    for _ in 0..4 {
        let mut g = Game::new(true);
        for _ in 0..20 {
            let m = g.valid_moves();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            g.apply_fast(m[(x % m.len() as u64) as usize]);
        }
        if g.state().is_over() {
            continue;
        }
        let mut s = Searcher::new(18);
        let r = s.search(
            &g,
            &SearchLimits {
                max_depth: 4,
                time_ms: 1e9,
                ..Default::default()
            },
        );
        total += r.nodes;
        println!(
            "depth {} nodes {} {:.0} ms {:.0} nps",
            r.depth,
            r.nodes,
            r.elapsed_ms,
            r.nodes as f64 / r.elapsed_ms * 1000.0
        );
    }
    println!("total {total}");
}
