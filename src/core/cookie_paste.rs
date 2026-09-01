//! Reads the sign-in paste: a raw Cookie header value, or a whole
//! "Copy as cURL" command whose cookie the parser extracts.
//!
//! Pastes fail early and with a named reason here, because the
//! failures they cause otherwise surface late and look like an empty
//! library: YouTube treats a session with missing cookies as signed
//! out while the SAPISID hash still passes.

/// The Cookie header value from the paste, validated.
pub fn cookies_from_paste(paste: &str) -> Result<String, String> {
    let text = paste.trim();
    let cookies = if text.starts_with("curl ") {
        curl_cookie_header(text).ok_or(
            "The cURL paste carries no Cookie header. Copy the request \
             as cURL again, from a signed-in music.youtube.com tab.",
        )?
    } else {
        text.to_string()
    };
    validate(&cookies)?;
    Ok(cookies)
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

/// The cookie value inside a "Copy as cURL" command: a `-H 'Cookie: …'`
/// header (Safari, Firefox) or a `-b '…'` option (Chrome).
fn curl_cookie_header(curl: &str) -> Option<String> {
    let arguments = shell_arguments(curl);
    let mut previous: Option<&String> = None;
    for argument in &arguments {
        if let Some(cookies) = cookie_of_argument_pair(previous, argument) {
            return Some(cookies);
        }
        previous = Some(argument);
    }
    None
}

fn cookie_of_argument_pair(flag: Option<&String>, value: &str) -> Option<String> {
    match flag.map(String::as_str) {
        Some("-H") => {
            let (name, rest) = value.split_once(':')?;
            name.eq_ignore_ascii_case("cookie")
                .then(|| rest.trim().to_string())
        }
        Some("-b") | Some("--cookie") => Some(value.trim().to_string()),
        _ => None,
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

    #[test]
    fn a_raw_header_value_passes() {
        assert_eq!(
            cookies_from_paste(&format!(" {FULL} ")),
            Ok(FULL.to_string())
        );
    }

    #[test]
    fn a_truncated_paste_names_the_cause() {
        let error = cookies_from_paste("SID=a…more").unwrap_err();
        assert!(error.contains("truncation mark"));
    }

    #[test]
    fn missing_cookies_are_named() {
        let error = cookies_from_paste("SAPISID=b; YSC=x").unwrap_err();
        assert!(error.contains("SID"));
        assert!(error.contains("__Secure-3PSID"));
    }

    #[test]
    fn a_safari_curl_paste_yields_the_header() {
        let curl = format!("curl 'https://music.youtube.com/' -H 'Cookie: {FULL}' --compressed");
        assert_eq!(cookies_from_paste(&curl), Ok(FULL.to_string()));
    }

    #[test]
    fn a_chrome_curl_paste_yields_the_cookie_option() {
        let curl = format!("curl 'https://music.youtube.com/' -b '{FULL}' -H 'accept: */*'");
        assert_eq!(cookies_from_paste(&curl), Ok(FULL.to_string()));
    }

    #[test]
    fn a_curl_paste_without_cookies_names_the_cause() {
        let error = cookies_from_paste("curl 'https://x' -H 'accept: */*'").unwrap_err();
        assert!(error.contains("no Cookie header"));
    }

    #[test]
    fn the_cookie_header_name_matches_any_case() {
        let curl = format!("curl 'https://x' -H 'cookie: {FULL}'");
        assert_eq!(cookies_from_paste(&curl), Ok(FULL.to_string()));
    }
}
