//! Installed stdlib gems use the compiler's runtime unless source_gems opts in.
use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::gems::{fate_of, GemFate};
use roundhouse::ingest::ingest_app_from_tree;

#[test]
fn packaged_standard_libraries_are_not_unknown_gems() {
    let missing: Vec<_> = [
        "date", "did_you_mean", "fiddle", "fileutils", "find", "forwardable", "getoptlong",
        "net-protocol", "open3", "openssl", "optparse", "pathname", "pp", "prettyprint",
        "psych", "resolv", "set", "shellwords", "singleton", "stringio", "strscan", "tempfile",
        "time", "timeout", "tmpdir", "weakref", "zlib",
    ].into_iter().filter(|gem| fate_of(gem) != GemFate::Stdlib).collect();
    assert!(missing.is_empty(), "stdlib gems classified as source candidates: {missing:?}");
    assert_eq!(fate_of("unfamiliar-library"), GemFate::Unknown);
}

fn app(gem: &str, constant: &str, explicit: bool, transitive: bool) -> roundhouse::App {
    let mut files = HashMap::<PathBuf, Vec<u8>>::new();
    let mut write = |path: &str, source: &str| { files.insert(path.into(), source.as_bytes().to_vec()); };
    write(".bundle/config", "BUNDLE_PATH: vendor/bundle\n");
    write("roundhouse.yml", "extensions: [ext]\n");
    let mut facts = String::from("source_gems:\n");
    if explicit { facts.push_str(&format!("  - {{gem: {gem}}}\n")); }
    if transitive { facts.push_str("  - {gem: messenger}\n"); }
    if !explicit && !transitive { facts = String::new(); }
    write("ext/gems.yml", &facts);
    let source = if transitive {
        format!("    messenger (1.0)\n      {gem}\n")
    } else { String::new() };
    let direct = if transitive { "messenger" } else { gem };
    write("Gemfile.lock", &format!("GEM\n  remote: https://rubygems.org/\n  specs:\n    {gem} (1.0)\n{source}\nDEPENDENCIES\n  {direct}\n"));
    let feature = gem.replace('-', "/");
    write(&format!("vendor/bundle/ruby/3.4.0/gems/{gem}-1.0/lib/{feature}.rb"),
          &format!("module {constant}\n  def self.source_marker\n    42\n  end\nend\n"));
    if transitive {
        write("vendor/bundle/ruby/3.4.0/gems/messenger-1.0/lib/messenger.rb",
              &format!("class Messenger\n  def self.value\n    {constant}.source_marker\n  end\nend\n"));
    }
    write("lib/probe.rb", &format!("class Probe\n  def value\n    {constant}.source_marker\n  end\nend\n"));
    ingest_app_from_tree(files).expect("ingest")
}

#[test]
fn installed_stdlib_sources_stay_out_for_direct_and_sourced_dependencies() {
    for (gem, constant) in [("net-protocol", "Net"), ("fileutils", "FileUtils"), ("timeout", "Timeout")] {
        for transitive in [false, true] {
            let app = app(gem, constant, false, transitive);
            let lock = app.gem_lock.as_ref().unwrap();
            assert!(!lock.is_source(gem), "{gem} auto-sourced, transitive={transitive}");
            assert!(!app.sources.iter().any(|s| s.path.contains(&format!("{gem}-1.0/lib/"))),
                    "{gem} source read, transitive={transitive}");
            if transitive { assert!(lock.is_source("messenger")); }
        }
    }
}

#[test]
fn explicit_source_gems_still_compiles_stdlib_source() {
    let app = app("net-protocol", "Net", true, false);
    assert!(app.gem_lock.as_ref().unwrap().is_source("net-protocol"));
    assert!(app.sources.iter().any(|s| s.path.contains("net-protocol-1.0/lib/net/protocol.rb")));
    assert!(app.library_classes.iter().any(|c| c.name.0.as_str() == "Net"));
}

#[test]
fn unfamiliar_installed_gems_still_compile_from_source() {
    let app = app("widgets", "Widgets", false, false);
    assert!(app.gem_lock.as_ref().unwrap().is_source("widgets"));
    assert!(app.library_classes.iter().any(|c| c.name.0.as_str() == "Widgets"));
}
