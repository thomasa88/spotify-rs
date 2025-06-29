use std::{collections::HashMap, env, path::Path, time::Duration};

use anyhow::Context;
use spotify_rs::{AuthCodeClient, RedirectUrl, Token, client::Client};
use tokio::io::AsyncBufReadExt;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _sub = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::new("trace,hyper=debug"))
        .init();

    dotenvy::dotenv()?;
    let client_id = env::var("CLIENT_ID")?;
    let client_secret = env::var("CLIENT_SECRET")?;
    let redirect_uri = env::var("REDIRECT_URI")?;
    let auto_refresh = true;
    let scopes: Vec<&'static str> = vec![
        "playlist-modify-public",
        "playlist-modify-private",
        "playlist-read-private",
    ];

    if let Ok(refresh_token) = load_refresh_token() {
        info!("Continue session");
        let auto_refresh = true;
        let scopes = None;
        let refresh_client = Client::from_refresh_token(
            client_id,
            Some(&client_secret),
            scopes,
            auto_refresh,
            refresh_token,
        )
        .await?;

        info!("Grab items");
        let playlist = spotify_rs::playlist(env::var("PLAYLIST_ID")?)
            .get(&refresh_client)
            .await?;
        let tracks = playlist.tracks.get_all(&refresh_client).await?;
        tracks.iter().for_each(|_| {});
    } else {
        info!("New session");
        let (auth_client, url) = AuthCodeClient::new(
            client_id,
            client_secret,
            scopes,
            RedirectUrl::new(redirect_uri)?,
            auto_refresh,
        );

        eprintln!(
            "Open the following URL with the account that should authorize the session:\n{url}"
        );
        let mut url_buf = String::new();
        // tokio does not have stdin()::read_line().
        let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let mut u;
        let mut query: HashMap<_, _>;
        let (auth_code, csrf_state) = loop {
            eprintln!("Enter the redirection URL:");
            stdin.read_line(&mut url_buf).await?;
            u = match url::Url::parse(&url_buf) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Parse error: {e}");
                    continue;
                }
            };

            query = u.query_pairs().collect();
            // if let (auth_code = query.get("auth_code");
            // let csrf_state = query.get("csrf_state");
            let (Some(auth_code), Some(csrf_state)) = (query.get("code"), query.get("state"))
            else {
                eprintln!("Failed to find code and/or state in query string.");
                continue;
            };
            break (auth_code, csrf_state);
        };

        let auth_client = auth_client
            .authenticate(auth_code.clone().into_owned(), csrf_state)
            .await?;

        info!("Logged in");

        if let Err(e) = save_refresh_token(&auth_client) {
            warn!("Failed to save Spotify token: {e}");
        }

        println!("Run the program again to use the refresh token");
    }

    Ok(())
}

fn save_refresh_token(client: &AuthCodeClient<Token>) -> anyhow::Result<()> {
    // The token (lock) must not be held across await points (there are clone variants available).
    let token = client.token();
    let token = token.read().expect("Failed to lock token for saving");

    let refresh_token = token
        .refresh_secret()
        .context("New client should have a refresh token")?;

    let path = Path::new("refresh_token");
    // serde_json does not support AsyncWrite with to_writer(),
    // so using the sync version. It should be fast enough..
    let file = std::fs::File::create(path).context("Failed to create token file")?;
    let writer = std::io::BufWriter::new(file);
    serde_json::to_writer(writer, refresh_token)?;
    info!("Saved refresh token to {path:?}");
    Ok(())
}

fn load_refresh_token() -> anyhow::Result<String> {
    let path = Path::new("refresh_token");
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let token = serde_json::from_reader(reader)?;
    info!("Loaded refresh token from {path:?}");
    Ok(token)
}
