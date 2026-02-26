pub const SKILL_ID: &str = "prx-memory-governance";
pub const SKILL_MAIN_URI: &str = "prx://skills/prx-memory-governance/SKILL.md";
pub const SKILL_GOVERNANCE_URI: &str =
    "prx://skills/prx-memory-governance/references/memory-governance.md";
pub const SKILL_TAGS_URI: &str = "prx://skills/prx-memory-governance/references/tag-taxonomy.md";

pub const SKILL_MAIN_TEXT: &str = include_str!("../../../skills/prx-memory-governance/SKILL.md");
pub const SKILL_GOVERNANCE_TEXT: &str =
    include_str!("../../../skills/prx-memory-governance/references/memory-governance.md");
pub const SKILL_TAGS_TEXT: &str =
    include_str!("../../../skills/prx-memory-governance/references/tag-taxonomy.md");

#[derive(Debug, Clone, Copy)]
pub struct SkillResource {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
    pub text: &'static str,
}

static SKILL_RESOURCES: [SkillResource; 3] = [
    SkillResource {
        uri: SKILL_MAIN_URI,
        name: "prx-memory-governance/SKILL.md",
        description: "Governance workflow for prx-memory.",
        mime_type: "text/markdown",
        text: SKILL_MAIN_TEXT,
    },
    SkillResource {
        uri: SKILL_GOVERNANCE_URI,
        name: "prx-memory-governance/references/memory-governance.md",
        description: "Allowed/forbidden memory governance rules.",
        mime_type: "text/markdown",
        text: SKILL_GOVERNANCE_TEXT,
    },
    SkillResource {
        uri: SKILL_TAGS_URI,
        name: "prx-memory-governance/references/tag-taxonomy.md",
        description: "Standard tag taxonomy for governed memories.",
        mime_type: "text/markdown",
        text: SKILL_TAGS_TEXT,
    },
];

pub fn resources() -> &'static [SkillResource] {
    &SKILL_RESOURCES
}

pub fn resource_text(uri: &str) -> Option<&'static str> {
    SKILL_RESOURCES
        .iter()
        .find(|resource| resource.uri == uri)
        .map(|resource| resource.text)
}
