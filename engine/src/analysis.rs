//! Move analysis for people: how good a move was, and *why*, in plain
//! Korean sentences built from concrete board facts (queen surroundings,
//! pinned/covered pieces, immediate threats), never from raw eval numbers.

use crate::ai::{SearchLimits, Searcher, WIN};
use crate::game::{articulation_points, piece_dests, Game, GameState, Move};
use crate::hex::{Hex, E, NE, NW, SE, SW, W};
use crate::piece::{Bug, Color, Piece};
use crate::uhp::move_to_string;

// --------------------------------------------------------------------------
// Words
// --------------------------------------------------------------------------

pub fn color_ko(c: Color) -> &'static str {
    match c {
        Color::White => "백",
        Color::Black => "흑",
    }
}

fn bug_ko(b: Bug) -> &'static str {
    match b {
        Bug::Queen => "여왕벌",
        Bug::Spider => "거미",
        Bug::Beetle => "딱정벌레",
        Bug::Grasshopper => "메뚜기",
        Bug::Ant => "개미",
    }
}

/// Piece name without color: "개미 2", "여왕벌".
fn bug_name(p: Piece) -> String {
    if p.bug().count() > 1 {
        format!("{} {}", bug_ko(p.bug()), p.number())
    } else {
        bug_ko(p.bug()).to_string()
    }
}

/// "흑 개미 2", "백 여왕벌".
pub fn piece_ko(p: Piece) -> String {
    format!("{} {}", color_ko(p.color()), bug_name(p))
}

/// The part of `word` a particle attaches to by sound: a trailing
/// "(...)" is not read ("여왕벌 옆(오른쪽 위)" → "여왕벌 옆").
fn spoken(word: &str) -> &str {
    match (word.ends_with(')'), word.rfind('(')) {
        (true, Some(i)) => word[..i].trim_end(),
        _ => word,
    }
}

/// Whether the word ends in a final consonant (받침), for choosing particles.
fn has_batchim(word: &str) -> bool {
    match spoken(word).chars().last() {
        Some(c @ '가'..='힣') => !(c as u32 - '가' as u32).is_multiple_of(28),
        // Digits as read in Korean: 일 이 삼 사 오 육 칠 팔 구 영.
        Some(d @ '0'..='9') => matches!(d, '0' | '1' | '3' | '6' | '7' | '8'),
        _ => false,
    }
}

/// Direction particle 으로/로 (ㄹ 받침 also takes 로).
fn euro(word: &str) -> String {
    let rieul = match spoken(word).chars().last() {
        Some(c @ '가'..='힣') => (c as u32 - '가' as u32) % 28 == 8,
        Some(d) => matches!(d, '1' | '7' | '8'),
        None => false,
    };
    if rieul || !has_batchim(word) {
        format!("{word}로")
    } else {
        format!("{word}으로")
    }
}

/// Appends the right particle: `josa("개미 2", "을", "를")` → "개미 2를".
pub fn josa(word: &str, with_batchim: &str, without: &str) -> String {
    format!(
        "{word}{}",
        if has_batchim(word) {
            with_batchim
        } else {
            without
        }
    )
}

fn list_ko(pieces: &[Piece], with_color: bool) -> String {
    pieces
        .iter()
        .map(|&p| if with_color { piece_ko(p) } else { bug_name(p) })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Screen direction of `dir` as drawn on the board (pointy-top hexes).
fn dir_ko(dir: usize) -> &'static str {
    match dir {
        E => "오른쪽",
        W => "왼쪽",
        NE => "오른쪽 위",
        NW => "왼쪽 위",
        SE => "오른쪽 아래",
        SW => "왼쪽 아래",
        _ => "옆",
    }
}

// --------------------------------------------------------------------------
// Board facts
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    /// Can move somewhere.
    Free,
    /// Holds the hive together: removing it would split it.
    Pinned,
    /// A beetle sits on it.
    Covered,
    /// On top and not pinned, but has nowhere to go (blocked in).
    Stuck,
    /// Its side has not placed the queen yet, so nothing of it can move.
    Waiting,
}

struct Facts {
    queen: [Option<Hex>; 2],
    /// Occupied sides around each queen.
    ring: [usize; 2],
    queen_free: [bool; 2],
    status: Vec<(Piece, Status)>,
    /// Movable pieces per color.
    free: [usize; 2],
    /// Pieces per color that could step next to the opponent queen next
    /// move (not counting ones already touching it).
    reach: [usize; 2],
    /// Empty cells where each color could place a new piece.
    spots: [usize; 2],
}

impl Facts {
    fn of(g: &Game) -> Facts {
        let b = g.board();
        let pinned = articulation_points(b);
        let queen = [
            b.position(Piece::new(Color::White, Bug::Queen, 1)),
            b.position(Piece::new(Color::Black, Bug::Queen, 1)),
        ];
        let ring = queen.map(|q| {
            q.map_or(0, |q| {
                q.neighbors().iter().filter(|&&n| b.is_occupied(n)).count()
            })
        });
        let mut status = Vec::new();
        let mut free = [0; 2];
        let mut reach = [0; 2];
        let mut queen_free = [false; 2];
        let mut dests = Vec::new();
        for p in Piece::all() {
            let Some(h) = b.position(p) else { continue };
            let ci = p.color() as usize;
            dests.clear();
            let s = if !b.is_on_top(p) {
                Status::Covered
            } else if queen[ci].is_none() {
                Status::Waiting
            } else if b.height(h) == 1 && pinned.contains(&h) {
                Status::Pinned
            } else {
                piece_dests(b, p, h, &mut dests);
                if dests.is_empty() {
                    Status::Stuck
                } else {
                    Status::Free
                }
            };
            if s == Status::Free {
                free[ci] += 1;
                if p.bug() == Bug::Queen {
                    queen_free[ci] = true;
                } else if let Some(oq) = queen[1 - ci] {
                    let touching = oq.distance(h) == 1;
                    let gets_there = dests
                        .iter()
                        .any(|&d| !b.is_occupied(d) && oq.distance(d) == 1);
                    if !touching && gets_there {
                        reach[ci] += 1;
                    }
                }
            }
            status.push((p, s));
        }
        Facts {
            queen,
            ring,
            queen_free,
            status,
            free,
            reach,
            spots: drop_spots(g),
        }
    }

    fn status(&self, p: Piece) -> Option<Status> {
        self.status.iter().find(|(x, _)| *x == p).map(|&(_, s)| s)
    }
}

/// Empty cells each color could place on: touching its own pieces only
/// (top pieces decide a stack's color).
fn drop_spots(g: &Game) -> [usize; 2] {
    let b = g.board();
    let mut empty: Vec<Hex> = b
        .occupied_cells()
        .iter()
        .flat_map(|h| h.neighbors())
        .filter(|&n| !b.is_occupied(n))
        .collect();
    empty.sort();
    empty.dedup();
    let mut out = [0; 2];
    for e in empty {
        let mut touches = [false; 2];
        for n in e.neighbors() {
            if let Some(c) = b.top_color(n) {
                touches[c as usize] = true;
            }
        }
        match touches {
            [true, false] => out[0] += 1,
            [false, true] => out[1] += 1,
            _ => {}
        }
    }
    out
}

/// Moves that win on the spot for the side to move.
fn winning_moves(g: &Game) -> Vec<Move> {
    let me = g.to_move();
    let mut out = Vec::new();
    let mut x = g.clone();
    for m in g.valid_moves() {
        x.apply_fast(m);
        let won = matches!(
            (x.state(), me),
            (GameState::WhiteWins, Color::White) | (GameState::BlackWins, Color::Black)
        );
        x.undo();
        if won {
            out.push(m);
        }
    }
    out
}

/// Winning moves `c` would have if it were `c`'s turn now.
fn threats(g: &Game, c: Color) -> Vec<Move> {
    if g.state().is_over() {
        return Vec::new();
    }
    if g.to_move() == c {
        winning_moves(g)
    } else {
        let mut x = g.clone();
        x.apply_fast(Move::Pass);
        winning_moves(&x)
    }
}

// --------------------------------------------------------------------------
// Describing moves
// --------------------------------------------------------------------------

/// Where a move lands, in words: "흑 여왕벌 옆(오른쪽 위)", "흑 개미 1 위",
/// "백 메뚜기 1의 왼쪽 아래".
fn where_ko(g: &Game, m: Move) -> String {
    let b = g.board();
    let me = g.to_move();
    let Some(to) = m.to() else {
        return String::new();
    };
    if let Some(top) = b.top(to) {
        return format!("{} 위", piece_ko(top));
    }
    if b.piece_count() == 0 {
        return "판 가운데".to_string();
    }
    let my_q = b.position(Piece::new(me, Bug::Queen, 1));
    let op_q = b.position(Piece::new(me.opponent(), Bug::Queen, 1));
    // Which side of the queen, so neighboring candidates read differently.
    let side = |q: Option<Hex>| q.and_then(|q| q.dir_to(to)).map(dir_ko);
    if let Some(d) = side(op_q) {
        return format!("{} 여왕벌 옆({d})", color_ko(me.opponent()));
    }
    if let (Some(d), true) = (side(my_q), m.piece().is_some_and(|p| p.bug() != Bug::Queen)) {
        return format!("자기 여왕벌 옆({d})");
    }
    // Otherwise relative to a neighboring piece (not the one moving).
    for dir in [E, W, NE, NW, SE, SW] {
        let reference = to.step((dir + 3) % 6);
        if let Some(r) = b.top(reference) {
            if Some(r) == m.piece() && b.height(reference) == 1 {
                continue;
            }
            return format!("{}의 {}", piece_ko(r), dir_ko(dir));
        }
    }
    "새 자리".to_string()
}

/// Noun phrase for a move: "백 개미 2를 흑 여왕벌 옆으로 옮기기 (wA2 bQ\)".
pub fn describe_move(g: &Game, m: Move) -> String {
    let uhp = move_to_string(g, m);
    match m {
        Move::Pass => "패스 (둘 수 있는 수가 없음)".to_string(),
        Move::Place { piece, .. } => {
            format!(
                "{} {}에 놓기 ({uhp})",
                josa(&piece_ko(piece), "을", "를"),
                where_ko(g, m)
            )
        }
        Move::Move { piece, to, .. } => {
            let verb = if g.board().is_occupied(to) {
                "올리기"
            } else {
                "옮기기"
            };
            format!(
                "{} {} {verb} ({uhp})",
                josa(&piece_ko(piece), "을", "를"),
                euro(&where_ko(g, m))
            )
        }
    }
}

/// Short form for move lists: "백 개미 2 → 흑 여왕벌 옆".
pub fn short_move(g: &Game, m: Move) -> String {
    match m {
        Move::Pass => format!("{} 패스", color_ko(g.to_move())),
        _ => format!("{} → {}", piece_ko(m.piece().unwrap()), where_ko(g, m)),
    }
}

fn reply_phrase(g: &Game, r: Move) -> String {
    let uhp = move_to_string(g, r);
    let piece = josa(&piece_ko(r.piece().unwrap()), "을", "를");
    let (place, verb) = if matches!(r, Move::Place { .. }) {
        (format!("{}에", where_ko(g, r)), "놓는")
    } else {
        (euro(&where_ko(g, r)), "옮기는")
    };
    format!("{piece} {place} {verb} 수({uhp})")
}

// --------------------------------------------------------------------------
// Reasons
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Win,
    Draw,
    Loss,
}

fn result_for(g: &Game, me: Color) -> Option<Outcome> {
    match (g.state(), me) {
        (GameState::WhiteWins, Color::White) | (GameState::BlackWins, Color::Black) => {
            Some(Outcome::Win)
        }
        (GameState::Draw, _) => Some(Outcome::Draw),
        (GameState::WhiteWins, _) | (GameState::BlackWins, _) => Some(Outcome::Loss),
        _ => None,
    }
}

/// Plain-language reasons for what `m` changes, from the mover's side.
/// Most important first.
pub fn reasons(before: &Game, m: Move) -> Vec<String> {
    let me = before.to_move();
    let op = me.opponent();
    let (mi, oi) = (me as usize, op as usize);
    let me_ko = color_ko(me);
    let op_ko = color_ko(op);
    let mut after = before.clone();
    after.apply_fast(m);
    let mut out = Vec::new();

    match result_for(&after, me) {
        Some(Outcome::Win) => return vec![format!("{op_ko} 여왕벌의 여섯 면을 모두 막아 이겼다.")],
        Some(Outcome::Draw) => {
            return vec!["양쪽 여왕벌이 동시에 완전히 둘러싸여 비겼다.".to_string()]
        }
        Some(Outcome::Loss) => {
            return vec!["자기 여왕벌의 마지막 빈칸을 스스로 막아서 졌다.".to_string()]
        }
        None => {}
    }

    // Immediate tactics first: they decide games.
    let op_wins_now = winning_moves(&after);
    if let Some(&r) = op_wins_now.first() {
        out.push(format!(
            "이 수를 두면 {op_ko}이 {}를 둬서 {me_ko} 여왕벌을 완전히 둘러싸 바로 이긴다.",
            reply_phrase(&after, r)
        ));
    }
    let op_threat_before = !threats(before, op).is_empty();
    if op_threat_before && op_wins_now.is_empty() {
        out.push(format!("{op_ko}이 다음 수에 이기려던 위협을 막았다."));
    }
    if op_wins_now.is_empty() && !threats(&after, me).is_empty() {
        out.push(format!(
            "다음 수에 {op_ko} 여왕벌을 완전히 둘러쌀 수 있는 위협을 만들었다. {op_ko}은 이것부터 막아야 한다."
        ));
    }

    let fb = Facts::of(before);
    let fa = Facts::of(&after);

    // Opponent queen.
    if fa.queen[oi].is_some() {
        let (b, a) = (fb.ring[oi], fa.ring[oi]);
        if a > b {
            let tail = if a == 5 {
                " 한 칸만 더 막으면 이긴다."
            } else {
                ""
            };
            out.push(format!(
                "{op_ko} 여왕벌 주변을 한 칸 더 막았다 (6면 중 {a}면).{tail}"
            ));
        } else if a < b {
            out.push(format!(
                "{op_ko} 여왕벌 옆에 있던 말을 빼서 포위가 느슨해졌다 (6면 중 {b}면 → {a}면)."
            ));
        }
        if fb.queen_free[oi] && !fa.queen_free[oi] {
            out.push(format!(
                "{op_ko} 여왕벌이 이제 움직일 수 없다. 포위를 피해 도망칠 수 없게 됐다."
            ));
        } else if !fb.queen_free[oi] && fa.queen_free[oi] {
            out.push(format!("{op_ko} 여왕벌이 다시 움직일 수 있게 풀어 줬다."));
        }
    }

    // Own queen.
    if fa.queen[mi].is_some() {
        let (b, a) = (fb.ring[mi], fa.ring[mi]);
        if fb.queen[mi].is_some() && a > b {
            out.push(format!(
                "자기 여왕벌 옆을 스스로 막았다 (6면 중 {a}면). 여왕벌은 6면이 다 막히면 지기 때문에 그만큼 위험해진다."
            ));
        } else if a < b {
            out.push(format!(
                "자기 여왕벌 주변을 비워 숨 쉴 틈을 만들었다 (6면 중 {b}면 → {a}면)."
            ));
        }
        if fb.queen[mi].is_some() && !fb.queen_free[mi] && fa.queen_free[mi] {
            out.push("자기 여왕벌이 다시 움직일 수 있게 됐다. 위험할 때 피할 수 있다.".to_string());
        } else if fb.queen_free[mi] && !fa.queen_free[mi] {
            out.push("자기 여왕벌이 움직일 수 없게 됐다. 포위당해도 피할 수 없다.".to_string());
        }
    } else if let Move::Place { piece, .. } = m {
        if piece.bug() == Bug::Queen {
            out.push("여왕벌을 판에 놓았다. 이제부터 말을 움직일 수 있다.".to_string());
        }
    }

    // Pieces locked or released (queens are covered by the lines above,
    // except a beetle landing on one).
    let mut locked: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    let mut blocked: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    let mut covered: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    let mut released: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
    for &(p, sa) in &fa.status {
        if Some(p) == m.piece() {
            continue;
        }
        let ci = p.color() as usize;
        let sb = fb.status(p);
        if sa == Status::Waiting || sb == Some(Status::Waiting) {
            continue;
        }
        let was_free = sb == Some(Status::Free);
        if sa == Status::Covered && sb != Some(Status::Covered) {
            covered[ci].push(p);
        } else if p.bug() == Bug::Queen {
            // Reported through the queen lines.
        } else if sa == Status::Pinned && was_free {
            locked[ci].push(p);
        } else if sa == Status::Stuck && was_free {
            blocked[ci].push(p);
        } else if sa == Status::Free && sb.is_some() && !was_free {
            released[ci].push(p);
        }
    }
    for p in &covered[oi] {
        if p.bug() == Bug::Queen {
            out.push(format!(
                "딱정벌레로 {op_ko} 여왕벌 위에 올라탔다. 그 여왕벌은 움직일 수 없고, 그 칸은 {me_ko} 칸으로 쳐서 {op_ko}이 그 주변에 말을 놓기 어려워진다."
            ));
        } else {
            out.push(format!(
                "딱정벌레로 {} 위에 올라타 그 말을 움직이지 못하게 눌렀다.",
                piece_ko(*p)
            ));
        }
    }
    if !locked[oi].is_empty() {
        out.push(format!(
            "{} 움직일 수 없게 묶었다. 벌집을 이어 주는 자리라 빠지면 벌집이 끊어지기 때문이다.",
            josa(&list_ko(&locked[oi], true), "을", "를")
        ));
    }
    if !blocked[oi].is_empty() {
        out.push(format!(
            "{} 갈 곳이 없게 가뒀다 (주변이 막혀 빠져나갈 틈이 없다).",
            josa(&list_ko(&blocked[oi], true), "을", "를")
        ));
    }
    if !released[mi].is_empty() {
        out.push(format!(
            "자기 {} 다시 움직일 수 있게 됐다.",
            josa(&list_ko(&released[mi], false), "이", "가")
        ));
    }
    if !locked[mi].is_empty() {
        out.push(format!(
            "자기 {} 움직일 수 없게 됐다. 벌집을 이어 주는 자리가 돼서 빠지면 벌집이 끊어진다.",
            josa(&list_ko(&locked[mi], false), "이", "가")
        ));
    }
    if !blocked[mi].is_empty() {
        out.push(format!(
            "자기 {} 주변이 막혀 갈 곳이 없어졌다.",
            josa(&list_ko(&blocked[mi], false), "이", "가")
        ));
    }
    if !released[oi].is_empty() {
        out.push(format!(
            "{} 다시 움직일 수 있게 풀어 줬다.",
            josa(&list_ko(&released[oi], true), "을", "를")
        ));
    }

    // Attack potential.
    if fa.queen[oi].is_some() && fa.reach[mi] >= fb.reach[mi] + 2 {
        out.push(format!(
            "다음 수에 {op_ko} 여왕벌 옆으로 갈 수 있는 자기 말이 {}개에서 {}개로 늘었다.",
            fb.reach[mi], fa.reach[mi]
        ));
    }

    // Room to place new pieces (matters most while hands are full). Only
    // from the third move on, when the "own color only" rule applies.
    if before.ply() >= 2 {
        let (b, a) = (fb.spots[mi], fa.spots[mi]);
        if !before.hand(me).is_empty() && a + 2 <= b {
            out.push(format!(
                "자기 말을 새로 놓을 수 있는 자리가 {b}곳에서 {a}곳으로 줄었다."
            ));
        } else if !before.hand(me).is_empty() && a >= b + 2 {
            out.push(format!(
                "자기 말을 새로 놓을 수 있는 자리가 {b}곳에서 {a}곳으로 늘었다."
            ));
        }
        let (b, a) = (fb.spots[oi], fa.spots[oi]);
        if !before.hand(op).is_empty() && a + 2 <= b {
            out.push(format!(
                "{op_ko}이 새 말을 놓을 수 있는 자리를 {b}곳에서 {a}곳으로 줄였다."
            ));
        }
    }

    // Queen deadline: own fourth move at the latest.
    if fa.queen[mi].is_none() && before.turn_number() == 3 {
        out.push(
            "아직 여왕벌을 놓지 않았다. 다음 자기 차례에는 규칙상 반드시 여왕벌을 놓아야 한다."
                .to_string(),
        );
    }

    if out.is_empty() {
        out.push(match m {
            Move::Place { piece, .. } if before.ply() < 2 => format!(
                "{} 첫 말로 내놓았다. 아직 판이 작아 차이는 앞으로 놓일 말들에서 생긴다.",
                josa(bug_ko(piece.bug()), "을", "를")
            ),
            Move::Place { .. } => {
                "손패에서 말을 하나 더 판에 내놓았다. 당장 큰 변화는 없다.".to_string()
            }
            Move::Move { .. } => "말의 자리를 바꿨다. 당장 눈에 띄는 변화는 없다.".to_string(),
            Move::Pass => "둘 수 있는 수가 없어 차례를 넘겼다.".to_string(),
        });
    }
    out
}

/// Why `best` beats `played`, as direct comparisons of the resulting
/// positions ("자기 여왕벌 주변이 2면이 아니라 1면만 막힌 상태로 남는다").
pub fn compare(before: &Game, played: Move, best: Move) -> Vec<String> {
    let me = before.to_move();
    let op = me.opponent();
    let (mi, oi) = (me as usize, op as usize);
    let op_ko = color_ko(op);
    let mut ap = before.clone();
    ap.apply_fast(played);
    let mut ab = before.clone();
    ab.apply_fast(best);
    let mut out = Vec::new();

    match result_for(&ab, me) {
        Some(Outcome::Win) => return vec!["그 수를 두면 바로 이긴다.".to_string()],
        Some(Outcome::Draw) if result_for(&ap, me) == Some(Outcome::Loss) => {
            return vec!["그 수를 두면 지지 않고 비긴다.".to_string()];
        }
        _ => {}
    }
    let wins_p = winning_moves(&ap);
    let wins_b = winning_moves(&ab);
    if !wins_p.is_empty() && wins_b.is_empty() {
        out.push(format!("그 수는 {op_ko}에게 바로 이기는 수를 주지 않는다."));
    }
    let threat_p = wins_p.is_empty() && !threats(&ap, me).is_empty();
    let threat_b = wins_b.is_empty() && !threats(&ab, me).is_empty();
    if threat_b && !threat_p {
        out.push(format!(
            "그 수는 다음 수에 {op_ko} 여왕벌을 완전히 둘러쌀 수 있는 위협을 만든다."
        ));
    }

    let fp = Facts::of(&ap);
    let fbst = Facts::of(&ab);
    if fbst.queen[oi].is_some() && fbst.ring[oi] > fp.ring[oi] {
        out.push(format!(
            "{op_ko} 여왕벌 주변을 {}면이 아니라 {}면까지 막는다.",
            fp.ring[oi], fbst.ring[oi]
        ));
    }
    if fbst.queen[mi].is_some() && fbst.ring[mi] < fp.ring[mi] {
        out.push(format!(
            "자기 여왕벌 주변이 {}면이 아니라 {}면만 막힌 상태로 남는다.",
            fp.ring[mi], fbst.ring[mi]
        ));
    }
    if fbst.queen[mi].is_some() && fbst.queen_free[mi] && !fp.queen_free[mi] {
        out.push("자기 여왕벌이 움직일 수 있는 상태로 남는다.".to_string());
    }
    if fbst.queen[oi].is_some() && !fbst.queen_free[oi] && fp.queen_free[oi] {
        out.push(format!("{op_ko} 여왕벌을 움직이지 못하게 만든다."));
    }
    if fbst.queen[oi].is_some() && fbst.reach[mi] > fp.reach[mi] {
        out.push(format!(
            "다음 수에 {op_ko} 여왕벌 옆으로 갈 수 있는 자기 말이 {}개가 아니라 {}개다.",
            fp.reach[mi], fbst.reach[mi]
        ));
    }
    if fbst.queen[mi].is_some() && fbst.reach[oi] < fp.reach[oi] {
        out.push(format!(
            "{op_ko}이 다음 수에 {} 여왕벌 옆으로 붙일 수 있는 말이 {}개가 아니라 {}개뿐이다.",
            color_ko(me),
            fp.reach[oi],
            fbst.reach[oi]
        ));
    }
    if fbst.free[oi] + 1 < fp.free[oi] {
        out.push(format!(
            "{op_ko}이 움직일 수 있는 말이 {}개가 아니라 {}개로 줄어든다.",
            fp.free[oi], fbst.free[oi]
        ));
    }
    if fbst.free[mi] > fp.free[mi] + 1 {
        out.push(format!(
            "자기 말 중 움직일 수 있는 것이 {}개가 아니라 {}개다.",
            fp.free[mi], fbst.free[mi]
        ));
    }
    if out.is_empty() {
        out.push(
            "당장 판에서 보이는 차이는 크지 않다. 엔진이 몇 수 앞까지 읽어 보면 이쪽이 더 나은 결과로 이어진다 (아래 예상 수순 참고)."
                .to_string(),
        );
    }
    out
}

// --------------------------------------------------------------------------
// Judging moves
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Best,
    Good,
    Fine,
    Inaccuracy,
    Mistake,
    Blunder,
    MissedWin,
}

impl Quality {
    pub fn ko(self) -> &'static str {
        match self {
            Quality::Best => "최선의 수",
            Quality::Good => "좋은 수",
            Quality::Fine => "무난한 수",
            Quality::Inaccuracy => "부정확한 수",
            Quality::Mistake => "실수",
            Quality::Blunder => "큰 실수",
            Quality::MissedWin => "이기는 수를 놓침",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Quality::Best => "best",
            Quality::Good => "good",
            Quality::Fine => "fine",
            Quality::Inaccuracy => "inaccuracy",
            Quality::Mistake => "mistake",
            Quality::Blunder => "blunder",
            Quality::MissedWin => "missed-win",
        }
    }
    /// Whether the alternative is worth presenting as "a better move".
    pub fn shows_alternative(self) -> bool {
        matches!(
            self,
            Quality::Inaccuracy | Quality::Mistake | Quality::Blunder | Quality::MissedWin
        )
    }
}

fn is_win(score: i32) -> bool {
    score > WIN - 1000
}

fn is_loss(score: i32) -> bool {
    score < -WIN + 1000
}

/// Own turns until the forced result, for a mate score.
fn mate_moves(score: i32) -> i32 {
    ((WIN - score.abs()) + 1) / 2
}

/// "백이 약간 유리하다" from a White-perspective score.
pub fn standing_ko(score_white: i32) -> String {
    let side = if score_white > 0 { "백" } else { "흑" };
    let s = score_white.abs();
    if is_win(s) {
        let n = mate_moves(s);
        if n <= 1 {
            format!("{side}이 다음 수에 이긴다")
        } else {
            format!("{side}이 {n}수 안에 이길 수 있다 (상대가 어떻게 막아도)")
        }
    } else if s < 40 {
        "형세가 비슷하다".to_string()
    } else if s < 150 {
        format!("{side}이 약간 유리하다")
    } else if s < 400 {
        format!("{side}이 유리하다")
    } else {
        format!("{side}이 크게 유리하다")
    }
}

/// Graph value in [-1, 1] (White advantage) for a White-perspective score.
pub fn advantage(score_white: i32) -> f64 {
    if is_win(score_white) {
        1.0
    } else if is_loss(score_white) {
        -1.0
    } else {
        2.0 / (1.0 + (-(score_white as f64) / 250.0).exp()) - 1.0
    }
}

pub struct MoveReport {
    pub mover: Color,
    pub played: Move,
    pub played_uhp: String,
    pub played_desc: String,
    pub best: Move,
    pub best_uhp: String,
    pub best_desc: String,
    /// Scores from the mover's point of view.
    pub played_score: i32,
    pub best_score: i32,
    pub depth: u32,
    pub quality: Quality,
    pub summary: String,
    pub reasons: Vec<String>,
    /// Why the best move is better than the played one (empty if same).
    pub best_reasons: Vec<String>,
    /// Expected continuation after the best move, in short Korean.
    pub pv: Vec<String>,
    /// Score after the played move, from White's point of view (for graphs).
    pub after_white: i32,
}

/// Analyzes `played` in `before`. `time_ms` bounds the main search.
pub fn analyze_move(
    before: &Game,
    played: Move,
    searcher: &mut Searcher,
    time_ms: f64,
) -> MoveReport {
    let me = before.to_move();
    let r = searcher.search(
        before,
        &SearchLimits {
            time_ms,
            ..Default::default()
        },
    );
    let best = r.best;
    let best_score = r.score;
    let depth = r.depth.max(1);

    let played_score = if played == best {
        best_score
    } else {
        let mut after = before.clone();
        after.apply_fast(played);
        let s = if let Some(t) = terminal(&after, me) {
            t
        } else if depth <= 1 {
            -crate::ai::evaluate(&after, &searcher.params)
        } else {
            let child = searcher.search(
                &after,
                &SearchLimits {
                    max_depth: depth - 1,
                    time_ms: time_ms * 3.0,
                    ..Default::default()
                },
            );
            -child.score
        };
        // A shallower re-search can come out higher than the root; never
        // call the played move better than the best one.
        s.min(best_score)
    };

    let loss = best_score - played_score;
    let quality = if played == best || loss <= 10 {
        Quality::Best
    } else if is_win(best_score) && !is_win(played_score) {
        Quality::MissedWin
    } else if is_loss(played_score) && !is_loss(best_score) {
        Quality::Blunder
    } else if loss < 40 {
        Quality::Good
    } else if loss < 100 {
        Quality::Fine
    } else if loss < 250 {
        Quality::Inaccuracy
    } else if loss < 600 {
        Quality::Mistake
    } else {
        Quality::Blunder
    };

    let white = |s: i32| if me == Color::White { s } else { -s };
    let (sb, sp) = (
        standing_ko(white(best_score)),
        standing_ko(white(played_score)),
    );
    let summary = match quality {
        Quality::MissedWin => format!(
            "이 국면에서는 {}수 안에 이기는 길이 있었다. 둔 뒤에는 {sp}.",
            mate_moves(best_score)
        ),
        _ if sb == sp || !quality.shows_alternative() => format!("둔 뒤 {sp}."),
        _ => format!("제대로 뒀다면 {sb}. 이 수를 둔 뒤에는 {sp}."),
    };

    let mut pv_game = before.clone();
    let mut pv = Vec::new();
    for &m in &r.pv {
        pv.push(short_move(&pv_game, m));
        pv_game.apply_fast(m);
    }

    let op_ko = color_ko(me.opponent());
    let mut best_reasons = if played == best {
        Vec::new()
    } else {
        compare(before, played, best)
    };
    if played != best && is_win(best_score) && !is_win(played_score) {
        // A forced win beats any board-feature comparison; say it plainly.
        best_reasons.retain(|r| !r.starts_with("당장 판에서 보이는 차이는 크지 않다"));
        let n = mate_moves(best_score);
        let line = if n <= 1 {
            "그 수를 두면 바로 이긴다.".to_string()
        } else {
            format!(
                "그 수부터 두면 {op_ko}이 어떻게 막아도 {n}수 안에 {op_ko} 여왕벌을 완전히 둘러쌀 수 있다 (아래 예상 수순)."
            )
        };
        if !best_reasons.contains(&line) {
            best_reasons.insert(0, line);
        }
    }
    let mut played_reasons = reasons(before, played);
    if is_loss(played_score)
        && !is_loss(best_score)
        && !played_reasons.iter().any(|r| r.contains("바로 이긴다"))
    {
        let n = mate_moves(played_score);
        played_reasons.insert(
            0,
            format!(
                "이 수 뒤로는 {op_ko}이 정확히 두면 {n}수 안에 {} 여왕벌이 완전히 둘러싸인다.",
                color_ko(me)
            ),
        );
    }

    MoveReport {
        mover: me,
        played,
        played_uhp: move_to_string(before, played),
        played_desc: describe_move(before, played),
        best,
        best_uhp: move_to_string(before, best),
        best_desc: describe_move(before, best),
        played_score,
        best_score,
        depth,
        quality,
        summary,
        reasons: played_reasons,
        best_reasons,
        pv,
        after_white: white(played_score),
    }
}

/// Score of a finished game from `me`'s point of view, if finished.
fn terminal(g: &Game, me: Color) -> Option<i32> {
    let s = match g.state() {
        GameState::Draw => 0,
        GameState::WhiteWins => WIN - 1,
        GameState::BlackWins => -(WIN - 1),
        _ => return None,
    };
    Some(if me == Color::White { s } else { -s })
}

// --------------------------------------------------------------------------
// Candidate moves (study board)
// --------------------------------------------------------------------------

/// Piece id, position relative to the shape's corner, stack level.
pub type ShapeKey = Vec<(u8, i32, i32, u8)>;

/// Position shape up to rotation, reflection and translation: moves that
/// lead to the same shape are the same move for analysis (all six spots
/// next to the first piece, for example).
pub fn shape_key(g: &Game) -> ShapeKey {
    let b = g.board();
    let pieces: Vec<(u8, Hex, u8)> = Piece::all()
        .filter_map(|p| {
            let h = b.position(p)?;
            let level = b.stack(h).iter().position(|&x| x == p).unwrap_or(0);
            Some((p.0, h, level as u8))
        })
        .collect();
    let mut best: Option<ShapeKey> = None;
    for t in 0..12 {
        let moved: Vec<(u8, Hex, u8)> = pieces
            .iter()
            .map(|&(id, h, l)| {
                let mut x = if t >= 6 { Hex { q: h.r, r: h.q } } else { h };
                for _ in 0..t % 6 {
                    x = Hex {
                        q: -x.r,
                        r: x.q + x.r,
                    };
                }
                (id, x, l)
            })
            .collect();
        let origin = moved
            .iter()
            .map(|&(_, h, _)| h)
            .min()
            .unwrap_or(Hex::ORIGIN);
        let key: ShapeKey = moved
            .iter()
            .map(|&(id, h, l)| (id, h.q - origin.q, h.r - origin.r, l))
            .collect();
        if best.as_ref().is_none_or(|b| key < *b) {
            best = Some(key);
        }
    }
    best.unwrap_or_default()
}

/// What a piece does, for placing it (rules, not opinions).
pub fn piece_note(bug: Bug) -> &'static str {
    match bug {
        Bug::Queen => "여왕벌은 한 번에 한 칸만 움직인다. 이 말의 여섯 면이 모두 막히면 진다.",
        Bug::Spider => "거미는 벌집 둘레를 따라 정확히 세 칸 움직인다. 갈 수 있는 자리가 몇 곳으로 정해져 있다.",
        Bug::Beetle => "딱정벌레는 한 칸씩 움직이지만 다른 말 위로 올라탈 수 있다. 깔린 말은 움직이지 못한다.",
        Bug::Grasshopper => "메뚜기는 일직선으로 말을 뛰어넘어 첫 빈칸에 내린다. 좁은 틈도 건너뛴다.",
        Bug::Ant => "개미는 벌집 둘레라면 어디로든 갈 수 있다. 가장 자유롭게 움직이는 말이다.",
    }
}

pub struct Candidate {
    pub mv: Move,
    pub uhp: String,
    pub desc: String,
    /// UHP strings of moves with the same resulting shape (including `uhp`).
    pub same: Vec<String>,
    /// From the mover's point of view.
    pub score: i32,
    pub depth: u32,
    /// How it compares with the first candidate, in words.
    pub gap: String,
    pub reasons: Vec<String>,
    /// Why the first candidate is better (only when clearly better).
    pub vs_best: Vec<String>,
    pub note: Option<String>,
    /// The mover's next move in the expected line and what it achieves.
    pub plan: Option<(String, Vec<String>)>,
    pub pv: Vec<String>,
    pub after_white: i32,
}

pub struct Candidates {
    pub to_move: Color,
    /// Best score from White's point of view.
    pub score_white: i32,
    pub notes: Vec<String>,
    pub legal: usize,
    /// Legal moves after merging same-shape ones.
    pub distinct: usize,
    pub list: Vec<Candidate>,
}

fn gap_ko(best: i32, s: i32) -> String {
    let loss = best - s;
    if is_win(s) {
        let n = mate_moves(s);
        return if n <= 1 {
            "바로 이기는 수".to_string()
        } else {
            format!("이 수부터 {n}수 안에 이길 수 있다")
        };
    }
    if is_loss(s) {
        return format!(
            "이 수를 두면 상대가 정확히 둘 때 {}수 안에 진다",
            mate_moves(s)
        );
    }
    if is_win(best) {
        return "1순위는 이기는 수라 이 수는 그보다 못하다".to_string();
    }
    if loss <= 10 {
        "1순위와 사실상 같다"
    } else if loss < 40 {
        "1순위보다 아주 조금 못하다"
    } else if loss < 100 {
        "1순위보다 조금 못하다"
    } else if loss < 250 {
        "1순위보다 분명히 못하다"
    } else {
        "1순위보다 크게 못하다"
    }
    .to_string()
}

/// Reasons that say nothing happened.
fn is_filler(r: &str) -> bool {
    r.contains("당장 큰 변화는 없다")
        || r.contains("당장 눈에 띄는 변화는 없다")
        || r.contains("첫 말로 내놓았다")
        || r.starts_with("둘 수 있는 수가 없어")
}

/// The mover's next move in `pv` (after the reply) and its reasons, if it
/// achieves something visible.
fn plan_of(g: &Game, pv: &[Move]) -> Option<(String, Vec<String>)> {
    if pv.len() < 3 {
        return None;
    }
    let mut x = g.clone();
    for &m in &pv[..2] {
        x.apply_fast(m);
        if x.state().is_over() {
            return None;
        }
    }
    let rs: Vec<String> = reasons(&x, pv[2])
        .into_iter()
        .filter(|r| !is_filler(r))
        .take(2)
        .collect();
    if rs.is_empty() {
        return None;
    }
    Some((describe_move(&x, pv[2]), rs))
}

/// General facts about the position for the side to move.
fn position_notes(g: &Game) -> Vec<String> {
    let me = g.to_move();
    let op = me.opponent();
    let op_ko = color_ko(op);
    let mut out = Vec::new();
    if g.ply() == 0 {
        out.push(
            "판이 비어 있어 첫 말은 어디에 놓아도 모양이 같다. 고를 것은 어떤 말을 먼저 내느냐뿐이다."
                .to_string(),
        );
        if g.tournament() {
            out.push("토너먼트 규칙이라 첫 수에는 여왕벌을 놓을 수 없다.".to_string());
        }
    } else if g.ply() == 1 {
        out.push(
            "첫 말 옆 여섯 자리는 돌려 보면 모두 같은 모양이다. 그래서 후보는 어떤 말을 내느냐로만 갈린다."
                .to_string(),
        );
        if g.tournament() {
            out.push("토너먼트 규칙이라 첫 수에는 여왕벌을 놓을 수 없다.".to_string());
        }
    } else if g.ply() == 2 || g.ply() == 3 {
        out.push(
            "이제부터 새 말은 자기 색 말에만 닿게 놓아야 한다 (상대 말에 닿으면 안 된다)."
                .to_string(),
        );
    }
    let queen_in_hand = g.board().position(g.queen(me)).is_none();
    if queen_in_hand && g.ply() >= 2 {
        if g.turn_number() >= 4 {
            out.push("이번 수에는 규칙상 반드시 여왕벌을 놓아야 한다 (4번째 수까지).".to_string());
        } else {
            out.push(format!(
                "여왕벌을 놓기 전에는 판 위의 자기 말을 움직일 수 없다. 늦어도 {}수 뒤(자기 4번째 수)까지는 놓아야 한다.",
                4 - g.turn_number()
            ));
        }
    }
    if !winning_moves(g).is_empty() {
        out.push("지금 바로 이기는 수가 있다.".to_string());
    } else if !threats(g, op).is_empty() {
        out.push(format!(
            "{op_ko}이 다음 수에 이길 수 있는 수가 있다. 이번 수에 막지 않으면 진다."
        ));
    }
    out
}

/// Score of `m` from the mover's view by searching the position after it.
fn score_after(g: &Game, m: Move, searcher: &mut Searcher, time_ms: f64) -> (i32, Vec<Move>) {
    let me = g.to_move();
    let mut after = g.clone();
    after.apply_fast(m);
    if let Some(t) = terminal(&after, me) {
        return (t, vec![m]);
    }
    let r = searcher.search(
        &after,
        &SearchLimits {
            time_ms,
            ..Default::default()
        },
    );
    let mut pv = vec![m];
    pv.extend(r.pv);
    (-r.score, pv)
}

/// The `n` best moves of the position (same-shape moves merged), each with
/// plain-language reasons. `time_ms` is the total budget.
pub fn candidates(g: &Game, searcher: &mut Searcher, time_ms: f64, n: usize) -> Candidates {
    let me = g.to_move();
    let white = |s: i32| if me == Color::White { s } else { -s };
    let moves = if g.state().is_over() {
        Vec::new()
    } else {
        g.valid_moves()
    };

    // Group legal moves by resulting shape, keeping move order.
    let mut groups: Vec<(ShapeKey, Vec<Move>)> = Vec::new();
    let mut x = g.clone();
    for &m in &moves {
        x.apply_fast(m);
        let k = shape_key(&x);
        x.undo();
        match groups.iter_mut().find(|(gk, _)| *gk == k) {
            Some((_, ms)) => ms.push(m),
            None => groups.push((k, vec![m])),
        }
    }

    let count = n.min(groups.len());
    let per = time_ms / count.max(1) as f64;
    let mut found: Vec<(Move, i32, u32, Vec<Move>, usize)> = Vec::new();
    let mut exclude: Vec<Move> = Vec::new();
    for _ in 0..count {
        let (m, score, depth, pv) = if moves.len() == 1 {
            let (s, pv) = score_after(g, moves[0], searcher, per);
            (moves[0], s, 0, pv)
        } else {
            let r = searcher.search(
                g,
                &SearchLimits {
                    time_ms: per,
                    exclude: exclude.clone(),
                    ..Default::default()
                },
            );
            (r.best, r.score, r.depth, r.pv)
        };
        // The TT can lose the line's tail; re-read it from the child.
        let pv = if pv.len() < 3 && depth >= 3 && moves.len() > 1 {
            score_after(g, m, searcher, per / 4.0).1
        } else {
            pv
        };
        let gi = groups.iter().position(|(_, ms)| ms.contains(&m)).unwrap();
        exclude.extend(groups[gi].1.iter().copied());
        found.push((m, score, depth, pv, gi));
    }
    // Later searches can come out deeper; order by what was found.
    found.sort_by_key(|f| -f.1);

    let best = found.first().map(|f| (f.0, f.1));
    let list = found
        .into_iter()
        .enumerate()
        .map(|(i, (m, score, depth, pv, gi))| {
            let (best_m, best_s) = best.unwrap();
            let vs_best = if i > 0 && best_s - score >= 40 {
                let mut v = compare(g, m, best_m);
                v.retain(|r| !r.starts_with("당장 판에서 보이는 차이는 크지 않다"));
                if v.is_empty() {
                    v.push(
                        "판에서 바로 보이는 차이는 없고, 엔진이 몇 수 앞을 읽어 본 결과 차이가 난다."
                            .to_string(),
                    );
                }
                v
            } else {
                Vec::new()
            };
            let mut pv_game = g.clone();
            let mut pv_ko = Vec::new();
            for &pm in &pv {
                if pv_game.state().is_over() {
                    break;
                }
                pv_ko.push(short_move(&pv_game, pm));
                pv_game.apply_fast(pm);
            }
            Candidate {
                mv: m,
                uhp: move_to_string(g, m),
                desc: describe_move(g, m),
                same: groups[gi].1.iter().map(|&x| move_to_string(g, x)).collect(),
                score,
                depth,
                gap: if i == 0 {
                    if is_win(score) {
                        gap_ko(score, score)
                    } else {
                        "엔진 1순위".to_string()
                    }
                } else {
                    gap_ko(best_s, score)
                },
                reasons: {
                    let mut rs = reasons(g, m);
                    // The best move with no visible effect: say why it is first.
                    if i == 0 && rs.len() == 1 && is_filler(&rs[0]) && !rs[0].contains("첫 말로") {
                        rs[0] = "판에서 바로 보이는 효과는 없지만, 엔진이 몇 수 앞까지 읽어 보면 이 수가 가장 좋은 결과로 이어진다 (예상 수순 참고)."
                            .to_string();
                    }
                    rs
                },
                vs_best,
                note: match m {
                    Move::Place { piece, .. } => Some(piece_note(piece.bug()).to_string()),
                    _ => None,
                },
                plan: plan_of(g, &pv),
                pv: pv_ko,
                after_white: white(score),
            }
        })
        .collect::<Vec<_>>();

    Candidates {
        to_move: me,
        score_white: list.first().map_or(0, |c| c.after_white),
        notes: position_notes(g),
        legal: moves.len(),
        distinct: groups.len(),
        list,
    }
}
