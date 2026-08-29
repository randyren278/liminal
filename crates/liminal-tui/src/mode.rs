//! TUI mode navigation.
//!
//! The enum retains the master-plan names internally, while the operator labels are deliberately
//! plain-language and numbered so the keyboard shortcut and the screen purpose are visible in the
//! same place. REFERENCE is presented as POSE because that is what the screen actually renders;
//! SPECTRAL is presented as LIVE FIELD because new users should not need project lore to navigate.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Spectral,
    Belief,
    Memory,
    FieldNotes,
    Reference,
    Calibration,
}

impl Mode {
    pub const ALL: [Mode; 6] = [
        Mode::Spectral,
        Mode::Belief,
        Mode::Memory,
        Mode::FieldNotes,
        Mode::Reference,
        Mode::Calibration,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Mode::Spectral => "1 LIVE FIELD",
            Mode::Belief => "2 BELIEF",
            Mode::Memory => "3 MEMORY",
            Mode::FieldNotes => "4 NOTES",
            Mode::Reference => "5 POSE",
            Mode::Calibration => "6 CALIBRATE",
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
        assert_eq!(Mode::Calibration.next(), Mode::Spectral);
    }

    #[test]
    fn previous_wraps_around_from_the_first_mode_to_the_last() {
        assert_eq!(Mode::Spectral.previous(), Mode::Calibration);
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

    #[test]
    fn operator_labels_explain_the_shortcuts() {
        assert_eq!(Mode::Spectral.title(), "1 LIVE FIELD");
        assert_eq!(Mode::Reference.title(), "5 POSE");
        assert_eq!(Mode::Calibration.title(), "6 CALIBRATE");
    }
}
