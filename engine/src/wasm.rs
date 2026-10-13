//! wasm-bindgen bindings used by the web worker.

use wasm_bindgen::prelude::*;

use crate::ai::{SearchLimits, Searcher};
use crate::game::Game;
use crate::json;

#[wasm_bindgen]
pub struct HiveGame {
    g: Game,
    searcher: Option<Searcher>,
}

#[wasm_bindgen]
impl HiveGame {
    #[wasm_bindgen(constructor)]
    pub fn new(tournament: bool) -> HiveGame {
        HiveGame {
            g: Game::new(tournament),
            searcher: None,
        }
    }

    /// Loads a UHP GameString.
    pub fn load(game_string: &str, tournament: bool) -> Result<HiveGame, JsError> {
        Game::from_game_string(game_string, tournament)
            .map(|g| HiveGame { g, searcher: None })
            .map_err(|e| JsError::new(&e))
    }

    /// Plays a UHP move string.
    pub fn play(&mut self, mv: &str) -> Result<(), JsError> {
        self.g
            .play_uhp(mv)
            .map(|_| ())
            .map_err(|e| JsError::new(&e))
    }

    /// Takes back up to `n` moves; returns how many were undone.
    pub fn undo(&mut self, n: u32) -> u32 {
        let mut k = 0;
        while k < n && self.g.undo().is_some() {
            k += 1;
        }
        k
    }

    #[wasm_bindgen(js_name = validMoves)]
    pub fn valid_moves(&self) -> String {
        json::valid_moves_json(&self.g)
    }

    pub fn state(&self) -> String {
        json::state_json(&self.g)
    }

    #[wasm_bindgen(js_name = gameString)]
    pub fn game_string(&self) -> String {
        self.g.game_string()
    }

    /// Searches for the best move (does not play it). Returns JSON:
    /// `{"uhp":..,"score":..,"depth":..,"nodes":..,"ms":..,"pv":[..]}`,
    /// score from the side to move's point of view.
    pub fn search(&mut self, time_ms: f64, max_depth: u32) -> Result<String, JsError> {
        if self.g.state().is_over() {
            return Err(JsError::new("game is over"));
        }
        // 2^18 entries * 24 bytes = 6 MB, kept across moves.
        let s = self.searcher.get_or_insert_with(|| Searcher::new(18));
        let r = s.search(
            &self.g,
            &SearchLimits {
                max_depth: max_depth.max(1),
                time_ms,
                ..Default::default()
            },
        );
        Ok(json::search_json(&self.g, &r))
    }
}

thread_local! {
    /// Searcher reused across analysis calls (keeps its transposition table).
    static ANALYSIS: std::cell::RefCell<Option<Searcher>> = const { std::cell::RefCell::new(None) };
}

/// Analyzes `move_uhp` played in the position given by `game_string`
/// (UHP GameString of the moves before it). Returns `json::report_json`.
#[wasm_bindgen(js_name = analyzeMove)]
pub fn analyze_move(
    game_string: &str,
    move_uhp: &str,
    time_ms: f64,
    tournament: bool,
) -> Result<String, JsError> {
    let g = Game::from_game_string(game_string, tournament).map_err(|e| JsError::new(&e))?;
    let m = crate::uhp::parse_move(&g, move_uhp).map_err(|e| JsError::new(&e))?;
    ANALYSIS.with(|cell| {
        let mut slot = cell.borrow_mut();
        let s = slot.get_or_insert_with(|| Searcher::new(18));
        let r = crate::analysis::analyze_move(&g, m, s, time_ms);
        Ok(json::report_json(&r))
    })
}

/// Board snapshot (same shape as `HiveGame.state()`) for a GameString.
#[wasm_bindgen(js_name = positionState)]
pub fn position_state(game_string: &str, tournament: bool) -> Result<String, JsError> {
    let g = Game::from_game_string(game_string, tournament).map_err(|e| JsError::new(&e))?;
    Ok(json::state_json(&g))
}
