//! TUI mode navigation.
//!
//! Master plan reference: §72 (Native Visual Experience -- SPECTRAL/BELIEF/MEMORY/FIELD NOTES/
//! REFERENCE modes). Per the 2026-08-26 architecture pivot (ROADMAP.md), these modes now live in
//! the Rust TUI as the primary interface, not a native SwiftUI/Metal app.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Spectral,
    Belief,
    Memory,
    FieldNotes,
    Reference,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Spectral,
        Mode::Belief,
        Mode::Memory,
        Mode::FieldNotes,
        Mode::Reference,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Mode::Spectral => "SPECTRAL",
            Mode::Belief => "BELIEF",
            Mode::Memory => "MEMORY",
            Mode::FieldNotes => "FIELD NOTES",
            Mode::Reference => "REFERENCE",
        }
    }

    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|m| m == self).unwrap()
    }

    pub fn from_index(i: usize) -> Mode {
        Self::ALL[i % Self::ALL.len()]
    }

    pub fn next(&self) -> Mode {
        Self::from_index(self.index() + 1)
    }

    pub fn previous(&self) -> Mode {
        Self::from_index(self.index() + Self::ALL.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_wraps_around_from_the_last_mode_to_the_first() {
        assert_eq!(Mode::Reference.next(), Mode::Spectral);
    }

    #[test]
    fn previous_wraps_around_from_the_first_mode_to_the_last() {
        assert_eq!(Mode::Spectral.previous(), Mode::Reference);
    }

    #[test]
    fn next_and_previous_are_inverses_for_every_mode() {
        for mode in Mode::ALL {
            assert_eq!(mode.next().previous(), mode);
        }
    }

    #[test]
    fn from_index_matches_the_all_array_order() {
        for (i, mode) in Mode::ALL.iter().enumerate() {
            assert_eq!(Mode::from_index(i), *mode);
        }
    }
}
