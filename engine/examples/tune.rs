//! SPSA tuning of the evaluation weights by self-play.
//!
//! `cargo run --release --example tune -- [iterations] [pairs] [depth] [out-file] [start-file]`
//!
//! Each iteration perturbs every weight by +/- its step `c_i` at once
//! (random signs), plays `pairs` game pairs (same random opening, colors
//! swapped) between theta+ and theta-, and moves theta toward the winner:
//! `theta_i += lr_k * c_i * r * sign_i` with `r = (score+ - score-) / games`.
//! The current theta is written to `out-file` after every iteration, so a
//! run can be stopped at any time and resumed with it as `start-file`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use hive_engine::ai::{EvalParams, SearchLimits, Searcher};
use hive_engine::{Color, Game, GameState};

/// Perturbation step per weight (same order as `EvalParams::TUNABLE`).
const STEP: [f64; 19] = [
    3.0, 5.0, 10.0, 15.0, 25.0, // ring1..5
    3.0, 4.0, 3.0, 4.0, // free S B G A
    1.0, 5.0, 10.0, 3.0, // perDest queenFree beetleOnQueen attacker
    2.0, 2.0, 2.0, 2.0, // hand S B G A
    8.0, 4.0, // ringDiscount reach
];
const MAX_PLIES: u32 = 160;
const OPENING: u32 = 4;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Plays one game; returns the score (1, 0.5, 0) of `a`.
fn play(a: &EvalParams, b: &EvalParams, a_white: bool, depth: u32, seed: u64) -> f64 {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut sa = Searcher::new(16);
    let mut sb = Searcher::new(16);
    sa.params = a.clone();
    sb.params = b.clone();
    let a_color = if a_white { Color::White } else { Color::Black };
    let limits = SearchLimits {
        max_depth: depth,
        time_ms: f64::INFINITY,
        ..Default::default()
    };
    let mut g = Game::new(true);
    while !g.state().is_over() && g.ply() < MAX_PLIES {
        let m = if g.ply() < OPENING {
            let ms = g.valid_moves();
            ms[rng.below(ms.len())]
        } else if g.to_move() == a_color {
            sa.search(&g, &limits).best
        } else {
            sb.search(&g, &limits).best
        };
        g.apply_fast(m);
    }
    match (g.state(), a_white) {
        (GameState::WhiteWins, true) | (GameState::BlackWins, false) => 1.0,
        (GameState::WhiteWins, false) | (GameState::BlackWins, true) => 0.0,
        _ => 0.5,
    }
}

fn read_vec(path: &str) -> Vec<f64> {
    let text = std::fs::read_to_string(path).expect("read start file");
    let v: Vec<f64> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("number"))
        .collect();
    assert_eq!(
        v.len(),
        STEP.len(),
        "start file must have {} numbers",
        STEP.len()
    );
    v
}

fn params_of(theta: &[f64]) -> EvalParams {
    EvalParams::from_vec(&theta.iter().map(|x| x.round() as i32).collect::<Vec<_>>())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let iterations: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let pairs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let depth: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let out = args.get(4).cloned().unwrap_or_else(|| "tuned.txt".into());
    let mut theta: Vec<f64> = match args.get(5) {
        Some(p) => read_vec(p),
        None => EvalParams::default()
            .to_vec()
            .iter()
            .map(|&x| x as f64)
            .collect(),
    };
    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let start = theta.clone();
    let mut sum_r = 0.0;

    for k in 0..iterations {
        // Learning rate decays (SPSA's usual 0.602 exponent).
        let lr = 1.0 / ((k as f64 + 50.0) / 50.0).powf(0.602);
        let signs: Vec<f64> = (0..theta.len())
            .map(|_| if rng.next() & 1 == 0 { 1.0 } else { -1.0 })
            .collect();
        let plus: Vec<f64> = theta
            .iter()
            .zip(&STEP)
            .zip(&signs)
            .map(|((t, c), s)| t + c * s)
            .collect();
        let minus: Vec<f64> = theta
            .iter()
            .zip(&STEP)
            .zip(&signs)
            .map(|((t, c), s)| t - c * s)
            .collect();
        let (pp, pm) = (Arc::new(params_of(&plus)), Arc::new(params_of(&minus)));
        let base_seed = rng.next();

        let next = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(Mutex::new(0.0f64));
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (pp, pm, next, total) = (pp.clone(), pm.clone(), next.clone(), total.clone());
                thread::spawn(move || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= pairs * 2 {
                        break;
                    }
                    let s = play(&pp, &pm, i % 2 == 0, depth, base_seed ^ (i / 2) as u64);
                    *total.lock().unwrap() += s;
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let games = (pairs * 2) as f64;
        let score_plus = *total.lock().unwrap();
        // r in [-1, 1]: positive means theta+ was better.
        let r = (2.0 * score_plus - games) / games;
        sum_r += r.abs();
        for i in 0..theta.len() {
            theta[i] += lr * STEP[i] * r * signs[i];
        }
        // Keep weights in their valid ranges.
        for t in theta.iter_mut() {
            *t = t.max(0.0);
        }
        theta[17] = theta[17].min(100.0);

        let line: Vec<String> = theta.iter().map(|x| format!("{x:.2}")).collect();
        std::fs::write(&out, line.join(",") + "\n").expect("write out file");
        if (k + 1) % 20 == 0 || k + 1 == iterations {
            let rounded = params_of(&theta).to_vec();
            println!(
                "iter {:>5}  lr {lr:.3}  mean|r| {:.3}  theta {:?}",
                k + 1,
                sum_r / 20.0,
                rounded
            );
            sum_r = 0.0;
        }
    }
    println!("start {:?}", params_of(&start).to_vec());
    println!("final {:?}", params_of(&theta).to_vec());
    for (name, (a, b)) in EvalParams::TUNABLE.iter().zip(
        params_of(&start)
            .to_vec()
            .into_iter()
            .zip(params_of(&theta).to_vec()),
    ) {
        println!("  {name:<14} {a:>5} -> {b:>5}");
    }
}
