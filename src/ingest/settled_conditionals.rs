// Not both arms kept: a `defined?` of a class the app carries is settled when the body loads, and Ruby never defines the other arm's methods (et-orbi's tzinfo 1.x fallbacks).
use crate::app::App;
use crate::dialect::{ConditionalDefinitions, LibraryClass};
use crate::expr::ExprNode;

pub(super) fn settle(app: &mut App) {
    let known: crate::det::HashSet<String> = app
        .library_classes
        .iter()
        .map(|c| c.name.0.as_str().to_string())
        .chain(app.models.iter().map(|m| m.name.0.as_str().to_string()))
        .collect();
    for lc in app.library_classes.iter_mut() {
        let records = std::mem::take(&mut lc.conditional_definitions);
        for record in records {
            match settled_arm(&record, &known) {
                Some(kept) => drop_other_arms(lc, &record, kept),
                None => lc.conditional_definitions.push(record),
            }
        }
    }
}

fn settled_arm(record: &ConditionalDefinitions, known: &crate::det::HashSet<String>) -> Option<usize> {
    if record.arms.len() != 2 || record.arms.iter().any(|a| !a.nested.is_empty() || !a.declarations.is_empty()) {
        return None;
    }
    let cond = record.arms[0].condition.as_ref()?;
    let ExprNode::Defined { operand } = &*cond.node else { return None };
    let ExprNode::Const { path } = &*operand.node else { return None };
    let name = path.iter().map(|p| p.as_str()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("::");
    known.contains(&name).then_some(if record.negated { 1 } else { 0 })
}

fn drop_other_arms(lc: &mut LibraryClass, record: &ConditionalDefinitions, kept: usize) {
    for (i, arm) in record.arms.iter().enumerate() {
        if i != kept {
            lc.methods.retain(|m| !arm.methods.iter().any(|member| member.is(m)));
        }
    }
}
