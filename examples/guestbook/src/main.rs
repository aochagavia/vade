use axum::{
    Router,
    extract::{Form, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::prelude::*;
use jiff::Timestamp;
use serde::Deserialize;
use std::{env, fs};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct AppState {
    entries_dir: PathBuf,
    auth_username: String,
    auth_password: String,
}

#[derive(Deserialize)]
struct SignForm {
    name: String,
    message: String,
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let mut guestbook_entry_paths = Vec::new();
    let mut guestbook_entries = Vec::new();

    match fs::read_dir(&state.entries_dir) {
        Ok(entries) => {
            for entry in entries {
                let Ok(entry) = entry else { continue };
                guestbook_entry_paths.push(entry.path());
            }
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read entries dir",
            )
                .into_response();
        }
    }

    guestbook_entry_paths.sort();
    for entry in guestbook_entry_paths {
        let Ok(entry_str) = fs::read_to_string(entry) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to read entry").into_response();
        };

        guestbook_entries.push(entry_str);
    }

    let mut html = String::new();
    html.push_str(
        r#"<html><body>
<h1>~* GUESTBOOK *~</h1><hr>
<h2>Sign the Guestbook:</h2>
<form method="POST" action="/sign">
<b>Name:</b><br>
<input type="text" name="name" size="40" required><br><br>
<b>Message:</b><br>
<textarea name="message" rows="4" cols="50" required></textarea><br><br>
<input type="submit" value="Sign Guestbook">
</form><hr><h2>Previous Entries:</h2>
"#,
    );

    if guestbook_entries.is_empty() {
        html.push_str("<p><i>No entries yet. Be the first to sign!</i></p>");
    } else {
        for entry in guestbook_entries {
            html.push_str("<blockquote><pre>");
            // Note: this is vulnerable to XSS! Make sure you fix it in the unlikely case you reuse
            // this code for something serious.
            html.push_str(&entry);
            html.push_str("</pre></blockquote><hr>");
        }
    }

    html.push_str("</body></html>");
    Html(html).into_response()
}

async fn sign(State(state): State<AppState>, Form(input): Form<SignForm>) -> impl IntoResponse {
    if !input.message.trim().is_empty() {
        let timestamp = Timestamp::now();
        let timestamp_file = jiff::fmt::strtime::format("%Y%m%d_%H%M%S_%f", timestamp).unwrap();
        let timestamp_web = jiff::fmt::strtime::format("%Y-%m-%d %H:%M:%S", timestamp).unwrap();

        let filename = format!("{timestamp_file}.txt");
        let entry_content = format!(
            "Date: {timestamp_web}\nName: {}\n\n{}\n",
            input.name, input.message
        );

        let _ = fs::write(state.entries_dir.join(filename), entry_content);
    }

    Redirect::to("/")
}

async fn basic_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if let Some(auth_header) = headers.get("authorization")
        && let Ok(auth_header) = auth_header.to_str()
        && let Some(encoded_credentials) = auth_header.strip_prefix("Basic ")
        && let Ok(decoded) = BASE64_STANDARD.decode(encoded_credentials)
        && let Ok(credentials) = String::from_utf8(decoded)
        && let parts = credentials.splitn(2, ':').collect::<Vec<_>>()
        && let &[username, password] = parts.as_slice()
        && username == state.auth_username
        && password == state.auth_password
    {
        next.run(request).await
    } else {
        // Upon seeing this response, browsers will show a login prompt
        (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Basic realm=\"Guestbook\"")],
            "Unauthorized",
        )
            .into_response()
    }
}

fn main() -> std::io::Result<()> {
    let entries_dir = Path::new("entries");
    if !entries_dir.exists() {
        fs::create_dir_all(entries_dir)?;
    }

    let auth_username =
        std::env::var("AUTH_USERNAME").expect("AUTH_USERNAME environment variable must be set");
    let auth_password =
        std::env::var("AUTH_PASSWORD").expect("AUTH_PASSWORD environment variable must be set");

    let state = AppState {
        entries_dir: entries_dir.to_path_buf(),
        auth_username,
        auth_password,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/sign", post(sign))
        .fallback((StatusCode::NOT_FOUND, "Page not found"))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            basic_auth_middleware,
        ))
        .with_state(state);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let port = match env::var("PORT") {
        Ok(port) => port.parse::<u16>().unwrap(),
        Err(_) => 8080
    };
    rt.block_on(async move {
        let ip = Ipv4Addr::new(0, 0, 0, 0);
        let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(ip, port)).await?;
        println!("Listening on http://{}", listener.local_addr().unwrap());
        axum::serve(listener, app).await?;
        Ok(())
    })
}
