//! "Now playing" for the media widget (app side): whatever is playing through Windows'
//! media controls — Spotify, YouTube in a browser, the Media Player… — with play/pause/skip.

use serde_json::{json, Value};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

fn session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    let mgr = Manager::RequestAsync().ok()?.join().ok()?;
    mgr.GetCurrentSession().ok()
}

pub fn now_playing() -> Value {
    let Some(s) = session() else { return Value::Null };
    let props = s.TryGetMediaPropertiesAsync().ok().and_then(|a| a.join().ok());
    let title = props.as_ref().and_then(|p| p.Title().ok()).map(|t| t.to_string()).unwrap_or_default();
    if title.is_empty() {
        return Value::Null;
    }
    let artist = props.as_ref().and_then(|p| p.Artist().ok()).map(|t| t.to_string()).unwrap_or_default();
    let playing = s.GetPlaybackInfo().ok().and_then(|i| i.PlaybackStatus().ok()).map(|st| st == Status::Playing).unwrap_or(false);
    let app = s.SourceAppUserModelId().map(|t| t.to_string()).unwrap_or_default();
    let app = app.rsplit(['!', '\\']).next().unwrap_or("").trim_end_matches(".exe").to_string();
    json!({ "title": title, "artist": artist, "playing": playing, "app": app })
}

pub fn control(action: &str) -> Result<(), String> {
    let s = session().ok_or("Nothing is playing")?;
    let op = match action {
        "toggle" => s.TryTogglePlayPauseAsync(),
        "next" => s.TrySkipNextAsync(),
        "prev" => s.TrySkipPreviousAsync(),
        _ => return Err("unknown action".into()),
    }
    .map_err(|e| e.to_string())?;
    op.join().map(|_| ()).map_err(|e| e.to_string())
}
