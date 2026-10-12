//! Universal Hive Protocol engine: a line-based command interpreter.
//!
//! Every response ends with a line `ok`. Failures print `err <message>`
//! (or `invalidmove <message>` for an illegal move) before the `ok`.
//! Supported commands: `info`, `newgame [Base | GameString]`, `play <move>`,
//! `pass`, `validmoves`, `bestmove [time hh:mm:ss | depth n]`, `undo [n]`,
//! `options [get <name> | set <name> <value>]`. `exit` is handled by the
//! caller (the binary's stdin loop).

use crate::ai::{SearchLimits, Searcher};
use crate::game::Game;
use crate::uhp::move_to_string;

pub const NAME: &str = "HiveMelissa";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default `bestmove` budget when no argument is given.
const DEFAULT_TIME_MS: f64 = 1000.0;

pub struct UhpEngine {
    game: Option<Game>,
    searcher: Searcher,
    /// Option `TimeMs`: default thinking time for a bare `bestmove`.
    time_ms: u64,
    /// Option `TranspositionTableBits`: log2 of TT entries.
    tt_bits: u32,
}

impl Default for UhpEngine {
    fn default() -> Self {
        UhpEngine::new()
    }
}

impl UhpEngine {
    pub fn new() -> UhpEngine {
        UhpEngine {
            game: None,
            searcher: Searcher::new(20),
            time_ms: DEFAULT_TIME_MS as u64,
            tt_bits: 20,
        }
    }

    /// Lines printed when the engine starts (same as `info`).
    pub fn greeting(&self) -> String {
        self.info()
    }

    fn info(&self) -> String {
        // A second line would list supported expansions; base game only.
        format!("id {NAME} v{VERSION}\nok")
    }

    /// Handles one command line and returns the full response including the
    /// trailing `ok`.
    pub fn handle(&mut self, line: &str) -> String {
        let line = line.trim();
        let (cmd, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim()),
            None => (line, ""),
        };
        let body = match cmd {
            "" => return String::new(),
            "info" => return self.info(),
            "newgame" => self.newgame(rest),
            "play" => self.play(rest),
            "pass" => self.play("pass"),
            "validmoves" => self.with_game(|g| Ok(g.valid_moves_uhp())),
            "bestmove" => self.bestmove(rest),
            "undo" => self.undo(rest),
            "options" => self.options(rest),
            _ => Err(Reply::Err(format!("unknown command '{cmd}'"))),
        };
        match body {
            Ok(text) => format!("{text}\nok"),
            Err(Reply::Err(m)) => format!("err {m}\nok"),
            Err(Reply::Invalid(m)) => format!("invalidmove {m}\nok"),
        }
    }

    fn with_game<T>(&self, f: impl FnOnce(&Game) -> Result<T, Reply>) -> Result<T, Reply> {
        match &self.game {
            Some(g) => f(g),
            None => Err(Reply::Err("no game in progress".into())),
        }
    }

    fn newgame(&mut self, arg: &str) -> Result<String, Reply> {
        let g = if arg.is_empty() || arg == "Base" {
            Game::new(true)
        } else if arg.starts_with("Base;") {
            Game::from_game_string(arg, true).map_err(Reply::Err)?
        } else {
            return Err(Reply::Err(format!(
                "unsupported game type '{arg}' (only Base)"
            )));
        };
        self.searcher.clear();
        let s = g.game_string();
        self.game = Some(g);
        Ok(s)
    }

    fn play(&mut self, mv: &str) -> Result<String, Reply> {
        let g = self
            .game
            .as_mut()
            .ok_or_else(|| Reply::Err("no game in progress".into()))?;
        if g.state().is_over() {
            return Err(Reply::Invalid("the game is over".into()));
        }
        g.play_uhp(mv).map_err(Reply::Invalid)?;
        Ok(g.game_string())
    }

    fn undo(&mut self, arg: &str) -> Result<String, Reply> {
        let n: usize = if arg.is_empty() {
            1
        } else {
            arg.parse()
                .map_err(|_| Reply::Err(format!("bad count '{arg}'")))?
        };
        let g = self
            .game
            .as_mut()
            .ok_or_else(|| Reply::Err("no game in progress".into()))?;
        if n > g.history().len() {
            return Err(Reply::Err(format!("cannot undo {n} moves")));
        }
        for _ in 0..n {
            g.undo();
        }
        Ok(g.game_string())
    }

    fn bestmove(&mut self, arg: &str) -> Result<String, Reply> {
        let g = self
            .game
            .as_ref()
            .ok_or_else(|| Reply::Err("no game in progress".into()))?;
        if g.state().is_over() {
            return Err(Reply::Err("the game is over".into()));
        }
        let mut limits = SearchLimits {
            time_ms: self.time_ms as f64,
            ..Default::default()
        };
        let parts: Vec<&str> = arg.split_whitespace().collect();
        match parts.as_slice() {
            [] => {}
            ["time", t] => {
                limits.time_ms =
                    parse_hms(t).ok_or_else(|| Reply::Err(format!("bad time '{t}'")))?;
            }
            ["depth", d] => {
                limits.max_depth = d
                    .parse()
                    .map_err(|_| Reply::Err(format!("bad depth '{d}'")))?;
                limits.time_ms = f64::INFINITY;
            }
            _ => return Err(Reply::Err(format!("bad bestmove arguments '{arg}'"))),
        }
        let r = self.searcher.search(g, &limits);
        Ok(move_to_string(g, r.best))
    }

    fn options(&mut self, arg: &str) -> Result<String, Reply> {
        let parts: Vec<&str> = arg.split_whitespace().collect();
        match parts.as_slice() {
            [] => Ok(format!(
                "{}\n{}",
                self.option_line("TimeMs"),
                self.option_line("TranspositionTableBits")
            )),
            ["get", name] => self.known(name).map(|n| self.option_line(n)),
            ["set", name, value] => {
                let n = self.known(name)?;
                let v: u64 = value
                    .parse()
                    .map_err(|_| Reply::Err(format!("bad value '{value}'")))?;
                match n {
                    "TimeMs" if (1..=3_600_000).contains(&v) => self.time_ms = v,
                    "TranspositionTableBits" if (10..=26).contains(&v) => {
                        self.tt_bits = v as u32;
                        self.searcher = Searcher::new(self.tt_bits);
                    }
                    _ => return Err(Reply::Err(format!("value out of range for {n}"))),
                }
                Ok(self.option_line(n))
            }
            _ => Err(Reply::Err(format!("bad options arguments '{arg}'"))),
        }
    }

    fn known(&self, name: &str) -> Result<&'static str, Reply> {
        match name {
            "TimeMs" => Ok("TimeMs"),
            "TranspositionTableBits" => Ok("TranspositionTableBits"),
            _ => Err(Reply::Err(format!("unknown option '{name}'"))),
        }
    }

    /// `Name;Type;Value;Default;Min;Max`
    fn option_line(&self, name: &str) -> String {
        match name {
            "TimeMs" => format!(
                "TimeMs;int;{};{};1;3600000",
                self.time_ms, DEFAULT_TIME_MS as u64
            ),
            _ => format!("TranspositionTableBits;int;{};20;10;26", self.tt_bits),
        }
    }
}

enum Reply {
    Err(String),
    Invalid(String),
}

/// Parses `hh:mm:ss` (also `mm:ss` or `ss`, fractional seconds allowed) to ms.
fn parse_hms(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.split(':') {
        let v: f64 = part.parse().ok()?;
        if v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total * 1000.0)
}
