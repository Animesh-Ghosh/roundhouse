//! A Symbol passed to an optional segment (`/media_proxy/:id/(*any)`, so
//! `?String? any`) is its String, as it is for a required one: Rails
//! renders it with `to_param`. On Rails 8.1.4, `media_proxy_path(1,
//! :original)` and `media_proxy_path(1, "original")` are both
//! "/media_proxy/1/original", and `media_proxy_path(1)` is "/media_proxy/1".
#[path = "support/emit_and_run.rs"]
mod emit_and_run;

use roundhouse::project::BuildTarget;

const CONTROLLER: &str = "class MediaController < ApplicationController
  def show
    render json: proxy_paths(params[:id].to_i)
  end

  def proxy_paths(id)
    [media_proxy_path(id, :original), media_proxy_path(id, params[:style].to_s), media_proxy_path(id)]
  end
end
";

fn example() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write("db/schema.rb", "ActiveRecord::Schema.define do\nend\n")
        .write("app/controllers/application_controller.rb", "class ApplicationController < ActionController::Base\nend\n")
        .write("app/controllers/media_controller.rb", CONTROLLER)
        .write(
            "config/routes.rb",
            "Rails.application.routes.draw do\n  get \"/media_proxy/:id/(*any)\", to: \"media#show\", as: :media_proxy, format: false\nend\n",
        )
}

#[test]
fn a_symbol_in_an_optional_segment_is_its_string_at_the_call_site() {
    let (emitted, _) = example().emit(BuildTarget::Ruby);
    let helpers = std::fs::read_to_string(emitted.join("sig/app/route_helpers.rbs"))
        .or_else(|_| std::fs::read_to_string(emitted.join("app/route_helpers.rbs")))
        .expect("read the emitted helper signatures");
    assert!(helpers.contains("def self.media_proxy_path: (Integer id, ?String? any) -> String"), "{helpers}");
    let controller = std::fs::read_to_string(emitted.join("app/controllers/media_controller.rb")).expect("read the emitted controller");
    assert!(controller.contains("RouteHelpers.media_proxy_path(id, \"original\")"), "{controller}");
    assert!(!controller.contains(":original"), "{controller}");
}

#[test]
fn a_symbol_in_an_optional_segment_renders_as_rails_does() {
    example()
        .run_ruby(
            r#"
require_relative "app/controllers/media_controller"
c = MediaController.new
c.params = { "style" => "small" }
paths = c.proxy_paths(1)
raise paths.inspect unless paths == ["/media_proxy/1/original", "/media_proxy/1/small", "/media_proxy/1"]
puts "proxy paths passed"
"#,
        )
        .assert_passes();
}
