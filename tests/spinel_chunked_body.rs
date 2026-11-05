use std::path::Path;
use std::process::Command;

#[path = "support/emit_and_run.rs"]
mod emit_and_run;
#[path = "support/native_http.rs"]
mod native_http;

fn ruby_driver(name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("ruby")
        .arg(root.join(format!("tests/{name}.rb")))
        .output()
        .expect("ruby is on PATH");
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("done") || stdout.contains("skip:"),
        "{stdout}"
    );
}

#[test]
fn chunked_framing_across_all_servers() {
    ruby_driver("spinel_chunked_body");
}

#[test]
fn puma_confirms_the_chunked_body_contract() {
    ruby_driver("spinel_chunked_body_puma");
}

#[test]
#[ignore = "deterministic CRuby slice-work contract; run in the framework lane"]
fn chunk_count_has_linear_cost() {
    ruby_driver("spinel_chunked_body_work");
}

#[test]
#[ignore = "requires native Spinel; set SPINEL to its executable"]
fn chunked_body_on_spinel() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("roundhouse-chunked-body-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("server");
    let compiled = Command::new(std::env::var("SPINEL").unwrap_or_else(|_| "spinel".into()))
        .arg(root.join("tests/spinel_chunked_body_native.rb"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile native server");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let out = Command::new("ruby")
        .arg(root.join("tests/spinel_chunked_body_native_check.rb"))
        .arg(&binary)
        .output()
        .expect("drive native server");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "requires the native Spinel toolchain"]
fn generated_controller_receives_decoded_params_and_headers() {
    let (tree, errors) = emit_and_run::real_blog()
        .edit(
            "config/routes.rb",
            "  root \"articles#index\"\n",
            "  root \"articles#index\"\n  post '/frames', to: 'frames#echo'\n",
        )
        .write(
            "app/controllers/frames_controller.rb",
            r#"
class FramesController < ActionController::API
  def echo
    render plain: params[:title].to_s + "|" + request.headers["Content-Length"].to_s +
      "|" + request.headers["Transfer-Encoding"].to_s
  end
end
"#,
        )
        .emit(roundhouse::project::BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:?}");
    native_http::build(&tree);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("ruby")
        .arg(root.join("tests/spinel_chunked_body_app_check.rb"))
        .arg(tree.join("build/bin/blog"))
        .current_dir(&tree)
        .output()
        .expect("drive generated controller");
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
