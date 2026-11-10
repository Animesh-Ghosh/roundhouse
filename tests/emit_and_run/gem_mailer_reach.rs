//! A gem's mailer, as Devise's `app/mailers/devise/mailer.rb` writes it inside `if defined?(ActionMailer)`: the
//! class is the app's like any `app/` class of the gem. Its actions run only where something calls them, here
//! through a model callback (`after_save :notify_saved`) of a module the model includes; the template of an
//! action nothing calls is left out of the Spinel tree with the action (`source_gem_shake`). The template reads
//! an ivar a module of the mailer sets, as Devise's `Devise::Mailers::Helpers#initialize_from_record` sets
//! `@resource` with `instance_variable_set`: typed, though the module's `mail` is not rendered yet.

use roundhouse::project::BuildTarget;

use super::emit_and_run;

const GEMS: &str = "vendor/bundle/ruby/3.4.0/gems/notices-1.0";

fn installed() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .edit("Gemfile.lock", "    debug (1.11.1)\n", "    debug (1.11.1)\n    notices (1.0)\n")
        .edit("Gemfile.lock", "DEPENDENCIES\n", "DEPENDENCIES\n  notices\n")
        .write(".bundle/config", "---\nBUNDLE_PATH: \"vendor/bundle\"\n")
        .write(&format!("{GEMS}/lib/notices.rb"), "require \"notices/engine\"\nrequire \"notices/notifiable\"\nrequire \"notices/mailers\"\n")
        .write(&format!("{GEMS}/lib/notices/engine.rb"), "module Notices\n  class Engine < ::Rails::Engine\n  end\nend\n")
        .write(
            &format!("{GEMS}/lib/notices/notifiable.rb"),
            "module Notices\n  module Notifiable\n    extend ActiveSupport::Concern\n\n    included do\n      after_save :notify_saved\n    end\n\n    def notify_saved\n      Notices::Mailer.saved(title).deliver_now\n    end\n  end\nend\n",
        )
        .write(
            &format!("{GEMS}/lib/notices/mailers.rb"),
            "module Notices\n  module Mailers\n    def notify(title, action)\n      @title = instance_variable_set(\"@#{action}_title\", title)\n      mail(to: \"a@example.com\", subject: action.to_s)\n    end\n  end\nend\n",
        )
        .write(
            &format!("{GEMS}/app/mailers/notices/mailer.rb"),
            "if defined?(ActionMailer)\n  class Notices::Mailer < ActionMailer::Base\n    include Notices::Mailers\n\n    def saved(title)\n      notify(title, :saved)\n    end\n\n    def removed(title)\n      notify(title, :removed)\n    end\n  end\nend\n",
        )
        .write(&format!("{GEMS}/app/views/notices/mailer/saved.text.erb"), "Saved <%= @title %>\n")
        .write(&format!("{GEMS}/app/views/notices/mailer/removed.text.erb"), "Removed <%= @title %>\n")
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n", "class Article < ApplicationRecord\n  include Notices::Notifiable\n\n")
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn a_gem_mailer_keeps_the_action_a_callback_reaches_and_drops_the_other() {
    let (tree, _errors) = installed().emit(BuildTarget::Spinel);
    let texts: Vec<String> = walk(&tree).iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    let mentions = |needle: &str| texts.iter().any(|t| t.contains(needle));
    assert!(mentions("Mailer < "), "the gem's mailer is in the Spinel tree");
    assert!(mentions("def saved(title)"), "the action the callback reaches");
    assert!(!mentions("def removed(title)"), "an action nothing calls");
    assert!(mentions("Saved "), "the reached action's template");
    assert!(!mentions("Removed "), "the unreached action's template");
}

/// Not sent empty: the `mail` its module calls renders the template of the action that calls it, which the
/// template rewrite (`lower::mailer_templates`) does not pick, so the call is reported, as Devise's `devise_mail`.
#[test]
fn a_mail_in_a_mailers_module_is_reported() {
    let (_, errors) = installed().emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("`mail` in a module a mailer includes")), "{errors:#?}");
    assert!(!errors.iter().any(|e| e.contains("has no known type")), "the template's ivars: {errors:#?}");
}
