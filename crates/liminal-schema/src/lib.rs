//! Canonical epistemic types shared across Liminal's Rust crates.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §8 (Epistemic Layers), §63-68 (Agent Layer).

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod sensorium;
pub use sensorium::{
    AudioProfile, BluetoothProfile, CameraProfile, SensorState, SensoriumProfile, WifiProfile,
};

/// The four epistemic layers. Every piece of Liminal knowledge belongs to exactly one.
/// §8 hard boundary: no IMAGINED artifact may become evidence for OBSERVED, INFERRED, or
/// INTERPRETED claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EpistemicLayer {
    Observed,
    Inferred,
    Interpreted,
    Imagined,
}

/// A pointer to a piece of supporting evidence and the layer it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub layer: EpistemicLayer,
}

impl Evidence {
    pub fn new(id: impl Into<String>, layer: EpistemicLayer) -> Self {
        Self {
            id: id.into(),
            layer,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("IMAGINED evidence `{0}` cannot support a {1:?} claim")]
    ImaginedEvidenceUsedAsFact(String, EpistemicLayer),
    #[error("agent role {0:?} is not permitted to write {1:?} claims")]
    AgentRoleLayerViolation(AgentRole, EpistemicLayer),
    #[error("OBSERVED claims must cite zero derived evidence (they are direct measurement)")]
    ObservedClaimHasEvidence,
}

/// A claim at a specific epistemic layer, citing the evidence it rests on.
///
/// Construction enforces the §8 hard boundary: IMAGINED evidence can never back a
/// non-IMAGINED claim, and OBSERVED claims (direct measurement) never cite derived evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub layer: EpistemicLayer,
    pub evidence: Vec<Evidence>,
}

impl Claim {
    pub fn new(
        id: impl Into<String>,
        layer: EpistemicLayer,
        evidence: Vec<Evidence>,
    ) -> Result<Self, SchemaError> {
        if layer == EpistemicLayer::Observed && !evidence.is_empty() {
            return Err(SchemaError::ObservedClaimHasEvidence);
        }
        if layer != EpistemicLayer::Imagined {
            if let Some(bad) = evidence
                .iter()
                .find(|e| e.layer == EpistemicLayer::Imagined)
            {
                return Err(SchemaError::ImaginedEvidenceUsedAsFact(
                    bad.id.clone(),
                    layer,
                ));
            }
        }
        Ok(Self {
            id: id.into(),
            layer,
            evidence,
        })
    }
}

/// Interpretation-layer agents, per §63.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Archivist,
    Ethnographer,
    Skeptic,
    Cartographer,
    Poet,
}

impl AgentRole {
    /// Epistemic layers this role is permitted to author claims in.
    /// §64: Archivist states what evidence supports (OBSERVED/INFERRED, minimal interpretation).
    /// §65: Ethnographer proposes interpretations (INTERPRETED only, enters PENDING_INTERPRETATION).
    /// §66: Skeptic renders verdicts on interpretations (INTERPRETED).
    /// §67: Cartographer describes field regions without inventing geometry (INFERRED).
    /// §68: Poet output is always IMAGINED and only IMAGINED.
    pub fn allowed_layers(&self) -> &'static [EpistemicLayer] {
        use EpistemicLayer::*;
        match self {
            AgentRole::Archivist => &[Observed, Inferred],
            AgentRole::Ethnographer => &[Interpreted],
            AgentRole::Skeptic => &[Interpreted],
            AgentRole::Cartographer => &[Inferred],
            AgentRole::Poet => &[Imagined],
        }
    }
}

/// Author a claim on behalf of an agent role, enforcing the role's epistemic boundary
/// (§65: Ethnographer/interpretation agents must never write OBSERVED fact — mutation test #5).
pub fn write_claim(
    role: AgentRole,
    id: impl Into<String>,
    layer: EpistemicLayer,
    evidence: Vec<Evidence>,
) -> Result<Claim, SchemaError> {
    if !role.allowed_layers().contains(&layer) {
        return Err(SchemaError::AgentRoleLayerViolation(role, layer));
    }
    Claim::new(id, layer, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_may_cite_matching_layer_evidence() {
        let ev = Evidence::new("obs_1", EpistemicLayer::Observed);
        let claim = Claim::new("claim_1", EpistemicLayer::Inferred, vec![ev]).unwrap();
        assert_eq!(claim.layer, EpistemicLayer::Inferred);
    }

    #[test]
    fn imagined_evidence_cannot_back_an_interpreted_claim() {
        let ev = Evidence::new("poem_1", EpistemicLayer::Imagined);
        let err = Claim::new("claim_1", EpistemicLayer::Interpreted, vec![ev]).unwrap_err();
        assert!(matches!(err, SchemaError::ImaginedEvidenceUsedAsFact(_, _)));
    }

    #[test]
    fn imagined_claims_may_cite_imagined_evidence() {
        let ev = Evidence::new("poem_1", EpistemicLayer::Imagined);
        let claim = Claim::new("dream_1", EpistemicLayer::Imagined, vec![ev]).unwrap();
        assert_eq!(claim.layer, EpistemicLayer::Imagined);
    }

    #[test]
    fn observed_claims_cannot_cite_evidence() {
        let ev = Evidence::new("obs_1", EpistemicLayer::Observed);
        let err = Claim::new("claim_1", EpistemicLayer::Observed, vec![ev]).unwrap_err();
        assert_eq!(err, SchemaError::ObservedClaimHasEvidence);
    }

    #[test]
    fn ethnographer_cannot_write_observed_claim() {
        let err = write_claim(
            AgentRole::Ethnographer,
            "c1",
            EpistemicLayer::Observed,
            vec![],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            SchemaError::AgentRoleLayerViolation(AgentRole::Ethnographer, EpistemicLayer::Observed)
        ));
    }

    #[test]
    fn ethnographer_can_write_interpreted_claim() {
        let claim = write_claim(
            AgentRole::Ethnographer,
            "c1",
            EpistemicLayer::Interpreted,
            vec![],
        )
        .unwrap();
        assert_eq!(claim.layer, EpistemicLayer::Interpreted);
    }

    #[test]
    fn poet_can_only_write_imagined() {
        assert!(write_claim(AgentRole::Poet, "p1", EpistemicLayer::Imagined, vec![]).is_ok());
        assert!(write_claim(AgentRole::Poet, "p1", EpistemicLayer::Interpreted, vec![]).is_err());
    }
}
