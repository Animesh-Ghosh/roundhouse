//! A constant nothing defines (no gem of the lockfile, Ruby's standard
//! library or the app), read where the code rescues it or tests for it
//! first, raises Ruby's NameError instead of standing as unsupported
//! (et-orbi 1.4 probes for the optional chronic gem this way). A constant
//! of a gem the lockfile lacks, nested in one it has (tzinfo-data's
//! `TZInfo::Data` in tzinfo's `TZInfo`), raises it wherever it is read,
//! and a require of that gem raises LoadError (tzinfo's own probe for
//! tzinfo-data rescues it and falls back to the zoneinfo files).

use super::emit_and_run;

const PROBE: &str = "module GuardProbe\n  def self.version\n    Chronic::VERSION rescue \"none\"\n  end\n\n  def self.parse(text)\n    return false unless defined?(::Chronic)\n    ::Chronic.parse(text)\n  end\n\n  def self.rescued\n    begin\n      Chronic::Parser\n    rescue NameError => e\n      e.message\n    end\n  end\nend\n";

const TZ_PROBE: &str = "module TzProbe\n  def self.data_version\n    TZInfo::Data::VERSION\n  rescue NameError => e\n    e.message\n  end\n\n  def self.defined_data\n    TZInfo::Data.const_defined?(:LOCATION)\n  rescue NameError\n    \"absent\"\n  end\n\n  def self.loads_data\n    require \"tzinfo/data\"\n    \"loaded\"\n  rescue LoadError => e\n    e.message\n  end\nend\n";

const ASSERTIONS: &str = "got = [GuardProbe.version, GuardProbe.parse(\"x\"), GuardProbe.rescued, TzProbe.data_version, TzProbe.defined_data, TzProbe.loads_data]\nraise got.inspect unless got == [\"none\", false, \"uninitialized constant GuardProbe::Chronic\", \"uninitialized constant TZInfo::Data\", \"absent\", \"cannot load such file -- tzinfo/data\"]\nputs \"guarded absent constant passed\"\n";

#[test]
fn a_guarded_constant_nothing_defines_raises_name_error() {
    let run = emit_and_run::real_blog().write("app/models/guard_probe.rb", PROBE).write("app/models/tz_probe.rb", TZ_PROBE).run_ruby(ASSERTIONS);
    run.assert_passes();
    assert!(run.stdout.contains("guarded absent constant passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_guarded_constant_nothing_defines_raises_name_error_on_spinel() {
    let run = emit_and_run::real_blog().write("app/models/guard_probe.rb", PROBE).write("app/models/tz_probe.rb", TZ_PROBE).run_spinel(ASSERTIONS);
    run.assert_passes();
    assert!(run.stdout.contains("guarded absent constant passed"), "{}", run.stdout);
}

const CORE_PROBE: &str = "module CoreProbe\n  module Parts\n    module Basic\n      def part_name = \"basic\"\n    end\n  end\n\n  class Gadget\n    def attach(kind)\n      extend(Object.class_eval(\"CoreProbe::Parts::#{kind.capitalize}\"))\n      self\n    rescue StandardError\n      nil\n    end\n\n    def attach_string(kind)\n      extend(String.class_eval(\"CoreProbe::Parts::#{kind.capitalize}\"))\n      self\n    rescue StandardError\n      nil\n    end\n  end\nend\n";

const CORE_RUN_PROBE: &str = "module CoreRun\n  class Gadget\n    def jump\n      raise LocalJumpError, \"no block given\" unless block_given?\n      yield\n    rescue StandardError => e\n      e.class.name\n    end\n\n    def counted\n      Enumerator.new { |y| y << 1; y << 2 }.to_a\n    rescue StandardError => e\n      e.class.name\n    end\n  end\nend\n";

const CORE_ASSERTIONS: &str = "got = [CoreRun::Gadget.new.jump, CoreRun::Gadget.new.counted]\nraise got.inspect unless got == [\"LocalJumpError\", [1, 2]]\nputs \"core class read passed\"\n";

/// A core class is always defined, so a read inside a rescuing method is
/// never turned into a `raise NameError` that the rescue then hides. One
/// the model declares (`LocalJumpError`, `Enumerator`) is emitted as
/// written and runs as on CRuby; one the analyzer still cannot type there
/// (`Object` and `String` in `extend(...)`) keeps its diagnostic.
#[test]
fn a_core_class_read_in_a_rescuing_method_is_not_taken_for_absent() {
    let errors = emit_and_run::real_blog()
        .write("app/models/core_probe.rb", CORE_PROBE)
        .write("app/models/core_run.rb", CORE_RUN_PROBE)
        .check_errors();
    for root in ["Object", "String"] {
        let kept = format!("constant not supported (all targets): {root}");
        assert!(errors.iter().any(|e| e.ends_with(&kept)), "`{root}` lost its unsupported-constant diagnostic: {errors:#?}");
    }
    for root in ["LocalJumpError", "Enumerator"] {
        assert!(!errors.iter().any(|e| e.contains(root)), "`{root}` is a core class the analyzer declares: {errors:#?}");
    }
    let run = emit_and_run::real_blog().write("app/models/core_run.rb", CORE_RUN_PROBE).run_ruby(CORE_ASSERTIONS);
    run.assert_passes();
    assert!(run.stdout.contains("core class read passed"), "{}", run.stdout);
}
