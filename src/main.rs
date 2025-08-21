use axum::{Router, extract::Query, response::Html, routing::get};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tokio::sync::oneshot;
use url::Url;

#[derive(Serialize, Deserialize, Clone)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    timestamp: u64,
}

#[derive(Deserialize)]
struct Track {
    item: Option<TrackItem>,
}

#[derive(Deserialize)]
struct TrackItem {
    id: String,
    name: String,
    artists: Vec<Artist>,
}

#[derive(Deserialize)]
struct Artist {
    name: String,
}

fn auth_url(client_id: &str) -> String {
    let scope = "playlist-read-private user-read-playback-state user-read-currently-playing user-library-read user-library-modify";
    let state = format!("{}", rand::random::<u32>());
    let redirect_uri = "http://127.0.0.1:8888/callback";

    let mut url = Url::parse("https://accounts.spotify.com/authorize").unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scope)
        .append_pair("state", &state);

    url.to_string()
}

async fn callback(
    Query(params): Query<HashMap<String, String>>,
    tx: tokio::sync::oneshot::Sender<String>,
) -> Html<&'static str> {
    if let Some(code) = params.get("code") {
        let _ = tx.send(code.clone());
        Html("<h1>Spotify auth complete. You can close this window.</h1>")
    } else {
        Html("<h1>Error: No code received</h1>")
    }
}

async fn start_server() -> Result<String, Box<dyn std::error::Error>> {
    use std::sync::{Arc, Mutex};
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let app = Router::new().route(
        "/callback",
        get({
            let tx = tx.clone();
            move |query| {
                let tx = tx.clone();
                async move {
                    let tx_opt = tx.lock().unwrap().take();
                    callback(query, tx_opt.expect("Sender already used")).await
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8888").await?;
    println!("Listening for Spotify callback on port 8888...");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let code = rx.await?;
    Ok(code)
}

async fn exchange_token(
    code: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Tokens, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let redirect_uri = "http://127.0.0.1:8888/callback";

    let mut form_data = HashMap::new();
    form_data.insert("grant_type", "authorization_code");
    form_data.insert("code", code);
    form_data.insert("redirect_uri", redirect_uri);

    let response = client
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(client_id, Some(client_secret))
        .form(&form_data)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err("Token exchange failed".into());
    }

    let data: Value = response.json().await?;

    let tokens = Tokens {
        access_token: data["access_token"].as_str().unwrap().to_string(),
        refresh_token: data["refresh_token"].as_str().unwrap().to_string(),
        expires_in: data["expires_in"].as_u64().unwrap_or(3600),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    Ok(tokens)
}

async fn refresh_token(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Tokens, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let mut form_data = HashMap::new();
    form_data.insert("grant_type", "refresh_token");
    form_data.insert("refresh_token", refresh_token);

    let response = client
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(client_id, Some(client_secret))
        .form(&form_data)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err("Refresh failed".into());
    }

    let data: Value = response.json().await?;

    let tokens = Tokens {
        access_token: data["access_token"].as_str().unwrap().to_string(),
        refresh_token: data["refresh_token"]
            .as_str()
            .unwrap_or(refresh_token)
            .to_string(),
        expires_in: data["expires_in"].as_u64().unwrap_or(3600),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    Ok(tokens)
}

fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazyfav")
}

async fn load_tokens() -> Option<Tokens> {
    let token_file = data_dir().join("spotify_tokens.json");

    match fs::read_to_string(token_file) {
        Ok(contents) => serde_json::from_str(&contents).ok(),
        Err(_) => None,
    }
}

async fn save_tokens(tokens: &Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = data_dir();
    fs::create_dir_all(&data_dir)?;

    let token_file = data_dir.join("spotify_tokens.json");
    let json = serde_json::to_string_pretty(tokens)?;
    fs::write(token_file, json)?;

    Ok(())
}

async fn playing_track(access_token: &str) -> Result<Option<Track>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://api.spotify.com/v1/me/player/currently-playing")
        .bearer_auth(access_token)
        .send()
        .await?;

    if response.status() == 204 {
        return Ok(None); // no track playing
    }

    if !response.status().is_success() {
        let error_text = response.text().await?;
        eprintln!("Fetch error: {}", error_text);
        return Ok(None);
    }

    let track: Track = response.json().await?;
    Ok(Some(track))
}

async fn is_liked(access_token: &str, track_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let url = format!(
        "https://api.spotify.com/v1/me/tracks/contains?ids={}",
        track_id
    );
    let response = client.get(&url).bearer_auth(access_token).send().await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        eprintln!("Fetch error: {}", error_text);
        return Ok(false);
    }

    let data: Vec<bool> = response.json().await?;
    Ok(data.get(0).copied().unwrap_or(false))
}

async fn like_song(access_token: &str, track_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let body = json!({ "ids": [track_id] });
    let response = client
        .put("https://api.spotify.com/v1/me/tracks")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await?;

    Ok(response.status().is_success())
}

#[cfg(target_os = "linux")]
async fn notify(title: &String, msg: &String) {
    Command::new("notify-send")
        .args(&[title, msg])
        .output()
        .expect("failed to send notifs");
}

#[cfg(target_os = "windows")]
async fn notify(title: &String, msg: &String) {
    Command::new("powershell")
        .args(&[
            "-Command",
            &format!("New-BurntToastNotification -Text '{}', '{}'", title, msg),
        ])
        .output()
        .expect("failed to send notifs");
}

#[cfg(target_os = "macos")]
async fn notify(title: &String, msg: &String) {
    Command::new("osascript")
        .args(&[
            "-e",
            &format!(
                "display notification \"{}\" with title \"{}\"",
                message, title
            ),
        ])
        .output()
        .expect("failed to send notifs");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = std::env::var("SPOTIFY_CLIENT_ID")
        .expect("SPOTIFY_CLIENT_ID environment variable must be set");
    let client_secret = std::env::var("SPOTIFY_CLIENT_SECRET")
        .expect("SPOTIFY_CLIENT_SECRET environment variable must be set");

    let mut tokens = load_tokens().await;

    if tokens.is_none() {
        println!("Opening browser to log in...");
        let auth_url = auth_url(&client_id);
        if let Err(e) = open::that(&auth_url) {
            println!("Failed to open browser: {}", e);
            println!("Please visit: {}", auth_url);
        }

        let code = start_server().await?;
        let new_tokens = exchange_token(&code, &client_id, &client_secret).await?;
        save_tokens(&new_tokens).await?;
        tokens = Some(new_tokens);
    }

    let mut tokens = tokens.unwrap();

    // check if refresh needed
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let age = current_time - tokens.timestamp;

    if age > tokens.expires_in - 60 {
        // println!("Access token expired. Refreshing...");
        tokens = refresh_token(&tokens.refresh_token, &client_id, &client_secret).await?;
        save_tokens(&tokens).await?;
    }

    let track = playing_track(&tokens.access_token).await?;

    match track {
        Some(track) if track.item.is_some() => {
            let item = track.item.unwrap();
            let artists: Vec<String> = item.artists.iter().map(|a| a.name.clone()).collect();

            // println!("Now playing: {} by {}", item.name, artists.join(", "));

            let is_liked = is_liked(&tokens.access_token, &item.id).await?;

            if is_liked {
                // println!("Playing track is already liked!");
                notify(
                    &format!("Now playing: {} by {}", item.name, &artists.join(", ")),
                    &"Playing track has already been liked!".to_string(),
                )
                .await;
            } else {
                if like_song(&tokens.access_token, &item.id).await? {
                    // println!("Track liked!");
                    notify(
                        &format!("Now playing: {} by {}", item.name, &artists.join(", ")),
                        &"Liking track...".to_string(),
                    )
                    .await;
                } else {
                    //println!("Failed to like track.");
                    notify(
                        &format!("Now playing: {} by {}", item.name, &artists.join(", ")),
                        &"Failed to like track.".to_string(),
                    )
                    .await;
                }
            }
        }
        _ => {
            println!("No track currently playing.");
        }
    }

    Ok(())
}
