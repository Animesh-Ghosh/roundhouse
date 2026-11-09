//! Rails' ActionController::Base includes `ActionView::Rendering` (through `ActionView::Layouts`); the
//! responders gem asks a controller's class for it before it renders a default template.

use super::emit_and_run;

const SCRIPT: &str = r#"raise "Base" unless ActionController::Base.include?(ActionView::Rendering)
puts "view rendering passed"
"#;

#[test]
fn a_controller_class_includes_action_view_rendering() {
    let run = emit_and_run::real_blog().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("view rendering passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_controller_class_includes_action_view_rendering_natively() {
    let run = emit_and_run::real_blog().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("view rendering passed"), "{}", run.stdout);
}
