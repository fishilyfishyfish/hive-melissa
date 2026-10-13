//! Evaluation and alpha-beta search (iterative deepening, PVS, Zobrist
//! transposition table, killer and history move ordering).

use crate::board::Board;
use crate::game::{ant_reaches, articulation_points, piece_dests, Game, GameState, Move};
use crate::hex::Hex;
use crate::piece::{Bug, Color, Piece, NUM_PIECES};

pub const WIN: i32 = 100_000;
const INF: i32 = 1_000_000;
const MAX_PLY: usize = 128;

// --------------------------------------------------------------------------
// Clock
// --------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod clock {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = Date, js_name = now)]
        fn date_now() -> f64;
    }
    pub fn now_ms() -> f64 {
        date_now()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod clock {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    pub fn now_ms() -> f64 {
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

pub use clock::now_ms;

// --------------------------------------------------------------------------
// Evaluation
// --------------------------------------------------------------------------

/// Evaluation weights. Units are arbitrary ("centi-bugs"); a won game is
/// `WIN`. Kept as a struct so the arena can pit variants against each other.
#[derive(Clone, Debug)]
pub struct EvalParams {
    /// Penalty for having `n` occupied cells around one's own queen.
    pub queen_ring: [i32; 7],
    /// Bonus for a movable piece, by bug (Q, S, B, G, A).
    pub free_piece: [i32; 5],
    /// Bonus per destination for short-range bugs (capped at 6).
    pub per_dest: i32,
    /// Own queen can still move.
    pub queen_free: i32,
    /// Own beetle sitting on top of the opponent queen.
    pub beetle_on_queen: i32,
    /// Per own piece touching the opponent queen.
    pub attacker: i32,
    /// Piece still in hand, by bug (Q, S, B, G, A).
    pub in_hand: [i32; 5],
    /// How much (percent) an own movable piece next to one's own queen is
    /// discounted from the queen-ring count.
    pub own_free_ring_discount: i32,
    /// Count every destination of ants and spiders too (slower, more exact).
    pub full_mobility: bool,
    /// Per own piece (not yet touching the opponent queen) that can move next
    /// to it in one move. 0 disables (and skips the extra move generation).
    pub reach: i32,
}

impl Default for EvalParams {
    fn default() -> Self {
        EvalParams {
            // Tuned by SPSA at depth 4 (examples/tune.rs, 700 iterations,
            // 8400 games); see CLAUDE.md "가중치 자동 튜닝".
            queen_ring: [0, 7, 36, 88, 147, 299, 0],
            free_piece: [0, 14, 29, 5, 41],
            per_dest: 2,
            queen_free: 26,
            beetle_on_queen: 69,
            attacker: 15,
            in_hand: [0, 11, 12, 18, 20],
            own_free_ring_discount: 57,
            full_mobility: false,
            reach: 26,
        }
    }
}

impl EvalParams {
    /// Names of the tunable weights, in `to_vec` order.
    pub const TUNABLE: [&'static str; 19] = [
        "ring1",
        "ring2",
        "ring3",
        "ring4",
        "ring5",
        "freeS",
        "freeB",
        "freeG",
        "freeA",
        "perDest",
        "queenFree",
        "beetleOnQueen",
        "attacker",
        "handS",
        "handB",
        "handG",
        "handA",
        "ringDiscount",
        "reach",
    ];

    /// Tunable weights as a flat vector (see `TUNABLE`).
    pub fn to_vec(&self) -> Vec<i32> {
        let mut v = Vec::with_capacity(Self::TUNABLE.len());
        v.extend_from_slice(&self.queen_ring[1..=5]);
        v.extend_from_slice(&self.free_piece[1..=4]);
        v.extend([
            self.per_dest,
            self.queen_free,
            self.beetle_on_queen,
            self.attacker,
        ]);
        v.extend_from_slice(&self.in_hand[1..=4]);
        v.extend([self.own_free_ring_discount, self.reach]);
        v
    }

    /// Inverse of `to_vec`; values are clamped to their valid ranges.
    pub fn from_vec(v: &[i32]) -> EvalParams {
        assert_eq!(v.len(), Self::TUNABLE.len());
        let mut p = EvalParams::default();
        let nn = |x: i32| x.max(0);
        for (dst, &x) in p.queen_ring[1..=5].iter_mut().zip(&v[0..5]) {
            *dst = nn(x);
        }
        for (dst, &x) in p.free_piece[1..=4].iter_mut().zip(&v[5..9]) {
            *dst = nn(x);
        }
        for (dst, &x) in p.in_hand[1..=4].iter_mut().zip(&v[13..17]) {
            *dst = nn(x);
        }
        p.per_dest = nn(v[9]);
        p.queen_free = nn(v[10]);
        p.beetle_on_queen = nn(v[11]);
        p.attacker = nn(v[12]);
        p.own_free_ring_discount = v[17].clamp(0, 100);
        p.reach = v[18].max(1); // 0 would switch the feature off
        p
    }
}

fn bug_index(b: Bug) -> usize {
    match b {
        Bug::Queen => 0,
        Bug::Spider => 1,
        Bug::Beetle => 2,
        Bug::Grasshopper => 3,
        Bug::Ant => 4,
    }
}

/// Static evaluation from the point of view of the side to move.
/// Terminal positions are not handled here (the search checks them first).
pub fn evaluate(g: &Game, p: &EvalParams) -> i32 {
    let b = g.board();
    let pinned = articulation_points(b);
    let mut score = [0i32; 2];
    let queens = [
        b.position(Piece::new(Color::White, Bug::Queen, 1)),
        b.position(Piece::new(Color::Black, Bug::Queen, 1)),
    ];
    let adjacent = |q: Option<Hex>, h: Hex| q.is_some_and(|q| q.distance(h) == 1);
    let mut dests: Vec<Hex> = Vec::with_capacity(64);
    // Own movable pieces touching one's own queen (they can step away).
    let mut own_free_near_queen = [0i32; 2];

    for c in [Color::White, Color::Black] {
        let ci = c as usize;
        let s = &mut score[ci];
        // Ants of one color mostly share the same outer perimeter, so the
        // first free ant's answer is reused for the others.
        let mut ant_reach: Option<bool> = None;
        if let Some(oq) = queens[1 - ci] {
            if let Some(top) = b.top(oq) {
                if top.color() == c && top.bug() == Bug::Beetle {
                    *s += p.beetle_on_queen;
                }
            }
        }
        for piece in Piece::all().filter(|x| x.color() == c) {
            let Some(from) = b.position(piece) else {
                *s += p.in_hand[bug_index(piece.bug())];
                continue;
            };
            if adjacent(queens[1 - ci], from) {
                *s += p.attacker;
            }
            if !b.is_on_top(piece) || (b.height(from) == 1 && pinned.contains(&from)) {
                continue;
            }
            dests.clear();
            let bug = piece.bug();
            if bug == Bug::Queen {
                piece_dests(b, piece, from, &mut dests);
                if !dests.is_empty() {
                    *s += p.queen_free;
                }
                continue;
            }
            // Attack potential: real destinations for every bug except ants,
            // which use an early-exit walk shared by all ants of this color.
            // Spiders reach at most 3 cells away and beetles 1, so a farther
            // queen cannot be touched next move: skip their walk entirely.
            let range = match bug {
                Bug::Spider => 4,
                Bug::Beetle => 2,
                _ => i32::MAX,
            };
            let wants_reach = p.reach != 0
                && queens[1 - ci].is_some_and(|q| from.distance(q) <= range)
                && !adjacent(queens[1 - ci], from);
            if p.full_mobility
                || (wants_reach && bug == Bug::Spider)
                || !matches!(bug, Bug::Ant | Bug::Spider)
            {
                piece_dests(b, piece, from, &mut dests);
            } else {
                // Cheap mobility test: one free slide is enough.
                one_step_slides(b, from, &mut dests);
            }
            if dests.is_empty() {
                continue;
            }
            *s += p.free_piece[bug_index(bug)];
            if wants_reach {
                let target = |d: Hex| !b.is_occupied(d) && adjacent(queens[1 - ci], d);
                let hits = if bug == Bug::Ant && !p.full_mobility {
                    *ant_reach.get_or_insert_with(|| ant_reaches(b, from, target))
                } else {
                    dests.iter().any(|&d| target(d))
                };
                if hits {
                    *s += p.reach;
                }
            }
            let cap = if bug == Bug::Ant { 30 } else { 6 };
            if p.full_mobility || bug != Bug::Ant && bug != Bug::Spider {
                *s += p.per_dest * dests.len().min(cap) as i32;
            }
            if adjacent(queens[ci], from) {
                own_free_near_queen[ci] += 1;
            }
        }
    }
    for c in [Color::White, Color::Black] {
        let ci = c as usize;
        if let Some(q) = queens[ci] {
            let ring = q.neighbors().iter().filter(|&&n| b.is_occupied(n)).count() as i32;
            // Effective ring in tenths: own free pieces count only partially.
            let eff = (ring * 100 - own_free_near_queen[ci] * p.own_free_ring_discount).max(0);
            let lo = (eff / 100) as usize;
            let frac = eff % 100;
            let lo_v = p.queen_ring[lo.min(6)];
            let hi_v = p.queen_ring[(lo + 1).min(6)];
            score[ci] -= lo_v + (hi_v - lo_v) * frac / 100;
        }
    }
    let me = g.to_move() as usize;
    score[me] - score[1 - me]
}

fn one_step_slides(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    // Queen movement == one ground slide.
    piece_dests(b, Piece::new(Color::White, Bug::Queen, 1), from, out);
}

// --------------------------------------------------------------------------
// Transposition table
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
struct TtEntry {
    key: u64,
    mv: u32,
    score: i32,
    depth: i16,
    bound: Bound,
}

const EMPTY: TtEntry = TtEntry {
    key: 0,
    mv: 0,
    score: 0,
    depth: -1,
    bound: Bound::Exact,
};

/// Compact move code used by the TT and killer slots.
fn encode(m: Move) -> u32 {
    match m {
        Move::Pass => 3 << 29,
        Move::Place { piece, to } => {
            1 << 29 | (piece.0 as u32) << 24 | Board::cell_index(to) as u32
        }
        Move::Move { piece, from, to } => {
            2 << 29
                | (piece.0 as u32) << 24
                | (Board::cell_index(from) as u32) << 12
                | Board::cell_index(to) as u32
        }
    }
}

fn to_tt(score: i32, ply: i32) -> i32 {
    if score > WIN - 1000 {
        score + ply
    } else if score < -WIN + 1000 {
        score - ply
    } else {
        score
    }
}

fn from_tt(score: i32, ply: i32) -> i32 {
    if score > WIN - 1000 {
        score - ply
    } else if score < -WIN + 1000 {
        score + ply
    } else {
        score
    }
}

// --------------------------------------------------------------------------
// Search
// --------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SearchLimits {
    pub max_depth: u32,
    /// Wall-clock budget; the last fully searched depth is used.
    pub time_ms: f64,
    pub max_nodes: u64,
    /// Root moves to leave out (for listing the next-best moves). With any
    /// excluded move the root score is always searched, even for a single
    /// remaining move, and the root is not stored in the TT.
    pub exclude: Vec<Move>,
}

impl Default for SearchLimits {
    fn default() -> Self {
        SearchLimits {
            max_depth: 64,
            time_ms: 1000.0,
            max_nodes: u64::MAX,
            exclude: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best: Move,
    /// Score from the side to move's point of view.
    pub score: i32,
    /// Deepest fully completed iteration.
    pub depth: u32,
    pub nodes: u64,
    pub elapsed_ms: f64,
    pub pv: Vec<Move>,
}

pub struct Searcher {
    pub params: EvalParams,
    /// Disable to compare against plain alpha-beta in tests.
    pub use_tt: bool,
    /// Late move reductions.
    pub lmr: bool,
    /// First move index (0-based) that gets reduced; 4x this gets 2 plies.
    pub lmr_start: usize,
    /// At the root, cap the score of moves that recreate a position from the
    /// game at 0 (a draw), so a winning side looks for progress instead of
    /// shuffling. Root-only on purpose: draw scores inside the tree leak into
    /// the transposition table and measured weaker.
    pub repetition: bool,
    tt: Vec<TtEntry>,
    tt_mask: usize,
    killers: [[u32; 2]; MAX_PLY],
    history: Vec<i32>,
    move_bufs: Vec<Vec<Move>>,
    order_buf: Vec<(i32, Move)>,
    score_bufs: Vec<Vec<i32>>,
    /// Keys of the positions that occurred in the game before the root.
    seen: Vec<u64>,
    nodes: u64,
    deadline: f64,
    max_nodes: u64,
    stop: bool,
}

impl Searcher {
    /// `tt_bits`: log2 of the TT entry count (24 bytes each).
    pub fn new(tt_bits: u32) -> Searcher {
        Searcher {
            params: EvalParams::default(),
            use_tt: true,
            lmr: true,
            lmr_start: 6,
            repetition: true,
            seen: Vec::with_capacity(512),
            tt: vec![EMPTY; 1 << tt_bits],
            tt_mask: (1 << tt_bits) - 1,
            killers: [[0; 2]; MAX_PLY],
            history: vec![0; NUM_PIECES * Board::CELLS],
            move_bufs: (0..MAX_PLY).map(|_| Vec::with_capacity(128)).collect(),
            order_buf: Vec::with_capacity(256),
            score_bufs: (0..MAX_PLY).map(|_| Vec::with_capacity(128)).collect(),
            nodes: 0,
            deadline: f64::INFINITY,
            max_nodes: u64::MAX,
            stop: false,
        }
    }

    pub fn clear(&mut self) {
        self.tt.fill(EMPTY);
        self.history.fill(0);
        self.killers = [[0; 2]; MAX_PLY];
    }

    fn position_key(g: &Game) -> u64 {
        let mut k = g.board().hash();
        if g.to_move() == Color::Black {
            k ^= 0x5bd1_e995_9d2c_5680;
        }
        // Early turns change the legal moves (tournament and turn-4 queen
        // rules), so they are part of the position.
        if g.ply() < 8 {
            k ^= (g.ply() as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        k
    }

    fn check_time(&mut self) {
        if self.nodes >= self.max_nodes || (self.nodes & 511 == 0 && now_ms() >= self.deadline) {
            self.stop = true;
        }
    }

    fn terminal_score(g: &Game, ply: i32) -> Option<i32> {
        let st = g.state();
        let winner = match st {
            GameState::Draw => return Some(0),
            GameState::WhiteWins => Color::White,
            GameState::BlackWins => Color::Black,
            _ => return None,
        };
        Some(if winner == g.to_move() {
            WIN - ply
        } else {
            -(WIN - ply)
        })
    }

    /// Orders `moves` in place: TT move, killers, then heuristics + history.
    /// Fully orders `moves` (used at the root, where every move is searched).
    fn order(&mut self, g: &Game, moves: &mut [Move], tt_move: u32, ply: usize) {
        let mut scores = std::mem::take(&mut self.score_bufs[ply]);
        self.score_moves(g, moves, tt_move, ply, &mut scores);
        let mut keyed = std::mem::take(&mut self.order_buf);
        keyed.clear();
        keyed.extend(scores.iter().copied().zip(moves.iter().copied()));
        // Stable: equal keys keep generation order, so search is deterministic.
        keyed.sort_by_key(|&(s, _)| std::cmp::Reverse(s));
        for (dst, &(_, m)) in moves.iter_mut().zip(keyed.iter()) {
            *dst = m;
        }
        self.order_buf = keyed;
        self.score_bufs[ply] = scores;
    }

    /// Ordering keys: TT move, killers, then heuristics + history.
    fn score_moves(&self, g: &Game, moves: &[Move], tt_move: u32, ply: usize, out: &mut Vec<i32>) {
        let b = g.board();
        let me = g.to_move();
        let my_q = b.position(Piece::new(me, Bug::Queen, 1));
        let op_q = b.position(Piece::new(me.opponent(), Bug::Queen, 1));
        let adj = |q: Option<Hex>, h: Hex| q.is_some_and(|q| q.distance(h) == 1);
        let k = self.killers.get(ply).copied().unwrap_or([0; 2]);
        out.clear();
        out.extend(moves.iter().map(|&m| {
            let code = encode(m);
            if code == tt_move {
                1 << 30
            } else if code == k[0] || code == k[1] {
                1 << 24
            } else {
                let mut s = 0;
                if let (Some(p), Some(to)) = (m.piece(), m.to()) {
                    let from = m.from();
                    let was_on_q = from.is_some_and(|f| adj(op_q, f));
                    if op_q == Some(to) {
                        s += 8000; // beetle onto their queen
                    } else if adj(op_q, to) && !was_on_q {
                        s += 5000;
                    } else if was_on_q && !adj(op_q, to) {
                        s -= 3000;
                    }
                    let was_on_mine = from.is_some_and(|f| adj(my_q, f));
                    if adj(my_q, to) && !was_on_mine && p.bug() != Bug::Queen {
                        s -= 2000;
                    } else if was_on_mine && !adj(my_q, to) {
                        s += 1500;
                    }
                    s += self.history[p.0 as usize * Board::CELLS + Board::cell_index(to)];
                }
                s
            }
        }));
    }

    fn negamax(
        &mut self,
        g: &mut Game,
        depth: i32,
        ply: usize,
        mut alpha: i32,
        mut beta: i32,
    ) -> i32 {
        self.nodes += 1;
        self.check_time();
        if self.stop {
            return 0;
        }
        if let Some(t) = Self::terminal_score(g, ply as i32) {
            return t;
        }
        if depth <= 0 || ply + 1 >= MAX_PLY {
            return evaluate(g, &self.params);
        }

        let key = Self::position_key(g);
        let slot = (key as usize) & self.tt_mask;
        let mut tt_move = 0;
        if self.use_tt {
            let e = self.tt[slot];
            if e.key == key {
                tt_move = e.mv;
                if e.depth as i32 >= depth {
                    let s = from_tt(e.score, ply as i32);
                    match e.bound {
                        Bound::Exact => return s,
                        Bound::Lower => alpha = alpha.max(s),
                        Bound::Upper => beta = beta.min(s),
                    }
                    if alpha >= beta {
                        return s;
                    }
                }
            }
        }

        let mut moves = std::mem::take(&mut self.move_bufs[ply]);
        let mut scores = std::mem::take(&mut self.score_bufs[ply]);
        g.generate_moves(&mut moves);
        self.score_moves(g, &moves, tt_move, ply, &mut scores);

        let alpha0 = alpha;
        let mut best = -INF;
        let mut best_move = moves[0];
        for i in 0..moves.len() {
            // Pick the next move lazily (most nodes cut off after a few
            // moves). Rotating keeps equal keys in generation order, exactly
            // like a stable sort.
            let mut j = i;
            for k in i + 1..scores.len() {
                if scores[k] > scores[j] {
                    j = k;
                }
            }
            moves[i..=j].rotate_right(1);
            scores[i..=j].rotate_right(1);
            let m = moves[i];
            g.apply_fast(m);
            let child = depth - 1;
            let mut score;
            if i == 0 {
                score = -self.negamax(g, child, ply + 1, -beta, -alpha);
            } else {
                // Late moves (after the ordered head of the list) are first
                // searched one ply shallower; a fail-high triggers a re-search.
                let reduce = if self.lmr && depth >= 3 && i >= self.lmr_start {
                    1 + (i >= self.lmr_start * 4) as i32
                } else {
                    0
                };
                score = -self.negamax(g, child - reduce, ply + 1, -alpha - 1, -alpha);
                if reduce > 0 && score > alpha && !self.stop {
                    score = -self.negamax(g, child, ply + 1, -alpha - 1, -alpha);
                }
                if score > alpha && score < beta && !self.stop {
                    score = -self.negamax(g, child, ply + 1, -beta, -alpha);
                }
            }
            g.undo();
            if self.stop {
                self.move_bufs[ply] = moves;
                self.score_bufs[ply] = scores;
                return 0;
            }
            if score > best {
                best = score;
                best_move = m;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                let code = encode(m);
                if !matches!(m, Move::Pass) {
                    let kk = &mut self.killers[ply];
                    if kk[0] != code {
                        kk[1] = kk[0];
                        kk[0] = code;
                    }
                    if let (Some(p), Some(to)) = (m.piece(), m.to()) {
                        let h =
                            &mut self.history[p.0 as usize * Board::CELLS + Board::cell_index(to)];
                        *h = (*h + depth * depth).min(4000);
                    }
                }
                break;
            }
        }
        self.move_bufs[ply] = moves;
        self.score_bufs[ply] = scores;

        if self.use_tt {
            let bound = if best <= alpha0 {
                Bound::Upper
            } else if best >= beta {
                Bound::Lower
            } else {
                Bound::Exact
            };
            let e = &mut self.tt[slot];
            if e.key != key || depth as i16 >= e.depth {
                *e = TtEntry {
                    key,
                    mv: encode(best_move),
                    score: to_tt(best, ply as i32),
                    depth: depth as i16,
                    bound,
                };
            }
        }
        best
    }

    /// Searches the position and returns the best move found.
    /// Panics if the game is already over.
    pub fn search(&mut self, game: &Game, limits: &SearchLimits) -> SearchResult {
        let start = now_ms();
        self.deadline = start + limits.time_ms;
        self.max_nodes = limits.max_nodes;
        self.nodes = 0;
        self.stop = false;
        for h in self.history.iter_mut() {
            *h /= 4;
        }
        let mut g = game.clone();
        let mut root = g.valid_moves();
        assert!(!root.is_empty(), "search called on a finished game");
        let restricted = !limits.exclude.is_empty();
        root.retain(|m| !limits.exclude.contains(m));
        assert!(!root.is_empty(), "every root move excluded");

        // Positions of the game so far, for repetition detection.
        self.seen.clear();
        let mut replay = Game::new(game.tournament());
        if game.history().len() == game.ply() as usize {
            for &m in game.history() {
                self.seen.push(Self::position_key(&replay));
                replay.apply_fast(m);
            }
        }

        let mut result = SearchResult {
            best: root[0],
            score: 0,
            depth: 0,
            nodes: 0,
            elapsed_ms: 0.0,
            pv: vec![root[0]],
        };
        if root.len() == 1 && !restricted {
            result.elapsed_ms = now_ms() - start;
            return result;
        }

        let key = Self::position_key(&g);
        for depth in 1..=limits.max_depth.min(MAX_PLY as u32 - 2) {
            let tt_move = if self.use_tt {
                let e = self.tt[(key as usize) & self.tt_mask];
                if e.key == key {
                    e.mv
                } else {
                    0
                }
            } else {
                0
            };
            let tt_move = if depth == 1 {
                tt_move
            } else {
                encode(result.best)
            };
            self.order(&g, &mut root, tt_move, 0);

            let mut alpha = -INF;
            let beta = INF;
            let mut iter_best: Option<(Move, i32)> = None;
            for (i, &m) in root.iter().enumerate() {
                g.apply_fast(m);
                let repeats = self.repetition && self.seen.contains(&Self::position_key(&g));
                let mut score;
                if i == 0 {
                    score = -self.negamax(&mut g, depth as i32 - 1, 1, -beta, -alpha);
                } else {
                    score = -self.negamax(&mut g, depth as i32 - 1, 1, -alpha - 1, -alpha);
                    if score > alpha && !self.stop {
                        score = -self.negamax(&mut g, depth as i32 - 1, 1, -beta, -alpha);
                    }
                }
                g.undo();
                if self.stop {
                    break;
                }
                if repeats {
                    score = score.min(0);
                }
                if score > alpha {
                    alpha = score;
                    iter_best = Some((m, score));
                }
            }
            match iter_best {
                // A partial iteration only counts if it beat the previous best,
                // which was searched first.
                Some((m, s)) if !self.stop || m != result.best => {
                    result.best = m;
                    result.score = s;
                    if !self.stop {
                        result.depth = depth;
                    }
                }
                _ => {}
            }
            if self.use_tt && !restricted {
                let slot = (key as usize) & self.tt_mask;
                self.tt[slot] = TtEntry {
                    key,
                    mv: encode(result.best),
                    score: to_tt(result.score, 0),
                    depth: result.depth as i16,
                    bound: Bound::Exact,
                };
            }
            if self.stop {
                break;
            }
            // Forced result found: deeper search cannot change it.
            if result.score.abs() > WIN - 1000 && (WIN - result.score.abs()) as u32 <= depth {
                break;
            }
            if now_ms() - start > limits.time_ms * 0.5 {
                break; // the next iteration would almost surely not finish
            }
        }
        result.nodes = self.nodes;
        result.elapsed_ms = now_ms() - start;
        result.pv = self.principal_variation(&g, result.best, result.depth.max(1) as usize);
        result
    }

    fn principal_variation(&self, game: &Game, first: Move, max_len: usize) -> Vec<Move> {
        let mut g = game.clone();
        let mut pv = vec![first];
        g.apply_fast(first);
        while pv.len() < max_len && self.use_tt {
            let key = Self::position_key(&g);
            let e = self.tt[(key as usize) & self.tt_mask];
            if e.key != key {
                break;
            }
            let Some(m) = g.valid_moves().into_iter().find(|&m| encode(m) == e.mv) else {
                break;
            };
            pv.push(m);
            g.apply_fast(m);
        }
        pv
    }

    /// Plain negamax without pruning, for testing the search.
    pub fn minimax(&self, g: &mut Game, depth: i32, ply: i32) -> i32 {
        if let Some(t) = Self::terminal_score(g, ply) {
            return t;
        }
        if depth == 0 {
            return evaluate(g, &self.params);
        }
        let mut best = -INF;
        for m in g.valid_moves() {
            g.apply_fast(m);
            best = best.max(-self.minimax(g, depth - 1, ply + 1));
            g.undo();
        }
        best
    }

    /// Fixed-depth alpha-beta score of the root (for tests).
    pub fn alphabeta_score(&mut self, g: &Game, depth: i32) -> i32 {
        self.stop = false;
        self.deadline = f64::INFINITY;
        self.max_nodes = u64::MAX;
        let mut g = g.clone();
        self.negamax(&mut g, depth, 0, -INF, INF)
    }
}
