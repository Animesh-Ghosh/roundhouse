//! A controller's `layout` picks the layout an action renders in, as Rails
//! 8.1.4's `ActionView::Layouts` does: the pages in
//! `support/controller_layouts.rs`, served by the emitted Ruby app.
//!
//! What Rails decides on each request (a method body, a Proc) or raises
//! for (a missing template, `render layout: :symbol`) is refused, and
//! off the ruby family a render in any layout but `layouts/application`
//! is refused, since those targets wrap every page in it.
#[path = "support/emit_and_run.rs"]
mod emit_and_run;
#[path = "support/controller_layouts.rs"]
mod contract;

use roundhouse::project::BuildTarget;

/// `PAGES` as a script against the emitted Ruby app.
fn pages_script() -> String {
    let mut script = String::from(
        r#"def page(path, query)
  status, _headers, body = Main.run_rack(
    "REQUEST_METHOD" => "GET",
    "PATH_INFO" => path,
    "QUERY_STRING" => query,
    "HTTP_HOST" => "localhost",
    "rack.input" => StringIO.new("")
  )
  text = +""
  body.each { |chunk| text << chunk.to_s }
  [status, text.gsub(/\s+/, " ").strip]
end
"#,
    );
    for (path, query, status, body) in contract::PAGES {
        script.push_str(&format!(
            "got = page({path:?}, {query:?})\nraise \"{path}?{query}: #{{got.inspect}}\" unless got == [{status}, {body:?}]\n"
        ));
    }
    script.push_str("puts \"controller layouts passed\"\n");
    script
}

#[test]
fn each_action_renders_in_the_layout_rails_picks() {
    let run = contract::blog().run_ruby(&pages_script());
    run.assert_passes();
    assert!(run.stdout.contains("controller layouts passed"), "{}", run.stdout);
}

#[test]
fn spinel_wraps_each_render_in_its_layout() {
    let (emitted, errors) = contract::blog().emit(BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
    let controller = |path: &str| {
        std::fs::read_to_string(emitted.join(format!("app/controllers/{path}_controller.rb"))).expect(path)
    };
    assert!(controller("articles").contains("render(Views::Layouts.reading(Views::Articles.index("), "{}", controller("articles"));
    assert!(controller("feeds").contains("render(Views::Layouts.print(Views::Feeds.index("), "{}", controller("feeds"));
    assert!(controller("drafts").contains("render(Views::Drafts.index("), "{}", controller("drafts"));
    assert!(controller("admin/stats").contains("Views::Layouts.admin("), "{}", controller("admin/stats"));
    assert!(controller("authors").contains("Views::Layouts.authors("), "{}", controller("authors"));
    let archives = controller("archives");
    assert!(archives.contains("render(Views::Layouts.reading(Views::Shared.closed("), "{archives}");
    assert!(archives.contains("render(Views::Layouts.application(Views::Shared.closed("), "{archives}");
    assert!(archives.contains("render(if controller_path == \"archives\" && action_name == \"show\""), "{archives}");
    assert!(archives.contains("assign_action_name("), "{archives}");
    let notices = controller("notices");
    assert!(notices.contains("render(if controller_path == \"notices\" && action_name == \"index\""), "{notices}");
    let editions = controller("editions");
    assert!(editions.contains("render(if controller_path == \"special_editions\""), "{editions}");
    let comments = controller("comments");
    assert!(comments.contains("if controller_path == \"archived_comments\""), "{comments}");
}

#[test]
fn a_layout_method_rails_runs_on_each_render_is_refused() {
    let errors = contract::blog()
        .write(
            "app/controllers/feeds_controller.rb",
            "class FeedsController < ApplicationController\n  layout :feed_layout\n\n  def index\n  end\n\nprivate\n\n  def feed_layout\n    params[:print] ? \"print\" : \"reading\"\n  end\nend\n",
        )
        .check_errors();
    assert!(
        errors.iter().any(|e| e.contains("controller layout not supported (all targets): `layout :feed_layout` calls `feed_layout` on each render")),
        "{errors:#?}"
    );
}

#[test]
fn a_layout_with_no_template_is_refused_as_rails_raises() {
    let errors = contract::blog()
        .write("app/controllers/articles_controller.rb", "class ArticlesController < ApplicationController\n  layout \"magazine\"\n\n  def index\n  end\nend\n")
        .check_errors();
    assert!(
        errors.iter().any(|e| e.contains("`layouts/magazine` has no html template: Rails raises ActionView::MissingTemplate")),
        "{errors:#?}"
    );
}

#[test]
fn a_symbol_render_layout_is_refused_as_rails_raises() {
    let errors = contract::blog()
        .write("app/controllers/exports_controller.rb", "class ExportsController < ApplicationController\n  def printable\n    render layout: :print\n  end\nend\n")
        .check_errors();
    assert!(
        errors.iter().any(|e| e.contains("`render layout: :print`: Rails takes a String, true or false here and raises ArgumentError")),
        "{errors:#?}"
    );
}

#[test]
fn a_target_with_one_layout_refuses_the_others() {
    for target in [BuildTarget::Go, BuildTarget::Typescript] {
        let (_emitted, errors) = contract::blog().emit(target);
        let ours = |text: &str| errors.iter().any(|e| e.contains(&format!("controller layout not supported ({}): {text}", target.as_str())));
        assert!(
            ours("`ArticlesController#index` renders in `layouts/reading`, and this target wraps every page in `layouts/application`"),
            "{target:?}: {errors:#?}"
        );
        assert!(ours("`DraftsController#index` renders without a layout"), "{target:?}: {errors:#?}");
        assert!(!ours("`TagsController#index`"), "{target:?}: {errors:#?}");
    }
}

#[test]
fn an_application_only_app_has_no_layout_refusal_on_any_target() {
    for target in [
        BuildTarget::Ruby,
        BuildTarget::Spinel,
        BuildTarget::Rust,
        BuildTarget::Go,
        BuildTarget::Typescript,
        BuildTarget::Crystal,
        BuildTarget::Python,
    ] {
        let (_emitted, errors) = emit_and_run::real_blog().emit(target);
        assert!(!errors.iter().any(|e| e.contains("controller layout")), "{target:?}: {errors:#?}");
    }
}
