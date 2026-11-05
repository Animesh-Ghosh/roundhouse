//! App files that hold constants and no class or module declaration,
//! on the CRuby tree, against the same files run natively.
//!
//! Two shapes Rails apps keep in app/models: a class alias kept for old
//! references (`RemoteThing = Thing`) and a computed value
//! (`Labels = LabelFactory.build(...)`). Rails' autoloader maps each file
//! to its constant by name, so a reader anywhere in the app finds it.
//! Neither file was carried before: every read of the constant was
//! refused as an unsupported constant.
#[path = "support/emit_and_run.rs"]
mod emit_and_run;

use roundhouse::project::BuildTarget;
use std::process::Command;

const THING: &str = r##"class Thing
  def self.kind
    "thing"
  end

  def initialize(name = "t")
    @name = name
  end

  def name
    @name
  end
end
"##;

const NAMED: &str = r##"module Named
  class Widget
    def self.kind
      "widget"
    end
  end
end
"##;

const LABEL_FACTORY: &str = r##"class LabelFactory
  def self.build(names)
    names.map(&:upcase).freeze
  end
end
"##;

/// A class alias of an app class.
const REMOTE_THING: &str = "# frozen_string_literal: true\n\nRemoteThing = Thing\n";

/// A class alias of a namespaced app class.
const OLD_WIDGET: &str = "OldWidget = Named::Widget\n";

/// A value computed from another app class.
const LABELS: &str = "Labels = LabelFactory.build(%w[a b])\n";

/// Another file, reading every one of them.
const READER: &str = r##"class Reader
  def self.read
    [
      RemoteThing.kind,
      RemoteThing.new("r").name,
      OldWidget.kind,
      Labels.join(","),
      Labels.size,
      RemoteThing.name,
    ]
  end
end
"##;

const FILES: [(&str, &str); 7] = [
    ("app/models/thing.rb", THING),
    ("app/models/named.rb", NAMED),
    ("app/models/label_factory.rb", LABEL_FACTORY),
    ("app/models/remote_thing.rb", REMOTE_THING),
    ("app/models/old_widget.rb", OLD_WIDGET),
    ("app/models/labels.rb", LABELS),
    ("app/models/reader.rb", READER),
];

const SCRIPT: &str = r##"def seen(label)
  value = begin
    yield
  rescue StandardError => e
    [e.class.name, e.message]
  end
  puts "#{label}: #{value.inspect}"
end
seen("reader") { Reader.read }
seen("alias") { [RemoteThing.equal?(Thing), RemoteThing.name, OldWidget.name, Labels.frozen?] }
"##;

const EXPECTED: [&str; 2] = [
    "reader: [\"thing\", \"r\", \"widget\", \"A,B\", 2, \"Thing\"]",
    "alias: [true, \"Thing\", \"Named::Widget\", true]",
];

fn app() -> emit_and_run::Overlay {
    let mut app = emit_and_run::empty_app()
        .write("roundhouse.yml", "test_paths:\n  - test\n")
        .write("test/test_helper.rb", "require \"active_support/test_case\"\n");
    for (path, source) in FILES {
        app = app.write(path, source);
    }
    app
}

/// The same files on plain Ruby, required in order.
fn native() -> String {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rh_constant_only_native_{}_{seq}", std::process::id()));
    std::fs::create_dir_all(dir.join("app/models")).unwrap();
    let mut program = String::new();
    for (path, source) in FILES {
        std::fs::write(dir.join(path), source).unwrap();
        program.push_str(&format!("require_relative {:?}\n", path.trim_end_matches(".rb")));
    }
    program.push_str(SCRIPT);
    std::fs::write(dir.join("native.rb"), program).unwrap();
    let out = Command::new("ruby").arg("native.rb").current_dir(&dir).output().expect("ruby is on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "native run failed:\n{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
    stdout
}

#[test]
fn constant_only_files_check_without_errors() {
    let errors = app().check_errors();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn constant_only_files_run_like_the_native_run() {
    let native = native();
    assert_eq!(native.lines().collect::<Vec<_>>(), EXPECTED, "native:\n{native}");
    let emitted = app().run_ruby(SCRIPT);
    emitted.assert_passes();
    assert_eq!(emitted.stdout.lines().collect::<Vec<_>>(), EXPECTED, "emitted:\n{}", emitted.stdout);
}

/// Only the CRuby and JRuby trees write a file of bare top-level constants;
/// another target would have to invent a class for it.
#[test]
fn other_targets_refuse_a_file_of_constants() {
    let tree: std::collections::HashMap<std::path::PathBuf, Vec<u8>> = [
        ("app/models/thing.rb", THING),
        ("app/models/remote_thing.rb", REMOTE_THING),
    ]
    .into_iter()
    .map(|(path, source)| (std::path::PathBuf::from(path), source.as_bytes().to_vec()))
    .collect();
    let app = roundhouse::ingest::ingest_app_from_tree(tree).expect("ingest");
    let (result, _) = roundhouse::emit::diagnostics::scope(|| {
        roundhouse::project::target_files(&app, std::path::Path::new("not-a-fixture"), BuildTarget::Typescript)
    });
    let error = result.expect_err("typescript emitted a file of constants");
    assert!(error.contains("RemoteThing is a file of top-level constants"), "{error}");
}

/// An alias of a class the app does not have is reported where it is
/// written, not silently written into a file that raises NameError when
/// it loads.
#[test]
fn an_alias_of_a_missing_class_is_reported() {
    let errors = emit_and_run::empty_app()
        .write("roundhouse.yml", "test_paths:\n  - test\n")
        .write("app/models/ghost.rb", "Ghost = Missing::Thing\n")
        .write("app/models/haunt.rb", "class Haunt\n  def self.ghost\n    Ghost.name\n  end\nend\n")
        .check_errors();
    assert!(errors.iter().any(|error| error.contains("constant not supported (all targets): Missing::Thing")), "{errors:?}");
}
