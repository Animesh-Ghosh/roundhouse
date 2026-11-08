//! A request constraint, a lambda or an object Rails asks with the
//! request (`constraints: ->(req) { req.format == :json }`,
//! `constraints(SomeGuard.new) do … end`), guards its routes. The
//! Ruby-family router asks it once the path matches
//! (`RoundhouseRouteGuards`), and a request it refuses reaches the routes
//! after it. A target whose router matches by verb and path alone refuses
//! the routes instead, with an error where the constraint is written:
//! serving them answered every request the constraint refuses, which left
//! a later route on the same path unreachable (#727). A constraint no
//! router can ask is refused at ingest.

use roundhouse::App;
use roundhouse::ingest::ingest_routes;
use roundhouse::lower::route_guards::{guard_class, route_guard_gaps_for_target};
use roundhouse::lower::routes::{flatten_routes, flatten_routes_asking_guards, flatten_routes_for_helpers};
use roundhouse::project::BuildTarget;

fn app(source: &str) -> App {
    let mut app = App::default();
    app.routes = ingest_routes(format!("Rails.application.routes.draw do\n{source}\nend\n").as_bytes(), "config/routes.rb")
        .expect("ingest routes");
    app
}

fn strict_refusals(app: &App, target: BuildTarget) -> usize {
    route_guard_gaps_for_target(app, target).iter().filter(|d| d.severity == roundhouse::diagnostic::Severity::Error).count()
}

#[test]
fn a_lambda_constraint_is_asked_by_the_ruby_family_and_refused_elsewhere() {
    let app = app(
        r#"get "/invite/:code", to: "widgets#show", constraints: ->(req) { req.format == :json }
get "/invite/:code", to: "gadgets#new""#,
    );
    let asked: Vec<(String, Vec<usize>)> =
        flatten_routes_asking_guards(&app).into_iter().map(|(r, guards)| (r.controller.0.as_str().to_string(), guards)).collect();
    assert_eq!(asked, [("WidgetsController".to_string(), vec![0]), ("GadgetsController".to_string(), vec![])], "{asked:?}");
    let (class, rbs) = guard_class(&app.routes.entries).expect("a guard method");
    assert_eq!(class.methods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["guard_0"]);
    assert!(rbs.contains("def self.guard_0: (ActionDispatch::Request req) -> bool"), "{rbs}");
    assert!(!app.routes.diagnostics.iter().any(|d| d.message.contains("request constraint")), "{:#?}", app.routes.diagnostics);
    let dispatched: Vec<String> = flatten_routes(&app).iter().map(|r| r.controller.0.as_str().to_string()).collect();
    assert_eq!(dispatched, ["GadgetsController"], "{dispatched:?}");
    assert_eq!(strict_refusals(&app, BuildTarget::Rust), 1);
    assert_eq!(strict_refusals(&app, BuildTarget::Spinel), 0);
}

#[test]
fn a_guard_object_block_is_asked_with_matches_and_keeps_its_helpers() {
    let app = app(
        r#"constraints(AdminGuard.new) do
  get "/reports", to: "reports#index", as: :reports
  resources :audits, only: :index
end
get "/ping", to: "pings#show""#,
    );
    let asked: Vec<(String, Vec<usize>)> = flatten_routes_asking_guards(&app).into_iter().map(|(r, guards)| (r.path, guards)).collect();
    assert_eq!(asked, [("/reports".to_string(), vec![0]), ("/audits".to_string(), vec![0]), ("/ping".to_string(), vec![])], "{asked:?}");
    let (class, _) = guard_class(&app.routes.entries).expect("a guard method");
    assert_eq!(roundhouse::emit::ruby::emit_expr(&class.methods[0].body), "AdminGuard.new.matches?(request)");
    let helpers: Vec<String> = flatten_routes_for_helpers(&app).iter().map(|r| r.as_name.clone()).collect();
    assert!(helpers.iter().any(|h| h == "reports") && helpers.iter().any(|h| h == "audits"), "{helpers:?}");
    assert_eq!(strict_refusals(&app, BuildTarget::Crystal), 1, "one error at the constraint");
}

/// A constraint whose expression does not ingest (`->(req, &block) { … }`) is refused where it is
/// written all the same: it is never run, so the rest of the table and the route's helper stand.
#[test]
fn a_constraint_that_does_not_ingest_is_still_refused_in_place() {
    let app = app(
        r#"get "/reports", to: "reports#index", as: :reports, constraints: ->(req, &block) { true }
get "/ping", to: "pings#show""#,
    );
    let flat = flatten_routes(&app);
    let dispatched: Vec<&str> = flat.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(dispatched, ["/ping"], "{dispatched:?}");
    assert!(flatten_routes_for_helpers(&app).iter().any(|r| r.as_name == "reports"));
    let error = app.routes.diagnostics.iter().find(|d| d.message.contains("request constraint")).expect("located refusal");
    assert!(!error.span.is_synthetic(), "{error:?}");
}

#[test]
fn constraints_that_do_not_ingest_while_surveying_keep_their_own_places() {
    roundhouse::ingest::survey::activate();
    let app = app(
        r#"get "/reports", to: "reports#index", constraints: ->(req, &block) { true }
get "/audits", to: "audits#index", constraints: ->(req, &block) { true }"#,
    );
    roundhouse::ingest::survey::drain();
    let spans: Vec<_> = app.routes.diagnostics.iter().filter(|d| d.message.contains("request constraint")).map(|d| d.span).collect();
    assert_eq!(spans.len(), 2, "{:#?}", app.routes.diagnostics);
    assert!(spans.iter().all(|s| !s.is_synthetic()) && spans[0] != spans[1], "{spans:?}");
}
