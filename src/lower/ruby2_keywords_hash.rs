//! `Hash.ruby2_keywords_hash(hash)` → `hash.dup`.
//!
//! The method answers a copy of the Hash flagged so that a later `*args`
//! splat passes it as keywords. Spinel refuses the call by name and no
//! target carries the flag, so the copy is all that is kept.
//! solid_queue's `RecurringTask#arguments_with_kwargs` flags a recurring
//! task's last Hash argument. A job whose `perform` takes keywords keeps
//! its Proc rather than a payload (`lower::job_payload`), so nothing here
//! passes such a Hash to keywords. What differs is the payload text:
//! `ActiveJob::Arguments.serialize` writes `_aj_symbol_keys` where Rails
//! writes `_aj_ruby2_keywords`, which a Rails worker reading it would
//! not splat as keywords. Both close with keyword `perform` payloads.

use crate::expr::{Expr, ExprNode};
use crate::ident::Symbol;

pub(crate) fn rewrite_node(expr: &mut Expr) {
    let ExprNode::Send { recv: Some(recv), method, args, block: None, .. } = &mut *expr.node else { return };
    if method.as_str() != "ruby2_keywords_hash" || args.len() != 1 {
        return;
    }
    let ExprNode::Const { path } = &*recv.node else { return };
    let hash = path.iter().filter(|p| !p.as_str().is_empty()).map(Symbol::as_str).eq(["Hash"]);
    if !hash {
        return;
    }
    let arg = args.pop().expect("one argument");
    *expr.node = ExprNode::Send { recv: Some(arg), method: Symbol::from("dup"), args: Vec::new(), block: None, parenthesized: false };
}
