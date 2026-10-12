//! Game state, legal move generation, apply/undo and game-end detection.

use crate::board::Board;
use crate::hex::Hex;
use crate::piece::{Bug, Color, Piece};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Move {
    Place { piece: Piece, to: Hex },
    Move { piece: Piece, from: Hex, to: Hex },
    Pass,
}

impl Move {
    pub fn piece(self) -> Option<Piece> {
        match self {
            Move::Place { piece, .. } | Move::Move { piece, .. } => Some(piece),
            Move::Pass => None,
        }
    }
    pub fn to(self) -> Option<Hex> {
        match self {
            Move::Place { to, .. } | Move::Move { to, .. } => Some(to),
            Move::Pass => None,
        }
    }
    pub fn from(self) -> Option<Hex> {
        match self {
            Move::Move { from, .. } => Some(from),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameState {
    NotStarted,
    InProgress,
    Draw,
    WhiteWins,
    BlackWins,
}

impl GameState {
    pub fn name(self) -> &'static str {
        match self {
            GameState::NotStarted => "NotStarted",
            GameState::InProgress => "InProgress",
            GameState::Draw => "Draw",
            GameState::WhiteWins => "WhiteWins",
            GameState::BlackWins => "BlackWins",
        }
    }
    pub fn is_over(self) -> bool {
        matches!(
            self,
            GameState::Draw | GameState::WhiteWins | GameState::BlackWins
        )
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    board: Board,
    /// Half-moves played so far (passes included). White moves on even plies.
    ply: u32,
    history: Vec<Move>,
    /// UHP strings of `history`, captured at the time each move was played.
    history_uhp: Vec<String>,
    /// Tournament rule: no queen on a player's first placement.
    tournament: bool,
}

impl Default for Game {
    fn default() -> Self {
        Game::new(true)
    }
}

impl PartialEq for Game {
    fn eq(&self, o: &Self) -> bool {
        self.board == o.board && self.ply == o.ply && self.tournament == o.tournament
    }
}

impl Game {
    pub fn new(tournament: bool) -> Game {
        Game {
            board: Board::new(),
            ply: 0,
            history: Vec::new(),
            history_uhp: Vec::new(),
            tournament,
        }
    }

    /// Builds an arbitrary position without checking legality (for tests and
    /// analysis). `layout` lists `(piece, q, r)`; pieces sharing a cell are
    /// stacked in list order (first is bottom). `ply` sets whose turn it is
    /// and which turn number each player is on.
    pub fn from_layout(
        layout: &[(&str, i32, i32)],
        ply: u32,
        tournament: bool,
    ) -> Result<Game, String> {
        let mut g = Game::new(tournament);
        for &(name, q, r) in layout {
            let p = Piece::parse(name).ok_or_else(|| format!("bad piece {name}"))?;
            if g.board.position(p).is_some() {
                return Err(format!("{name} listed twice"));
            }
            g.board.push(p, Hex { q, r });
        }
        g.ply = ply;
        Ok(g)
    }

    pub fn board(&self) -> &Board {
        &self.board
    }
    pub fn ply(&self) -> u32 {
        self.ply
    }
    pub fn tournament(&self) -> bool {
        self.tournament
    }
    pub fn history(&self) -> &[Move] {
        &self.history
    }
    pub fn history_uhp(&self) -> &[String] {
        &self.history_uhp
    }

    pub fn to_move(&self) -> Color {
        if self.ply.is_multiple_of(2) {
            Color::White
        } else {
            Color::Black
        }
    }

    /// 1-based turn number of the side to move (UHP `White[3]`).
    pub fn turn_number(&self) -> u32 {
        self.ply / 2 + 1
    }

    pub fn queen(&self, c: Color) -> Piece {
        Piece::new(c, Bug::Queen, 1)
    }

    pub fn state(&self) -> GameState {
        let wq = self.board.position(self.queen(Color::White));
        let bq = self.board.position(self.queen(Color::Black));
        let w_dead = wq.is_some_and(|h| self.board.is_surrounded(h));
        let b_dead = bq.is_some_and(|h| self.board.is_surrounded(h));
        match (w_dead, b_dead) {
            (true, true) => GameState::Draw,
            (true, false) => GameState::BlackWins,
            (false, true) => GameState::WhiteWins,
            _ if self.ply == 0 => GameState::NotStarted,
            _ => GameState::InProgress,
        }
    }

    /// Pieces of `c` not yet on the board, in id order.
    pub fn hand(&self, c: Color) -> Vec<Piece> {
        Piece::all()
            .filter(|p| p.color() == c && self.board.position(*p).is_none())
            .collect()
    }

    // ----------------------------------------------------------------------
    // Move generation
    // ----------------------------------------------------------------------

    /// All legal moves for the side to move. Returns `[Pass]` when the side
    /// has nothing else, and an empty list once the game is over.
    pub fn valid_moves(&self) -> Vec<Move> {
        if self.state().is_over() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(128);
        let mut scratch = self.board.clone();
        self.gen_placements(&mut out);
        self.gen_movements(&mut scratch, &mut out);
        if out.is_empty() {
            out.push(Move::Pass);
        }
        out
    }

    /// Next placeable piece of each bug (UHP requires playing copies in order).
    fn placeable_pieces(&self, c: Color) -> Vec<Piece> {
        let turn = self.turn_number();
        let queen_placed = self.board.position(self.queen(c)).is_some();
        let mut out = Vec::new();
        for bug in Bug::ALL {
            if bug == Bug::Queen && turn == 1 && self.tournament {
                continue;
            }
            // Turn 4 without the queen: the queen is the only legal placement.
            if bug != Bug::Queen && turn == 4 && !queen_placed {
                continue;
            }
            if let Some(n) =
                (1..=bug.count()).find(|&n| self.board.position(Piece::new(c, bug, n)).is_none())
            {
                out.push(Piece::new(c, bug, n));
            }
        }
        out
    }

    fn placement_cells(&self, c: Color) -> Vec<Hex> {
        match self.board.piece_count() {
            0 => return vec![Hex::ORIGIN],
            1 => {
                let only = self.board.occupied_cells()[0];
                return only.neighbors().to_vec();
            }
            _ => {}
        }
        let mut out: Vec<Hex> = Vec::new();
        for cell in self.board.occupied_cells() {
            if self.board.top_color(cell) != Some(c) {
                continue;
            }
            for n in cell.neighbors() {
                if self.board.is_occupied(n) || out.contains(&n) {
                    continue;
                }
                let touches_enemy = n
                    .neighbors()
                    .iter()
                    .any(|&m| self.board.top_color(m) == Some(c.opponent()));
                if !touches_enemy {
                    out.push(n);
                }
            }
        }
        out
    }

    fn gen_placements(&self, out: &mut Vec<Move>) {
        let c = self.to_move();
        let pieces = self.placeable_pieces(c);
        if pieces.is_empty() {
            return;
        }
        let cells = self.placement_cells(c);
        for &to in &cells {
            for &piece in &pieces {
                out.push(Move::Place { piece, to });
            }
        }
    }

    fn gen_movements(&self, scratch: &mut Board, out: &mut Vec<Move>) {
        let c = self.to_move();
        if self.board.position(self.queen(c)).is_none() {
            return;
        }
        let pinned = articulation_points(&self.board);
        let mut dests: Vec<Hex> = Vec::with_capacity(64);
        for p in Piece::all().filter(|p| p.color() == c) {
            let Some(from) = self.board.position(p) else {
                continue;
            };
            if !self.board.is_on_top(p) {
                continue; // covered by a beetle
            }
            if self.board.height(from) == 1 && pinned.contains(&from) {
                continue; // one-hive rule
            }
            // Lift the piece so its own cell counts as empty while it travels.
            scratch.pop(from);
            dests.clear();
            match p.bug() {
                Bug::Queen => queen_dests(scratch, from, &mut dests),
                Bug::Spider => spider_dests(scratch, from, &mut dests),
                Bug::Ant => ant_dests(scratch, from, &mut dests),
                Bug::Grasshopper => grasshopper_dests(scratch, from, &mut dests),
                Bug::Beetle => beetle_dests(scratch, from, &mut dests),
            }
            scratch.push(p, from);
            for &to in &dests {
                out.push(Move::Move { piece: p, from, to });
            }
        }
    }

    // ----------------------------------------------------------------------
    // Apply / undo
    // ----------------------------------------------------------------------

    /// Plays a move without legality checks (use `valid_moves` / `play_uhp`
    /// for checked input). Records its UHP string for the game log.
    pub fn apply(&mut self, m: Move) {
        let s = crate::uhp::move_to_string(self, m);
        self.apply_unrecorded(m);
        self.history_uhp.push(s);
    }

    /// Like `apply`, but skips building the UHP string (for search/perft).
    pub fn apply_fast(&mut self, m: Move) {
        self.apply_unrecorded(m);
        self.history_uhp.push(String::new());
    }

    fn apply_unrecorded(&mut self, m: Move) {
        match m {
            Move::Place { piece, to } => self.board.push(piece, to),
            Move::Move { piece, from, to } => {
                let p = self.board.pop(from);
                debug_assert_eq!(p, piece);
                self.board.push(piece, to);
            }
            Move::Pass => {}
        }
        self.history.push(m);
        self.ply += 1;
    }

    /// Takes back the last move. Returns it, or `None` at the start.
    pub fn undo(&mut self) -> Option<Move> {
        let m = self.history.pop()?;
        self.history_uhp.pop();
        match m {
            Move::Place { to, .. } => {
                self.board.pop(to);
            }
            Move::Move { piece, from, to } => {
                let p = self.board.pop(to);
                debug_assert_eq!(p, piece);
                self.board.push(piece, from);
            }
            Move::Pass => {}
        }
        self.ply -= 1;
        Some(m)
    }
}

// --------------------------------------------------------------------------
// Movement helpers. `b` has the moving piece already lifted off `from`.
// --------------------------------------------------------------------------

/// Ground-level slide from `a` in direction `dir`: the target must be empty
/// and exactly one of the two shared neighbors occupied (both occupied = gate
/// too narrow; both empty = the piece would lose contact with the hive).
#[inline]
pub(crate) fn can_slide(b: &Board, a: Hex, dir: usize) -> bool {
    let to = a.step(dir);
    if b.is_occupied(to) {
        return false;
    }
    let (c1, c2) = a.common_neighbors(dir);
    b.is_occupied(c1) != b.is_occupied(c2)
}

fn queen_dests(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    for d in 0..6 {
        if can_slide(b, from, d) {
            out.push(from.step(d));
        }
    }
}

fn spider_dests(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    fn walk(b: &Board, path: &mut [Hex; 4], depth: usize, out: &mut Vec<Hex>) {
        let cur = path[depth];
        for d in 0..6 {
            if !can_slide(b, cur, d) {
                continue;
            }
            let next = cur.step(d);
            if path[..=depth].contains(&next) {
                continue; // no backtracking / revisiting
            }
            if depth == 2 {
                if !out.contains(&next) {
                    out.push(next);
                }
            } else {
                path[depth + 1] = next;
                walk(b, path, depth + 1, out);
            }
        }
    }
    let mut path = [from; 4];
    walk(b, &mut path, 0, out);
}

fn ant_dests(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    let mut seen = vec![from];
    let mut i = 0;
    while i < seen.len() {
        let cur = seen[i];
        i += 1;
        for d in 0..6 {
            if can_slide(b, cur, d) {
                let next = cur.step(d);
                if !seen.contains(&next) {
                    seen.push(next);
                    out.push(next);
                }
            }
        }
    }
}

fn grasshopper_dests(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    for d in 0..6 {
        let mut cur = from.step(d);
        if !b.is_occupied(cur) {
            continue; // must jump over at least one piece
        }
        while b.is_occupied(cur) {
            cur = cur.step(d);
        }
        out.push(cur);
    }
}

/// Beetle: one step in any direction, possibly onto or off a stack.
///
/// Height gate: the beetle travels at level `L = max(height(from) without the
/// beetle, height(to))`. It cannot pass if both shared neighbors are strictly
/// taller than `L`. At ground level (`L == 0`) it must also keep contact, i.e.
/// the ordinary slide rule applies.
fn beetle_dests(b: &Board, from: Hex, out: &mut Vec<Hex>) {
    let ha = b.height(from);
    for d in 0..6 {
        let to = from.step(d);
        let hb = b.height(to);
        let (c1, c2) = from.common_neighbors(d);
        let (h1, h2) = (b.height(c1), b.height(c2));
        let level = ha.max(hb);
        if h1.min(h2) > level {
            continue;
        }
        if level == 0 && h1 == 0 && h2 == 0 {
            continue;
        }
        out.push(to);
    }
}

/// Cells whose removal would split the hive (Tarjan's algorithm over occupied
/// cells). Only meaningful for single-piece stacks: lifting a piece off a
/// taller stack never empties the cell.
pub fn articulation_points(b: &Board) -> Vec<Hex> {
    let cells = b.occupied_cells();
    let n = cells.len();
    let mut result = Vec::new();
    if n < 3 {
        return result;
    }
    let index_of = |h: Hex| cells.iter().position(|&c| c == h);
    let adj: Vec<Vec<usize>> = cells
        .iter()
        .map(|c| {
            c.neighbors()
                .iter()
                .filter_map(|&nb| index_of(nb))
                .collect()
        })
        .collect();

    let mut disc = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut is_ap = vec![false; n];
    let mut timer = 0;
    // Iterative DFS: (node, parent, next adjacency index).
    let mut stack: Vec<(usize, usize, usize)> = vec![(0, usize::MAX, 0)];
    disc[0] = 0;
    low[0] = 0;
    timer += 1;
    let mut root_children = 0;
    while let Some(top) = stack.last_mut() {
        let (u, parent) = (top.0, top.1);
        if top.2 < adj[u].len() {
            let v = adj[u][top.2];
            top.2 += 1;
            if disc[v] == usize::MAX {
                disc[v] = timer;
                low[v] = timer;
                timer += 1;
                if u == 0 {
                    root_children += 1;
                }
                stack.push((v, u, 0));
            } else if v != parent {
                low[u] = low[u].min(disc[v]);
            }
        } else {
            stack.pop();
            if parent != usize::MAX {
                low[parent] = low[parent].min(low[u]);
                if parent != 0 && low[u] >= disc[parent] {
                    is_ap[parent] = true;
                }
            }
        }
    }
    if root_children > 1 {
        is_ap[0] = true;
    }
    for i in 0..n {
        if is_ap[i] {
            result.push(cells[i]);
        }
    }
    result
}
