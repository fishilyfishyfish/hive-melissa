//! Colors, bug kinds and piece identifiers (base game only).

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    pub fn opponent(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Color::White => "White",
            Color::Black => "Black",
        }
    }
    pub fn prefix(self) -> char {
        match self {
            Color::White => 'w',
            Color::Black => 'b',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bug {
    Queen,
    Spider,
    Beetle,
    Grasshopper,
    Ant,
}

impl Bug {
    pub const ALL: [Bug; 5] = [
        Bug::Queen,
        Bug::Spider,
        Bug::Beetle,
        Bug::Grasshopper,
        Bug::Ant,
    ];

    pub fn letter(self) -> char {
        match self {
            Bug::Queen => 'Q',
            Bug::Spider => 'S',
            Bug::Beetle => 'B',
            Bug::Grasshopper => 'G',
            Bug::Ant => 'A',
        }
    }
    pub fn from_letter(c: char) -> Option<Bug> {
        Bug::ALL.into_iter().find(|b| b.letter() == c)
    }
    /// Number of copies per player in the base game.
    pub fn count(self) -> u8 {
        match self {
            Bug::Queen => 1,
            Bug::Spider => 2,
            Bug::Beetle => 2,
            Bug::Grasshopper => 3,
            Bug::Ant => 3,
        }
    }
    /// Offset of this bug's first copy inside a color's block of ids.
    fn offset(self) -> u8 {
        match self {
            Bug::Queen => 0,
            Bug::Spider => 1,
            Bug::Beetle => 3,
            Bug::Grasshopper => 5,
            Bug::Ant => 8,
        }
    }
}

pub const PIECES_PER_COLOR: usize = 11;
pub const NUM_PIECES: usize = 2 * PIECES_PER_COLOR;

/// Piece id: `color * 11 + bug offset + (number - 1)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Piece(pub u8);

impl Piece {
    pub fn new(color: Color, bug: Bug, number: u8) -> Piece {
        debug_assert!(number >= 1 && number <= bug.count());
        Piece(color as u8 * PIECES_PER_COLOR as u8 + bug.offset() + number - 1)
    }

    pub fn all() -> impl Iterator<Item = Piece> {
        (0..NUM_PIECES as u8).map(Piece)
    }

    pub fn color(self) -> Color {
        if (self.0 as usize) < PIECES_PER_COLOR {
            Color::White
        } else {
            Color::Black
        }
    }

    fn local(self) -> u8 {
        self.0 % PIECES_PER_COLOR as u8
    }

    pub fn bug(self) -> Bug {
        match self.local() {
            0 => Bug::Queen,
            1..=2 => Bug::Spider,
            3..=4 => Bug::Beetle,
            5..=7 => Bug::Grasshopper,
            _ => Bug::Ant,
        }
    }

    pub fn number(self) -> u8 {
        self.local() - self.bug().offset() + 1
    }

    /// UHP piece name: `wQ`, `bS2`, ...
    pub fn name(self) -> String {
        let mut s = String::with_capacity(3);
        s.push(self.color().prefix());
        s.push(self.bug().letter());
        if self.bug().count() > 1 {
            s.push((b'0' + self.number()) as char);
        }
        s
    }

    pub fn parse(s: &str) -> Option<Piece> {
        let mut it = s.chars();
        let color = match it.next()? {
            'w' => Color::White,
            'b' => Color::Black,
            _ => return None,
        };
        let bug = Bug::from_letter(it.next()?)?;
        let rest: String = it.collect();
        let number = if bug.count() == 1 {
            if !rest.is_empty() {
                return None;
            }
            1
        } else {
            let n: u8 = rest.parse().ok()?;
            if n < 1 || n > bug.count() {
                return None;
            }
            n
        };
        Some(Piece::new(color, bug, number))
    }
}
