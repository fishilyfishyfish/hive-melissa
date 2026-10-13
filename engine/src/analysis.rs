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

/// Whether the word ends in a final consonant (받침), for choosing particles.
fn has_batchim(word: &str) -> bool {
    match word.chars().last() {
        Some(c @ '가'..='힣') => !(c as u32 - '가' as u32).is_multiple_of(28),
        // Digits as read in Korean: 일 이 삼 사 오 육 칠 팔 구 영.
        Some(d @ '0'..='9') => matches!(d, '0' | '1' | '3' | '6' | '7' | '8'),
        _ => false,
    }
}

/// Direction particle 으로/로 (ㄹ 받침 also takes 로).
fn euro(word: &str) -> String {
    let rieul = match word.chars().last() {
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
        }
    }

    fn status(&self, p: Piece) -> Option<Status> {
        self.status.iter().find(|(x, _)| *x == p).map(|&(_, s)| s)
    }
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

/// Where a move lands, in words: "흑 여왕벌 옆", "흑 개미 1 위",
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
    let my_q = b.position(Piece::new(me, Bug::Queen, 1));
    let op_q = b.position(Piece::new(me.opponent(), Bug::Queen, 1));
    let near = |q: Option<Hex>| q.is_some_and(|q| q.distance(to) == 1);
    if near(op_q) {
        return format!("{} 여왕벌 옆", color_ko(me.opponent()));
    }
    if near(my_q) && m.piece().is_some_and(|p| p.bug() != Bug::Queen) {
        return "자기 여왕벌 옆".to_string();
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

    if out.is_empty() {
        out.push(match m {
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
