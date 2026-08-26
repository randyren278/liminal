//! Retention tiers as a pure decision function.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §85 (Retention Defaults), §102 (Memory Decay).
//!
//! §85 lists nine tiers; this module covers the three that are pure-function eligibility
//! decisions over a stored record's age: high-resolution derived observations (7 days), belief
//! frames (30 days), and events (1 year). The remaining six are out of scope here, not silently
//! dropped: `raw camera` and `raw continuous audio` are listed as "never" retained, but this
//! crate never persists raw media in the first place -- there is no record kind for it to apply
//! to -- so no eligibility check is needed for those tiers. `episodes`, `patterns`, and
//! `interpretations` are indefinite/derived-artifact tiers handled by the ledger's cascade-erase
//! logic (§103), not by age-based eligibility.

/// The record kinds this module makes retention decisions for. §85's `raw camera` / `raw
/// continuous audio` tiers have no corresponding variant: this crate never stores raw media, so
/// there is nothing for those "never retained" tiers to gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// §85 "high-resolution derived observations".
    Observation,
    BeliefFrame,
    Event,
}

const MICROS_PER_DAY: i64 = 24 * 60 * 60 * 1_000_000;

/// §85 retention cutoffs, expressed in microseconds so callers can compare directly against
/// microsecond-resolution timestamps without a conversion step at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub observations_us: i64,
    pub belief_frames_us: i64,
    pub events_us: i64,
}

impl RetentionPolicy {
    /// §85 defaults: observations 7 days, belief frames 30 days, events 1 year (365 days).
    pub const DEFAULT: Self = Self {
        observations_us: 7 * MICROS_PER_DAY,
        belief_frames_us: 30 * MICROS_PER_DAY,
        events_us: 365 * MICROS_PER_DAY,
    };
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Whether a record of `kind`, recorded at `recorded_at_us`, is eligible for deletion as of
/// `now_us` under `policy`. Pure function: no filesystem or DB access.
///
/// Boundary behavior: a record exactly `policy`'s cutoff old (`now_us - recorded_at_us ==
/// cutoff`) is eligible. §85's tiers exist to bound how long private data survives, so at the
/// exact boundary this leans toward deleting rather than retaining -- retaining data one instant
/// past its promised cutoff is the more dangerous failure mode for a privacy-facing policy than
/// deleting it one instant early.
pub fn eligible_for_deletion(
    policy: &RetentionPolicy,
    kind: RecordKind,
    recorded_at_us: i64,
    now_us: i64,
) -> bool {
    let cutoff_us = match kind {
        RecordKind::Observation => policy.observations_us,
        RecordKind::BeliefFrame => policy.belief_frames_us,
        RecordKind::Event => policy.events_us,
    };
    now_us - recorded_at_us >= cutoff_us
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_not_eligible_one_second_before_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.observations_us - 1_000_000;
        assert!(!eligible_for_deletion(
            &policy,
            RecordKind::Observation,
            0,
            now_us
        ));
    }

    #[test]
    fn observation_eligible_one_second_after_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.observations_us + 1_000_000;
        assert!(eligible_for_deletion(
            &policy,
            RecordKind::Observation,
            0,
            now_us
        ));
    }

    #[test]
    fn belief_frame_not_eligible_one_second_before_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.belief_frames_us - 1_000_000;
        assert!(!eligible_for_deletion(
            &policy,
            RecordKind::BeliefFrame,
            0,
            now_us
        ));
    }

    #[test]
    fn belief_frame_eligible_one_second_after_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.belief_frames_us + 1_000_000;
        assert!(eligible_for_deletion(
            &policy,
            RecordKind::BeliefFrame,
            0,
            now_us
        ));
    }

    #[test]
    fn event_not_eligible_one_second_before_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.events_us - 1_000_000;
        assert!(!eligible_for_deletion(
            &policy,
            RecordKind::Event,
            0,
            now_us
        ));
    }

    #[test]
    fn event_eligible_one_second_after_cutoff() {
        let policy = RetentionPolicy::DEFAULT;
        let now_us = policy.events_us + 1_000_000;
        assert!(eligible_for_deletion(&policy, RecordKind::Event, 0, now_us));
    }

    /// Documents the chosen boundary behavior: exactly at the cutoff, the record is already
    /// eligible (see the doc comment on `eligible_for_deletion`).
    #[test]
    fn exactly_at_cutoff_is_eligible() {
        let policy = RetentionPolicy::DEFAULT;
        assert!(eligible_for_deletion(
            &policy,
            RecordKind::Observation,
            0,
            policy.observations_us
        ));
    }
}
