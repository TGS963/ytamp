//! Offline inspection of a user-selected audio or video file.

use std::{
    collections::hash_map::DefaultHasher,
    fs::File,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use symphonia::{
    core::{
        formats::FormatReader,
        io::MediaSourceStream,
        meta::{MetadataRevision, StandardTagKey},
        probe::Hint,
    },
    default::{get_codecs, get_probe},
};

use crate::core::{
    lyrics::{Lyrics, parse_lrc},
    model::{ArtistRef, MediaSource, Track, TrackId},
};

#[derive(Clone, Debug)]
pub struct ImportResult {
    pub tracks: Vec<Track>,
    pub failures: Vec<(PathBuf, String)>,
}

/// Returns whether a dropped path is a container this release probes locally.
pub fn supports_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "mp3"
                | "wav"
                | "flac"
                | "ogg"
                | "m4a"
                | "mp4"
                | "mov"
                | "mkv"
                | "webm"
                | "aif"
                | "aiff"
        )
    )
}

pub fn inspect(path: &Path, cancel: &AtomicBool) -> Result<Track, String> {
    inspect_inner(path, cancel)
}

pub fn import(
    paths: &[PathBuf],
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize),
) -> ImportResult {
    let mut result = ImportResult {
        tracks: Vec::new(),
        failures: Vec::new(),
    };
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match inspect_inner(path, cancel) {
            Ok(track) => result.tracks.push(track),
            Err(error) => result.failures.push((path.clone(), error)),
        }
        on_progress(result.tracks.len() + result.failures.len());
    }
    result
}

pub fn local_lyrics(path: &Path) -> Result<Option<Lyrics>, String> {
    let lrc = path.with_extension("lrc");
    if !lrc.exists() {
        return Ok(None);
    }
    const MAX_LRC_BYTES: u64 = 2 * 1024 * 1024;
    let metadata = std::fs::metadata(&lrc).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("local lyric path is not a regular file".into());
    }
    if metadata.len() > MAX_LRC_BYTES {
        return Err("local lyric file exceeds 2 MiB".into());
    }
    let text = std::fs::read_to_string(&lrc).map_err(|error| error.to_string())?;
    Ok(Some(Lyrics {
        timed_lines: parse_lrc(&text),
        text,
        source: "Local file".into(),
    }))
}

fn inspect_inner(path: &Path, cancel: &AtomicBool) -> Result<Track, String> {
    let path = media_path(path, cancel)?;
    let mut probed = probe_file(&path)?;
    let tags = metadata(&mut *probed.format, &mut probed.metadata);
    let (track_id, time_base, n_frames) = select_supported_audio_track(&*probed.format)
        .map(|track| {
            (
                track.id,
                track.codec_params.time_base,
                track.codec_params.n_frames,
            )
        })
        .ok_or_else(|| "no supported audio track".to_string())?;
    let duration = duration_for(&mut *probed.format, track_id, time_base, n_frames, cancel)?;
    let title = tag(&tags, StandardTagKey::TrackTitle).unwrap_or_else(|| filename(&path));
    let artists = tag(&tags, StandardTagKey::Artist)
        .map(|value| value.split('/').map(ArtistRef::named).collect())
        .unwrap_or_default();
    let id = TrackId::local_file(&path);
    let thumbnail_url = tags
        .visuals()
        .first()
        .and_then(|visual| cache_artwork(&visual.media_type, &visual.data));
    Ok(Track {
        source: MediaSource::LocalFile { path: path.clone() },
        id,
        title,
        artists,
        album: tag(&tags, StandardTagKey::Album),
        album_id: None,
        duration: Some(duration),
        thumbnail_url,
        playlist_item_id: None,
    })
}

fn media_path(path: &Path, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("import cancelled".into());
    }
    if !std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("local media path is not a regular file".into());
    }
    if !supports_path(path) {
        return Err("unsupported local media file type".into());
    }
    path.canonicalize().map_err(|error| error.to_string())
}

fn duration_for(
    format: &mut dyn FormatReader,
    id: u32,
    base: Option<symphonia::core::units::TimeBase>,
    frames: Option<u64>,
    cancel: &AtomicBool,
) -> Result<Duration, String> {
    let first_end = validate_first_frame(format, id, cancel)?;
    let duration = match known_duration(base, frames) {
        Some(duration) => duration,
        None => scan_duration(format, id, base, first_end, cancel)?,
    };
    (!duration.is_zero())
        .then_some(duration)
        .ok_or_else(|| "could not determine duration".into())
}

fn probe_file(path: &Path) -> Result<symphonia::core::probe::ProbeResult, String> {
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let source = MediaSourceStream::new(
        Box::new(File::open(path).map_err(|error| error.to_string())?),
        Default::default(),
    );
    get_probe()
        .format(&hint, source, &Default::default(), &Default::default())
        .map_err(|error| error.to_string())
}

fn metadata(
    format: &mut dyn FormatReader,
    probed: &mut symphonia::core::probe::ProbedMetadata,
) -> MetadataRevision {
    if let Some(mut data) = probed.get()
        && let Some(revision) = data.skip_to_latest()
    {
        return revision.clone();
    }
    format
        .metadata()
        .skip_to_latest()
        .cloned()
        .unwrap_or_default()
}

/// Select the playable container default, falling back to the first decoder-supported track.
pub fn select_supported_audio_track(
    format: &dyn FormatReader,
) -> Option<&symphonia::core::formats::Track> {
    format
        .default_track()
        .filter(|track| get_codecs().get_codec(track.codec_params.codec).is_some())
        .or_else(|| {
            format
                .tracks()
                .iter()
                .find(|track| get_codecs().get_codec(track.codec_params.codec).is_some())
        })
}

fn known_duration(
    base: Option<symphonia::core::units::TimeBase>,
    frames: Option<u64>,
) -> Option<Duration> {
    base.zip(frames)
        .map(|(base, frames)| to_duration(base.calc_time(frames)))
}
fn scan_duration(
    format: &mut dyn FormatReader,
    id: u32,
    base: Option<symphonia::core::units::TimeBase>,
    initial_end: u64,
    cancel: &AtomicBool,
) -> Result<Duration, String> {
    let base = base.ok_or("no stream timebase")?;
    let mut end = initial_end;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("import cancelled".into());
        }
        match format.next_packet() {
            Ok(packet) if packet.track_id() == id => end = end.max(packet.ts() + packet.dur()),
            Ok(_) => {}
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(to_duration(base.calc_time(end)))
}
fn validate_first_frame(
    format: &mut dyn FormatReader,
    id: u32,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let parameters = format
        .tracks()
        .iter()
        .find(|track| track.id == id)
        .ok_or("selected track disappeared")?
        .codec_params
        .clone();
    let mut decoder = get_codecs()
        .make(&parameters, &Default::default())
        .map_err(|error| error.to_string())?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("import cancelled".into());
        }
        let packet = format.next_packet().map_err(|error| error.to_string())?;
        if packet.track_id() == id {
            decoder.decode(&packet).map_err(|error| error.to_string())?;
            return Ok(packet.ts() + packet.dur());
        }
    }
}
fn to_duration(time: symphonia::core::units::Time) -> Duration {
    Duration::from_secs(time.seconds) + Duration::from_secs_f64(time.frac)
}
fn tag(tags: &MetadataRevision, key: StandardTagKey) -> Option<String> {
    tags.tags()
        .iter()
        .find(|tag| tag.std_key == Some(key))
        .map(|tag| tag.value.to_string())
}
fn filename(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown title")
        .to_owned()
}
fn cache_artwork(mime: &str, bytes: &[u8]) -> Option<String> {
    const LIMIT: usize = 5 * 1024 * 1024;
    if bytes.len() > LIMIT {
        return None;
    }
    let format = match mime {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/png" => image::ImageFormat::Png,
        _ => return None,
    };
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    reader.set_format(format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(512, 512);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
    let bytes = encoded.into_inner();
    let directory = directories::ProjectDirs::from("io", "github", "ytamp")?
        .cache_dir()
        .join("local-art");
    std::fs::create_dir_all(&directory).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let name = format!("{:016x}.png", hasher.finish());
    let path = directory.join(name);
    std::fs::write(&path, bytes).ok()?;
    Some(format!("file://{}", path.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/local-media")
            .join(name)
    }

    #[test]
    fn wav_uses_the_filename_and_its_real_duration() {
        let track = inspect(&fixture("tone.wav"), &AtomicBool::new(false)).unwrap();
        assert_eq!(track.title, "tone");
        assert!((track.duration.unwrap().as_secs_f64() - 0.25).abs() < 0.02);
        assert!(track.is_local());
        assert!(track.id.0.starts_with("local:"));
    }

    #[test]
    fn aac_mp4_reads_container_metadata() {
        let track = inspect(&fixture("tagged-aac.mp4"), &AtomicBool::new(false)).unwrap();
        assert_eq!(track.title, "Fixture title");
        assert_eq!(track.artist_names(), "Fixture artist");
        assert_eq!(track.album.as_deref(), Some("Fixture album"));
    }

    #[test]
    fn advertised_audio_containers_have_a_real_duration() {
        for name in [
            "tone.mp3",
            "tone.flac",
            "tone.ogg",
            "tone-alac.m4a",
            "tone.aiff",
            "tone-aac.mov",
            "tone-aac.mkv",
            "tone-vorbis.webm",
            "video-first-aac.mp4",
        ] {
            let track = inspect(&fixture(name), &AtomicBool::new(false))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(track.duration.unwrap().as_millis() >= 100, "{name}");
        }
    }

    #[test]
    fn cancelled_or_invalid_paths_do_not_become_tracks() {
        assert!(inspect(&fixture("tone.wav"), &AtomicBool::new(true)).is_err());
        assert!(
            inspect(
                Path::new("/definitely/not/ytamp-media.wav"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(inspect(&fixture("corrupt.mp3"), &AtomicBool::new(false)).is_err());
        assert!(inspect(&fixture("no-audio.mp4"), &AtomicBool::new(false)).is_err());
        assert_eq!(
            inspect(&fixture("tone.lrc"), &AtomicBool::new(false)).unwrap_err(),
            "unsupported local media file type"
        );
        assert!(!supports_path(Path::new("notes.txt")));
        assert!(supports_path(Path::new("clip.WEBM")));
    }

    #[test]
    fn artwork_cache_uses_a_short_raw_file_uri() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2048, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let uri = cache_artwork("image/png", &bytes.into_inner()).unwrap();
        let path = Path::new(uri.strip_prefix("file://").unwrap());
        assert!(path.file_name().unwrap().len() < 80);
        assert!(!uri.contains("%20"));
        assert!(!std::fs::read(path).unwrap().is_empty());
    }

    #[test]
    fn import_reports_each_completed_file_in_input_order() {
        let mut progress = Vec::new();
        let result = import(
            &[
                fixture("tone.wav"),
                fixture("corrupt.mp3"),
                fixture("tone.flac"),
            ],
            &AtomicBool::new(false),
            |done| progress.push(done),
        );
        assert_eq!(progress, [1, 2, 3]);
        assert_eq!(result.tracks.len(), 2);
        assert_eq!(result.failures.len(), 1);
    }

    #[test]
    fn import_scales_to_hundreds_without_reordering_progress() {
        let paths: Vec<_> = (0..240)
            .map(|index| {
                fixture(if index % 2 == 0 {
                    "tone.wav"
                } else {
                    "tone.flac"
                })
            })
            .collect();
        let mut progress = Vec::new();
        let result = import(&paths, &AtomicBool::new(false), |done| progress.push(done));
        assert_eq!(result.tracks.len(), paths.len());
        assert!(result.failures.is_empty());
        assert_eq!(progress, (1..=paths.len()).collect::<Vec<_>>());
    }

    #[test]
    fn import_stops_after_callback_cancels_a_batch() {
        let paths = vec![fixture("tone.wav"); 240];
        let cancel = AtomicBool::new(false);
        let mut progress = Vec::new();
        let result = import(&paths, &cancel, |done| {
            progress.push(done);
            if done == 12 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(progress, (1..=12).collect::<Vec<_>>());
        assert_eq!(result.tracks.len(), 12);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn unicode_paths_have_lossless_ids_and_adjacent_lrc() {
        let root = std::env::temp_dir().join(format!("ytamp-媒体-{}", std::process::id()));
        let nested = root.join("é".repeat(80)).join("音楽");
        std::fs::create_dir_all(&nested).unwrap();
        let media = nested.join("曲.wav");
        std::fs::copy(fixture("tone.wav"), &media).unwrap();
        std::fs::copy(fixture("tone.lrc"), media.with_extension("lrc")).unwrap();
        let track = inspect(&media, &AtomicBool::new(false)).unwrap();
        assert!(track.id.0.starts_with("local:"));
        assert_eq!(
            local_lyrics(&media).unwrap().unwrap().timed_lines[0].text,
            "Fixture lyric"
        );
    }
}
