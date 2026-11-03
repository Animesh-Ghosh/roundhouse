// Not on ActiveJob::Base's own singleton: an engine's reopen (`def self.queue_adapter; current_queue_adapter || super; end`) replaces that, and its `super` reaches this module, as Rails' QueueAdapter::ClassMethods.
use crate::app::App;
use crate::expr::{Expr, ExprNode};
use crate::ident::{ClassId, Symbol};

pub(super) fn synthesize(app: &mut App) {
    let base = ClassId(Symbol::from("ActiveJob::Base"));
    if !app.library_classes.iter().any(|lc| lc.name == base) {
        return;
    }
    // Not SolidQueueAdapter without solid_queue's models: only the runtime that runs them defines it (`project::drop_solid_queue_standin`).
    let solid_queue = app.models.iter().any(|m| m.name.0.as_str() == "SolidQueue::Job");
    let adapter = match app.queue_adapter.as_deref() {
        None | Some("async") => "AsyncAdapter",
        Some("solid_queue") if solid_queue => "SolidQueueAdapter",
        Some(_) => return,
    };
    let source = format!(
        "module ActiveJob::QueueAdapter::ClassMethods\n  def queue_adapter\n    @queue_adapter ||= ActiveJob::QueueAdapters::{adapter}.new\n  end\nend\n"
    );
    let Ok(classes) = super::library_class::ingest_library_classes(source.as_bytes(), "<queue_adapter>") else { return };
    app.library_classes.extend(classes);
    let module = Expr::new(
        crate::span::Span::synthetic(),
        ExprNode::Const { path: ["ActiveJob", "QueueAdapter", "ClassMethods"].into_iter().map(Symbol::from).collect() },
    );
    if let Some(lc) = app.library_classes.iter_mut().find(|lc| lc.name == base) {
        lc.unknown_calls.push(Expr::new(
            crate::span::Span::synthetic(),
            ExprNode::Send { recv: None, method: Symbol::from("extend"), args: vec![module], block: None, parenthesized: false },
        ));
    }
}
