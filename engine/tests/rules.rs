//! One test per rule, each building the exact situation by hand.
//!
//! Coordinates are axial (q, r), pointy-top. Neighbors of (q, r):
//! E (q+1, r), NE (q+1, r-1), NW (q, r-1), W (q-1, r), SW (q-1, r+1), SE (q, r+1).

use std::collections::BTreeSet;

use hive_engine::{uhp, Color, Game, GameState, Hex, Move, Piece};

fn game(layout: &[(&str, i32, i32)], ply: u32) -> Game {
    Game::from_layout(layout, ply, true).unwrap()
}

fn set(cells: &[(i32, i32)]) -> BTreeSet<(i32, i32)> {
    cells.iter().copied().collect()
}

/// Destinations of `name` among the legal moves.
fn dests(g: &Game, name: &str) -> BTreeSet<(i32, i32)> {
    let p = Piece::parse(name).unwrap();
    g.valid_moves()
        .into_iter()
        .filter_map(|m| match m {
            Move::Move { piece, to, .. } if piece == p => Some((to.q, to.r)),
            _ => None,
        })
        .collect()
}

fn placement_cells(g: &Game) -> BTreeSet<(i32, i32)> {
    g.valid_moves()
        .into_iter()
        .filter_map(|m| match m {
            Move::Place { to, .. } => Some((to.q, to.r)),
            _ => None,
        })
        .collect()
}

fn placement_pieces(g: &Game) -> BTreeSet<String> {
    g.valid_moves()
        .into_iter()
        .filter_map(|m| match m {
            Move::Place { piece, .. } => Some(piece.name()),
            _ => None,
        })
        .collect()
}

fn names(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------- placement

#[test]
fn first_placement_is_origin_only_and_no_queen_in_tournament() {
    let g = Game::new(true);
    assert_eq!(placement_cells(&g), set(&[(0, 0)]));
    assert_eq!(placement_pieces(&g), names(&["wS1", "wB1", "wG1", "wA1"]));
    assert!(g
        .valid_moves()
        .iter()
        .all(|m| matches!(m, Move::Place { .. })));
}

#[test]
fn queen_allowed_first_without_tournament_rule() {
    let g = Game::new(false);
    assert_eq!(
        placement_pieces(&g),
        names(&["wQ", "wS1", "wB1", "wG1", "wA1"])
    );
    let mut g = Game::new(false);
    g.play_uhp("wQ").unwrap();
    assert!(placement_pieces(&g).contains("bQ"));
}

#[test]
fn second_placement_goes_next_to_first_piece_and_no_black_queen() {
    let mut g = Game::new(true);
    g.play_uhp("wS1").unwrap();
    assert_eq!(
        placement_cells(&g),
        Hex::ORIGIN.neighbors().iter().map(|h| (h.q, h.r)).collect()
    );
    assert_eq!(placement_pieces(&g), names(&["bS1", "bB1", "bG1", "bA1"]));
}

#[test]
fn later_placements_touch_own_color_only() {
    // wQ (0,0), bQ (1,0). White may only use cells next to wQ that do not
    // touch bQ.
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0)], 2);
    assert_eq!(placement_cells(&g), set(&[(0, -1), (-1, 0), (-1, 1)]));
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0)], 3);
    assert_eq!(placement_cells(&g), set(&[(2, -1), (2, 0), (1, 1)]));
}

#[test]
fn piece_copies_are_placed_in_number_order() {
    let g = game(&[("wS1", 0, 0), ("bS1", 1, 0)], 2);
    let p = placement_pieces(&g);
    assert!(p.contains("wS2") && !p.contains("wS1"));
    assert!(p.contains("wA1") && !p.contains("wA2"));
}

#[test]
fn stack_color_is_top_piece_for_placement() {
    // White beetle on black queen: the stack counts as white.
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wB1", 1, 0)], 4);
    // All eight cells around the two-cell hive are open to white.
    assert_eq!(placement_cells(&g).len(), 8);
}

// --------------------------------------------------------------- queen rule

#[test]
fn queen_must_be_placed_by_fourth_turn() {
    let layout = [
        ("wS1", 0, 0),
        ("bS1", 1, 0),
        ("wA1", -1, 0),
        ("bA1", 2, 0),
        ("wG1", -2, 0),
        ("bG1", 3, 0),
    ];
    // White turn 4 without queen: only queen placements.
    let g = game(&layout, 6);
    assert_eq!(g.turn_number(), 4);
    let moves = g.valid_moves();
    assert!(!moves.is_empty());
    assert!(moves
        .iter()
        .all(|m| matches!(m, Move::Place { piece, .. } if piece.name() == "wQ")));

    // Black turn 4 likewise.
    let mut black = layout.to_vec();
    black.push(("wQ", -1, -1));
    let g = game(&black, 7);
    assert_eq!(g.to_move(), Color::Black);
    let moves = g.valid_moves();
    assert!(!moves.is_empty());
    assert!(moves
        .iter()
        .all(|m| matches!(m, Move::Place { piece, .. } if piece.name() == "bQ")));

    // Turn 3: other pieces still allowed.
    let g = game(&layout[..4], 4);
    assert!(placement_pieces(&g).len() > 1);
}

#[test]
fn no_movement_before_own_queen_is_placed() {
    // The white ant could otherwise walk around; queen absent means placements only.
    let g = game(
        &[("wS1", 0, 0), ("bS1", 1, 0), ("wA1", -1, 0), ("bQ", 2, 0)],
        4,
    );
    assert!(g
        .valid_moves()
        .iter()
        .all(|m| matches!(m, Move::Place { .. })));
    // Black has its queen, so black may move.
    let g = game(
        &[("wS1", 0, 0), ("bQ", 1, 0), ("wA1", -1, 0), ("bA1", 2, 0)],
        5,
    );
    assert!(!dests(&g, "bA1").is_empty());
}

// ---------------------------------------------------------------- one hive

#[test]
fn piece_at_articulation_point_cannot_move() {
    let g = game(&[("bQ", -1, 0), ("wQ", 0, 0), ("wA1", 1, 0)], 4);
    assert!(
        dests(&g, "wQ").is_empty(),
        "wQ joins the hive and is pinned"
    );
    assert!(!dests(&g, "wA1").is_empty());
}

#[test]
fn piece_in_ring_is_not_pinned() {
    // Triangle: no single removal disconnects it.
    let g = game(
        &[("wQ", 0, 0), ("bQ", 1, 0), ("wA1", 0, 1), ("bA1", 2, 0)],
        4,
    );
    assert!(!dests(&g, "wQ").is_empty());
}

#[test]
fn articulation_points_are_found() {
    use hive_engine::game::articulation_points;
    let g = game(
        &[
            ("wA1", 0, 0),
            ("wA2", 1, 0),
            ("wA3", 2, 0),
            ("bA1", 1, 1),
            ("bA2", 3, -1),
        ],
        0,
    );
    let ap: BTreeSet<_> = articulation_points(g.board())
        .iter()
        .map(|h| (h.q, h.r))
        .collect();
    assert_eq!(ap, set(&[(1, 0), (2, 0)]));
}

#[test]
fn piece_may_not_slide_off_the_hive() {
    // Two-piece hive: wQ can only slide along bQ, never away from it.
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0)], 2);
    assert_eq!(dests(&g, "wQ"), set(&[(1, -1), (0, 1)]));
}

// -------------------------------------------------------------- freedom gate

#[test]
fn gate_blocks_sliding_between_two_pieces() {
    // Ring around the empty cell (1,0); wQ at (0,0) is the ring's opening.
    // Entering (1,0) means squeezing between (1,-1) and (0,1).
    let g = game(
        &[
            ("wQ", 0, 0),
            ("bQ", 1, -1),
            ("wS1", 2, -1),
            ("bS1", 2, 0),
            ("wG1", 1, 1),
            ("bG1", 0, 1),
        ],
        8,
    );
    assert_eq!(dests(&g, "wQ"), set(&[(0, -1), (-1, 1)]));
}

#[test]
fn ant_cannot_enter_gated_hole() {
    // Five pieces around (1,0); the only opening, from (0,0), is a gate.
    let g = game(
        &[
            ("bQ", 1, -1),
            ("wQ", 2, -1),
            ("bS1", 2, 0),
            ("wG1", 1, 1),
            ("bG1", 0, 1),
            ("wA1", 3, -1),
        ],
        8,
    );
    let d = dests(&g, "wA1");
    assert!(d.contains(&(0, 0)), "ant reaches the mouth of the hole");
    assert!(!d.contains(&(1, 0)), "but cannot squeeze into it");
    assert!(!d.contains(&(3, -1)), "never ends on its start cell");
}

// --------------------------------------------------------------- bug moves

#[test]
fn queen_moves_one_cell() {
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wA1", -1, 0)], 4);
    // wQ is pinned here (middle of the line); use the ant's neighbor instead.
    assert!(dests(&g, "wQ").is_empty());
    let g = game(&[("wQ", -1, 0), ("bQ", 1, 0), ("wA1", 0, 0)], 4);
    assert_eq!(dests(&g, "wQ"), set(&[(0, -1), (-1, 1)]));
}

#[test]
fn spider_moves_exactly_three_without_backtracking() {
    // Spider at (-1,0) next to the line (0,0)-(1,0). The 8-cell perimeter
    // ring lets it go 3 steps either way.
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wS1", -1, 0)], 4);
    assert_eq!(dests(&g, "wS1"), set(&[(2, -1), (1, 1)]));
}

#[test]
fn spider_respects_gates_along_its_path() {
    // Same hole as the ant test: the spider at (0,0)'s neighbor cannot use
    // the hole (1,0) as a waypoint.
    let g = game(
        &[
            ("bQ", 1, -1),
            ("wQ", 2, -1),
            ("bS1", 2, 0),
            ("wG1", 1, 1),
            ("bG1", 0, 1),
            ("wS1", 0, 0),
        ],
        8,
    );
    let d = dests(&g, "wS1");
    assert!(!d.contains(&(1, 0)));
    // Around the outside: (0,0)->(0,-1)->(1,-2)->(2,-2) and
    // (0,0)->(-1,1)->(-1,2)->(0,2).
    assert_eq!(d, set(&[(2, -2), (0, 2)]));
}

#[test]
fn ant_moves_anywhere_around_the_outside() {
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wA1", -1, 0)], 4);
    assert_eq!(
        dests(&g, "wA1"),
        set(&[(0, -1), (1, -1), (2, -1), (2, 0), (1, 1), (0, 1), (-1, 1)])
    );
}

#[test]
fn grasshopper_jumps_straight_over_pieces() {
    let g = game(
        &[("wG1", 0, 0), ("bQ", 1, 0), ("bA1", 2, 0), ("wQ", 0, 1)],
        4,
    );
    // East over two pieces to (3,0); south-east over wQ to (0,2).
    assert_eq!(dests(&g, "wG1"), set(&[(3, 0), (0, 2)]));
}

#[test]
fn grasshopper_ignores_gates() {
    // From inside the gated hole the grasshopper still jumps out.
    let g = game(
        &[
            ("bQ", 1, -1),
            ("wQ", 2, -1),
            ("bS1", 2, 0),
            ("wA1", 1, 1),
            ("bG1", 0, 1),
            ("wB1", 0, 0),
            ("wG1", 1, 0),
        ],
        12,
    );
    assert_eq!(
        dests(&g, "wG1"),
        set(&[(3, 0), (3, -2), (1, -2), (-1, 0), (-1, 2), (1, 2)])
    );
}

// ---------------------------------------------------------------- beetle

#[test]
fn beetle_moves_one_step_and_climbs() {
    let g = game(&[("wB1", 0, 0), ("bQ", 1, 0), ("wQ", 0, 1)], 4);
    assert_eq!(dests(&g, "wB1"), set(&[(1, 0), (0, 1), (1, -1), (-1, 1)]));
}

#[test]
fn beetle_on_top_moves_down_anywhere_adjacent() {
    // Beetle on bQ at (1,0), hive wQ (0,0). From height 1 every neighbor is
    // reachable (no gates of height >= 2 around).
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wB1", 1, 0)], 4);
    assert_eq!(
        dests(&g, "wB1"),
        set(&[(2, 0), (2, -1), (1, -1), (0, 0), (0, 1), (1, 1)])
    );
}

#[test]
fn covered_piece_cannot_move_and_stack_has_top_color() {
    // Black queen under a white beetle: black has nothing to do but pass.
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wB1", 1, 0)], 5);
    assert_eq!(g.valid_moves(), vec![Move::Pass]);
}

#[test]
fn beetle_height_gate_when_moving_down() {
    // Beetle on wQ at (0,0) (level 1). Target (1,0) is empty; the shared
    // neighbors (1,-1) and (0,1) are both 2 high -> blocked.
    let blocked = game(
        &[
            ("wQ", 0, 0),
            ("wB1", 0, 0),
            ("bQ", 1, -1),
            ("bB1", 1, -1),
            ("wA1", 0, 1),
            ("bB2", 0, 1),
        ],
        6,
    );
    assert!(!dests(&blocked, "wB1").contains(&(1, 0)));
    assert!(
        dests(&blocked, "wB1").contains(&(1, -1)),
        "climbing onto the gate post is fine"
    );

    // Lower one post to height 1: the gap is passable at level 1.
    let open = game(
        &[
            ("wQ", 0, 0),
            ("wB1", 0, 0),
            ("bQ", 1, -1),
            ("bB1", 1, -1),
            ("wA1", 0, 1),
            ("bB2", -1, 1),
        ],
        6,
    );
    assert!(dests(&open, "wB1").contains(&(1, 0)));
}

#[test]
fn beetle_height_gate_when_climbing() {
    // Ground beetle climbing onto a 1-high piece between two 2-high stacks.
    let g = game(
        &[
            ("wB1", 0, 0),
            ("wA1", 1, 0),
            ("bQ", 1, -1),
            ("bB1", 1, -1),
            ("wQ", 0, 1),
            ("bB2", 0, 1),
        ],
        6,
    );
    let d = dests(&g, "wB1");
    assert!(!d.contains(&(1, 0)));
    assert!(d.contains(&(1, -1)));
    assert!(d.contains(&(0, 1)));
}

#[test]
fn beetle_on_top_ignores_ground_gate() {
    // Ground gate (both shared neighbors 1 high) does not stop a beetle
    // travelling at level 1.
    let g = game(
        &[("wQ", 0, 0), ("wB1", 0, 0), ("bQ", 1, -1), ("bA1", 0, 1)],
        6,
    );
    assert!(dests(&g, "wB1").contains(&(1, 0)));
}

// ------------------------------------------------------------ pass and end

#[test]
fn pass_only_when_nothing_else() {
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0), ("wB1", 1, 0)], 5);
    assert_eq!(g.valid_moves(), vec![Move::Pass]);
    let mut g2 = g.clone();
    g2.play_uhp("pass").unwrap();
    assert_eq!(g2.to_move(), Color::White);
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0)], 2);
    assert!(uhp::parse_move(&g, "pass").is_err());
}

#[test]
fn surrounding_queen_wins() {
    let mut g = game(
        &[
            ("bQ", 0, 0),
            ("wQ", 1, 0),
            ("wA1", 1, -1),
            ("bA1", 0, -1),
            ("wG1", -1, 0),
            ("bG1", -1, 1),
            ("wA2", 1, 1),
        ],
        10,
    );
    assert_eq!(g.state(), GameState::InProgress);
    assert!(dests(&g, "wA2").contains(&(0, 1)));
    g.play_uhp("wA2 bQ\\").unwrap();
    assert_eq!(g.state(), GameState::WhiteWins);
    assert!(g.valid_moves().is_empty());
    g.undo();
    assert_eq!(g.state(), GameState::InProgress);
}

#[test]
fn surrounding_both_queens_is_a_draw() {
    let mut g = game(
        &[
            ("wQ", 0, 0),
            ("bQ", 1, 0),
            ("bA1", 1, -1),
            ("wG1", 0, -1),
            ("bG1", -1, 0),
            ("wG2", -1, 1),
            ("bG2", 2, 0),
            ("wG3", 2, -1),
            ("bA2", 1, 1),
            ("wA1", 0, 2),
        ],
        10,
    );
    assert!(dests(&g, "wA1").contains(&(0, 1)));
    g.apply(Move::Move {
        piece: Piece::parse("wA1").unwrap(),
        from: Hex { q: 0, r: 2 },
        to: Hex { q: 0, r: 1 },
    });
    assert_eq!(g.state(), GameState::Draw);
    assert!(g.valid_moves().is_empty());
}

#[test]
fn queen_surrounded_by_own_pieces_also_loses() {
    let g = game(
        &[
            ("wQ", 0, 0),
            ("wA1", 1, 0),
            ("wA2", 1, -1),
            ("wA3", 0, -1),
            ("wG1", -1, 0),
            ("wG2", -1, 1),
            ("wG3", 0, 1),
            ("bQ", 2, 0),
        ],
        9,
    );
    assert_eq!(g.state(), GameState::BlackWins);
}

// ------------------------------------------------------------ apply / undo

#[test]
fn apply_and_undo_restore_position() {
    let mut g = Game::new(true);
    for s in [
        "wS1", "bS1 wS1-", "wQ -wS1", "bQ bS1-", "wB1 \\wQ", "bA1 bQ/", "wB1 wQ", "bA1 -wB1",
    ] {
        let before = g.clone();
        g.play_uhp(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        let after = g.clone();
        g.undo();
        assert!(g == before, "undo of {s}");
        g.play_uhp(s).unwrap();
        assert!(g == after);
    }
    assert_eq!(
        g.board().stack(Hex { q: -1, r: 0 }).len(),
        2,
        "beetle sits on wQ"
    );
}

// --------------------------------------------------------------------- UHP

#[test]
fn uhp_game_string_round_trip() {
    let mut g = Game::new(true);
    for s in [
        "wS1", "bS1 wS1-", "wQ -wS1", "bQ bS1/", "wB1 \\wQ", "bB1 bQ\\",
    ] {
        g.play_uhp(s).unwrap();
    }
    assert_eq!(g.history_uhp().len(), 6);
    let gs = g.game_string();
    assert!(
        gs.starts_with("Base;InProgress;White[4];wS1;bS1 wS1-;wQ -wS1;"),
        "{gs}"
    );
    let g2 = Game::from_game_string(&gs, true).unwrap();
    assert!(g2 == g);
    assert_eq!(g2.game_string(), gs);
    assert_eq!(Game::new(true).game_string(), "Base;NotStarted;White[1]");
}

#[test]
fn uhp_rejects_bad_moves() {
    let mut g = Game::new(true);
    assert!(g.play_uhp("wQ").is_err(), "tournament rule");
    assert!(g.play_uhp("wX1").is_err());
    g.play_uhp("wS1").unwrap();
    assert!(g.play_uhp("bS1").is_err(), "needs a position");
    assert!(g.play_uhp("bS1 wS2-").is_err(), "reference not on board");
    assert!(g.play_uhp("bS2 wS1-").is_err(), "must place bS1 first");
    g.play_uhp("bS1 wS1-").unwrap();
    assert!(g.play_uhp("wS1 bS1-").is_err(), "no movement before queen");
}

#[test]
fn uhp_beetle_on_top_notation() {
    let mut g = Game::new(false);
    for s in ["wQ", "bQ wQ-", "wB1 -wQ", "bA1 bQ-"] {
        g.play_uhp(s).unwrap();
    }
    let m = g.play_uhp("wB1 wQ").unwrap();
    assert_eq!(m.to(), Some(Hex { q: 0, r: 0 }));
    assert_eq!(g.history_uhp().last().unwrap(), "wB1 wQ");
    // Climbing onto a stack names the top piece; a beetle can also climb a beetle.
    g.play_uhp("bA1 -wB1").unwrap();
    assert_eq!(g.board().stack(Hex { q: 0, r: 0 }).len(), 2);
}
