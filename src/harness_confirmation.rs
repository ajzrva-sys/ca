//! Confirmation is a separate phase. It does not satisfy or weaken development
//! `HarnessPlan` validation.
#![allow(dead_code)]
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationSpec {
    pub phase: String,
    pub family: String,
    pub reserved: Vec<String>,
    pub consumed: BTreeSet<String>,
    pub repetitions: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmationError {
    Shape,
    Reused,
    Exhausted,
    StaleIdentity,
    MissingReceipt,
}

impl ConfirmationSpec {
    pub fn reserve(family: &str, pool: &[&str]) -> Result<Self, ConfirmationError> {
        if pool.len() != 4 {
            return Err(ConfirmationError::Shape);
        }
        Ok(Self {
            phase: "confirmation".into(),
            family: family.into(),
            reserved: pool.iter().take(2).map(|id| (*id).to_string()).collect(),
            consumed: BTreeSet::new(),
            repetitions: 3,
        })
    }

    pub fn validate(&self) -> Result<(), ConfirmationError> {
        if self.phase != "confirmation"
            || self.family.is_empty()
            || self.reserved.len() != 2
            || self.repetitions != 3
            || self.reserved.iter().collect::<BTreeSet<_>>().len() != 2
        {
            return Err(ConfirmationError::Shape);
        }
        Ok(())
    }

    pub fn consume(&mut self, case_id: &str) -> Result<(), ConfirmationError> {
        self.validate()?;
        if !self.reserved.iter().any(|id| id == case_id) {
            return Err(ConfirmationError::Shape);
        }
        if !self.consumed.insert(case_id.to_string()) {
            return Err(ConfirmationError::Reused);
        }
        Ok(())
    }

    pub fn release_unexposed(&mut self) -> Result<(), ConfirmationError> {
        if !self.consumed.is_empty() {
            return Err(ConfirmationError::Reused);
        }
        self.reserved.clear();
        Ok(())
    }
}

pub fn adoption_allowed(
    development_ok: bool,
    confirmation_ok: bool,
    identities_match: bool,
    receipt_present: bool,
) -> Result<(), ConfirmationError> {
    if !receipt_present {
        return Err(ConfirmationError::MissingReceipt);
    }
    if !identities_match {
        return Err(ConfirmationError::StaleIdentity);
    }
    if !development_ok || !confirmation_ok {
        return Err(ConfirmationError::Shape);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_is_not_a_development_plan_and_cases_are_single_use() {
        let mut spec = ConfirmationSpec::reserve("family", &["a", "b", "c", "d"]).unwrap();
        spec.validate().unwrap();
        spec.consume("a").unwrap();
        assert_eq!(spec.consume("a"), Err(ConfirmationError::Reused));
        assert_eq!(
            ConfirmationSpec::reserve("family", &["only"]),
            Err(ConfirmationError::Exhausted).or(Err(ConfirmationError::Shape))
        );
        assert_eq!(
            adoption_allowed(true, false, true, true),
            Err(ConfirmationError::Shape)
        );
        assert_eq!(
            adoption_allowed(true, true, false, true),
            Err(ConfirmationError::StaleIdentity)
        );
        assert_eq!(
            adoption_allowed(true, true, true, false),
            Err(ConfirmationError::MissingReceipt)
        );
        adoption_allowed(true, true, true, true).unwrap();
        let mut fresh = ConfirmationSpec::reserve("family", &["a", "b", "c", "d"]).unwrap();
        fresh.release_unexposed().unwrap();
        assert!(fresh.reserved.is_empty());
    }

    #[tokio::test]
    async fn new_cycle_cannot_activate_promote_or_enter_guidance_without_confirmation() {
        let provider = crate::harness_budget::tests::frozen();
        let (plan, _, _) = crate::harness_trials::tests::fixture_plan(&provider, &"a".repeat(64));
        let root = tempfile::tempdir().unwrap();
        let vault = codex_team::self_improvement::LearningVault::open_at(root.path().join("vault"))
            .unwrap();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        let activation = crate::harness_activation::prepare(vault.clone(), &plan.id)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            activation.contains("confirmation is required"),
            "{activation}"
        );
        let promotion =
            super::super::harness_promotion::promote(vault.clone(), &plan.id, root.path())
                .await
                .unwrap_err()
                .to_string();
        assert!(
            promotion.contains("confirmation is required"),
            "{promotion}"
        );
        let guidance = crate::harness_guidance::promote(vault, &plan.id, root.path(), root.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(guidance.contains("confirmation is required"), "{guidance}");
    }
}
