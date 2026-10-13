//! Plays matches between two UHP engines (any UHP engine: this one, an older
//! build of it, Mzinga, ...).
//!
//! ```text
//! uhp-match --a "<cmd>" --b "<cmd>" [--games 100] [--time-ms 100]
//!           [--time-ms-a MS] [--time-ms-b MS]
//!           [--opening 4] [--max-plies 200] [--threads N] [--seed 1]
//!           [--init-a "cmd;cmd"] [--init-b "cmd;cmd"]
//! ```
//!
//! Colors alternate; each pair of games shares the same random opening
//! (`--opening` plies chosen uniformly from the legal moves), so both engines
//! play both sides of it. Moves are checked against this crate's rules; an
//! illegal move, an engine error or a crash loses the game. Games reaching
//! `--max-plies` are draws.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use hive_engine::uhp::{move_to_string, parse_move};
use hive_engine::{Color, Game, GameState};

struct Engine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    name: String,
}

impl Engine {
    fn start(cmd: &str) -> Result<Engine, String> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start '{cmd}': {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut e = Engine {
            child,
            stdin,
            stdout,
            name: cmd.to_string(),
        };
        let greeting = e.read_reply()?;
        if let Some(id) = greeting.iter().find(|l| l.starts_with("id ")) {
            e.name = id[3..].to_string();
        }
        Ok(e)
    }

    /// Lines up to (not including) `ok`.
    fn read_reply(&mut self) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        loop {
            let mut l = String::new();
            if self.stdout.read_line(&mut l).map_err(|e| e.to_string())? == 0 {
                return Err(format!("{} exited", self.name));
            }
            let l = l.trim_end().to_string();
            if l == "ok" {
                return Ok(lines);
            }
            lines.push(l);
        }
    }

    fn cmd(&mut self, c: &str) -> Result<Vec<String>, String> {
        writeln!(self.stdin, "{c}").map_err(|e| format!("{}: {e}", self.name))?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let reply = self.read_reply()?;
        if let Some(bad) = reply
            .iter()
            .find(|l| l.starts_with("err") || l.starts_with("invalidmove"))
        {
            return Err(format!("{} answered '{c}' with '{bad}'", self.name));
        }
        Ok(reply)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "exit");
        let _ = self.child.wait();
    }
}

struct Args {
    a: String,
    b: String,
    games: u64,
    time_a: u64,
    time_b: u64,
    opening: u32,
    max_plies: u32,
    threads: usize,
    seed: u64,
    /// Commands sent to each engine right after it starts, `;`-separated
    /// (e.g. `options set MaxHelperThreads None`).
    init_a: String,
    init_b: String,
}

fn parse_args() -> Args {
    let mut a = Args {
        a: String::new(),
        b: String::new(),
        games: 100,
        time_a: 0,
        time_b: 0,
        opening: 4,
        max_plies: 200,
        threads: thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2),
        seed: 1,
        init_a: String::new(),
        init_b: String::new(),
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let v = argv
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| panic!("missing value for {}", argv[i]));
        match argv[i].as_str() {
            "--a" => a.a = v,
            "--b" => a.b = v,
            "--games" => a.games = v.parse().expect("games"),
            "--time-ms" => {
                a.time_a = v.parse().expect("time-ms");
                a.time_b = a.time_a;
            }
            "--time-ms-a" => a.time_a = v.parse().expect("time-ms-a"),
            "--time-ms-b" => a.time_b = v.parse().expect("time-ms-b"),
            "--opening" => a.opening = v.parse().expect("opening"),
            "--max-plies" => a.max_plies = v.parse().expect("max-plies"),
            "--threads" => a.threads = v.parse().expect("threads"),
            "--seed" => a.seed = v.parse().expect("seed"),
            "--init-a" => a.init_a = v,
            "--init-b" => a.init_b = v,
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    assert!(
        !a.a.is_empty() && !a.b.is_empty(),
        "usage: uhp-match --a <cmd> --b <cmd> [...]"
    );
    if a.time_a == 0 {
        a.time_a = 100;
    }
    if a.time_b == 0 {
        a.time_b = 100;
    }
    a
}

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    AWins,
    BWins,
    Draw,
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

fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        ms % 1000
    )
}

/// Plays one game; `a_white` says which engine has White.
fn play_game(
    ea: &mut Engine,
    eb: &mut Engine,
    a_white: bool,
    opening_seed: u64,
    args: &Args,
) -> (Outcome, u32, String) {
    let mut g = Game::new(true);
    let mut rng = Rng(opening_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let a_color = if a_white { Color::White } else { Color::Black };
    // An engine failure loses the game for the side that failed.
    let lose = |who_is_a: bool, why: String, g: &Game| {
        (
            if who_is_a {
                Outcome::BWins
            } else {
                Outcome::AWins
            },
            g.ply(),
            why,
        )
    };
    for e in [&mut *ea, &mut *eb] {
        if let Err(why) = e.cmd("newgame Base") {
            return (Outcome::Draw, 0, format!("setup failed: {why}"));
        }
    }
    while !g.state().is_over() && g.ply() < args.max_plies {
        let mover_is_a = g.to_move() == a_color;
        let mv_str = if g.ply() < args.opening {
            let ms = g.valid_moves();
            move_to_string(&g, ms[rng.below(ms.len())])
        } else {
            let e = if mover_is_a { &mut *ea } else { &mut *eb };
            let ms = if mover_is_a { args.time_a } else { args.time_b };
            match e.cmd(&format!("bestmove time {}", fmt_time(ms))) {
                Ok(lines) => lines.first().cloned().unwrap_or_default(),
                Err(why) => return lose(mover_is_a, why, &g),
            }
        };
        let m = match parse_move(&g, &mv_str) {
            Ok(m) => m,
            Err(why) => return lose(mover_is_a, format!("illegal move '{mv_str}': {why}"), &g),
        };
        for (is_a, e) in [(true, &mut *ea), (false, &mut *eb)] {
            if let Err(why) = e.cmd(&format!("play {mv_str}")) {
                return lose(is_a, why, &g);
            }
        }
        g.apply(m);
    }
    let out = match (g.state(), a_white) {
        (GameState::WhiteWins, true) | (GameState::BlackWins, false) => Outcome::AWins,
        (GameState::WhiteWins, false) | (GameState::BlackWins, true) => Outcome::BWins,
        _ => Outcome::Draw,
    };
    (out, g.ply(), g.game_string())
}

/// Elo difference for score fraction `s` and a rough 95% interval.
fn elo(wins: u64, losses: u64, draws: u64) -> (f64, f64) {
    let n = (wins + losses + draws) as f64;
    let s = (wins as f64 + 0.5 * draws as f64) / n;
    let to_elo = |p: f64| -400.0 * (1.0 / p.clamp(1e-3, 1.0 - 1e-3) - 1.0).log10();
    // Per-game score variance.
    let var = (wins as f64 * (1.0 - s).powi(2)
        + losses as f64 * s.powi(2)
        + draws as f64 * (0.5 - s).powi(2))
        / n;
    let margin = 1.96 * (var / n).sqrt();
    (
        to_elo(s),
        (to_elo((s + margin).min(0.999)) - to_elo((s - margin).max(0.001))) / 2.0,
    )
}

fn main() {
    let args = Arc::new(parse_args());
    let started = Instant::now();
    let next = Arc::new(AtomicU64::new(0));
    let tally = Arc::new(Mutex::new((0u64, 0u64, 0u64, 0u64, Vec::<String>::new())));
    let names = Arc::new(Mutex::new((String::new(), String::new())));
    let threads = args.threads.min(args.games as usize).max(1);
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (args, next, tally, names) =
                (args.clone(), next.clone(), tally.clone(), names.clone());
            thread::spawn(move || {
                let mut ea = Engine::start(&args.a).unwrap_or_else(|e| panic!("{e}"));
                let mut eb = Engine::start(&args.b).unwrap_or_else(|e| panic!("{e}"));
                for (e, init) in [(&mut ea, &args.init_a), (&mut eb, &args.init_b)] {
                    for c in init.split(';').map(str::trim).filter(|c| !c.is_empty()) {
                        e.cmd(c).unwrap_or_else(|why| panic!("init '{c}': {why}"));
                    }
                }
                *names.lock().unwrap() = (ea.name.clone(), eb.name.clone());
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= args.games {
                        break;
                    }
                    let (out, plies, record) = play_game(
                        &mut ea,
                        &mut eb,
                        i % 2 == 0,
                        args.seed * 1_000_003 + i / 2,
                        &args,
                    );
                    let mut t = tally.lock().unwrap();
                    match out {
                        Outcome::AWins => t.0 += 1,
                        Outcome::BWins => t.1 += 1,
                        Outcome::Draw => t.2 += 1,
                    }
                    t.3 += plies as u64;
                    if !record.starts_with("Base;") {
                        t.4.push(format!("game {i}: {record}"));
                    } else if std::env::var_os("UHP_MATCH_SHOW").is_some() {
                        let a_side = if i % 2 == 0 { "White" } else { "Black" };
                        eprintln!("\ngame {i} (A = {a_side}): {record}");
                    }
                    let done = t.0 + t.1 + t.2;
                    eprint!("\r{done}/{}  A {}  B {}  D {}", args.games, t.0, t.1, t.2);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker panicked");
    }
    eprintln!();
    let t = tally.lock().unwrap();
    let (na, nb) = names.lock().unwrap().clone();
    let n = t.0 + t.1 + t.2;
    let score = (t.0 as f64 + 0.5 * t.2 as f64) / n as f64;
    let (e, margin) = elo(t.0, t.1, t.2);
    for p in &t.4 {
        println!("problem: {p}");
    }
    println!("A = {na}\nB = {nb}");
    println!(
        "games {n}  A wins {}  B wins {}  draws {}  A score {:.1}%  Elo(A-B) {:+.0} ± {:.0}  avg plies {:.0}  {:.0}s",
        t.0,
        t.1,
        t.2,
        score * 100.0,
        e,
        margin,
        t.3 as f64 / n as f64,
        started.elapsed().as_secs_f64()
    );
}
