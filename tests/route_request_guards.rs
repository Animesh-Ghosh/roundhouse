//! A request constraint, a lambda or an object Rails asks with the
//! request (`constraints: ->(req) { req.format == :json }`,
//! `constraints(SomeGuard.new) do … end`), guards its routes. The router
//! matches by verb and path alone, so it cannot check one: the routes it
//! guards are left out of the dispatch table and the constraint is an
//! error where it is written, and a request reaches the routes after
//! them. Serving them anyway answered every request the constraint
//! refuses, which left a later route on the same path unreachable (#727).

use roundhouse::App;
use roundhouse::ingest::ingest_routes;
use roundhouse::lower::routes::{flatten_routes, flatten_routes_for_helpers};

fn app(source: &str) -> App {
    let mut app = App::default();
    app.routes = ingest_routes(format!("Rails.application.routes.draw do\n{source}\nend\n").as_bytes(), "config/routes.rb")
        .expect("ingest routes");
    app
}

#[test]
fn a_lambda_constraint_leaves_its_route_out_and_the_next_one_answers() {
    let app = app(
        r#"get "/invite/:code", to: "widgets#show", constraints: ->(req) { req.format == :json }
get "/invite/:code", to: "gadgets#new""#,
    );
    let dispatched: Vec<(String, String)> = flatten_routes(&app).iter().map(|r| (r.path.clone(), r.controller.0.as_str().to_string())).collect();
    assert_eq!(dispatched, [("/invite/:code".to_string(), "GadgetsController".to_string())], "{dispatched:?}");
    let errors: Vec<_> = app.routes.diagnostics.iter().filter(|d| d.message.contains("request constraint")).collect();
    assert_eq!(errors.len(), 1, "{:#?}", app.routes.diagnostics);
    assert_eq!(errors[0].severity, roundhouse::diagnostic::Severity::Error);
}

#[test]
fn a_guard_object_block_refuses_every_route_inside_and_keeps_their_helpers() {
    let app = app(
        r#"constraints(AdminGuard.new) do
  get "/reports", to: "reports#index", as: :reports
  resources :audits, only: :index
end
get "/ping", to: "pings#show""#,
    );
    let flat = flatten_routes(&app);
    let dispatched: Vec<&str> = flat.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(dispatched, ["/ping"], "{dispatched:?}");
    let helpers: Vec<String> = flatten_routes_for_helpers(&app).iter().map(|r| r.as_name.clone()).collect();
    assert!(helpers.iter().any(|h| h == "reports") && helpers.iter().any(|h| h == "audits"), "{helpers:?}");
    let errors = app.routes.diagnostics.iter().filter(|d| d.message.contains("request constraint")).count();
    assert_eq!(errors, 1, "one error at the constraint: {:#?}", app.routes.diagnostics);
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
