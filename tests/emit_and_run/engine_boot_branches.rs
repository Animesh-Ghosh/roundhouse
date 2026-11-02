//! A source gem's engine that prepends an adapter extension only for the
//! adapter the app runs, as mission_control-jobs does for `:resque` and
//! `:solid_queue`. Boot fills `Relay.adapters` from
//! `config.active_job.queue_adapter` and the branch for any other adapter
//! is never taken, so the module only that branch names is left out of
//! the tree (its body reaches `Resque`, which the app has not got) while
//! the one the app's adapter prepends is kept.

use super::emit_and_run;

const GEM: &str = "vendor/bundle/ruby/3.4.0/gems/relay-1.0";

const ENTRY: &str = r##"require "zeitwerk"
require "relay/engine"

loader = Zeitwerk::Loader.new
loader.push_dir(__dir__)
loader.setup

module Relay
  mattr_accessor :adapters, default: Set.new
end
"##;

const ENGINE: &str = r##"module Relay
  class Engine < ::Rails::Engine
    config.relay = ActiveSupport::OrderedOptions.new

    config.before_initialize do
      config.relay.each do |key, value|
        Relay.public_send("#{key}=", value)
      end

      if Relay.adapters.empty?
        Relay.adapters << (config.active_job.queue_adapter || :async)
      end
    end

    config.before_initialize do
      if Relay.adapters.include?(:resque)
        ActiveJob::QueueAdapters::ResqueAdapter.prepend Relay::ResqueExt
      end

      if Relay.adapters.include?(:solid_queue)
        ActiveJob::QueueAdapters::SolidQueueAdapter.prepend Relay::SolidQueueExt
      end
    end
  end
end
"##;

const RESQUE_EXT: &str = r##"module Relay::ResqueExt
  def relay_queues
    Resque.queues
  end
end
"##;

const SOLID_QUEUE_EXT: &str = r##"module Relay::SolidQueueExt
  def relay_label
    "solid"
  end
end
"##;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .edit("Gemfile.lock", "    debug (1.11.1)\n", "    debug (1.11.1)\n    relay (1.0)\n")
        .edit("Gemfile.lock", "DEPENDENCIES\n", "DEPENDENCIES\n  relay\n")
        .write(".bundle/config", "---\nBUNDLE_PATH: \"vendor/bundle\"\n")
        .write("roundhouse.yml", "extensions:\n  - config/roundhouse\n")
        .write("config/roundhouse/gems.yml", "source_gems:\n  - {gem: \"relay\"}\n")
        .write(&format!("{GEM}/lib/relay.rb"), ENTRY)
        .write(&format!("{GEM}/lib/relay/engine.rb"), ENGINE)
        .write(&format!("{GEM}/lib/relay/resque_ext.rb"), RESQUE_EXT)
        .write(&format!("{GEM}/lib/relay/solid_queue_ext.rb"), SOLID_QUEUE_EXT)
}

#[test]
fn the_extension_of_an_adapter_the_app_does_not_run_is_left_out() {
    let errors = app().check_errors();
    assert!(errors.is_empty(), "{errors:#?}");
    let warnings = app().ingest_warnings();
    assert!(warnings.iter().any(|w| w.contains("`Relay::ResqueExt`") && w.contains("left out of the tree")), "{warnings:#?}");
    assert!(!warnings.iter().any(|w| w.contains("`Relay::SolidQueueExt`")), "{warnings:#?}");
}

#[test]
fn the_tree_boots_without_the_extension_it_left_out() {
    let run = app().run_ruby(
        "raise \"resque ext\" if defined?(Relay::ResqueExt)\n\
         puts \"engine boot branches passed\"\n",
    );
    run.assert_passes();
    assert!(run.stdout.contains("engine boot branches passed"), "{}", run.stdout);
}

#[test]
fn the_extension_of_the_adapter_the_app_runs_is_kept() {
    let errors = app()
        .edit("config/environments/production.rb", "  config.active_job.queue_adapter = :solid_queue\n", "  config.active_job.queue_adapter = :resque\n")
        .check_errors();
    assert!(errors.iter().any(|e| e.contains("Resque")), "{errors:#?}");
}
