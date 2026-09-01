//! Reads the sign-in paste: a raw Cookie header value, or a whole
//! "Copy as cURL" command from which the cookie and the other request
//! headers get extracted.
//!
//! Pastes fail early and with a named reason here, because the
//! failures they cause otherwise surface late and look like an empty
//! library: YouTube treats a session with missing cookies as signed
//! out while the SAPISID hash still passes.
//!
//! The extra headers matter as much as the cookie. ytmusicapi keeps
//! every copied header (minus a small ignore set) and replays them on
//! each request: account selection (X-Goog-AuthUser, and X-Goog-PageId
//! for brand accounts) and consistency checks ride on them.

/// The cookie and the replayable request headers from a paste.
#[derive(Debug, PartialEq)]
pub struct PasteCredentials {
    pub cookies: String,
    /// Lowercased header names with their values.
    pub headers: Vec<(String, String)>,
}

pub fn credentials_from_paste(paste: &str) -> Result<PasteCredentials, String> {
    let text = paste.trim();
    let credentials = if text.starts_with("curl ") {
        curl_credentials(text).ok_or(
            "The cURL paste carries no Cookie header. Copy the request \
             as cURL again, from a signed-in music.youtube.com tab.",
        )?
    } else {
        PasteCredentials {
            cookies: text.to_string(),
            headers: vec![],
        }
    };
    validate(&credentials.cookies)?;
    Ok(credentials)
}

fn validate(cookies: &str) -> Result<(), String> {
    if cookies.contains('…') {
        return Err("The paste contains a truncation mark (…), so the tool \
                    copied the shortened display text. Right-click the request \
                    and use Copy as cURL instead."
            .to_string());
    }
    let missing = missing_session_cookies(cookies);
    if !missing.is_empty() {
        return Err(format!(
            "The paste misses the {} cookies. Right-click a signed-in \
             music.youtube.com request and use Copy as cURL.",
            missing.join(", ")
        ));
    }
    Ok(())
}

/// The session cookies a signed-in request always carries.
fn missing_session_cookies(cookies: &str) -> Vec<&'static str> {
    ["SID", "SAPISID", "__Secure-3PAPISID", "__Secure-3PSID"]
        .into_iter()
        .filter(|name| !has_cookie(cookies, name))
        .collect()
}

fn has_cookie(cookies: &str, name: &str) -> bool {
    cookies
        .split(';')
        .any(|pair| pair.trim().split('=').next() == Some(name))
}

/// Headers that never replay: the transport negotiates these itself,
/// and the auth layer computes its own cookie and authorization.
fn is_ignored_header(name: &str) -> bool {
    name.starts_with("sec-")
        || matches!(
            name,
            "cookie" | "authorization" | "host" | "content-length" | "accept-encoding"
        )
}

/// The cookie and headers inside a "Copy as cURL" command: `-H` pairs
/// (all browsers) and the `-b` cookie option (Chrome).
fn curl_credentials(curl: &str) -> Option<PasteCredentials> {
    let arguments = shell_arguments(curl);
    let mut cookies: Option<String> = None;
    let mut headers = Vec::new();
    let mut previous: Option<&String> = None;
    for argument in &arguments {
        match previous.map(String::as_str) {
            Some("-H") => collect_header(argument, &mut cookies, &mut headers),
            Some("-b") | Some("--cookie") => cookies = Some(argument.trim().to_string()),
            _ => {}
        }
        previous = Some(argument);
    }
    Some(PasteCredentials {
        cookies: cookies?,
        headers,
    })
}

fn collect_header(
    argument: &str,
    cookies: &mut Option<String>,
    headers: &mut Vec<(String, String)>,
) {
    let Some((name, value)) = argument.split_once(':') else {
        return;
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim().to_string();
    if name == "cookie" {
        *cookies = Some(value);
    } else if !is_ignored_header(&name) {
        headers.push((name, value));
    }
}

/// Splits a shell command into arguments: whitespace separates, and
/// single or double quotes group. Escapes stay simple on purpose; the
/// browsers' cURL output uses plain quoting for headers.
fn shell_arguments(command: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in command.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => quote = Some(character),
            (None, c) if c.is_whitespace() => flush(&mut arguments, &mut current),
            (None, c) => current.push(c),
        }
    }
    flush(&mut arguments, &mut current);
    arguments
}

fn flush(arguments: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        arguments.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "SID=a; SAPISID=b; __Secure-3PAPISID=c; __Secure-3PSID=d";

    fn cookies_of(paste: &str) -> Result<String, String> {
        credentials_from_paste(paste).map(|credentials| credentials.cookies)
    }

    #[test]
    fn a_raw_header_value_passes() {
        assert_eq!(cookies_of(&format!(" {FULL} ")), Ok(FULL.to_string()));
    }

    #[test]
    fn a_truncated_paste_names_the_cause() {
        let error = cookies_of("SID=a…more").unwrap_err();
        assert!(error.contains("truncation mark"));
    }

    #[test]
    fn missing_cookies_are_named() {
        let error = cookies_of("SAPISID=b; YSC=x").unwrap_err();
        assert!(error.contains("SID"));
        assert!(error.contains("__Secure-3PSID"));
    }

    #[test]
    fn a_safari_curl_paste_yields_cookie_and_headers() {
        let curl = format!(
            "curl 'https://music.youtube.com/' -H 'Cookie: {FULL}' \
             -H 'X-Goog-AuthUser: 1' -H 'X-Goog-PageId: 12345' \
             -H 'Sec-Fetch-Mode: cors' -H 'Host: music.youtube.com' --compressed"
        );
        let credentials = credentials_from_paste(&curl).unwrap();
        assert_eq!(credentials.cookies, FULL);
        assert_eq!(
            credentials.headers,
            vec![
                ("x-goog-authuser".to_string(), "1".to_string()),
                ("x-goog-pageid".to_string(), "12345".to_string()),
            ]
        );
    }

    #[test]
    fn a_chrome_curl_paste_yields_the_cookie_option() {
        let curl = format!("curl 'https://music.youtube.com/' -b '{FULL}' -H 'accept: */*'");
        let credentials = credentials_from_paste(&curl).unwrap();
        assert_eq!(credentials.cookies, FULL);
        assert_eq!(
            credentials.headers,
            vec![("accept".to_string(), "*/*".to_string())]
        );
    }

    #[test]
    fn a_curl_paste_without_cookies_names_the_cause() {
        let error = cookies_of("curl 'https://x' -H 'accept: */*'").unwrap_err();
        assert!(error.contains("no Cookie header"));
    }

    #[test]
    fn the_cookie_header_name_matches_any_case() {
        let curl = format!("curl 'https://x' -H 'cookie: {FULL}'");
        assert_eq!(cookies_of(&curl), Ok(FULL.to_string()));
    }
}
