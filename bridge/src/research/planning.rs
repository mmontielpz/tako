//! Strict, provider-independent local planning records. No review actor is invented.
use super::state::Result;
use super::{error, model::ResearchType, state};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

macro_rules! dto {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}
dto!(Hash {
    algorithm: String,
    value: String
});
dto!(Reference {
    kind: String,
    id: String,
    version: u64
});
dto!(Hypothesis {
    text: String,
    prediction: String,
    timing: String
});
dto!(MethodRequirement {
    kind: String,
    description: String
});
dto!(Unknown {
    field: String,
    reason: String
});
dto!(EvidenceRequirement {id:String,description:String,input_refs:Vec<Reference>,criterion:String});
dto!(Step {id:String,label:String,method:String,kind:String,input_refs:Vec<Reference>,
    dependencies:Vec<String>,completion_criterion:String,included:bool,unsupported_requirements:Vec<String>});
dto!(Body {
    objective:String,research_type:ResearchType,secondary_types:Vec<ResearchType>,intent:String,
    hypotheses:Vec<Hypothesis>,evaluation_questions:Vec<String>,methodology:String,
    method_requirements:Vec<MethodRequirement>,assumptions:Vec<String>,limitations:Vec<String>,
    data_requirements:Vec<String>,resource_requirements:Vec<String>,steps:Vec<Step>,
    evidence_requirements:Vec<EvidenceRequirement>,validation_criteria:Vec<String>,
    constraints:Vec<String>,risks:Vec<String>,uncertainties:Vec<String>,unknowns:Vec<Unknown>,
    expected_deliverables:Vec<String>
});
dto!(Origin {kind:String,description:String,provider:Option<String>,model:Option<String>});
dto!(PlanInput {question:Reference,selected_evidence:Vec<Reference>,body:Body,origin:Origin});
dto!(Material {reference:Reference,record_digest:Hash,integrity:String,availability:Option<String>,content_state:String,
    independent_verification:String,scientific_assessment:String});
dto!(Payload {input:PlanInput,question_digest:Hash,materials:Vec<Material>});
dto!(PlanRevision {schema_version:u64,record_id:String,project_id:String,plan_id:String,
    revision_id:String,revision_number:u64,created_at:Option<u64>,clock_reason:Option<String>,
    payload_digest:Hash,parent_revision_id:Option<String>,payload:Payload});
// Defined separately; no current native path may create a human decision.
dto!(PlanReviewDecision {schema_version:u64,record_id:String,project_id:String,plan_id:String,
    revision_id:String,revision_number:u64,created_at:Option<u64>,payload_digest:Hash,
    revision_digest:Hash,actor_id:String,decision:String,rationale:String});
dto!(PlanLifecycleEvent {schema_version:u64,record_id:String,project_id:String,plan_id:String,
    revision_id:String,revision_number:u64,created_at:Option<u64>,clock_reason:Option<String>,
    payload_digest:Hash,sequence:u64,operation_id:String,kind:String,revision_digest:Hash});
dto!(Write {schema_version:u64,project_id:String,operation_id:String,expected_project_version:u64,
    plan_id:Option<String>,parent_revision_id:Option<String>,input:PlanInput,revision_reason:String});
dto!(Target {
    schema_version: u64,
    project_id: String,
    plan_id: String,
    revision_id: String,
    expected_revision_digest: Hash,
    operation_id: String,
    expected_project_version: u64
});
dto!(Decision {
    schema_version: u64,
    project_id: String,
    plan_id: String,
    revision_id: String,
    expected_revision_digest: Hash,
    operation_id: String,
    expected_project_version: u64,
    human_review_intent: bool,
    review_rationale: String
});
dto!(Read {schema_version:u64,project_id:String,plan_id:String,revision_id:Option<String>});
dto!(Page {
    schema_version: u64,
    project_id: String,
    plan_id: String,
    cursor: u64,
    limit: u64
});
dto!(Validate {schema_version:u64,project_id:String,plan_id:Option<String>,revision_id:Option<String>,proposal_json:Option<String>});
dto!(Validation {structural_only:bool,reviewable:bool,errors:Vec<String>,warnings:Vec<String>});

pub fn handles(cmd: &str) -> bool {
    matches!(
        cmd,
        "create_plan_draft"
            | "read_plan"
            | "list_plan_revisions"
            | "validate_plan_revision"
            | "submit_plan_for_review"
            | "approve_plan_revision"
            | "reject_plan_revision"
            | "create_plan_revision"
            | "read_plan_review_history"
    )
}
pub(super) fn require(ok: bool) -> Result<()> {
    super::evidence::require(ok)
}
pub(super) fn text(v: &str, max: usize, required: bool) -> Result<()> {
    super::evidence::text(v, max, required)
}
pub(super) fn strings(v: &[String]) -> Result<()> {
    require(v.len() <= 16)?;
    for s in v {
        text(s, 512, true)?;
    }
    Ok(())
}
pub(super) fn hash(value: &impl Serialize, domain: &str) -> Result<Hash> {
    let mut bytes = domain.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(state::canonical(value)?.as_bytes());
    let h = super::evidence::digest(&bytes);
    Ok(Hash {
        algorithm: h.algorithm,
        value: h.value,
    })
}
pub(super) fn reference(r: &Reference) -> Result<()> {
    require(state::id(&r.id) && r.version > 0 && r.version <= super::model::SAFE_INTEGER)?;
    require(matches!(
        r.kind.as_str(),
        "question" | "evidence" | "source" | "artifact"
    ))
}
pub(super) fn scope(schema: u64, pid: &str) -> Result<()> {
    if schema != 3 {
        return Err(error("unsupported_schema"));
    }
    require(state::id(pid))
}
pub(super) fn digest_valid(h: &Hash) -> Result<()> {
    require(
        h.algorithm == "sha256"
            && h.value.len() == 64
            && h.value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
    )
}
pub(super) fn input(p: &PlanInput) -> Result<()> {
    reference(&p.question)?;
    require(p.question.kind == "question")?;
    require(p.selected_evidence.len() <= 16)?;
    let mut selected = BTreeSet::new();
    for r in &p.selected_evidence {
        reference(r)?;
        require(r.kind != "question" && selected.insert(state::canonical(r)?))?;
    }
    let b = &p.body;
    text(&b.objective, 2048, true)?;
    text(&b.methodology, 2048, false)?;
    require(b.secondary_types.len() <= 3)?;
    let kinds: BTreeSet<_> = b.secondary_types.iter().collect();
    require(kinds.len() == b.secondary_types.len() && !kinds.contains(&b.research_type))?;
    require(matches!(b.intent.as_str(), "exploratory" | "confirmatory"))?;
    require(b.hypotheses.len() <= 16)?;
    for h in &b.hypotheses {
        text(&h.text, 512, true)?;
        text(&h.prediction, 512, true)?;
        require(matches!(
            h.timing.as_str(),
            "prespecified" | "post_hoc" | "unknown"
        ))?;
    }
    for list in [
        &b.evaluation_questions,
        &b.assumptions,
        &b.limitations,
        &b.data_requirements,
        &b.resource_requirements,
        &b.validation_criteria,
        &b.constraints,
        &b.risks,
        &b.uncertainties,
        &b.expected_deliverables,
    ] {
        strings(list)?;
    }
    require(!b.limitations.is_empty())?;
    require(b.unknowns.len() <= 16)?;
    let mut fields = BTreeSet::new();
    for u in &b.unknowns {
        require(
            matches!(
                u.field.as_str(),
                "methodology"
                    | "data_requirements"
                    | "resource_requirements"
                    | "evidence_requirements"
                    | "validation_criteria"
                    | "expected_deliverables"
            ) && fields.insert(&u.field),
        )?;
        text(&u.reason, 512, true)?;
    }
    require(b.method_requirements.len() <= 16)?;
    let mut methods = BTreeSet::new();
    for m in &b.method_requirements {
        require(
            matches!(
                m.kind.as_str(),
                "selection"
                    | "appraisal"
                    | "synthesis"
                    | "design"
                    | "measurement"
                    | "controls"
                    | "sampling"
                    | "analysis"
                    | "inputs"
                    | "baseline"
                    | "reproducibility"
                    | "constraints"
                    | "alternatives"
                    | "evaluation"
            ) && methods.insert(&m.kind),
        )?;
        text(&m.description, 512, true)?;
    }
    for (field, missing) in [
        ("methodology", b.methodology.trim().is_empty()),
        ("data_requirements", b.data_requirements.is_empty()),
        ("resource_requirements", b.resource_requirements.is_empty()),
        ("evidence_requirements", b.evidence_requirements.is_empty()),
        ("validation_criteria", b.validation_criteria.is_empty()),
        ("expected_deliverables", b.expected_deliverables.is_empty()),
    ] {
        require(missing == fields.contains(&field.to_owned()))?;
    }
    require(b.steps.len() <= 6)?;
    let mut steps = BTreeSet::new();
    for s in &b.steps {
        require(state::id(&s.id) && !steps.contains(&s.id))?;
        text(&s.label, 960, true)?;
        require(s.label.chars().count() <= 240)?;
        text(&s.method, 2048, true)?;
        text(&s.completion_criterion, 512, true)?;
        require(matches!(
            s.kind.as_str(),
            "literature_review"
                | "experiment_design"
                | "computational_analysis"
                | "feasibility_analysis"
                | "writing"
        ))?;
        require(
            s.dependencies.len() <= 6
                && s.dependencies.iter().collect::<BTreeSet<_>>().len() == s.dependencies.len(),
        )?;
        require(s.dependencies.iter().all(|id| steps.contains(id)))?;
        require(s.input_refs.len() <= 16)?;
        for r in &s.input_refs {
            reference(r)?;
            require(selected.contains(&state::canonical(r)?))?;
        }
        strings(&s.unsupported_requirements)?;
        steps.insert(s.id.clone());
    }
    require(b.evidence_requirements.len() <= 16)?;
    let mut requirements = BTreeSet::new();
    for e in &b.evidence_requirements {
        require(state::id(&e.id) && requirements.insert(&e.id) && e.input_refs.len() <= 16)?;
        text(&e.description, 512, true)?;
        text(&e.criterion, 512, true)?;
        for r in &e.input_refs {
            reference(r)?;
            require(selected.contains(&state::canonical(r)?))?;
        }
    }
    require(matches!(
        p.origin.kind.as_str(),
        "human_authored" | "fixture" | "template" | "external_ai_proposal"
    ))?;
    text(&p.origin.description, 512, true)?;
    for v in [&p.origin.provider, &p.origin.model].into_iter().flatten() {
        text(v, 128, true)?;
    }
    require(
        p.origin.kind == "external_ai_proposal"
            || (p.origin.provider.is_none() && p.origin.model.is_none()),
    )?;
    require(state::canonical(p)?.len() <= 64 * 1024)
}
pub fn review(p: &PlanInput) -> Validation {
    let b = &p.body;
    let mut errors = Vec::new();
    let mut warnings = vec![
        "Structural validation is not scientific peer review or execution authorization.".into(),
    ];
    for u in &b.unknowns {
        errors.push(format!("Unresolved {}: {}", u.field, u.reason));
    }
    if b.steps.iter().filter(|s| s.included).count() == 0 {
        errors.push("At least one step must be included for review.".into());
    }
    if b.evaluation_questions.is_empty() && b.hypotheses.is_empty() {
        errors.push("A hypothesis or evaluation question is required for review.".into());
    }
    if b.intent == "confirmatory"
        && (b.hypotheses.is_empty() || b.hypotheses.iter().any(|h| h.timing != "prespecified"))
    {
        errors.push("Confirmatory intent requires prespecified predictions.".into());
    }
    let methods: &[&str] = match b.research_type {
        ResearchType::LiteratureReview => &["selection", "appraisal", "synthesis"],
        ResearchType::ExperimentalResearch => {
            &["design", "measurement", "controls", "sampling", "analysis"]
        }
        ResearchType::ComputationalResearch => {
            &["inputs", "baseline", "analysis", "reproducibility"]
        }
        ResearchType::TechnicalFeasibility => &["constraints", "alternatives", "evaluation"],
    };
    for m in methods {
        if !b.method_requirements.iter().any(|v| v.kind == *m) {
            errors.push(format!("Missing methodological requirement: {m}"));
        }
    }
    for s in &b.steps {
        let needed = match s.kind.as_str() {
            "experiment_design" => Some(ResearchType::ExperimentalResearch),
            "computational_analysis" => Some(ResearchType::ComputationalResearch),
            _ => None,
        };
        if needed
            .as_ref()
            .is_some_and(|t| t != &b.research_type && !b.secondary_types.contains(t))
        {
            errors.push("Step requires a compatible primary or secondary research type.".into());
        }
        if !s.unsupported_requirements.is_empty() {
            warnings.push(format!(
                "Unsupported requirements in step {} remain proposals.",
                s.id
            ));
        }
    }
    if p.origin.kind == "external_ai_proposal" {
        warnings
            .push("AI origin is externally reported; no provider execution is established.".into());
    }
    Validation {
        structural_only: true,
        reviewable: errors.is_empty(),
        errors,
        warnings,
    }
}
pub(super) fn event_digest(kind: &str, revision: &PlanRevision) -> Result<Hash> {
    hash(
        &json!({"kind":kind,"revision_id":revision.revision_id,"revision_digest":revision.payload_digest,"parent_revision_id":revision.parent_revision_id}),
        "TAKO-RESEARCH-PLAN-EVENT-v1",
    )
}
pub(super) fn proposal(raw: &str) -> Result<PlanInput> {
    text(raw, 64 * 1024, true)?;
    let parsed = state::parse(super::json::decode(raw.as_bytes())?)?;
    input(&parsed)?;
    require((3..=6).contains(&parsed.body.steps.len()))?;
    Ok(parsed)
}
