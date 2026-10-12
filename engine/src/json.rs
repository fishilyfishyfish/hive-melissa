//! Hand-written JSON views of the game for the web UI (keeps the WASM build
//! free of serde).

use crate::game::{Game, Move};
use crate::hex::Hex;
use crate::piece::Color;
use crate::uhp::move_to_string;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn hex_json(h: Option<Hex>) -> String {
    match h {
        Some(h) => format!("[{},{}]", h.q, h.r),
        None => "null".into(),
    }
}

fn list<I: IntoIterator<Item = String>>(it: I) -> String {
    format!("[{}]", it.into_iter().collect::<Vec<_>>().join(","))
}

/// `[{"uhp":"wS1 -wQ","piece":"wS1","from":null,"to":[q,r]}, ...]`
pub fn valid_moves_json(g: &Game) -> String {
    list(g.valid_moves().into_iter().map(|m| {
        let piece = m
            .piece()
            .map(|p| esc(&p.name()))
            .unwrap_or_else(|| "null".into());
        format!(
            "{{\"uhp\":{},\"piece\":{},\"from\":{},\"to\":{}}}",
            esc(&move_to_string(g, m)),
            piece,
            hex_json(m.from()),
            hex_json(m.to())
        )
    }))
}

/// Full position snapshot for rendering.
pub fn state_json(g: &Game) -> String {
    let b = g.board();
    let mut cells = b.occupied_cells();
    cells.sort();
    let cells_json = list(cells.into_iter().map(|c| {
        format!(
            "{{\"q\":{},\"r\":{},\"stack\":{}}}",
            c.q,
            c.r,
            list(b.stack(c).iter().map(|p| esc(&p.name())))
        )
    }));
    let hand = |c: Color| list(g.hand(c).into_iter().map(|p| esc(&p.name())));
    let last = g.history().last().copied();
    format!(
        "{{\"status\":{},\"toMove\":{},\"turn\":{},\"ply\":{},\"tournament\":{},\"gameString\":{},\
         \"cells\":{},\"hands\":{{\"White\":{},\"Black\":{}}},\"history\":{},\"lastFrom\":{},\"lastTo\":{}}}",
        esc(g.state().name()),
        esc(g.to_move().name()),
        g.turn_number(),
        g.ply(),
        g.tournament(),
        esc(&g.game_string()),
        cells_json,
        hand(Color::White),
        hand(Color::Black),
        list(g.history_uhp().iter().map(|s| esc(s))),
        hex_json(last.and_then(Move::from)),
        hex_json(last.and_then(Move::to)),
    )
}
