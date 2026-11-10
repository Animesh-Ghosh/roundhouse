//! A class under `ActionController::Metal` that mixes back the modules Rails builds Base from, as Devise's
//! `FailureApp < ActionController::Metal; include ActionController::UrlFor, ActionController::Redirecting,
//! AbstractController::Callbacks; around_action do … end`. Devise's boot names the class (`Devise::Mapping`'s
//! `@failure_app = … || Devise::FailureApp`), so it has to load; here Metal is Base under Metal's name.

use super::emit_and_run;

const PROBE: &str = r#"class Probe < ActionController::Metal
  include ActionController::UrlFor
  include ActionController::Redirecting
  include AbstractController::Callbacks

  around_action do |probe, action|
    action.call
  end

  def self.ping
    "pong"
  end
end
"#;

const SCRIPT: &str = r#"raise "ping #{Probe.ping.inspect}" unless Probe.ping == "pong"
puts "metal subclass passed"
"#;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog().write("app/models/probe.rb", PROBE)
}

#[test]
fn a_metal_subclass_with_the_modules_base_is_built_from_loads() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("metal subclass passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_metal_subclass_with_the_modules_base_is_built_from_loads_natively() {
    let run = app().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("metal subclass passed"), "{}", run.stdout);
}
