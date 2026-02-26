use std::fs;
use std::path::PathBuf;

use prx_memory_ai::{EvolutionPolicy, EvolutionRunner, VariantCandidate};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    parent_score: f32,
    lambda: f32,
    mu: f32,
    candidates: Vec<Candidate>,
    expected_accepted_variant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    id: String,
    score_train: f32,
    score_holdout: f32,
    cost_penalty: f32,
    risk_penalty: f32,
    constraints_satisfied: bool,
}

#[test]
fn holdout_cases_pass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root
        .join("..")
        .join("..")
        .join("data")
        .join("holdout")
        .join("evolution_cases.json");

    let content = fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", fixture.display()));
    let cases: Vec<Case> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", fixture.display()));

    for case in cases {
        let runner = EvolutionRunner::new(EvolutionPolicy {
            lambda: case.lambda,
            mu: case.mu,
        });

        let candidates = case
            .candidates
            .into_iter()
            .map(|c| VariantCandidate {
                id: c.id,
                score_train: c.score_train,
                score_holdout: c.score_holdout,
                cost_penalty: c.cost_penalty,
                risk_penalty: c.risk_penalty,
                constraints_satisfied: c.constraints_satisfied,
            })
            .collect::<Vec<_>>();

        let out = runner.run_generation(case.parent_score, &candidates);
        assert_eq!(
            out.accepted_variant_id, case.expected_accepted_variant_id,
            "case {} failed",
            case.name
        );
    }
}
