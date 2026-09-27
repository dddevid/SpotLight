use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use crate::app::Track;

#[derive(Deserialize)]
struct SpotifyUser {
    id: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistResponse {
    items: Vec<Playlist>,
}

#[derive(Deserialize)]
struct Playlist {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct PlaylistTracksResponse {
    items: Vec<PlaylistItem>,
}

#[derive(Deserialize)]
struct PlaylistItem {
    #[serde(alias = "item")]
    track: Option<SpotifyTrack>,
}

#[derive(Deserialize)]
struct SpotifyTrack {
    name: String,
    uri: String,
    duration_ms: Option<u64>,
    artists: Option<Vec<SpotifyArtist>>,
    album: Option<SpotifyAlbum>,
}

#[derive(Deserialize)]
struct SpotifyAlbum {
    name: String,
}

#[derive(Deserialize)]
struct SpotifyArtist {
    name: String,
}

pub async fn fetch_user_profile(token: &str) -> Result<String> {
    let client = Client::new();
    let res = client.get("https://api.spotify.com/v1/me")
        .bearer_auth(token)
        .send()
        .await?
        .json::<SpotifyUser>()
        .await?;
    
    Ok(res.display_name.unwrap_or(res.id))
}

pub async fn fetch_playlists(token: &str) -> Result<Vec<(String, String)>> {
    let client = Client::new();
    let res = client.get("https://api.spotify.com/v1/me/playlists?limit=50")
        .bearer_auth(token)
        .send()
        .await?;

    if !res.status().is_success() {
        let err_text = res.text().await?;
        println!("Error fetching playlists: {}", err_text);
        anyhow::bail!("Failed with status: {}", err_text);
    }
    
    let parsed = res.json::<PlaylistResponse>().await?;
    Ok(parsed.items.into_iter().map(|p| (p.id, p.name)).collect())
}

pub async fn fetch_playlist_tracks(token: &str, playlist_id: &str) -> Result<Vec<Track>> {
    let client = Client::new();
    let url = format!("https://api.spotify.com/v1/playlists/{}/items", playlist_id);
    let res = client.get(&url)
        .bearer_auth(token)
        .send()
        .await?;

    let status = res.status();
    let body_text = res.text().await?;
    
    if !status.is_success() {
        println!("Error fetching tracks for playlist '{}': {}", playlist_id, body_text);
        anyhow::bail!("Failed with status: {}", body_text);
    }
    
    match serde_json::from_str::<PlaylistTracksResponse>(&body_text) {
        Ok(parsed) => {
            let tracks = parsed.items.into_iter().filter_map(|item| {
                item.track.map(|t| Track {
                    name: t.name,
                    artist: t.artists.unwrap_or_default().into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
                    album: t.album.map(|a| a.name).unwrap_or_default(),
                    uri: t.uri,
                    duration_ms: t.duration_ms.unwrap_or(0),
                })
            }).collect();
            Ok(tracks)
        },
        Err(e) => {
            println!("JSON PARSE ERROR: {}", e);
            println!("JSON BODY WAS: {}", body_text);
            anyhow::bail!("Parse error: {}", e);
        }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    tracks: SearchTracks,
}

#[derive(Deserialize)]
struct SearchTracks {
    items: Vec<SpotifyTrack>,
}

pub async fn fetch_search(token: &str, query: &str) -> Result<Vec<Track>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    
    let client = Client::new();
    let url = format!(
        "https://api.spotify.com/v1/search?q={}&type=track",
        urlencoding::encode(query)
    );
    let res = client.get(&url)
        .bearer_auth(token)
        .send()
        .await?;

    if !res.status().is_success() {
        let err = res.text().await?;
        println!("API Search error: {}", err);
        anyhow::bail!("Search failed: {}", err);
    }
    
    let body_text = res.text().await?;
    let parsed: SearchResponse = match serde_json::from_str(&body_text) {
        Ok(p) => p,
        Err(e) => {
            println!("JSON PARSE ERROR in fetch_search: {}", e);
            println!("BODY: {}", body_text);
            anyhow::bail!("JSON Parse error");
        }
    };
    let tracks = parsed.tracks.items.into_iter().map(|t| Track {
        name: t.name,
        artist: t.artists.unwrap_or_default().into_iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
        album: t.album.map(|a| a.name).unwrap_or_default(),
        uri: t.uri,
        duration_ms: t.duration_ms.unwrap_or(0),
    }).collect();
    
    Ok(tracks)
}

#[derive(Deserialize)]
struct TopTracksResponse {
    items: Vec<SpotifyTrack>,
}

pub async fn fetch_top_tracks(token: &str) -> Result<Vec<Track>> {
    let client = Client::new();
    let res = client.get("https://api.spotify.com/v1/me/top/tracks?limit=10")
        .bearer_auth(token)
        .send()
        .await?;
    if !res.status().is_success() {
        println!("Error fetching top tracks: {}", res.text().await?);
        return Ok(vec![]);
    }
    let parsed: TopTracksResponse = res.json().await?;
    let mut tracks = Vec::new();
    for track in parsed.items {
        let artist_name = track.artists.and_then(|mut a| if a.is_empty() { None } else { Some(a.remove(0).name) }).unwrap_or_else(|| "Unknown Artist".to_string());
        let album_name = track.album.map(|a| a.name).unwrap_or_else(|| "Unknown Album".to_string());
        tracks.push(Track {
            name: track.name,
            artist: artist_name,
            album: album_name,
            uri: track.uri,
            duration_ms: track.duration_ms.unwrap_or(0),
        });
    }
    Ok(tracks)
}

pub async fn fetch_recent_tracks(token: &str) -> Result<Vec<Track>> {
    let client = Client::new();
    let res = client.get("https://api.spotify.com/v1/me/player/recently-played?limit=10")
        .bearer_auth(token)
        .send()
        .await?;
    if !res.status().is_success() {
        println!("Error fetching recent tracks: {}", res.text().await?);
        return Ok(vec![]);
    }
    let parsed: PlaylistTracksResponse = res.json().await?;
    let mut tracks = Vec::new();
    for item in parsed.items {
        if let Some(track) = item.track {
            let artist_name = track.artists.and_then(|mut a| if a.is_empty() { None } else { Some(a.remove(0).name) }).unwrap_or_else(|| "Unknown Artist".to_string());
            let album_name = track.album.map(|a| a.name).unwrap_or_else(|| "Unknown Album".to_string());
            tracks.push(Track {
                name: track.name,
                artist: artist_name,
                album: album_name,
                uri: track.uri,
                duration_ms: track.duration_ms.unwrap_or(0),
            });
        }
    }
    Ok(tracks)
}

pub async fn fetch_recommendations(token: &str, seed_tracks: &[Track], limit: u8) -> Result<Vec<Track>> {
    if seed_tracks.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut similar = Vec::new();
    let mut artists = Vec::new();
    let mut seen_uris = std::collections::HashSet::new();

    for seed in seed_tracks {
        for artist in seed.artist.split(',').map(str::trim).filter(|artist| !artist.is_empty()) {
            if artists.iter().any(|known| known == artist) {
                continue;
            }
            artists.push(artist.to_string());

            let mut results = fetch_search(token, &format!("artist:{}", artist)).await?;
            if results.len() < limit as usize {
                let mut broad_results = fetch_search(token, artist).await?;
                results.append(&mut broad_results);
            }
            for track in results {
                if track.uri == seed.uri || !seen_uris.insert(track.uri.clone()) {
                    continue;
                }
                similar.push(track);
                if similar.len() >= limit as usize {
                    return Ok(similar);
                }
            }
        }
    }

    Ok(similar)
}

#[derive(Deserialize)]
struct LrcLibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}
pub async fn fetch_lyrics(track: &str, artist: &str, duration_ms: u64) -> Result<Vec<(u64, String)>> {
    let client = Client::new();
    let url = if duration_ms > 0 {
        format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}",
            urlencoding::encode(track),
            urlencoding::encode(artist),
            duration_ms / 1000
        )
    } else {
        format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}",
            urlencoding::encode(track),
            urlencoding::encode(artist)
        )
    };
    let res = client.get(&url).send().await?;
    if !res.status().is_success() {
        anyhow::bail!("lrclib: status {}", res.status());
    }

    let data: LrcLibResponse = res.json().await?;
    let src = data.synced_lyrics.or(data.plain_lyrics).unwrap_or_default();
    let mut lines: Vec<(u64, String)> = Vec::new();
    let mut pseudo_ms: u64 = 0;

    for raw in src.lines() {
        let raw = raw.trim();
        if raw.is_empty() { continue; }
        if raw.starts_with('[') {
            if let Some(close) = raw.find(']') {
                let time_str = &raw[1..close];
                let text = raw[close + 1..].trim();
                if let Some(ms) = parse_lrc_time(time_str) {
                    lines.push((ms, text.to_string()));
                    continue;
                }
            }
        }
        lines.push((pseudo_ms, raw.to_string()));
        pseudo_ms += 3000;
    }
    Ok(lines)
}

fn parse_lrc_time(s: &str) -> Option<u64> {
    let (mins_str, rest) = s.split_once(':')?;
    let mins: u64 = mins_str.trim().parse().ok()?;
    let (secs_str, ms_opt) = if let Some((s, m)) = rest.split_once('.') { (s, Some(m)) } else { (rest, None) };
    let secs: u64 = secs_str.trim().parse().ok()?;
    let ms: u64 = ms_opt.map(|m| { let v: u64 = m.parse().unwrap_or(0); match m.len() { 1 => v*100, 2 => v*10, _ => v } }).unwrap_or(0);
    Some(mins * 60_000 + secs * 1_000 + ms)
}

