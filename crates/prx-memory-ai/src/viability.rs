#[derive(Debug, Clone)]
pub struct ViabilityInput {
    pub mcp_compatibility: f32,
    pub provider_coverage: f32,
    pub retrieval_quality: f32,
    pub governance_compliance: f32,
    pub operability: f32,
}

#[derive(Debug, Clone)]
pub struct ViabilityScore {
    pub total: f32,
    pub grade: &'static str,
}

pub fn score_viability(input: &ViabilityInput) -> ViabilityScore {
    let clamp = |x: f32| x.clamp(0.0, 1.0);

    let total = 0.25 * clamp(input.mcp_compatibility)
        + 0.20 * clamp(input.provider_coverage)
        + 0.25 * clamp(input.retrieval_quality)
        + 0.20 * clamp(input.governance_compliance)
        + 0.10 * clamp(input.operability);

    let grade = if total >= 0.85 {
        "A"
    } else if total >= 0.75 {
        "B"
    } else if total >= 0.65 {
        "C"
    } else {
        "D"
    };

    ViabilityScore { total, grade }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viability_score_is_weighted() {
        let input = ViabilityInput {
            mcp_compatibility: 1.0,
            provider_coverage: 0.9,
            retrieval_quality: 0.8,
            governance_compliance: 0.95,
            operability: 0.7,
        };

        let score = score_viability(&input);
        assert!(score.total > 0.85);
        assert_eq!(score.grade, "A");
    }
}
