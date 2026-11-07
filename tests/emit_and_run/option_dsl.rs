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
    let options = r##"    option :base_controller, default: (lambda do
      api_only ? "ActionController::API" : "ActionController::Base"
    end)
    option :api_only, default: false"##;
    let resolve = r##"    def resolve_controller(name)
      value = public_send(:"#{name}_controller")
      value.respond_to?(:call) ? instance_exec(&value) : value
    end"##;
    let run = emit_and_run::real_blog()
        .write("app/models/settings/option.rb", option_dsl::OPTION)
        .write("app/models/settings/config.rb", &option_dsl::config(options, resolve))
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
