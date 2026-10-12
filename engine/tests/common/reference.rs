//! Deliberately naive, independent Hive rules implementation used only to
//! cross-check the engine. Favors obviousness over speed: a HashMap of stacks,
//! connectivity by full BFS after removing a piece (no articulation points),
//! shared neighbors found by brute-force intersection, spider paths by plain
//! enumeration. It shares no code with the engine.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

pub type Cell = (i32, i32);

/// Canonical move: `(piece name, from, to)`; `None` piece means pass.
pub type RefMove = (Option<String>, Option<Cell>, Option<Cell>);

const OFFS: [Cell; 6] = [(0, 1), (1, 0), (-1, 0), (1, -1), (-1, 1), (0, -1)];

fn nbrs(c: Cell) -> Vec<Cell> {
    OFFS.iter().map(|o| (c.0 + o.0, c.1 + o.1)).collect()
}

fn adjacent(a: Cell, b: Cell) -> bool {
    OFFS.contains(&(b.0 - a.0, b.1 - a.1))
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefGame {
    pub cells: HashMap<Cell, Vec<String>>,
    pub ply: u32,
    pub tournament: bool,
}

fn color_of(name: &str) -> char {
    name.chars().next().unwrap()
}

fn bug_of(name: &str) -> char {
    name.chars().nth(1).unwrap()
}

impl RefGame {
    pub fn new(tournament: bool) -> Self {
        RefGame {
            cells: HashMap::new(),
            ply: 0,
            tournament,
        }
    }

    fn me(&self) -> char {
        if self.ply.is_multiple_of(2) {
            'w'
        } else {
            'b'
        }
    }

    fn h(&self, c: Cell) -> usize {
        self.cells.get(&c).map_or(0, |v| v.len())
    }

    fn occ(&self, c: Cell) -> bool {
        self.h(c) > 0
    }

    fn top(&self, c: Cell) -> Option<&String> {
        self.cells.get(&c).and_then(|v| v.last())
    }

    fn find(&self, name: &str) -> Option<Cell> {
        self.cells
            .iter()
            .find(|(_, v)| v.iter().any(|p| p == name))
            .map(|(c, _)| *c)
    }

    fn on_board(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn connected(&self) -> bool {
        let occ: Vec<Cell> = self
            .cells
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(c, _)| *c)
            .collect();
        if occ.is_empty() {
            return true;
        }
        let set: HashSet<Cell> = occ.iter().copied().collect();
        let mut seen = HashSet::new();
        let mut q = VecDeque::new();
        q.push_back(occ[0]);
        seen.insert(occ[0]);
        while let Some(c) = q.pop_front() {
            for n in nbrs(c) {
                if set.contains(&n) && seen.insert(n) {
                    q.push_back(n);
                }
            }
        }
        seen.len() == set.len()
    }

    fn queen_dead(&self, color: char) -> bool {
        let q = format!("{color}Q");
        match self.find(&q) {
            Some(c) => nbrs(c).into_iter().all(|n| self.occ(n)),
            None => false,
        }
    }

    pub fn is_over(&self) -> bool {
        self.queen_dead('w') || self.queen_dead('b')
    }

    /// "w", "b", "draw" or "" while in progress.
    pub fn result(&self) -> &'static str {
        match (self.queen_dead('w'), self.queen_dead('b')) {
            (true, true) => "draw",
            (true, false) => "b",
            (false, true) => "w",
            _ => "",
        }
    }

    fn shared(a: Cell, b: Cell) -> Vec<Cell> {
        nbrs(a).into_iter().filter(|&x| adjacent(x, b)).collect()
    }

    /// One ground slide a -> b on this (already lifted) board.
    fn slide(&self, a: Cell, b: Cell) -> bool {
        if !adjacent(a, b) || self.occ(b) {
            return false;
        }
        let s = Self::shared(a, b);
        assert_eq!(s.len(), 2);
        let n = s.iter().filter(|&&x| self.occ(x)).count();
        n == 1
    }

    pub fn moves(&self) -> BTreeSet<RefMove> {
        let mut out = BTreeSet::new();
        if self.is_over() {
            return out;
        }
        let me = self.me();
        let them = if me == 'w' { 'b' } else { 'w' };
        let turn = self.ply / 2 + 1;
        let queen_down = self.on_board(&format!("{me}Q"));

        // --- placements
        let mut names = Vec::new();
        for (bug, count) in [('Q', 1), ('S', 2), ('B', 2), ('G', 3), ('A', 3)] {
            if bug == 'Q' && turn == 1 && self.tournament {
                continue;
            }
            if bug != 'Q' && turn == 4 && !queen_down {
                continue;
            }
            for n in 1..=count {
                let name = if count == 1 {
                    format!("{me}{bug}")
                } else {
                    format!("{me}{bug}{n}")
                };
                if !self.on_board(&name) {
                    names.push(name);
                    break;
                }
            }
        }
        let occupied: Vec<Cell> = self
            .cells
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(c, _)| *c)
            .collect();
        let mut targets: BTreeSet<Cell> = BTreeSet::new();
        if occupied.is_empty() {
            targets.insert((0, 0));
        } else if occupied.len() == 1 && self.cells.values().map(|v| v.len()).sum::<usize>() == 1 {
            targets.extend(nbrs(occupied[0]));
        } else {
            for &c in &occupied {
                for x in nbrs(c) {
                    if self.occ(x) {
                        continue;
                    }
                    let tops: Vec<char> = nbrs(x)
                        .into_iter()
                        .filter_map(|y| self.top(y))
                        .map(|p| color_of(p))
                        .collect();
                    if tops.contains(&me) && !tops.contains(&them) {
                        targets.insert(x);
                    }
                }
            }
        }
        for name in &names {
            for &t in &targets {
                out.insert((Some(name.clone()), None, Some(t)));
            }
        }

        // --- movements
        if queen_down {
            for &from in &occupied {
                let name = self.top(from).unwrap().clone();
                if color_of(&name) != me {
                    continue;
                }
                let mut lifted = self.clone();
                lifted.cells.get_mut(&from).unwrap().pop();
                if lifted.cells[&from].is_empty() {
                    lifted.cells.remove(&from);
                }
                if !lifted.connected() {
                    continue;
                }
                let dests: BTreeSet<Cell> = match bug_of(&name) {
                    'Q' => nbrs(from)
                        .into_iter()
                        .filter(|&b| lifted.slide(from, b))
                        .collect(),
                    'A' => {
                        let mut reach = BTreeSet::new();
                        reach.insert(from);
                        let mut todo = vec![from];
                        while let Some(c) = todo.pop() {
                            for b in nbrs(c) {
                                if lifted.slide(c, b) && reach.insert(b) {
                                    todo.push(b);
                                }
                            }
                        }
                        reach.remove(&from);
                        reach
                    }
                    'S' => {
                        let mut ends = BTreeSet::new();
                        for a in nbrs(from) {
                            if !lifted.slide(from, a) {
                                continue;
                            }
                            for b in nbrs(a) {
                                if b == from || !lifted.slide(a, b) {
                                    continue;
                                }
                                for c in nbrs(b) {
                                    if c == from || c == a || !lifted.slide(b, c) {
                                        continue;
                                    }
                                    ends.insert(c);
                                }
                            }
                        }
                        ends
                    }
                    'G' => {
                        let mut ends = BTreeSet::new();
                        for o in OFFS {
                            let mut c = (from.0 + o.0, from.1 + o.1);
                            let mut jumped = 0;
                            while lifted.occ(c) {
                                c = (c.0 + o.0, c.1 + o.1);
                                jumped += 1;
                            }
                            if jumped > 0 {
                                ends.insert(c);
                            }
                        }
                        ends
                    }
                    'B' => {
                        let mut ends = BTreeSet::new();
                        for b in nbrs(from) {
                            let ha = lifted.h(from);
                            let hb = lifted.h(b);
                            if ha == 0 && hb == 0 {
                                if lifted.slide(from, b) {
                                    ends.insert(b);
                                }
                                continue;
                            }
                            let top_level = ha.max(hb);
                            let s = Self::shared(from, b);
                            let blocked = s.iter().all(|&x| lifted.h(x) > top_level);
                            if !blocked {
                                ends.insert(b);
                            }
                        }
                        ends
                    }
                    other => panic!("unknown bug {other}"),
                };
                for to in dests {
                    out.insert((Some(name.clone()), Some(from), Some(to)));
                }
            }
        }

        if out.is_empty() {
            out.insert((None, None, None));
        }
        out
    }

    pub fn apply(&mut self, m: &RefMove) {
        match m {
            (None, _, _) => {}
            (Some(name), from, Some(to)) => {
                if let Some(f) = from {
                    let p = self.cells.get_mut(f).unwrap().pop().unwrap();
                    assert_eq!(&p, name);
                    if self.cells[f].is_empty() {
                        self.cells.remove(f);
                    }
                }
                self.cells.entry(*to).or_default().push(name.clone());
            }
            _ => panic!("malformed move"),
        }
        self.ply += 1;
        assert!(self.connected(), "hive split after {m:?}");
    }
}
