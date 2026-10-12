//! Search and evaluation tests.

mod common;

use common::Rng;
use hive_engine::ai::{SearchLimits, Searcher, WIN};
use hive_engine::{Color, Game, GameState};

fn game(layout: &[(&str, i32, i32)], ply: u32) -> Game {
    Game::from_layout(layout, ply, true).unwrap()
}

fn depth(d: u32) -> SearchLimits {
    SearchLimits {
        max_depth: d,
        time_ms: 60_000.0,
        ..Default::default()
    }
}

#[test]
fn finds_mate_in_one() {
    // Black queen has five neighbors; wA2 can slide into the sixth.
    let g = game(
        &[
            ("bQ", 0, 0),
            ("wQ", 1, 0),
            ("wA1", 1, -1),
            ("bA1", 0, -1),
            ("wG1", -1, 0),
            ("bG1", -1, 1),
            ("wA2", 1, 1),
        ],
        10,
    );
    let mut s = Searcher::new(16);
    let r = s.search(&g, &depth(3));
    let mut after = g.clone();
    after.apply(r.best);
    assert_eq!(after.state(), GameState::WhiteWins, "best {:?}", r.best);
    assert_eq!(r.score, WIN - 1);
}

#[test]
fn defends_against_mate_in_one() {
    // Mirror image: black threatens bA2 -> (0,1) surrounding wQ.
    let g = game(
        &[
            ("wQ", 0, 0),
            ("bQ", 1, 0),
            ("bA1", 1, -1),
            ("wA1", 0, -1),
            ("bG1", -1, 0),
            ("wG1", -1, 1),
            ("bA2", 1, 1),
        ],
        10,
    );
    assert_eq!(g.to_move(), Color::White);
    let mut s = Searcher::new(16);
    let r = s.search(&g, &depth(2));
    assert!(r.score > -WIN + 1000, "engine thinks it is lost: {r:?}");
    let mut after = g.clone();
    after.apply(r.best);
    assert_ne!(after.state(), GameState::BlackWins, "suicide {:?}", r.best);
    for reply in after.valid_moves() {
        let mut x = after.clone();
        x.apply(reply);
        assert_ne!(
            x.state(),
            GameState::BlackWins,
            "{:?} allows {:?}",
            r.best,
            reply
        );
    }
}

fn random_position(seed: u64, plies: u32) -> Option<Game> {
    let mut rng = Rng::new(seed);
    let mut g = Game::new(true);
    for _ in 0..plies {
        let m = g.valid_moves();
        if m.is_empty() {
            return None;
        }
        g.apply_fast(m[rng.below(m.len())]);
    }
    (!g.state().is_over()).then_some(g)
}

#[test]
fn alphabeta_matches_minimax() {
    let mut checked = 0;
    for seed in 1..=12u64 {
        let Some(g) = random_position(seed, 10 + (seed as u32 % 4) * 6) else {
            continue;
        };
        let d = if seed <= 2 { 3 } else { 2 };
        let mut s = Searcher::new(4);
        s.use_tt = false;
        let ab = s.alphabeta_score(&g, d);
        let mm = s.minimax(&mut g.clone(), d, 0);
        assert_eq!(ab, mm, "seed {seed} depth {d}");
        checked += 1;
    }
    assert!(checked >= 8);
}

#[test]
fn tt_search_agrees_with_plain_search_on_best_score_at_depth_one() {
    for seed in 1..=10u64 {
        let Some(g) = random_position(seed + 100, 20) else {
            continue;
        };
        let mut a = Searcher::new(16);
        let mut b = Searcher::new(4);
        b.use_tt = false;
        assert_eq!(a.search(&g, &depth(1)).score, b.search(&g, &depth(1)).score);
    }
}

#[test]
fn search_respects_time_limit_and_reports_pv() {
    let g = random_position(7, 24).unwrap();
    let mut s = Searcher::new(18);
    let r = s.search(
        &g,
        &SearchLimits {
            time_ms: 200.0,
            ..Default::default()
        },
    );
    assert!(r.elapsed_ms < 1000.0, "{r:?}");
    assert!(r.depth >= 1);
    assert!(g.valid_moves().contains(&r.best));
    assert_eq!(r.pv[0], r.best);
}

#[test]
fn shallow_search_beats_random_player() {
    let mut wins = 0;
    let games = 6;
    for seed in 0..games {
        let mut rng = Rng::new(seed + 1000);
        let mut g = Game::new(true);
        let engine = if seed % 2 == 0 {
            Color::White
        } else {
            Color::Black
        };
        let mut s = Searcher::new(16);
        while !g.state().is_over() && g.ply() < 200 {
            let m = if g.to_move() == engine {
                s.search(&g, &depth(3)).best
            } else {
                let ms = g.valid_moves();
                ms[rng.below(ms.len())]
            };
            g.apply_fast(m);
        }
        let won = matches!(
            (g.state(), engine),
            (GameState::WhiteWins, Color::White) | (GameState::BlackWins, Color::Black)
        );
        wins += won as u32;
    }
    assert!(wins >= 5, "only {wins}/{games} wins against random");
}

#[test]
fn eval_params_vector_round_trip() {
    use hive_engine::ai::EvalParams;
    let p = EvalParams::default();
    let v = p.to_vec();
    assert_eq!(v.len(), EvalParams::TUNABLE.len());
    assert_eq!(EvalParams::from_vec(&v).to_vec(), v);
}
