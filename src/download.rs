use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};

use id3::TagLike;

use crate::api::SunoClient;
use crate::api::types::{AlignedWord, Clip};
use crate::errors::CliError;

/// How long to wait for Suno to finish rendering an export before giving up.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(180);
const EXPORT_POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(serde::Deserialize)]
struct ExportResponse {
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

/// Ask Suno to export a clip, returning the URL to fetch it from.
///
/// This is the call the web app's Download > MP3 menu item makes, and it is the
/// only durable route to a clip's audio: the feed no longer hands out a CDN
/// link (`audio_url` is the `/api/forbidden` sentinel), and what the web player
/// streams from CloudFront is encrypted and decrypted in the client.
///
/// The endpoint is asynchronous. Until the file is rendered it answers
/// `{"ok": true, "status": "processing"}` with no URL, so poll until one shows
/// up. What comes back is a presigned S3 link valid for only a few minutes --
/// fetch it straight away and never cache it.
async fn resolve_export_url(api: &SunoClient, clip_id: &str) -> Result<String, CliError> {
    let path = format!("/api/download/clip/{clip_id}?format=mp3");
    let deadline = std::time::Instant::now() + EXPORT_TIMEOUT;
    let mut announced = false;
    loop {
        let body: ExportResponse = api
            .with_auth_retry(|| async {
                let resp = api.get(&path).send().await?;
                let resp = api.check_response(resp).await?;
                Ok(resp.json().await?)
            })
            .await?;

        if let Some(url) = body.download_url {
            return Ok(url);
        }
        if std::time::Instant::now() >= deadline {
            return Err(CliError::Download(format!(
                "Suno did not finish preparing the export for {clip_id} within {}s (status: {})",
                EXPORT_TIMEOUT.as_secs(),
                body.status.as_deref().unwrap_or("unknown"),
            )));
        }
        if !announced {
            eprintln!("Suno is preparing the export for {clip_id}...");
            announced = true;
        }
        tokio::time::sleep(EXPORT_POLL_INTERVAL).await;
    }
}

/// Legacy ID-addressed endpoint. It served plain MP3s until Suno moved to
/// encrypted delivery and now usually answers 200 with an empty body, so it
/// sits behind the export endpoint as a last resort rather than being trusted.
const AUDIO_BY_ID: &str = "https://audiopipe.suno.ai/?item_id=";

/// A feed URL is only usable if it is non-empty and not the sentinel Suno
/// substitutes when it won't release the CDN link. Empty strings matter: the
/// feed sends `""` rather than null, so a bare `Option` check lets one through
/// and reqwest then fails on a relative URL instead of saying what's wrong.
fn usable_url(url: Option<&str>) -> Option<&str> {
    let u = url?.trim();
    if u.is_empty() || u.ends_with("/api/forbidden") {
        None
    } else {
        Some(u)
    }
}

pub async fn download_clip(
    api: &SunoClient,
    clip: &Clip,
    output_dir: &str,
    video: bool,
) -> Result<String, CliError> {
    let mut export_err = None;
    // Audio goes through the export endpoint first, then anything the feed
    // still offers, then the legacy endpoint. Video has no export route we
    // have verified, so a missing feed URL stays a hard error.
    let urls: Vec<String> = if video {
        vec![
            usable_url(clip.video_url.as_deref())
                .ok_or_else(|| CliError::Download("no video URL available".into()))?
                .to_string(),
        ]
    } else {
        let mut candidates = Vec::new();
        // A failure here is not fatal on its own -- the fallbacks below may
        // still work -- but it is the most informative error if nothing does.
        match resolve_export_url(api, &clip.id).await {
            Ok(u) => candidates.push(u),
            Err(e) => export_err = Some(e),
        }
        if let Some(u) = usable_url(clip.audio_url.as_deref()) {
            candidates.push(u.to_string());
        }
        candidates.push(format!("{AUDIO_BY_ID}{}", clip.id));
        candidates
    };

    let ext = if video { "mp4" } else { "mp3" };
    let filename = clip_filename(&clip.title, &clip.id, ext);
    // Create the target dir up front: generation has already spent credits by
    // the time we download, so a missing `--download` dir must not error out.
    tokio::fs::create_dir_all(output_dir).await?;
    let path = Path::new(output_dir).join(&filename);
    // Stream into a sibling `.part` and only rename into place on full success,
    // so an interrupted or truncated transfer never leaves a file that looks
    // like a finished download.
    let part_path = path.with_extension(format!("{ext}.part"));

    // Bounded client: connect timeout, per-read inactivity timeout (catches a
    // stalled CDN mid-stream), and an overall cap. Without these a hung
    // connection would block the download forever.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(CliError::Http)?;

    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );
    pb.set_message(filename.clone());

    // Walk the candidates, retrying each: the ID-addressed endpoint throttles
    // by answering 200 with an empty body, which clears on a short wait.
    let mut last_err = None;
    let mut downloaded = false;
    'candidates: for url in &urls {
        for attempt_no in 1..=ATTEMPTS_PER_URL {
            match fetch_to_part(&client, url, &part_path, &pb).await {
                Ok(_) => {
                    downloaded = true;
                    break 'candidates;
                }
                Err(e) => {
                    // Every failure path leaves the .part behind; drop it so a
                    // later attempt starts clean rather than appending.
                    let _ = tokio::fs::remove_file(&part_path).await;
                    pb.set_position(0);
                    last_err = Some(e);
                    if attempt_no < ATTEMPTS_PER_URL {
                        tokio::time::sleep(Duration::from_secs(2 * attempt_no)).await;
                    }
                }
            }
        }
    }
    if !downloaded {
        return Err(export_err.or(last_err).unwrap_or_else(|| {
            CliError::Download(format!("no audio URL available for {}", clip.id))
        }));
    }

    if let Err(e) = tokio::fs::rename(&part_path, &path).await {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(e.into());
    }
    pb.finish_with_message("done");

    Ok(path.display().to_string())
}

/// How many times to try each candidate URL before moving to the next.
const ATTEMPTS_PER_URL: u64 = 3;

/// One attempt at one URL, streaming into `part_path`. The caller removes the
/// partial file and decides whether to retry.
///
/// An empty 200 is a failure, not a success: the ID-addressed endpoint answers
/// 200 `audio/mp3` with a zero-length body when it throttles, and a zero-byte
/// file still gets ID3-tagged downstream — which is how a "download" ends up
/// as a 40-byte file that every player and ffmpeg rejects.
async fn fetch_to_part(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    pb: &ProgressBar,
) -> Result<u64, CliError> {
    let resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(CliError::Http)?;

    let total = resp.content_length().unwrap_or(0);
    pb.set_length(total);

    let written = stream_to_file(part_path, resp, pb).await?;

    if written == 0 {
        return Err(CliError::Download(format!("empty response from {url}")));
    }
    // Reject a short read against the advertised size instead of tagging a
    // truncated MP3 downstream. Absent a content-length there is nothing to
    // check against, which is why the zero-byte case is caught separately.
    if total > 0 && written != total {
        return Err(CliError::Download(format!(
            "incomplete download: received {written} of {total} bytes from {url}"
        )));
    }
    Ok(written)
}

/// Stream a response body to `part_path`, returning the byte count written.
/// The caller removes the partial file on any error.
async fn stream_to_file(
    part_path: &Path,
    resp: reqwest::Response,
    pb: &ProgressBar,
) -> Result<u64, CliError> {
    use tokio::io::AsyncWriteExt as _;
    let mut file = tokio::fs::File::create(part_path).await?;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CliError::Http)?;
        pb.inc(chunk.len() as u64);
        written += chunk.len() as u64;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(written)
}

/// Build `<title-slug>-<id8>.<ext>`. Runs of non-alphanumeric chars collapse
/// to a single `-` (a naive `replace("--", "-")` leaves `--` behind for 3+
/// char runs), and empty/symbol-only titles must not yield a leading dash.
fn clip_filename(title: &str, id: &str, ext: &str) -> String {
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    // Clip IDs are ASCII UUIDs, so a byte slice is safe here.
    let short_id = &id[..8.min(id.len())];
    if slug.is_empty() {
        format!("{short_id}.{ext}")
    } else {
        format!("{slug}-{short_id}.{ext}")
    }
}

/// Embed lyrics and metadata into an MP3 file using ID3v2 tags.
/// - USLT: unsynchronized (plain) lyrics — shown in most players
/// - SYLT: synchronized lyrics with word timestamps — shown in Apple Music, Spotify, etc.
/// - TIT2: title, TPE1: artist
pub fn embed_lyrics_in_mp3(
    mp3_path: &str,
    title: &str,
    plain_lyrics: Option<&str>,
    aligned_words: Option<&[AlignedWord]>,
) -> Result<(), CliError> {
    let mut tag = id3::Tag::read_from_path(mp3_path).unwrap_or_else(|_| id3::Tag::new());

    // Set title
    tag.set_title(title);

    // Plain lyrics (USLT) — shown in most players
    if let Some(lyrics) = plain_lyrics {
        tag.add_frame(id3::frame::Lyrics {
            lang: "eng".to_string(),
            description: String::new(),
            text: lyrics.to_string(),
        });
    }

    // Synchronized lyrics (SYLT) — timed word-by-word display
    if let Some(words) = aligned_words {
        let content: Vec<(u32, String)> = words
            .iter()
            .filter(|w| w.success)
            .map(|w| ((w.start_s * 1000.0) as u32, w.word.clone()))
            .collect();

        if !content.is_empty() {
            tag.add_frame(id3::frame::SynchronisedLyrics {
                lang: "eng".to_string(),
                timestamp_format: id3::frame::TimestampFormat::Ms,
                content_type: id3::frame::SynchronisedLyricsType::Lyrics,
                description: String::new(),
                content,
            });
        }
    }

    tag.write_to_path(mp3_path, id3::Version::Id3v24)
        .map_err(|e| CliError::Download(format!("failed to write ID3 tags: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_and_empty_urls_are_not_usable() {
        // The sentinel Suno returns instead of the CDN link.
        assert_eq!(
            usable_url(Some("https://studio-api.prod.suno.com/api/forbidden")),
            None
        );
        // The feed sends "" for video on audio-only clips.
        assert_eq!(usable_url(Some("")), None);
        assert_eq!(usable_url(Some("   ")), None);
        assert_eq!(usable_url(None), None);
        assert_eq!(
            usable_url(Some("https://cdn1.suno.ai/abc.mp3")),
            Some("https://cdn1.suno.ai/abc.mp3")
        );
    }

    #[test]
    fn filename_collapses_separator_runs() {
        assert_eq!(
            clip_filename("Hello, World!", "0123456789abcdef", "mp3"),
            "hello-world-01234567.mp3"
        );
        // 3+ char symbol run — the old replace("--","-") left "--" behind.
        assert_eq!(
            clip_filename("a — b", "0123456789abcdef", "mp3"),
            "a-b-01234567.mp3"
        );
    }

    #[test]
    fn filename_handles_empty_and_symbol_only_titles() {
        // No leading dash when the title slugs away to nothing.
        assert_eq!(clip_filename("", "0123456789abcdef", "mp3"), "01234567.mp3");
        assert_eq!(
            clip_filename("!!!", "0123456789abcdef", "mp4"),
            "01234567.mp4"
        );
    }

    #[test]
    fn filename_keeps_unicode_titles() {
        assert_eq!(
            clip_filename("夜の歌 Remix", "0123456789abcdef", "mp3"),
            "夜の歌-remix-01234567.mp3"
        );
        assert_eq!(clip_filename("short", "abc", "mp3"), "short-abc.mp3");
    }
}
