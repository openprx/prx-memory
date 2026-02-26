#[derive(Debug, Clone)]
pub struct MsesState {
    pub goal_alignment: f32,
    pub hypothesis_diversity: f32,
    pub memory_retention: f32,
    pub environment_fidelity: f32,
    pub judge_stability: f32,
    pub constraint_enforcement: f32,
}

#[derive(Debug, Clone)]
pub struct EvolutionStepInput {
    pub parent_score: f32,
    pub candidate_score_train: f32,
    pub candidate_score_holdout: f32,
    pub cost_penalty: f32,
    pub risk_penalty: f32,
    pub lambda: f32,
    pub mu: f32,
    pub constraints_satisfied: bool,
}

#[derive(Debug, Clone)]
pub struct EvolutionStepResult {
    pub accepted: bool,
    pub effective_score: f32,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct EvolvabilityReport {
    pub score: f32,
    pub has_variation: bool,
    pub stable_judge: bool,
    pub selection_pressure: bool,
    pub retention_ready: bool,
    pub constraints_executable: bool,
}

fn c01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

pub fn evaluate_evolvability(state: &MsesState) -> EvolvabilityReport {
    let has_variation = state.hypothesis_diversity > 0.20;
    let stable_judge = state.judge_stability > 0.70 && state.goal_alignment > 0.70;
    let selection_pressure = state.environment_fidelity > 0.60;
    let retention_ready = state.memory_retention > 0.60;
    let constraints_executable = state.constraint_enforcement > 0.80;

    let score = 0.20 * c01(state.goal_alignment)
        + 0.20 * c01(state.hypothesis_diversity)
        + 0.20 * c01(state.judge_stability)
        + 0.15 * c01(state.environment_fidelity)
        + 0.15 * c01(state.memory_retention)
        + 0.10 * c01(state.constraint_enforcement);

    EvolvabilityReport {
        score,
        has_variation,
        stable_judge,
        selection_pressure,
        retention_ready,
        constraints_executable,
    }
}

pub fn select_candidate(step: &EvolutionStepInput) -> EvolutionStepResult {
    if !step.constraints_satisfied {
        return EvolutionStepResult {
            accepted: false,
            effective_score: 0.0,
            reason: "constraint_violation",
        };
    }

    let effective = step.candidate_score_holdout
        - step.lambda * step.cost_penalty
        - step.mu * step.risk_penalty;
    let train_improve = step.candidate_score_train > step.parent_score;
    let holdout_improve = step.candidate_score_holdout > step.parent_score;

    if train_improve && holdout_improve && effective > step.parent_score {
        EvolutionStepResult {
            accepted: true,
            effective_score: effective,
            reason: "accepted_dual_set_improvement",
        }
    } else {
        EvolutionStepResult {
            accepted: false,
            effective_score: effective,
            reason: "rejected_no_generalization",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_if_train_and_holdout_both_improve() {
        let step = EvolutionStepInput {
            parent_score: 0.72,
            candidate_score_train: 0.81,
            candidate_score_holdout: 0.79,
            cost_penalty: 0.02,
            risk_penalty: 0.01,
            lambda: 0.2,
            mu: 0.3,
            constraints_satisfied: true,
        };
        let out = select_candidate(&step);
        assert!(out.accepted);
        assert_eq!(out.reason, "accepted_dual_set_improvement");
    }

    #[test]
    fn rejects_if_only_train_improves() {
        let step = EvolutionStepInput {
            parent_score: 0.72,
            candidate_score_train: 0.80,
            candidate_score_holdout: 0.69,
            cost_penalty: 0.01,
            risk_penalty: 0.01,
            lambda: 0.2,
            mu: 0.2,
            constraints_satisfied: true,
        };
        let out = select_candidate(&step);
        assert!(!out.accepted);
    }
}
