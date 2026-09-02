//! Pure rules for thumbnail choice and sizing.
//!
//! ytmapi-rs hands back a list of thumbnail sizes, or none at all. This
//! module picks one URL from that list, builds a fallback URL from a
//! video id, and rewrites a chosen URL to the pixel size a widget asks
//! for.

use ytmapi_rs::common::Thumbnail;

/// The smallest thumbnail at width 300 or more, or the largest one
/// below that. A search result or a playlist row draws small, so a
/// huge banner wastes bandwidth; a row still wants enough pixels to
/// look sharp at a bigger size later.
pub fn preferred(thumbnails: &[Thumbnail]) -> Option<String> {
    let mut sorted: Vec<&Thumbnail> = thumbnails.iter().collect();
    sorted.sort_by_key(|thumbnail| thumbnail.width);
    sorted
        .iter()
        .find(|thumbnail| thumbnail.width >= 300)
        .or(sorted.last())
        .map(|thumbnail| thumbnail.url.clone())
}

/// The default YouTube video thumbnail for `video_id`. Every video id
/// has one, so this is the fallback when a source carries no
/// thumbnail list of its own.
pub fn video_thumbnail(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

/// A track's art: the preferred thumbnail when the source lists one,
/// else the video thumbnail built from `video_id`. Keeps the
/// thumbnail-or-fallback rule in one place for every track conversion.
pub fn track_art(thumbnails: &[Thumbnail], video_id: &str) -> Option<String> {
    preferred(thumbnails).or_else(|| Some(video_thumbnail(video_id)))
}

/// `url` resized to about `px` pixels on a side, for the two thumbnail
/// hosts the app draws. Any other URL comes back unchanged.
pub fn sized(url: &str, px: u32) -> String {
    if url.contains("googleusercontent.com") {
        return resized_googleusercontent(url, px);
    }
    if url.contains("i.ytimg.com/vi/") {
        return resized_ytimg(url, px);
    }
    url.to_string()
}

/// A googleusercontent URL's trailing `=w<W>-h<H>...` or `=s<N>...`
/// size suffix, replaced with a fresh `=w{px}-h{px}-l90-rj` suffix. A
/// URL with no such suffix comes back unchanged.
fn resized_googleusercontent(url: &str, px: u32) -> String {
    let Some(cut) = url.rfind("=w").or_else(|| url.rfind("=s")) else {
        return url.to_string();
    };
    format!("{}=w{px}-h{px}-l90-rj", &url[..cut])
}

/// An `i.ytimg.com/vi/<id>/<name>.jpg` URL, resized by naming the
/// still that best matches `px`: `mqdefault` up to 320px, else
/// `hqdefault`. Larger stills do not exist for every video, and a
/// missing image leaves a blank square.
fn resized_ytimg(url: &str, px: u32) -> String {
    let Some(cut) = url.rfind('/') else {
        return url.to_string();
    };
    let still = match px {
        0..=320 => "mqdefault.jpg",
        _ => "hqdefault.jpg",
    };
    format!("{}/{still}", &url[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumbnail(width: u64, height: u64, url: &str) -> Thumbnail {
        Thumbnail {
            width,
            height,
            url: url.to_string(),
        }
    }

    #[test]
    fn the_smallest_thumbnail_at_or_above_300_wins() {
        let thumbnails = vec![
            thumbnail(60, 60, "tiny"),
            thumbnail(320, 320, "just_over"),
            thumbnail(544, 544, "large"),
        ];
        assert_eq!(preferred(&thumbnails), Some("just_over".into()));
    }

    #[test]
    fn the_largest_thumbnail_wins_when_none_reach_300() {
        let thumbnails = vec![thumbnail(60, 60, "tiny"), thumbnail(120, 120, "small")];
        assert_eq!(preferred(&thumbnails), Some("small".into()));
    }

    #[test]
    fn an_empty_thumbnail_list_has_no_preferred_url() {
        assert_eq!(preferred(&[]), None);
    }

    #[test]
    fn a_video_thumbnail_url_uses_the_video_id() {
        assert_eq!(
            video_thumbnail("abc123"),
            "https://i.ytimg.com/vi/abc123/hqdefault.jpg"
        );
    }

    #[test]
    fn track_art_falls_back_to_the_video_thumbnail() {
        assert_eq!(
            track_art(&[], "abc123"),
            Some("https://i.ytimg.com/vi/abc123/hqdefault.jpg".into())
        );
    }

    #[test]
    fn track_art_prefers_a_listed_thumbnail_over_the_fallback() {
        let thumbnails = vec![thumbnail(544, 544, "listed")];
        assert_eq!(track_art(&thumbnails, "abc123"), Some("listed".into()));
    }

    #[test]
    fn a_google_user_content_w_h_suffix_is_replaced() {
        let url = "https://lh3.googleusercontent.com/abc=w544-h544-p-l90-rj";
        assert_eq!(
            sized(url, 72),
            "https://lh3.googleusercontent.com/abc=w72-h72-l90-rj"
        );
    }

    #[test]
    fn a_google_user_content_s_suffix_is_replaced() {
        let url = "https://yt3.googleusercontent.com/abc=s1200";
        assert_eq!(
            sized(url, 168),
            "https://yt3.googleusercontent.com/abc=w168-h168-l90-rj"
        );
    }

    #[test]
    fn a_ytimg_url_gets_the_still_matching_the_pixel_size() {
        let url = "https://i.ytimg.com/vi/abc123/hqdefault.jpg";
        assert_eq!(
            sized(url, 72),
            "https://i.ytimg.com/vi/abc123/mqdefault.jpg"
        );
        assert_eq!(
            sized(url, 400),
            "https://i.ytimg.com/vi/abc123/hqdefault.jpg"
        );
        assert_eq!(
            sized(url, 600),
            "https://i.ytimg.com/vi/abc123/hqdefault.jpg"
        );
    }

    #[test]
    fn an_unrecognized_host_url_is_unchanged() {
        assert_eq!(
            sized("https://example.com/x.jpg", 72),
            "https://example.com/x.jpg"
        );
    }
}
