//! The Rust target's model surface for an app whose model tests build
//! their own records (`support/rust_record_tests_app.rs`). The first two
//! tests read the emitted tree; the `#[ignore]`d one runs its `cargo
//! test`:
//!
//!     cargo test --test rust_record_tests -- --ignored

use std::process::Command;

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;
#[path = "support/rust_record_tests_app.rs"]
mod rust_record_tests_app;

/// An app whose model tests build their own records compiles on the Rust
/// target: the association reflection's `macro` field is raw-escaped, an
/// abstract `ApplicationRecord` (`self.abstract_class = true`) gets no
/// `to_param` over an `id` it lacks, a model answers `find_by`,
/// `create!`, `valid?`, `destroy!` and `==`, a test helper's omitted
/// keyword defaults are written in at the call, and `setup()` brings up
/// the schema with no fixtures to load.
#[test]
fn record_tests_app_emits_the_model_surface_its_tests_call() {
    let (emitted, errors) = rust_record_tests_app::app().emit(roundhouse::project::BuildTarget::Rust);
    assert!(errors.is_empty(), "{errors:?}");
    let read = |path: &str| std::fs::read_to_string(emitted.join(path)).unwrap_or_else(|_| panic!("{path}"));

    let base = read("src/active_record_base.rs");
    assert!(base.contains("pub r#macro: String,") && !base.contains("pub macro:"), "{base}");

    let record = read("src/models/application_record.rs");
    assert!(!record.contains("fn to_param") && !record.contains("self.id"), "{record}");

    let shelf = read("src/models/shelf.rs");
    for want in ["pub fn find_by<", "pub fn create_bang<", "pub fn valid_pred(&mut self)", "pub fn destroy_bang(&mut self)", "impl PartialEq for Shelf", "impl PartialEq<Option<Shelf>> for Shelf"] {
        assert!(shelf.contains(want), "missing `{want}`:\n{shelf}");
    }

    let fixtures = read("src/fixtures/mod.rs");
    assert!(fixtures.contains("crate::db::setup_test_db(crate::schema_sql::CREATE_TABLES);"), "{fixtures}");

    let item_test = read("src/tests/item.rs");
    assert!(item_test.contains("make_shelf(1_i64, 2_i64)") && !item_test.contains("Self::make_shelf"), "{item_test}");
}

/// `validates :x, presence:` on a NOT NULL number with no default cannot
/// fire on a strict target, whose field starts at 0, so the build reports
/// it rather than claiming it. A String column (`name`) checks blank, and
/// the ruby family keeps nil, so neither is reported.
#[test]
fn presence_on_a_not_null_number_is_reported_on_strict_targets() {
    let (_emitted, app, _errors) = rust_record_tests_app::app().emit_with_app(roundhouse::project::BuildTarget::Rust);
    let names = |target| -> Vec<String> {
        roundhouse::lower::validations::presence_gaps_for_target(&app, target)
            .into_iter()
            .map(|d| d.message)
            .collect()
    };
    let rust = names(roundhouse::project::BuildTarget::Rust);
    assert_eq!(rust.len(), 3, "{rust:?}");
    for attr in ["aisle", "position", "weight"] {
        assert!(rust.iter().any(|m| m.contains(&format!("validates :{attr}, presence:"))), "{attr}: {rust:?}");
    }
    assert!(names(roundhouse::project::BuildTarget::Ruby).is_empty());
}

/// The same app's emitted `cargo test`: every test whose rules the target
/// lowers passes (`rust_record_tests_app::PASSING`).
#[test]
#[ignore]
fn record_tests_app_cargo_test_passes_what_it_lowers() {
    let (emitted, errors) = rust_record_tests_app::app().emit(roundhouse::project::BuildTarget::Rust);
    assert!(errors.is_empty(), "{errors:?}");
    let output = Command::new("cargo")
        .args(["test", "--", "--test-threads=1"])
        .current_dir(&*emitted)
        .output()
        .expect("run cargo test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test result:"), "the emitted project did not build:\n{}", String::from_utf8_lossy(&output.stderr));
    for name in rust_record_tests_app::PASSING {
        assert!(stdout.contains(&format!("tests::{name} ... ok")), "{name}:\n{stdout}");
    }
}
