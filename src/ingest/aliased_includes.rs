//! `include Devise::DeprecatedConstantAccessor`, where devise wrote
//! `DeprecatedConstantAccessor = ActiveSupport::Deprecation::DeprecatedConstantAccessor`:
//! Ruby includes the module the constant holds, so the include names that module.

use crate::app::App;
use crate::expr::ExprNode;
use crate::ident::{ClassId, Symbol};

pub(super) fn resolve(app: &mut App) {
    let mut aliases: crate::det::HashMap<String, String> = crate::det::HashMap::default();
    for lc in &app.library_classes {
        for (name, value) in &lc.constants {
            if let ExprNode::Const { path } = &*value.node {
                let target = path.iter().map(|p| p.as_str()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("::");
                aliases.insert(format!("{}::{}", lc.name.0, name), target);
            }
        }
    }
    if aliases.is_empty() {
        return;
    }
    let declared: crate::det::HashSet<String> = app.library_classes.iter().map(|lc| lc.name.0.to_string()).collect();
    for lc in &mut app.library_classes {
        for included in &mut lc.includes {
            let mut name = included.0.to_string();
            let mut hops = 0;
            // Not a name a class or module of the tree declares: that declaration is what Ruby finds first.
            while !declared.contains(&name) && hops < 8 {
                let Some(target) = aliases.get(&name) else { break };
                name = target.clone();
                hops += 1;
            }
            if hops > 0 {
                *included = ClassId(Symbol::from(name.as_str()));
            }
        }
    }
}
