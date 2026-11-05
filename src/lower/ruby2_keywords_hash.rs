//! `Hash.ruby2_keywords_hash(hash)` → `hash.dup`.
//!
//! The method answers a copy of the Hash flagged so that a later `*args`
//! splat passes it as keywords, a shim for code written across the Ruby
//! 2.x-to-3.0 keyword change. Every target speaks modern keyword
//! semantics, where the flag toggles nothing, and Spinel refuses the call
//! by name. solid_queue's `RecurringTask#arguments_with_kwargs` splats it
//! into `job_class.new(*args)`, whose `initialize(*arguments)` takes it
//! positionally either way.

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
