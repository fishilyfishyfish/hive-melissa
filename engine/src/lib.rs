//! Hive (base game) engine: board representation, legal move generation,
//! apply/undo, game-end detection and UHP notation.

pub mod ai;
pub mod board;
pub mod game;
pub mod hex;
pub mod json;
pub mod piece;
pub mod uhp;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use board::Board;
pub use game::{Game, GameState, Move};
pub use hex::Hex;
pub use piece::{Bug, Color, Piece};
