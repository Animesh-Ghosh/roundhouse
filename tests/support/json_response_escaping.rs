use super::emit_and_run;
use serde_json::Value;

pub fn expected() -> Vec<Value> {
    serde_json::from_str(include_str!("json_response_escaping/rails.json")).unwrap()
}

pub fn app(defaults: &str) -> emit_and_run::Overlay {
    let defaults = if defaults == "none" {
        String::new()
    } else {
        format!("    config.load_defaults {defaults}\n")
    };
    let mut controller = String::from("class PayloadsController < ActionController::API\n");
    let mut routes = String::from("Rails.application.routes.draw do\n");
    for (form, source) in [
        ("hash", r#"JSON.parse('{"text":"<>&\u2028\u2029"}')"#),
        ("array", r#"JSON.parse('["<>&\u2028\u2029"]')"#),
        (
            "nested",
            r#"JSON.parse('{"outer":[{"text":"<>&\u2028\u2029"}]}')"#,
        ),
        ("string", r#""<>&\u2028\u2029""#),
        ("hash_primitive", r#"{text: "<>&\u2028\u2029"}"#),
        ("array_primitive", r#"["<>&\u2028\u2029"]"#),
        (
            "nested_primitive",
            r#"{outer: [{text: "<>&\u2028\u2029"}]}"#,
        ),
    ] {
        for callback in [false, true] {
            let name = format!("{form}{}", if callback { "_callback" } else { "" });
            let option = if callback {
                ", callback: \"receive\""
            } else {
                ""
            };
            controller.push_str(&format!(
                "  def {name}\n    payload = {source}\n    render json: payload{option}\n  end\n"
            ));
            routes.push_str(&format!("  get '/{name}', to: 'payloads#{name}'\n"));
        }
    }
    controller.push_str("end\n");
    routes.push_str("end\n");
    emit_and_run::empty_app()
        .write("config/application.rb", &format!("module EscapingApplication\n  class Application < Rails::Application\n{defaults}  end\nend\n"))
        .write("config/environments/force_true.rb", "Rails.application.configure do\n  config.action_controller.escape_json_responses = true\nend\n")
        .write("config/environments/force_false.rb", "Rails.application.configure do\n  config.action_controller.escape_json_responses = false\nend\n")
        .write("config/environments/self_defaults.rb", "Rails.application.configure do\n  self.config.load_defaults 8.1\nend\n")
        .write("config/environments/false_then_older.rb", "Rails.application.configure do\n  self.config.action_controller.escape_json_responses = false\n  self.config.active_support.escape_js_separators_in_json = false\n  self.config.load_defaults 8.0\nend\n")
        .write("config/routes.rb", &routes)
        .write("db/schema.rb", "ActiveRecord::Schema.define do\nend\n")
        .write("app/controllers/payloads_controller.rb", &controller)
}

pub fn program(defaults: &str) -> String {
    let mut program = String::from("require_relative \"app/controllers/payloads_controller\"\n");
    let mut rows: Vec<_> = expected().into_iter().filter(|r| r["defaults"] == defaults)
        .map(|row| {
            let env = match row["explicit"].as_str().unwrap() {
                "true" => "force_true", "false" => "force_false", _ => "test",
            };
            (row, env)
        }).collect();
    // Both environments end with the same two false flags as 8.1 defaults.
    // An older load_defaults call must not reset explicit false values.
    for env in ["self_defaults", "false_then_older"] {
        rows.extend(expected().into_iter()
            .filter(|r| r["defaults"] == "8.1" && r["explicit"] == "unset")
            .map(|row| (row, env)));
    }
    for (row, env) in rows {
        let form = row["form"].as_str().unwrap();
        let callback = if row["callback"] == true {
            "_callback"
        } else {
            ""
        };
        let body = serde_json::to_string(&row["body"]).unwrap();
        for primitive in [false, true] {
            if primitive && form == "string" {
                continue;
            }
            let action = format!(
                "{form}{}{callback}",
                if primitive { "_primitive" } else { "" }
            );
            program.push_str(&format!("Rails.env_name = {env:?}\nc = PayloadsController.new\nc.process_action(:{action})\nraise {action:?} unless c.body.bytes == ({body}).bytes\nraise \"status\" unless c.status == 200\n"));
        }
    }
    program.push_str("puts 'escaping responses passed'\n");
    program
}

pub fn oracle_matches_snapshot() {
    use std::path::Path;
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support/json_response_escaping/oracle.rb");
    for defaults in ["none", "8.0", "8.1"] {
        for explicit in ["unset", "false", "true"] {
            let output = emit_and_run::ruby()
                .arg(&script)
                .args([defaults, explicit])
                .output()
                .unwrap();
            if output.status.code() == Some(77) {
                eprintln!(
                    "skipping JSON response oracle: RailsOracle railties/actionpack unavailable"
                );
                return;
            }
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Vec<Value> = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let expected: Vec<_> = expected()
                .into_iter()
                .filter(|r| r["defaults"] == defaults && r["explicit"] == explicit)
                .collect();
            assert_eq!(actual, expected, "defaults={defaults}, explicit={explicit}");
        }
    }
}
