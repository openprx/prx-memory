use crate::mses::{select_candidate, EvolutionStepInput};

#[derive(Debug, Clone)]
pub struct VariantCandidate {
    pub id: String,
    pub score_train: f32,
    pub score_holdout: f32,
    pub cost_penalty: f32,
    pub risk_penalty: f32,
    pub constraints_satisfied: bool,
}

#[derive(Debug, Clone)]
pub struct EvolutionPolicy {
    pub lambda: f32,
    pub mu: f32,
}

impl Default for EvolutionPolicy {
    fn default() -> Self {
        Self {
            lambda: 0.2,
            mu: 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvolutionDecision {
    pub accepted_variant_id: Option<String>,
    pub effective_score: f32,
    pub reason: &'static str,
}

pub struct EvolutionRunner {
    policy: EvolutionPolicy,
}

impl EvolutionRunner {
    pub fn new(policy: EvolutionPolicy) -> Self {
        Self { policy }
    }

    pub fn run_generation(
        &self,
        parent_score: f32,
        candidates: &[VariantCandidate],
    ) -> EvolutionDecision {
        let mut best: Option<(String, f32, &'static str)> = None;

        for candidate in candidates {
            let step = EvolutionStepInput {
                parent_score,
                candidate_score_train: candidate.score_train,
                candidate_score_holdout: candidate.score_holdout,
                cost_penalty: candidate.cost_penalty,
                risk_penalty: candidate.risk_penalty,
                lambda: self.policy.lambda,
                mu: self.policy.mu,
                constraints_satisfied: candidate.constraints_satisfied,
            };

            let decision = select_candidate(&step);
            if !decision.accepted {
                continue;
            }

            match &best {
                Some((_, score, _)) if decision.effective_score <= *score => {}
                _ => {
                    best = Some((
                        candidate.id.clone(),
                        decision.effective_score,
                        decision.reason,
                    ))
                }
            }
        }

        if let Some((id, score, reason)) = best {
            EvolutionDecision {
                accepted_variant_id: Some(id),
                effective_score: score,
                reason,
            }
        } else {
            EvolutionDecision {
                accepted_variant_id: None,
                effective_score: parent_score,
                reason: "no_candidate_accepted",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        id: &str,
        score_train: f32,
        score_holdout: f32,
        cost_penalty: f32,
        risk_penalty: f32,
        constraints_satisfied: bool,
    ) -> VariantCandidate {
        VariantCandidate {
            id: id.to_string(),
            score_train,
            score_holdout,
            cost_penalty,
            risk_penalty,
            constraints_satisfied,
        }
    }

    #[test]
    fn picks_best_accepted_candidate() {
        let runner = EvolutionRunner::new(EvolutionPolicy::default());
        let parent = 0.70;
        let candidates = vec![
            candidate("a", 0.80, 0.78, 0.02, 0.02, true),
            candidate("b", 0.84, 0.82, 0.01, 0.01, true),
            candidate("c", 0.86, 0.66, 0.01, 0.01, true),
        ];

        let out = runner.run_generation(parent, &candidates);
        assert_eq!(out.accepted_variant_id.as_deref(), Some("b"));
    }

    #[test]
    fn rejects_constraint_violations() {
        let runner = EvolutionRunner::new(EvolutionPolicy::default());
        let parent = 0.70;
        let candidates = vec![candidate("a", 0.90, 0.88, 0.01, 0.01, false)];

        let out = runner.run_generation(parent, &candidates);
        assert!(out.accepted_variant_id.is_none());
        assert_eq!(out.reason, "no_candidate_accepted");
    }
}
