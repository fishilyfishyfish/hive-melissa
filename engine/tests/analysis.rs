//! Move analysis: plain-language reasons and quality labels.

use hive_engine::ai::Searcher;
use hive_engine::analysis::{
    analyze_move, candidates, describe_move, josa, piece_ko, reasons, shape_key, standing_ko,
    Quality,
};
use hive_engine::{Game, Hex, Move, Piece};

fn game(layout: &[(&str, i32, i32)], ply: u32) -> Game {
    Game::from_layout(layout, ply, true).unwrap()
}

fn mv(g: &Game, piece: &str, to: (i32, i32)) -> Move {
    let p = Piece::parse(piece).unwrap();
    let to = Hex { q: to.0, r: to.1 };
    *g.valid_moves()
        .iter()
        .find(|m| m.piece() == Some(p) && m.to() == Some(to))
        .unwrap_or_else(|| panic!("{piece} -> {to:?} not legal"))
}

fn has(rs: &[String], needle: &str) -> bool {
    rs.iter().any(|r| r.contains(needle))
}

#[test]
fn korean_particles_follow_the_last_sound() {
    assert_eq!(
        josa(&piece_ko(Piece::parse("bA2").unwrap()), "을", "를"),
        "흑 개미 2를"
    );
    assert_eq!(
        josa(&piece_ko(Piece::parse("bA1").unwrap()), "을", "를"),
        "흑 개미 1을"
    );
    assert_eq!(
        josa(&piece_ko(Piece::parse("wQ").unwrap()), "이", "가"),
        "백 여왕벌이"
    );
    assert_eq!(
        josa(&piece_ko(Piece::parse("wS1").unwrap()), "이", "가"),
        "백 거미 1이"
    );
    assert_eq!(josa("딱정벌레", "을", "를"), "딱정벌레를");
}

/// Black queen at the origin with five blocked sides; wA2 can fill the sixth.
fn mate_in_one() -> Game {
    game(
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
    )
}

#[test]
fn winning_move_is_explained_as_a_win() {
    let g = mate_in_one();
    let m = mv(&g, "wA2", (0, 1));
    assert_eq!(
        reasons(&g, m),
        vec!["흑 여왕벌의 여섯 면을 모두 막아 이겼다.".to_string()]
    );
    let mut s = Searcher::new(14);
    let r = analyze_move(&g, m, &mut s, 300.0);
    assert_eq!(r.quality, Quality::Best);
}

#[test]
fn missing_a_win_is_called_out_with_the_winning_move() {
    let g = mate_in_one();
    let winning = mv(&g, "wA2", (0, 1));
    let other = *g
        .valid_moves()
        .iter()
        .find(|&&m| m != winning && m.piece() != Some(Piece::parse("wA2").unwrap()))
        .unwrap();
    let mut s = Searcher::new(14);
    let r = analyze_move(&g, other, &mut s, 300.0);
    assert_eq!(r.quality, Quality::MissedWin, "{}", r.summary);
    assert_eq!(r.best, winning);
    assert!(r.summary.contains("이기는 길"), "{}", r.summary);
    assert_eq!(
        r.best_reasons,
        vec!["그 수를 두면 바로 이긴다.".to_string()]
    );
    assert!(r.best_desc.contains("흑 여왕벌 옆"), "{}", r.best_desc);
}

/// Mirror image: Black threatens bA2 -> (0,1) surrounding wQ; White to move.
fn under_threat() -> Game {
    game(
        &[
            ("wQ", 0, 0),
            ("bQ", 1, 0),
            ("bA1", 1, -1),
            ("wA1", 0, -1),
            ("bG1", -1, 0),
            ("wG1", -1, 1),
            ("bA2", 1, 1),
        ],
        10,
    )
}

#[test]
fn ignoring_a_mate_threat_is_a_blunder_with_the_refutation() {
    let g = under_threat();
    let bad = g
        .valid_moves()
        .into_iter()
        .find(|&m| has(&reasons(&g, m), "바로 이긴다"))
        .expect("some move ignores the threat");
    let rs = reasons(&g, bad);
    assert!(rs[0].contains("흑 개미 2를"), "{rs:?}");
    let mut s = Searcher::new(14);
    let r = analyze_move(&g, bad, &mut s, 300.0);
    assert_eq!(r.quality, Quality::Blunder, "{}", r.summary);
}

#[test]
fn stopping_a_mate_threat_is_explained() {
    let g = under_threat();
    let good = g
        .valid_moves()
        .into_iter()
        .find(|&m| has(&reasons(&g, m), "위협을 막았다"))
        .expect("some move defends");
    assert!(!has(&reasons(&g, good), "바로 이긴다"));
}

#[test]
fn beetle_on_the_queen_is_explained() {
    let g = game(&[("wB1", 0, 0), ("bQ", 1, 0), ("wQ", 0, 1)], 4);
    let m = mv(&g, "wB1", (1, 0));
    assert!(
        describe_move(&g, m).starts_with("백 딱정벌레 1을 흑 여왕벌 위로 올리기"),
        "{}",
        describe_move(&g, m)
    );
    assert!(
        has(&reasons(&g, m), "흑 여왕벌 위에 올라탔다"),
        "{:?}",
        reasons(&g, m)
    );
}

#[test]
fn blocking_your_own_queen_is_flagged() {
    let g = game(&[("wQ", 0, 0), ("bQ", 1, 0)], 2);
    let m = mv(&g, "wA1", (-1, 0));
    let rs = reasons(&g, m);
    assert!(has(&rs, "자기 여왕벌 옆을 스스로 막았다"), "{rs:?}");
}

#[test]
fn standings_read_naturally() {
    assert_eq!(standing_ko(0), "형세가 비슷하다");
    assert_eq!(standing_ko(100), "백이 약간 유리하다");
    assert_eq!(standing_ko(-500), "흑이 크게 유리하다");
}

#[test]
fn same_shape_moves_are_merged() {
    // Empty board: four bugs (no queen under tournament rules), one spot.
    let g = Game::new(true);
    let mut s = Searcher::new(14);
    let c = candidates(&g, &mut s, 200.0, 5);
    assert_eq!((c.legal, c.distinct, c.list.len()), (4, 4, 4));
    assert!(c.notes.iter().any(|n| n.contains("여왕벌을 놓을 수 없다")));
    assert!(c.list[0].desc.contains("판 가운데"), "{}", c.list[0].desc);

    // Second move: six spots around the first piece are all the same shape.
    let mut g = Game::new(true);
    g.play_uhp("wS1").unwrap();
    let c = candidates(&g, &mut s, 200.0, 5);
    assert_eq!((c.legal, c.distinct, c.list.len()), (24, 4, 4));
    assert!(c.list.iter().all(|x| x.same.len() == 6));
    let bugs: std::collections::HashSet<_> = c.list.iter().map(|x| &x.uhp[..2]).collect();
    assert_eq!(bugs.len(), 4, "one candidate per bug");
}

#[test]
fn shape_key_ignores_rotation_and_translation() {
    let a = game(&[("wS1", 0, 0), ("bS1", 1, 0), ("wQ", -1, 0)], 3);
    // Rotated by 60 degrees and moved.
    let b = game(&[("wS1", 5, 5), ("bS1", 5, 6), ("wQ", 5, 4)], 3);
    // Bent shape: different.
    let c = game(&[("wS1", 0, 0), ("bS1", 1, 0), ("wQ", 0, -1)], 3);
    assert_eq!(shape_key(&a), shape_key(&b));
    assert_ne!(shape_key(&a), shape_key(&c));
}

#[test]
fn candidates_lead_with_the_win_and_rank_the_rest() {
    let g = mate_in_one();
    let mut s = Searcher::new(14);
    let c = candidates(&g, &mut s, 400.0, 3);
    assert_eq!(c.list.len(), 3);
    assert_eq!(c.list[0].mv, mv(&g, "wA2", (0, 1)));
    assert_eq!(c.list[0].gap, "바로 이기는 수");
    assert!(c.notes.iter().any(|n| n.contains("바로 이기는 수가 있다")));
    // The others are different shapes, ranked below, and explain the gap.
    assert!(c.list[1].score <= c.list[0].score && c.list[2].score <= c.list[1].score);
    assert!(!c.list[1].vs_best.is_empty());
    assert_ne!(c.list[1].uhp, c.list[0].uhp);
}

#[test]
fn pieces_wait_for_the_queen() {
    // Before White's queen is down, nothing white "gets freed" or "locked".
    let mut g = Game::new(true);
    g.play_uhp("wS1").unwrap();
    for m in g.valid_moves() {
        let rs = reasons(&g, m);
        assert!(!has(&rs, "풀어 줬다") && !has(&rs, "묶었다"), "{rs:?}");
    }
    // Third own move without the queen: the deadline is stated.
    let g = Game::from_game_string("Base;wS1;bS1 wS1-;wA1 -wS1;bA1 bS1-", true).unwrap();
    let m = mv(&g, "wG1", (-2, 1));
    assert!(
        has(&reasons(&g, m), "반드시 여왕벌을 놓아야 한다"),
        "{:?}",
        reasons(&g, m)
    );
}
