//! Random-game cross-check of the engine against the naive reference rules.

mod common;

use std::collections::BTreeSet;

use common::reference::{RefGame, RefMove};
use common::{ref_cells, to_ref, Rng};
use hive_engine::{uhp, Game, GameState};

/// Override with `HIVE_CROSSCHECK_GAMES=<n>`.
const DEFAULT_GAMES: u64 = 3000;
const MAX_PLIES: u32 = 200;

#[test]
fn random_games_match_reference() {
    let games: u64 = std::env::var("HIVE_CROSSCHECK_GAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_GAMES);
    let mut finished = 0;
    let mut total_plies = 0u64;
    let mut total_moves = 0u64;
    let mut passes = 0;
    for seed in 0..games {
        let tournament = seed % 3 != 0;
        let mut rng = Rng::new(seed + 1);
        let mut g = Game::new(tournament);
        let mut r = RefGame::new(tournament);
        loop {
            let legal = g.valid_moves();
            let ours: BTreeSet<RefMove> = legal.iter().map(|&m| to_ref(m)).collect();
            assert_eq!(
                ours.len(),
                legal.len(),
                "duplicate moves (seed {seed}, ply {})",
                g.ply()
            );
            let theirs = r.moves();
            if ours != theirs {
                let extra: Vec<_> = ours.difference(&theirs).collect();
                let missing: Vec<_> = theirs.difference(&ours).collect();
                panic!(
                    "seed {seed} ply {}: engine-only {extra:?}\nreference-only {missing:?}\n{}",
                    g.ply(),
                    g.game_string()
                );
            }
            match g.state() {
                GameState::Draw => assert_eq!(r.result(), "draw"),
                GameState::WhiteWins => assert_eq!(r.result(), "w"),
                GameState::BlackWins => assert_eq!(r.result(), "b"),
                _ => assert_eq!(r.result(), ""),
            }
            if legal.is_empty() {
                finished += 1;
                break;
            }
            if g.ply() >= MAX_PLIES {
                break;
            }
            total_moves += legal.len() as u64;

            // UHP round trip for every legal move, on a sample of positions.
            if g.ply() % 11 == 3 {
                for &m in &legal {
                    let s = uhp::move_to_string(&g, m);
                    assert_eq!(uhp::parse_move(&g, &s), Ok(m), "round trip of '{s}'");
                }
            }

            let m = legal[rng.below(legal.len())];
            if m == hive_engine::Move::Pass {
                passes += 1;
            }
            // apply/undo must restore the position exactly.
            let before = g.clone();
            g.apply(m);
            let after = g.clone();
            g.undo();
            assert!(g == before, "undo mismatch at seed {seed} ply {}", g.ply());
            g.apply(m);
            assert!(g == after);

            r.apply(&to_ref(m));
            assert_eq!(ref_cells(&g), r.cells, "board mismatch seed {seed}");
            total_plies += 1;
        }
        // The whole game must be reloadable from its UHP game string.
        let reloaded = Game::from_game_string(&g.game_string(), tournament).expect("reload");
        assert!(reloaded == g, "game string reload mismatch seed {seed}");
    }
    eprintln!(
        "{games} games, {finished} finished, {total_plies} plies, {passes} passes, avg branching {:.1}",
        total_moves as f64 / total_plies.max(1) as f64
    );
    assert!(finished > 0);
}
