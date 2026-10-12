//! Universal Hive Protocol (UHP) notation: move strings and game strings.
//!
//! MoveString: `wS1` (first placement), `bG1 -wS1` (west of wS1),
//! `wB1 bQ` (on top of bQ), `pass`. Direction markers relative to the
//! reference piece:
//!
//! | marker   | target cell        |
//! |----------|--------------------|
//! | `ref-`   | east               |
//! | `-ref`   | west               |
//! | `ref/`   | north-east         |
//! | `/ref`   | south-west         |
//! | `ref\`   | south-east         |
//! | `\ref`   | north-west         |

use crate::game::{Game, GameState, Move};
use crate::hex::{Hex, E, NE, NW, SE, SW, W};
use crate::piece::Piece;

/// Formats `m` relative to the position *before* it is played.
pub fn move_to_string(g: &Game, m: Move) -> String {
    let (piece, to, from) = match m {
        Move::Pass => return "pass".to_string(),
        Move::Place { piece, to } => (piece, to, None),
        Move::Move { piece, from, to } => (piece, to, Some(from)),
    };
    let mut b = g.board().clone();
    if let Some(f) = from {
        b.pop(f);
    }
    let name = piece.name();
    if b.piece_count() == 0 {
        return name;
    }
    if let Some(top) = b.top(to) {
        return format!("{name} {}", top.name());
    }
    for d in [E, W, NE, SW, SE, NW] {
        // `to` = `ref` + DIRS[d]  <=>  `ref` = `to` stepped the opposite way.
        let ref_cell = to.step((d + 3) % 6);
        if let Some(r) = b.top(ref_cell) {
            let rn = r.name();
            let s = match d {
                E => format!("{rn}-"),
                W => format!("-{rn}"),
                NE => format!("{rn}/"),
                SW => format!("/{rn}"),
                SE => format!("{rn}\\"),
                _ => format!("\\{rn}"),
            };
            return format!("{name} {s}");
        }
    }
    // Unreachable for legal moves: every target touches the hive.
    format!("{name} ?")
}

/// Resolves a UHP move string against the current position and checks that
/// the move is legal.
pub fn parse_move(g: &Game, s: &str) -> Result<Move, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("pass") {
        let legal = g.valid_moves();
        return if legal == [Move::Pass] {
            Ok(Move::Pass)
        } else {
            Err("pass is only legal when no other move exists".into())
        };
    }
    let mut parts = s.split_whitespace();
    let pname = parts.next().ok_or("empty move")?;
    let piece = Piece::parse(pname).ok_or_else(|| format!("unknown piece '{pname}'"))?;
    let target: Hex = match parts.next() {
        None => {
            if g.board().piece_count() != 0 {
                return Err("missing target position".into());
            }
            Hex::ORIGIN
        }
        Some(pos) => parse_position(g, pos)?,
    };
    if parts.next().is_some() {
        return Err(format!("trailing input in '{s}'"));
    }
    let m = match g.board().position(piece) {
        Some(from) => Move::Move {
            piece,
            from,
            to: target,
        },
        None => Move::Place { piece, to: target },
    };
    if g.valid_moves().contains(&m) {
        Ok(m)
    } else {
        Err(format!("illegal move '{s}'"))
    }
}

fn parse_position(g: &Game, pos: &str) -> Result<Hex, String> {
    let first = pos.chars().next().ok_or("empty position")?;
    let last = pos.chars().last().unwrap();
    let (dir, rname) = match (first, last) {
        ('-', _) => (Some(W), &pos[1..]),
        ('/', _) => (Some(SW), &pos[1..]),
        ('\\', _) => (Some(NW), &pos[1..]),
        (_, '-') => (Some(E), &pos[..pos.len() - 1]),
        (_, '/') => (Some(NE), &pos[..pos.len() - 1]),
        (_, '\\') => (Some(SE), &pos[..pos.len() - 1]),
        _ => (None, pos),
    };
    let rp = Piece::parse(rname).ok_or_else(|| format!("unknown reference piece '{rname}'"))?;
    let rc = g
        .board()
        .position(rp)
        .ok_or_else(|| format!("reference piece {rname} is not on the board"))?;
    Ok(match dir {
        Some(d) => rc.step(d),
        None => rc,
    })
}

impl Game {
    /// Parses and plays a UHP move string.
    pub fn play_uhp(&mut self, s: &str) -> Result<Move, String> {
        let m = parse_move(self, s)?;
        self.apply(m);
        Ok(m)
    }

    /// UHP valid-moves string: moves separated by `;`.
    pub fn valid_moves_uhp(&self) -> String {
        self.valid_moves()
            .into_iter()
            .map(|m| move_to_string(self, m))
            .collect::<Vec<_>>()
            .join(";")
    }

    /// UHP GameString, e.g. `Base;InProgress;White[3];wS1;bS1 wS1-;...`.
    pub fn game_string(&self) -> String {
        let mut s = format!(
            "Base;{};{}[{}]",
            self.state().name(),
            self.to_move().name(),
            self.turn_number()
        );
        for m in self.history_uhp() {
            s.push(';');
            s.push_str(m);
        }
        s
    }

    /// Loads a UHP GameString (or just `Base`). Moves are replayed and
    /// checked; the state/turn fields are recomputed rather than trusted.
    pub fn from_game_string(s: &str, tournament: bool) -> Result<Game, String> {
        let mut fields = s.split(';');
        let gt = fields.next().unwrap_or("").trim();
        if gt != "Base" {
            return Err(format!("unsupported game type '{gt}'"));
        }
        let mut g = Game::new(tournament);
        let rest: Vec<&str> = fields.collect();
        // Optional GameStateString and TurnString precede the moves.
        let moves = if rest.len() >= 2
            && parse_state(rest[0]).is_some()
            && (rest[1].starts_with("White[") || rest[1].starts_with("Black["))
        {
            &rest[2..]
        } else {
            &rest[..]
        };
        for (i, m) in moves.iter().enumerate() {
            g.play_uhp(m).map_err(|e| format!("move {}: {e}", i + 1))?;
        }
        Ok(g)
    }
}

fn parse_state(s: &str) -> Option<GameState> {
    Some(match s {
        "NotStarted" => GameState::NotStarted,
        "InProgress" => GameState::InProgress,
        "Draw" => GameState::Draw,
        "WhiteWins" => GameState::WhiteWins,
        "BlackWins" => GameState::BlackWins,
        _ => return None,
    })
}
