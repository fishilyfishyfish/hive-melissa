//! wasm-bindgen bindings used by the web worker.

use wasm_bindgen::prelude::*;

use crate::game::Game;
use crate::json;

#[wasm_bindgen]
pub struct HiveGame {
    g: Game,
}

#[wasm_bindgen]
impl HiveGame {
    #[wasm_bindgen(constructor)]
    pub fn new(tournament: bool) -> HiveGame {
        HiveGame {
            g: Game::new(tournament),
        }
    }

    /// Loads a UHP GameString.
    pub fn load(game_string: &str, tournament: bool) -> Result<HiveGame, JsError> {
        Game::from_game_string(game_string, tournament)
            .map(|g| HiveGame { g })
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
}
