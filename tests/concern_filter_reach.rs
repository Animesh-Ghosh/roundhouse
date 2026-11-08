//! A concern's `included` filters reach the controller the way Rails
//! registers them: through concerns whose only content is another
//! `include`, from `lib/`, and not through a plain Ruby module.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::ingest::ingest_app_from_tree;

#[path = "support/emit_and_run.rs"]
mod emit_and_run;
#[path = "support/concern_filter_reach_app.rs"]
mod concern_filter_reach_app;

#[test]
fn concern_filters_reach_controllers_as_rails_registers_them() {
    let run = concern_filter_reach_app::app_with_plain_includer().run_ruby(&format!(
        "{}{}",
        concern_filter_reach_app::LOAD_CONTROLLERS,
        concern_filter_reach_app::ASSERTIONS
    ));
    run.assert_passes();
    assert!(run.stdout.contains("concern filters reached"), "{}", run.stdout);
}

fn tree_with_included(included: &str, controller_includes: bool) -> HashMap<PathBuf, Vec<u8>> {
    let include = if controller_includes { "  include Gated\n" } else { "" };
    [
        (
            "app/controllers/concerns/gated.rb",
            format!(
                "module Gated\n  extend ActiveSupport::Concern\n\n  included do\n{included}\n  end\n\n  def check\n    head :unauthorized\n  end\nend\n"
            ),
        ),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n".to_string(),
        ),
        (
            "app/controllers/widgets_controller.rb",
            format!("class WidgetsController < ApplicationController\n{include}\n  def index\n    render plain: \"ok\"\n  end\nend\n"),
        ),
        (
            "config/routes.rb",
            "Rails.application.routes.draw do\n  get \"/widgets\", to: \"widgets#index\"\nend\n".to_string(),
        ),
    ]
    .into_iter()
    .map(|(path, source)| (PathBuf::from(path), source.into_bytes()))
    .collect()
}

fn strict_refusal(included: &str) -> String {
    ingest_app_from_tree(tree_with_included(included, true))
        .err()
        .unwrap_or_else(|| panic!("expected a refusal for {included:?}"))
        .to_string()
}

#[test]
fn an_undecided_guard_around_a_filter_refuses_the_compile() {
    let error = strict_refusal("    before_action :check if ENV[\"LOCKED\"]");
    assert!(error.contains("`before_action` under a condition"), "{error}");
}

#[test]
fn an_unmodelled_callback_macro_refuses_the_compile() {
    let error = strict_refusal("    append_before_action :check");
    assert!(error.contains("macro not recognized: `append_before_action`"), "{error}");
}

#[test]
fn a_filter_registered_from_a_loop_refuses_the_compile() {
    let error = strict_refusal("    %i[check].each { |name| before_action name }");
    assert!(error.contains("`before_action` inside another statement"), "{error}");
}

#[test]
fn an_unread_filter_in_a_concern_no_controller_includes_is_not_a_refusal() {
    ingest_app_from_tree(tree_with_included("    before_action :check if ENV[\"LOCKED\"]", false))
        .expect("a concern no controller includes registers nothing");
}

/// A concern with no body of its own is a module the strict emitters now
/// see; it must not trip them.
#[test]
fn a_concern_that_only_includes_emits_on_the_strict_targets() {
    use roundhouse::project::BuildTarget;
    for target in [BuildTarget::Rust, BuildTarget::Typescript] {
        let (_, errors) = concern_filter_reach_app::app().emit(target);
        assert!(errors.is_empty(), "{target:?}: {errors:#?}");
    }
}

const CONTROLLER: &str = "class WidgetsController < ApplicationController\n  include Gated\n\n  def index\n    render plain: \"ok\"\n  end\nend\n";

fn hook_tree(module_body: &str, controller: &str) -> HashMap<PathBuf, Vec<u8>> {
    let mut tree = tree_with_included("", true);
    tree.insert(
        PathBuf::from("app/controllers/concerns/gated.rb"),
        format!("module Gated\n{module_body}\n\n  def check\n    head :unauthorized\n  end\nend\n").into_bytes(),
    );
    tree.insert(PathBuf::from("app/controllers/widgets_controller.rb"), controller.as_bytes().to_vec());
    tree
}

fn hook(body: &str) -> String {
    format!("  def self.included(base)\n    {body}\n  end")
}

fn refused(module_body: &str) -> String {
    ingest_app_from_tree(hook_tree(module_body, CONTROLLER))
        .err()
        .unwrap_or_else(|| panic!("expected a refusal for {module_body:?}"))
        .to_string()
}

#[test]
fn hook_shapes_the_compiler_cannot_read_refuse_the_compile() {
    for body in [
        "base.send(:before_action, :check)",
        "base.public_send(:before_action, :check)",
        "base.__send__(:before_action, :check)",
        "base.before_action :check if base.ready?",
        "base.before_action :check unless base.respond_to?(:other)",
        "base.before_action :check if base.method_defined?(:before_action)",
        "base.before_action :check if base < ActionController::Base",
        "base.before_action { head :unauthorized }",
        "base.before_action :check, only: ACTIONS",
        "%i[check].each { |name| base.before_action name }",
        "base.append_before_action :check",
        // Half a hook cannot be dropped, and the other half re-runs the filter.
        "base.before_action :check\n    base.extend(Enumerable)",
    ] {
        let error = refused(&hook(body));
        assert!(error.contains("`self.included` hook"), "{body}: {error}");
    }
}

#[test]
fn other_hook_methods_that_register_a_filter_refuse_the_compile() {
    for module_body in [
        "  def self.append_features(base)\n    super\n    base.before_action :check\n  end",
        "  def self.prepended(base)\n    base.before_action :check\n  end",
        "  class << self\n    def included(base)\n      base.before_action :check\n    end\n  end",
    ] {
        let error = refused(module_body);
        assert!(error.contains("hook is not modelled"), "{module_body}: {error}");
    }
}

#[test]
fn a_modelled_hook_leaves_the_emitted_module() {
    let app = ingest_app_from_tree(hook_tree(&hook("base.before_action :check"), CONTROLLER)).expect("modelled");
    let gated = app.library_classes.iter().find(|c| c.name.0.as_str() == "Gated").expect("module");
    assert!(gated.methods.iter().all(|m| m.name.as_str() != "included"), "{:?}", gated.methods);
}

#[test]
fn a_hook_with_other_statements_and_no_filter_stays_in_the_module() {
    let app = ingest_app_from_tree(hook_tree(&hook("base.extend(Enumerable)"), CONTROLLER)).expect("no filter");
    let gated = app.library_classes.iter().find(|c| c.name.0.as_str() == "Gated").expect("module");
    assert!(gated.methods.iter().any(|m| m.name.as_str() == "included"), "{:?}", gated.methods);
}

/// Ruby runs `included` on every `include`, so Rails re-registers the
/// filter and moves it to the end of the chain. The chain does not model
/// that move.
#[test]
fn a_hook_registered_twice_on_one_controller_refuses_the_compile() {
    let twice = [
        // Parent and child.
        "class WidgetsController < BaseWidgetsController\n  include Gated\n\n  def index\n    render plain: \"ok\"\n  end\nend\n",
        // One class, two statements.
        "class WidgetsController < ApplicationController\n  include Gated\n  include Gated\n\n  def index\n    render plain: \"ok\"\n  end\nend\n",
    ];
    for controller in twice {
        let mut tree = hook_tree(&format!("  extend ActiveSupport::Concern\n{}", hook("base.before_action :check")), controller);
        tree.insert(
            PathBuf::from("app/controllers/base_widgets_controller.rb"),
            b"class BaseWidgetsController < ApplicationController\n  include Gated\nend\n".to_vec(),
        );
        let error = ingest_app_from_tree(tree).err().unwrap_or_else(|| panic!("expected a refusal: {controller}")).to_string();
        assert!(error.contains("more than once"), "{error}");
    }
}

/// Two concerns each depending on the hook's concern: `base.include(dep)`
/// runs twice, and the second one re-registers.
#[test]
fn a_hook_reached_through_two_concerns_refuses_the_compile() {
    let mut tree = hook_tree(
        &format!("  extend ActiveSupport::Concern\n{}", hook("base.before_action :check")),
        "class WidgetsController < ApplicationController\n  include Left\n  include Right\n\n  def index\n    render plain: \"ok\"\n  end\nend\n",
    );
    for (file, name) in [("left", "Left"), ("right", "Right")] {
        tree.insert(
            PathBuf::from(format!("app/controllers/concerns/{file}.rb")),
            format!("module {name}\n  extend ActiveSupport::Concern\n  include Gated\nend\n").into_bytes(),
        );
    }
    let error = ingest_app_from_tree(tree).err().expect("expected a refusal").to_string();
    assert!(error.contains("more than once"), "{error}");
}

/// Rails never runs an unreadable plain hook on the controller here, so
/// there is nothing to refuse.
#[test]
fn a_plain_hook_behind_a_concern_is_not_a_refusal() {
    let mut tree = hook_tree(
        &hook("base.before_action :check if base.instance_of?(Class)"),
        "class WidgetsController < ApplicationController\n  include Wrapper\n\n  def index\n    render plain: \"ok\"\n  end\nend\n",
    );
    tree.insert(
        PathBuf::from("app/controllers/concerns/wrapper.rb"),
        b"module Wrapper\n  extend ActiveSupport::Concern\n  include Gated\n\n  def label\n    1\n  end\nend\n".to_vec(),
    );
    ingest_app_from_tree(tree).expect("the hook ran on the concern, not the controller");
}
