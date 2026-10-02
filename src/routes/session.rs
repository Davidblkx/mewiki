//! Logging in and out.

use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, extract::rejection::FormRejection};
use serde::Deserialize;

use super::{Chrome, Here, render};
use crate::App;
use crate::auth::safe_next;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    chrome: Chrome,
    next: String,
    error: bool,
}

/// Where to go after logging in.
#[derive(Deserialize)]
pub struct NextQuery {
    next: Option<String>,
}

/// The login form's fields.
#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
    next: Option<String>,
}

/// `GET /_/login`: the login form. An owner who is already logged in goes straight on to `next`.
pub async fn login_page(State(app): State<Arc<App>>, headers: HeaderMap, Query(query): Query<NextQuery>) -> Response {
    let next = safe_next(query.next.as_deref()).to_owned();
    if app.auth.is_owner(&headers) {
        return Redirect::to(&next).into_response();
    }
    login_form(&app, next, false, StatusCode::OK)
}

/// `POST /_/login`: checks the password and, when it matches, starts a session and goes on to `next`.
pub async fn login(State(app): State<Arc<App>>, form: Result<Form<LoginForm>, FormRejection>) -> Response {
    let Ok(Form(form)) = form else {
        return login_form(&app, "/".to_owned(), true, StatusCode::BAD_REQUEST);
    };
    let next = safe_next(form.next.as_deref()).to_owned();
    if !app.auth.login(&form.password).await {
        return login_form(&app, next, true, StatusCode::UNAUTHORIZED);
    }
    (
        [(header::SET_COOKIE, app.auth.new_session_cookie())],
        Redirect::to(&next),
    )
        .into_response()
}

/// `POST /_/logout`: clears the session cookie in this browser. Pages saved for offline reading stay.
pub async fn logout(State(app): State<Arc<App>>) -> Response {
    (
        [(header::SET_COOKIE, app.auth.clear_session_cookie())],
        Redirect::to("/"),
    )
        .into_response()
}

fn login_form(app: &App, next: String, error: bool, status: StatusCode) -> Response {
    let template = LoginTemplate {
        chrome: Chrome::new(app, "Log in", Here::Other, false),
        next,
        error,
    };
    render(status, &template)
}
