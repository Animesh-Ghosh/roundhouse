//! Container elements retain the distinction between a class and its instances.
use std::path::PathBuf;

#[path = "support/scratch.rs"]
mod scratch;

const SOURCE: &str = r#"class Beacon
  def self.label = "class"
  def label = "instance"
end
module Catalog
  def self.all = [Beacon]
  def self.mapped = [1].map { Beacon }
  def self.copied
    copy = Beacon
    [copy]
  end
  def self.nested = [{ "kind" => Beacon }]
  def self.instances = [Beacon.new]
  def self.mixed(flag) = [flag ? Beacon : Beacon.new]
  def self.tuple = [Beacon, Beacon.new]
  def self.keyed = { Beacon => "class" }
  def self.written
    result = {}
    result["class"] = Beacon
    result
  end
  def self.pushed
    result = []
    result << Beacon
    result
  end
  def self.memo_array = all.each_with_object([]) { |entry, result| result << entry }
  def self.class_keys = [1].each_with_object({}) { |entry, result| result[Beacon] = "class" }
  def self.registry
    @registry ||= all.each_with_object({}) do |entry, result|
      result[entry.label] = entry
    end.freeze
  end
  def self.named(name) = registry[name.to_s]
  def self.description(name) = named(name).label
  def self.unknown(value) = [value]
  def self.run
    [all.first.label, mapped.first.label, copied.first.label,
      written["class"].label, description("class"), instances.first.label]
  end
end
"#;

fn emit(source: &str) -> Vec<roundhouse::emit::EmittedFile> {
    let tree = [
        ("db/schema.rb", "ActiveRecord::Schema.define do\nend\n"),
        ("app/services/catalog.rb", source),
    ].into_iter().map(|(p, s)| (PathBuf::from(p), s.as_bytes().to_vec())).collect();
    let mut app = roundhouse::ingest::ingest_app_from_tree(tree).unwrap();
    roundhouse::session::analyze_and_lower(&mut app);
    roundhouse::emit::ruby::emit_library(&app)
}

fn rbs(source: &str) -> String {
    emit(source).into_iter().filter(|f| f.path.extension().is_some_and(|e| e == "rbs"))
        .map(|f| f.content).collect::<Vec<_>>().join("\n")
}

#[test]
fn literals_maps_and_writes_keep_class_elements() {
    let rbs = rbs(SOURCE);
    for expected in [
        "def self.all: () -> Array[singleton(Beacon)]",
        "def self.mapped: () -> Array[singleton(Beacon)]",
        "def self.copied: () -> Array[singleton(Beacon)]",
        "def self.nested: () -> Array[Hash[String, singleton(Beacon)]]",
        "def self.keyed: () -> Hash[singleton(Beacon), String]",
        "def self.written: () -> Hash[untyped, singleton(Beacon)]",
        "def self.pushed: () -> Array[singleton(Beacon)]",
        "def self.memo_array: () -> Array[singleton(Beacon)]",
        "def self.class_keys: () -> Hash[singleton(Beacon), String]",
        "def self.registry: () -> Hash[String, singleton(Beacon)]",
        "def self.named: (String name) -> singleton(Beacon)?",
        "def self.instances: () -> Array[Beacon]",
        "def self.mixed: (untyped flag) -> Array[(Beacon | singleton(Beacon))]",
        "def self.tuple: () -> [singleton(Beacon), Beacon]",
        "def self.unknown: (untyped value) -> Array[untyped]",
    ] {
        assert!(rbs.contains(expected), "missing {expected}\n{rbs}");
    }
}

const MAP: &str = "  def self.mapped = [1].map { Beacon }";

#[test]
fn next_values_join_the_tail_without_losing_class_identity() {
    for body in [
        "next Beacon.new if flag; Beacon",
        "next Beacon if flag; Beacon.new",
    ] {
        let source = SOURCE.replace(MAP, &format!("  def self.mapped = [true, false].map {{ |flag| {body} }}"));
        let rbs = rbs(&source);
        assert!(rbs.contains("def self.mapped: () -> Array[(Beacon | singleton(Beacon))]"), "{rbs}");
    }
    for (value, expected) in [("", "singleton(Beacon)?"), ("false", "(bool | singleton(Beacon))")] {
        let source = SOURCE.replace(MAP, &format!("  def self.mapped = [true, false].map {{ |flag| next {value} if flag; Beacon }}"));
        let rbs = rbs(&source);
        assert!(rbs.contains(&format!("def self.mapped: () -> Array[{expected}]")), "{rbs}");
    }
    let source = SOURCE.replace(MAP,
        "  def self.mapped = [1].map { unused = -> { next Beacon.new }; Beacon }");
    assert!(rbs(&source).contains("def self.mapped: () -> Array[singleton(Beacon)]"));
}

#[test]
fn filter_map_keeps_the_truthy_class_value() {
    let source = SOURCE.replace(MAP,
        "  def self.mapped = [true, false].filter_map { |flag| flag && Beacon }");
    let rbs = rbs(&source);
    assert!(rbs.contains("def self.mapped: () -> Array[singleton(Beacon)]"), "{rbs}");
}

fn map_sources() -> Vec<(String, &'static str)> {
    vec![
        (SOURCE.replace(MAP, "  def self.mapped = [true, false].map { |flag| next Beacon.new if flag; Beacon }"),
            "class\ninstance\nclass\nclass\nclass\ninstance\n"),
        (SOURCE.replace(MAP, "  def self.mapped = [true, false].map { |flag| next false if flag; Beacon }")
            .replace("mapped.first.label", "mapped.first.to_s"),
            "class\nfalse\nclass\nclass\nclass\ninstance\n"),
        (SOURCE.replace(MAP, "  def self.mapped = [true, false].map { |flag| next if flag; Beacon }")
            .replace("mapped.first.label", "mapped.first.to_s"),
            "class\n\nclass\nclass\nclass\ninstance\n"),
        (SOURCE.replace(MAP, "  def self.mapped = [true, false].filter_map { |flag| flag && Beacon }"),
            "class\nclass\nclass\nclass\nclass\ninstance\n"),
        (SOURCE.replace("mapped.first.label", "class_keys.keys.first.label"),
            "class\nclass\nclass\nclass\nclass\ninstance\n"),
    ]
}

fn overridden_source() -> String {
    let overrides = r#"class Selection
  def map = Beacon.new
  def each_with_object(seed) = "own memo"
end
class InheritedSelection < Selection
end
"#;
    SOURCE.replace("module Catalog", &format!("{overrides}\nmodule Catalog"))
        .replace("  def self.all", "  def self.custom = InheritedSelection.new.map { Beacon }\n  def self.custom_memo = InheritedSelection.new.each_with_object({}) { |entry, memo| memo[\"class\"] = Beacon }\n  def self.all")
        .replace("written[\"class\"].label, description(\"class\"), instances.first.label]",
            "written[\"class\"].label, description(\"class\"), instances.first.label, custom.label, custom_memo]")
}

#[test]
fn inherited_app_overrides_keep_their_own_return_types() {
    let rbs = rbs(&overridden_source());
    assert!(rbs.contains("def self.custom: () -> Beacon"), "{rbs}");
    assert!(rbs.contains("def self.custom_memo: () -> String"), "{rbs}");
}

#[test]
fn rooted_constants_and_block_copies_keep_the_same_elements() {
    let source = SOURCE.replace("[Beacon]", "[::Beacon]")
        .replace("{ Beacon }", "{ copy = ::Beacon; copy }")
        .replace("= entry\n", "= (copy = entry)\n");
    let rbs = rbs(&source);
    for method in ["all", "mapped", "copied"] {
        assert!(rbs.contains(&format!("def self.{method}: () -> Array[singleton(Beacon)]")), "{rbs}");
    }
    assert!(rbs.contains("def self.registry: () -> Hash[String, singleton(Beacon)]"), "{rbs}");
}

fn run(source: &str, native: bool, expected: &str) {
    let dir = scratch::dir("class_container");
    let mut script = String::new();
    for file in emit(source) {
        let path = dir.join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &file.content).unwrap();
        if path.extension().is_some_and(|e| e == "rb") {
            script.push_str(&format!("require_relative {:?}\n", file.path.to_string_lossy()));
        }
    }
    script.push_str("puts Catalog.run\n");
    std::fs::write(dir.join("contract.rb"), script).unwrap();
    let output = if native {
        let compiler = std::env::var("SPINEL").unwrap_or_else(|_| "spinel".into());
        let built = std::process::Command::new(compiler)
            .args(["--rbs", "sig", "contract.rb", "-o", "contract"])
            .current_dir(&dir).output().unwrap();
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        std::process::Command::new(dir.join("contract")).current_dir(&dir).output().unwrap()
    } else {
        std::process::Command::new("ruby").arg("contract.rb").current_dir(&dir).output().unwrap()
    };
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn containers_run_as_emitted_ruby() {
    run(SOURCE, false, "class\nclass\nclass\nclass\nclass\ninstance\n");
    run(&overridden_source(), false, "class\nclass\nclass\nclass\nclass\ninstance\ninstance\nown memo\n");
    for (source, expected) in map_sources() {
        run(&source, false, expected);
    }
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn containers_run_on_spinel_with_generated_sidecars() {
    run(SOURCE, true, "class\nclass\nclass\nclass\nclass\ninstance\n");
    run(&overridden_source(), true, "class\nclass\nclass\nclass\nclass\ninstance\ninstance\nown memo\n");
    for (source, expected) in map_sources() {
        run(&source, true, expected);
    }
}
