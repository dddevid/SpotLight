use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::net::TcpListener;
use std::io::{Read, Write};
use oauth2::{
    basic::BasicClient,
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl,
    TokenResponse, TokenUrl,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokens {
    pub web_token: AuthToken,
    pub playback_token: AuthToken,
}

impl AuthTokens {
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            // 1. Save to local config folder as tokens.json (reliable on all platforms)
            let mut path = crate::config::get_config_dir();
            std::fs::create_dir_all(&path).ok();
            path.push("tokens.json");
            if let Err(e) = std::fs::write(&path, &json) {
                eprintln!("[SpotLight] Error saving tokens to file: {}", e);
            }

            // 2. Also save to keyring as backup
            if let Ok(entry) = keyring::Entry::new("SpotLightApp", "OAuthTokens") {
                let _ = entry.set_password(&json);
            }
        }
    }

    pub fn load() -> Option<Self> {
        // 1. Try local config folder tokens.json first
        let mut path = crate::config::get_config_dir();
        path.push("tokens.json");
        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(tokens) = serde_json::from_str::<Self>(&json) {
                return Some(tokens);
            }
        }

        // 2. Fallback to keyring
        if let Ok(entry) = keyring::Entry::new("SpotLightApp", "OAuthTokens") {
            if let Ok(json) = entry.get_password() {
                if let Ok(tokens) = serde_json::from_str::<Self>(&json) {
                    return Some(tokens);
                }
            }
        }

        None
    }
}

pub async fn authenticate(config: &crate::config::AppConfig) -> anyhow::Result<AuthTokens> {
    if config.client_id.is_empty() {
        anyhow::bail!("Client ID mancante in config.toml!");
    }

    println!("Avvio del doppio login: 1 per i Dati (Ricerca/Playlist), 1 per il Player Audio...");
    let web_client_id = config.client_id.clone();
    let web_redirect = "http://127.0.0.1:8888/callback";

    let web_client = BasicClient::new(
        ClientId::new(web_client_id),
        if config.client_secret.is_empty() { None } else { Some(oauth2::ClientSecret::new(config.client_secret.clone())) },
        AuthUrl::new("https://accounts.spotify.com/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://accounts.spotify.com/api/token".to_string()).unwrap()),
    ).set_redirect_uri(RedirectUrl::new(web_redirect.to_string()).unwrap());

    let (web_pkce_challenge, web_pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut web_auth_request = web_client
        .authorize_url(CsrfToken::new_random)
        .add_scope(oauth2::Scope::new("user-read-private".to_string()))
        .add_scope(oauth2::Scope::new("playlist-read-private".to_string()))
        .add_scope(oauth2::Scope::new("playlist-read-collaborative".to_string()))
        .add_scope(oauth2::Scope::new("user-modify-playback-state".to_string()))
        .add_scope(oauth2::Scope::new("user-top-read".to_string()))
        .add_scope(oauth2::Scope::new("user-read-recently-played".to_string()));

    if config.client_secret.is_empty() {
        web_auth_request = web_auth_request.set_pkce_challenge(web_pkce_challenge);
    }

    let (mut web_auth_url, web_csrf) = web_auth_request.url();
    web_auth_url.query_pairs_mut().append_pair("show_dialog", "true");

    let expected_state = web_csrf.secret().clone();
    let web_server = tokio::task::spawn_blocking(move || {
        let listener = TcpListener::bind("127.0.0.1:8888").map_err(|e| anyhow::anyhow!("Bind error 8888: {}", e))?;
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0; 4096];
                if let Ok(bytes_read) = stream.read(&mut buf) {
                    let request = String::from_utf8_lossy(&buf[..bytes_read]);
                    if request.starts_with("GET /callback") {
                        if let Some(query) = request.split(' ').nth(1).and_then(|path| path.split('?').nth(1)) {
                            let mut code = None;
                            let mut state = None;
                            for pair in query.split('&') {
                                let mut kv = pair.split('=');
                                let k = kv.next().unwrap_or("");
                                let v = kv.next().unwrap_or("");
                                if k == "code" { code = Some(v.to_string()); }
                                if k == "state" { state = Some(v.to_string()); }
                            }
                            
                            if let (Some(c), Some(s)) = (code, state) {
                                if s == expected_state {
                                    let _ = stream.write_all("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Login 1/2 Completato!</h1><p>Attendi un istante, si aprira' la seconda scheda per il player audio...</p></body></html>".as_bytes());
                                    return Ok::<String, anyhow::Error>(c);
                                } else {
                                    let _ = stream.write_all("HTTP/1.1 403 Forbidden\r\n\r\nCSRF Mismatch".as_bytes());
                                }
                            } else {
                                let _ = stream.write_all("HTTP/1.1 400 Bad Request\r\n\r\nMissing code or state".as_bytes());
                            }
                        }
                    }
                }
            }
        }
        Ok::<String, anyhow::Error>(String::new())
    });
    let _ = open::that(web_auth_url.as_str());
    let web_code = web_server.await??;
    let mut web_token_req = web_client.exchange_code(AuthorizationCode::new(web_code));
    if config.client_secret.is_empty() {
        web_token_req = web_token_req.set_pkce_verifier(web_pkce_verifier);
    }
    let web_token_res = web_token_req.request_async(oauth2::reqwest::async_http_client).await?;
    // We MUST use the official Spotify Web Player Client ID here to obtain a token capable of streaming raw audio.
    // Spotify restricts the 'streaming' scope for standard Developer API apps.
    let play_client_id = "65b708073fc0480ea92a077233ca87bd"; 
    let play_redirect = "http://127.0.0.1:8898/login";

    let playback_client = BasicClient::new(
        ClientId::new(play_client_id.to_string()),
        None,
        AuthUrl::new("https://accounts.spotify.com/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://accounts.spotify.com/api/token".to_string()).unwrap()),
    ).set_redirect_uri(RedirectUrl::new(play_redirect.to_string()).unwrap());

    let (play_pkce_challenge, play_pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (mut play_auth_url, play_csrf) = playback_client
        .authorize_url(CsrfToken::new_random)
        .add_scope(oauth2::Scope::new("streaming".to_string()))
        .add_scope(oauth2::Scope::new("user-read-email".to_string()))
        .add_scope(oauth2::Scope::new("user-read-private".to_string()))
        .set_pkce_challenge(play_pkce_challenge)
        .url();
    play_auth_url.query_pairs_mut().append_pair("show_dialog", "true");

    let play_expected_state = play_csrf.secret().clone();

    let play_server = tokio::task::spawn_blocking(move || {
        let listener = TcpListener::bind("127.0.0.1:8898").map_err(|e| anyhow::anyhow!("Bind error 8898: {}", e))?;
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0; 4096];
                if let Ok(bytes_read) = stream.read(&mut buf) {
                    let request = String::from_utf8_lossy(&buf[..bytes_read]);
                    if request.starts_with("GET /login") {
                        if let Some(query) = request.split(' ').nth(1).and_then(|path| path.split('?').nth(1)) {
                            let mut code = None;
                            let mut state = None;
                            for pair in query.split('&') {
                                let mut kv = pair.split('=');
                                let k = kv.next().unwrap_or("");
                                let v = kv.next().unwrap_or("");
                                if k == "code" { code = Some(v.to_string()); }
                                if k == "state" { state = Some(v.to_string()); }
                            }
                            
                            if let (Some(c), Some(s)) = (code, state) {
                                if s == play_expected_state {
                                    let _ = stream.write_all("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Login 2/2 Completato!</h1><p>Puoi chiudere questa finestra e tornare a SpotLight.</p></body></html>".as_bytes());
                                    return Ok::<String, anyhow::Error>(c);
                                } else {
                                    let _ = stream.write_all("HTTP/1.1 403 Forbidden\r\n\r\nCSRF Mismatch".as_bytes());
                                }
                            } else {
                                let _ = stream.write_all("HTTP/1.1 400 Bad Request\r\n\r\nMissing code or state".as_bytes());
                            }
                        }
                    }
                }
            }
        }
        Ok::<String, anyhow::Error>(String::new())
    });
    let _ = open::that(play_auth_url.as_str());
    let play_code = play_server.await??;
    let play_token_res = playback_client.exchange_code(AuthorizationCode::new(play_code))
        .set_pkce_verifier(play_pkce_verifier)
        .request_async(oauth2::reqwest::async_http_client).await?;

    let tokens = AuthTokens {
        web_token: AuthToken {
            access_token: web_token_res.access_token().secret().to_string(),
            refresh_token: web_token_res.refresh_token().map(|t| t.secret().to_string()),
        },
        playback_token: AuthToken {
            access_token: play_token_res.access_token().secret().to_string(),
            refresh_token: play_token_res.refresh_token().map(|t| t.secret().to_string()),
        }
    };
    tokens.save();
    Ok(tokens)
}

pub async fn try_refresh_tokens(config: &crate::config::AppConfig) -> anyhow::Result<AuthTokens> {
    if cfg!(debug_assertions) && config.mock_mode {
        return Ok(AuthTokens {
            web_token: AuthToken {
                access_token: "mock_web_token".into(),
                refresh_token: None,
            },
            playback_token: AuthToken {
                access_token: "mock_play_token".into(),
                refresh_token: None,
            }
        });
    }

    let saved = match AuthTokens::load() {
        Some(s) => s,
        None => {
            println!("No tokens found in keyring");
            anyhow::bail!("No saved tokens");
        }
    };
    
    let web_refresh = saved.web_token.refresh_token.clone().context("No web refresh token")?;
    let play_refresh = saved.playback_token.refresh_token.clone().context("No play refresh token")?;

    let web_client_id = config.client_id.clone();
    let mut web_client = BasicClient::new(
        ClientId::new(web_client_id),
        if config.client_secret.is_empty() { None } else { Some(oauth2::ClientSecret::new(config.client_secret.clone())) },
        AuthUrl::new("https://accounts.spotify.com/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://accounts.spotify.com/api/token".to_string()).unwrap()),
    );
    if config.client_secret.is_empty() {
        web_client = web_client.set_auth_type(oauth2::AuthType::RequestBody);
    }

    let web_token_res = match web_client.exchange_refresh_token(&oauth2::RefreshToken::new(web_refresh))
        .request_async(oauth2::reqwest::async_http_client).await {
            Ok(r) => r,
            Err(e) => {
                println!("Failed to refresh web token: {:?}", e);
                return Err(e.into());
            }
        };

    // We MUST use the official Spotify Web Player Client ID here for streaming tokens.
    let play_client_id = "65b708073fc0480ea92a077233ca87bd";
    let playback_client = BasicClient::new(
        ClientId::new(play_client_id.to_string()),
        None,
        AuthUrl::new("https://accounts.spotify.com/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://accounts.spotify.com/api/token".to_string()).unwrap()),
    ).set_auth_type(oauth2::AuthType::RequestBody);

    let play_token_res = playback_client.exchange_refresh_token(&oauth2::RefreshToken::new(play_refresh))
        .request_async(oauth2::reqwest::async_http_client).await?;

    let tokens = AuthTokens {
        web_token: AuthToken {
            access_token: web_token_res.access_token().secret().to_string(),
            refresh_token: web_token_res.refresh_token().map(|t| t.secret().to_string()).or(saved.web_token.refresh_token),
        },
        playback_token: AuthToken {
            access_token: play_token_res.access_token().secret().to_string(),
            refresh_token: play_token_res.refresh_token().map(|t| t.secret().to_string()).or(saved.playback_token.refresh_token),
        }
    };
    tokens.save();
    Ok(tokens)
}
