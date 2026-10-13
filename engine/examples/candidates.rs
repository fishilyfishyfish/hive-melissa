//! Prints the study-board candidates of a position:
//! `cargo run --release --example candidates -- "<GameString>" [ms] [self-play plies]`
use hive_engine::ai::Searcher;
use hive_engine::analysis::candidates;
use hive_engine::Game;
fn main() {
    let gs = std::env::args().nth(1).unwrap_or("Base".into());
    let ms: f64 = std::env::args()
        .nth(2)
        .map(|s| s.parse().unwrap())
        .unwrap_or(1000.0);
    let mut g = Game::from_game_string(&gs, true).unwrap();
    let mut s = Searcher::new(18);
    let plies: u32 = std::env::args()
        .nth(3)
        .map(|s| s.parse().unwrap())
        .unwrap_or(0);
    for _ in 0..plies {
        let r = s.search(
            &g,
            &hive_engine::ai::SearchLimits {
                time_ms: 100.0,
                ..Default::default()
            },
        );
        g.apply(r.best);
    }
    println!("{}", g.game_string());
    let t = std::time::Instant::now();
    let c = candidates(&g, &mut s, ms, 5);
    println!(
        "{:?} notes {:?} legal {} distinct {} score_white {} ({:?})",
        c.to_move,
        c.notes,
        c.legal,
        c.distinct,
        c.score_white,
        t.elapsed()
    );
    for (i, x) in c.list.iter().enumerate() {
        println!(
            "#{} {} [{}] score {} d{} gap={} same={:?}",
            i + 1,
            x.desc,
            x.uhp,
            x.score,
            x.depth,
            x.gap,
            x.same.len()
        );
        for r in &x.reasons {
            println!("   - {r}");
        }
        for r in &x.vs_best {
            println!("   vs: {r}");
        }
        if let Some(n) = &x.note {
            println!("   note: {n}");
        }
        if let Some((d, rs)) = &x.plan {
            println!("   plan: {d} :: {rs:?}");
        }
        println!("   pv: {}", x.pv.join(" / "));
    }
}
