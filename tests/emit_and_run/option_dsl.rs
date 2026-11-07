use super::{emit_and_run, option_dsl};

#[test]
fn an_extended_option_dsl_defines_the_readers_and_the_builders_writers() {
    let run = emit_and_run::real_blog()
        .write("app/models/settings/option.rb", option_dsl::OPTION)
        .write("app/models/settings/config.rb", &option_dsl::config(option_dsl::LITERAL_OPTIONS, ""))
        .run_ruby(option_dsl::LITERAL_ASSERTIONS);
    run.assert_passes();
    assert!(run.stdout.contains("option DSL passed"), "{}", run.stdout);
    let config = std::fs::read_to_string(run.emitted.join("app/models/settings/config.rb")).expect("emitted config");
    assert!(config.contains("def realm_name(*_args)") && config.contains("def realm(*args, &block)"), "{config}");
    assert!(!config.contains("define_method"), "{config}");
}

/// doorkeeper's `base_controller`: a lambda default the config runs with `instance_exec`, so it reads the config's own options.
#[test]
fn a_lambda_default_runs_on_the_config_that_reads_it() {
    let run = emit_and_run::real_blog()
        .write("app/models/settings/option.rb", option_dsl::OPTION)
        .write("app/models/settings/config.rb", &option_dsl::config(option_dsl::RESOLVING_OPTIONS, option_dsl::RESOLVE))
        .run_ruby(
            r##"raise "default" unless Settings::Config::Builder.new.build.resolve_controller(:base) == "ActionController::Base"
raise "api" unless Settings.configure { api_only true }.resolve_controller(:base) == "ActionController::API"
raise "own" unless Settings.configure { base_controller "ApplicationController" }.resolve_controller(:base) == "ApplicationController"
puts "lambda default passed"
"##,
        );
    run.assert_passes();
    assert!(run.stdout.contains("lambda default passed"), "{}", run.stdout);
}

fn configured_superclass_app(initializer: Option<&str>) -> emit_and_run::Overlay {
    let mut overlay = emit_and_run::real_blog()
        .write("app/models/settings/option.rb", option_dsl::OPTION)
        .write("app/models/settings/config.rb", &option_dsl::config(option_dsl::RESOLVING_OPTIONS, option_dsl::RESOLVE_CLASS))
        .edit(
            "app/controllers/application_controller.rb",
            "  stale_when_importmap_changes\n",
            "  stale_when_importmap_changes\n\n  private\n\n  def app_marker\n    \"app\"\n  end\n",
        )
        .write("app/controllers/settings_base_controller.rb", "class SettingsBaseController < Settings.config.resolve_controller(:base)\nend\n")
        .write("app/controllers/pings_controller.rb", "class PingsController < SettingsBaseController\n  def show\n    render plain: \"pong\"\n  end\nend\n");
    if let Some(body) = initializer {
        overlay = overlay.write("config/initializers/settings.rb", body);
    }
    overlay
}

/// doorkeeper's `class ApplicationController < Doorkeeper.config.resolve_controller(:base)`: the host's
/// initializer has configured the option by the time the class loads, so the parent is the one it names.
#[test]
fn a_superclass_read_off_the_configured_object_is_the_one_the_initializer_names() {
    let run = configured_superclass_app(Some("Settings.configure do\n  base_controller \"ApplicationController\"\nend\n")).run_ruby(
        r##"raise "parent: #{SettingsBaseController.superclass}" unless SettingsBaseController.superclass == ApplicationController
raise "inherited" unless PingsController.new.send(:app_marker) == "app"
puts "configured superclass passed"
"##,
    );
    run.assert_passes();
    assert!(run.stdout.contains("configured superclass passed"), "{}", run.stdout);
}

/// With no initializer, the option's lambda default answers, run on the config as `instance_exec` runs it.
#[test]
fn a_superclass_read_off_an_unconfigured_object_is_its_default() {
    let run = configured_superclass_app(None).run_ruby(
        r##"raise "parent: #{SettingsBaseController.superclass}" unless SettingsBaseController.superclass == ActionController::Base
puts "default superclass passed"
"##,
    );
    run.assert_passes();
    assert!(run.stdout.contains("default superclass passed"), "{}", run.stdout);
}

/// What the option holds is known only when the app boots (`ENV.fetch`): the superclass stays refused.
#[test]
fn a_superclass_from_a_value_known_only_at_boot_stays_refused() {
    let errors = configured_superclass_app(Some("Settings.configure do\n  base_controller ENV.fetch(\"BASE_CONTROLLER\", \"ApplicationController\")\nend\n")).check_errors();
    assert!(errors.iter().any(|e| e.contains("superclass is an expression")), "{errors:?}");
}
