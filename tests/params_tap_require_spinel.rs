//! The Spinel side of `tests/params_tap_require.rs`. Needs a Spinel
//! compiler, so `#[ignore]`d; CI's `spinel-framework` job runs it
//! (`scripts/ci-plan.py`'s `SPINEL_TESTS`).

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write("db/schema.rb", "ActiveRecord::Schema.define do\nend\n")
        .write("config/routes.rb", "Rails.application.routes.draw do\n  resources :widgets, only: :create\nend\n")
        .write("app/controllers/application_controller.rb", "class ApplicationController < ActionController::API\nend\n")
        .write(
            "app/controllers/widgets_controller.rb",
            r#"class WidgetsController < ApplicationController
  def create
    values = widget_params
    render plain: values[:email].to_s
  end

  private

  def widget_params
    params.require(:widget).permit(:name, :email).tap { |p| p.require(:email) }
  end
end
"#,
        )
}

const SCRIPT: &str = r#"
def answer(body)
  controller = WidgetsController.new
  controller.params = { "widget" => body }
  begin
    controller.process_action(:create)
    "ok:" + controller.body
  rescue ActionController::ParameterMissing => e
    "missing:" + e.param.to_s
  end
end
got = answer({ "name" => "n", "email" => "e@x" })
raise "present: #{got}" unless got == "ok:e@x"
got = answer({ "name" => "n" })
raise "absent: #{got}" unless got == "missing:email"
got = answer({ "name" => "n", "email" => "" })
raise "blank: #{got}" unless got == "missing:email"
puts "params require passed"
"#;

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_required_key_of_a_permitted_params_object_raises_when_blank_on_spinel() {
    app().run_spinel(SCRIPT).assert_passes();
}
