//! Engine-vs-engine matches for strength measurement.
//!
//! `cargo run --release --example arena -- <A> <B> [games] [random_opening_plies]`
//!
//! Player specs: `random`, `d<N>` (fixed depth N), `t<MS>` (time per move).
//! Colors alternate; games longer than 200 plies are scored as draws.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use hive_engine::ai::{SearchLimits, Searcher};
use hive_engine::{Color, Game, GameState, Move};

const MAX_PLIES: u32 = 200;

#[derive(Clone, Debug)]
enum Kind {
    Random,
    Depth(u32),
    Time(f64),
}

/// `kind[:flag,flag...]`, flags: `nolmr`, `norep`, `nott`, `fullmob`, `ring<pct>`, `pd<n>`,
/// `qr<pct>` (scale queen ring), `at<n>`, `bq<n>`, `qf<n>`, `nohand`, `reach<n>`.
#[derive(Clone, Debug)]
struct Spec {
    kind: Kind,
    flags: Vec<String>,
}

fn parse(s: &str) -> Spec {
    let mut parts = s.splitn(2, ':');
    let kind = parse_kind(parts.next().unwrap());
    let flags = parts
        .next()
        .map(|f| f.split(',').map(String::from).collect())
        .unwrap_or_default();
    Spec { kind, flags }
}

fn searcher(spec: &Spec) -> Searcher {
    let mut s = Searcher::new(18);
    for f in &spec.flags {
        match f.as_str() {
            "nolmr" => s.lmr = false,
            "norep" => s.repetition = false,
            "nott" => s.use_tt = false,
            "fullmob" => s.params.full_mobility = true,
            f if f.starts_with("ring") => s.params.own_free_ring_discount = f[4..].parse().unwrap(),
            f if f.starts_with("qr") => {
                let pct: i32 = f[2..].parse().unwrap();
                for v in s.params.queen_ring.iter_mut() {
                    *v = *v * pct / 100;
                }
            }
            f if f.starts_with("at") => s.params.attacker = f[2..].parse().unwrap(),
            f if f.starts_with("bq") => s.params.beetle_on_queen = f[2..].parse().unwrap(),
            f if f.starts_with("qf") => s.params.queen_free = f[2..].parse().unwrap(),
            f if f.starts_with("nohand") => s.params.in_hand = [0; 5],
            f if f.starts_with("reach") => s.params.reach = f[5..].parse().unwrap(),
            f if f.starts_with("pd") => s.params.per_dest = f[2..].parse().unwrap(),
            other => panic!("unknown flag {other}"),
        }
    }
    s
}

fn parse_kind(s: &str) -> Kind {
    if s == "random" {
        Kind::Random
    } else if let Some(d) = s.strip_prefix('d') {
        Kind::Depth(d.parse().expect("depth"))
    } else if let Some(t) = s.strip_prefix('t') {
        Kind::Time(t.parse().expect("ms"))
    } else {
        panic!("bad player spec {s}")
    }
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[derive(Default)]
struct Stats {
    nodes: u64,
    ms: f64,
    depth_sum: u64,
    searches: u64,
}

fn choose(spec: &Spec, s: &mut Searcher, g: &Game, rng: &mut Rng, st: &mut Stats) -> Move {
    let limits = match &spec.kind {
        Kind::Random => {
            let m = g.valid_moves();
            return m[rng.below(m.len())];
        }
        Kind::Depth(d) => SearchLimits {
            max_depth: *d,
            time_ms: 1e9,
            ..Default::default()
        },
        Kind::Time(t) => SearchLimits {
            time_ms: *t,
            ..Default::default()
        },
    };
    let r = s.search(g, &limits);
    st.nodes += r.nodes;
    st.ms += r.elapsed_ms;
    st.depth_sum += r.depth as u64;
    st.searches += 1;
    r.best
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = parse(&args[1]);
    let b = parse(&args[2]);
    let games: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    let opening: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(2);

    let next = Arc::new(AtomicU64::new(0));
    // (a wins, b wins, draws, plies)
    let tally = Arc::new(Mutex::new((
        0u32,
        0u32,
        0u32,
        0u64,
        Stats::default(),
        Stats::default(),
    )));
    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(games as usize);
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (a, b, next, tally) = (a.clone(), b.clone(), next.clone(), tally.clone());
            thread::spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= games {
                    break;
                }
                let mut rng =
                    Rng(0x9E37_79B9_7F4A_7C15 ^ (i / 2 + 1).wrapping_mul(0x2545_F491_4F6C_DD1D));
                let a_color = if i % 2 == 0 {
                    Color::White
                } else {
                    Color::Black
                };
                let mut sa = searcher(&a);
                let mut sb = searcher(&b);
                let (mut st_a, mut st_b) = (Stats::default(), Stats::default());
                let mut g = Game::new(true);
                while !g.state().is_over() && g.ply() < MAX_PLIES {
                    let m = if g.ply() < opening {
                        let ms = g.valid_moves();
                        ms[rng.below(ms.len())]
                    } else if g.to_move() == a_color {
                        choose(&a, &mut sa, &g, &mut rng, &mut st_a)
                    } else {
                        choose(&b, &mut sb, &g, &mut rng, &mut st_b)
                    };
                    g.apply_fast(m);
                }
                let winner = match g.state() {
                    GameState::WhiteWins => Some(Color::White),
                    GameState::BlackWins => Some(Color::Black),
                    _ => None,
                };
                let mut guard = tally.lock().unwrap();
                let t = &mut *guard;
                match winner {
                    Some(c) if c == a_color => t.0 += 1,
                    Some(_) => t.1 += 1,
                    None => t.2 += 1,
                }
                t.3 += g.ply() as u64;
                for (dst, src) in [(&mut t.4, st_a), (&mut t.5, st_b)] {
                    dst.nodes += src.nodes;
                    dst.ms += src.ms;
                    dst.depth_sum += src.depth_sum;
                    dst.searches += src.searches;
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let t = tally.lock().unwrap();
    let score = (t.0 as f64 + 0.5 * t.2 as f64) / games as f64;
    println!(
        "{} vs {}: {games} games  A wins {}  B wins {}  draws {}  A score {:.1}%  avg plies {:.0}",
        args[1],
        args[2],
        t.0,
        t.1,
        t.2,
        score * 100.0,
        t.3 as f64 / games as f64
    );
    for (name, st) in [(&args[1], &t.4), (&args[2], &t.5)] {
        if st.searches > 0 {
            println!(
                "  {name}: {:.0} nodes/s, avg depth {:.2}, avg {:.0} ms/move",
                st.nodes as f64 / (st.ms / 1000.0).max(1e-9),
                st.depth_sum as f64 / st.searches as f64,
                st.ms / st.searches as f64
            );
        }
    }
}
