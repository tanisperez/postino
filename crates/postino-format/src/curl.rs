//! Reads a `curl` command line into a [`Request`] (`postino import curl`, #242), such as the
//! "Copy as cURL" of a browser's developer tools. Pure: a file a flag names (`-d @body.json`) is
//! never read, the import warns about it instead.
//!
//! The command is split as a POSIX shell would: single and double quotes, `$'...'` strings,
//! backslash escapes and ` \` line continuations. Options that only change how curl itself runs
//! (`-s`, `-L`, `-o file`, `--compressed`, ...) are skipped, and the ones about sending
//! (`-k`, timeouts) are reported, since they are settings of the app, not of a request.

use postino_core::{Auth, Body, KeyValue, Method, Request};

use crate::snippet::percent_decode;

/// A request read from a curl command, and what could not be carried over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    /// The request.
    pub request: Request,
    /// The options that were skipped or only partly converted.
    pub warnings: Vec<String>,
}

/// Why a command could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CurlError {
    /// The first word is not `curl`.
    #[error("not a curl command")]
    NotCurl,
    /// A quote is never closed.
    #[error("a quote is not closed")]
    UnclosedQuote,
    /// An option needs a value and the command ends.
    #[error("{0} needs a value")]
    MissingValue(String),
    /// There is no URL.
    #[error("the command has no URL")]
    NoUrl,
}

/// Options that take a value and only change how curl runs: skipped with their value.
const SKIPPED_WITH_VALUE: &[&str] = &[
    "-o",
    "--output",
    "-w",
    "--write-out",
    "--retry",
    "-c",
    "--cookie-jar",
    "-D",
    "--dump-header",
    "--retry-delay",
    "--retry-max-time",
    "-T",
    "--upload-file",
    "--cacert",
    "--cert",
    "--key",
    "-r",
    "--range",
    "-Y",
    "--speed-limit",
    "-y",
    "--speed-time",
    "--limit-rate",
    "--resolve",
    "--connect-to",
    "--interface",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--config",
    "-K",
];

/// Options about sending that are settings of the app: reported, with their value.
const SETTINGS_WITH_VALUE: &[&str] = &[
    "-m",
    "--max-time",
    "--connect-timeout",
    "-x",
    "--proxy",
    "--max-redirs",
];

/// Splits `command` into words as a POSIX shell does.
fn split(command: &str) -> Result<Vec<String>, CurlError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // A line continuation.
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(next) => {
                    word.push(next);
                    in_word = true;
                }
                None => {}
            },
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(CurlError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(CurlError::UnclosedQuote),
                        },
                        Some(c) => word.push(c),
                        None => return Err(CurlError::UnclosedQuote),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => word.push('\n'),
                            Some('t') => word.push('\t'),
                            Some('r') => word.push('\r'),
                            Some(c) => word.push(c),
                            None => return Err(CurlError::UnclosedQuote),
                        },
                        Some(c) => word.push(c),
                        None => return Err(CurlError::UnclosedQuote),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// `key=value` pairs of an urlencoded text, decoded.
fn pairs(text: &str) -> Vec<KeyValue> {
    text.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            KeyValue::new(percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// Whether `text` reads as an urlencoded form: `key=value` pairs joined by `&`.
fn looks_like_form(text: &str) -> bool {
    !text.is_empty()
        && text.split('&').all(|part| {
            part.split_once('=').is_some_and(|(key, _)| {
                !key.is_empty() && !key.contains(char::is_whitespace) && !key.contains(['{', '<'])
            })
        })
}

/// What the command sends, collected while reading it.
#[derive(Default)]
struct Parts {
    method: Option<String>,
    url: Option<String>,
    headers: Vec<KeyValue>,
    data: Vec<String>,
    urlencoded: Vec<KeyValue>,
    form: Vec<KeyValue>,
    json: Vec<String>,
    auth: Auth,
    get: bool,
    head: bool,
    /// A body read from a file, left out: it still makes the request a POST, as in curl.
    file_body: bool,
    warnings: Vec<String>,
}

impl Parts {
    fn data(&mut self, value: String, at_file: bool, flag: &str) {
        if at_file && let Some(file) = value.strip_prefix('@') {
            self.warnings.push(format!(
                "{flag} @{file}: the body is read from a file, add it to the request by hand"
            ));
            self.file_body = true;
            return;
        }
        self.data.push(value);
    }
}

/// Reads `command`, a curl command line.
pub fn parse_curl(command: &str) -> Result<CurlImport, CurlError> {
    let words = split(command.trim())?;
    let mut words = words.into_iter();
    let program = words.next().ok_or(CurlError::NotCurl)?;
    let name = program.rsplit(['/', '\\']).next().unwrap_or(&program);
    if !name.eq_ignore_ascii_case("curl") && !name.eq_ignore_ascii_case("curl.exe") {
        return Err(CurlError::NotCurl);
    }
    let words: Vec<String> = words.collect();
    let mut parts = Parts::default();
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        index += 1;
        // `--name=value` and `-Xvalue` carry their value in the same word.
        let (flag, attached) = if word.starts_with("--") {
            match word.split_once('=') {
                Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
                None => (word.clone(), None),
            }
        } else if word.starts_with('-')
            && word.len() > 2
            && matches!(
                &word[..2],
                "-X" | "-H" | "-d" | "-u" | "-F" | "-A" | "-e" | "-b"
            )
        {
            (word[..2].to_string(), Some(word[2..].to_string()))
        } else {
            (word.clone(), None)
        };
        let mut value = || -> Result<String, CurlError> {
            if let Some(value) = attached.clone() {
                return Ok(value);
            }
            let value = words
                .get(index)
                .cloned()
                .ok_or_else(|| CurlError::MissingValue(flag.clone()))?;
            index += 1;
            Ok(value)
        };
        match flag.as_str() {
            "-X" | "--request" => parts.method = Some(value()?),
            "-H" | "--header" => {
                let header = value()?;
                match header.split_once(':') {
                    Some((name, value)) if !name.trim().is_empty() => {
                        parts.headers.push(KeyValue::new(name.trim(), value.trim()));
                    }
                    _ => parts
                        .warnings
                        .push(format!("-H {header:?}: not a header, skipped")),
                }
            }
            "-d" | "--data" | "--data-ascii" | "--data-binary" => {
                let data = value()?;
                parts.data(data, true, &flag);
            }
            "--data-raw" => {
                let data = value()?;
                parts.data(data, false, &flag);
            }
            "--data-urlencode" => {
                let data = value()?;
                match data.split_once('=') {
                    Some((key, value)) => parts.urlencoded.push(KeyValue::new(key, value)),
                    None => parts.urlencoded.push(KeyValue::new(data, "")),
                }
            }
            "--json" => {
                let data = value()?;
                if data.starts_with('@') {
                    parts.warnings.push(format!(
                        "--json {data}: the body is read from a file, add it to the request by hand"
                    ));
                    parts.file_body = true;
                } else {
                    parts.json.push(data);
                }
            }
            "-F" | "--form" | "--form-string" => {
                let field = value()?;
                let (key, value) = field.split_once('=').unwrap_or((field.as_str(), ""));
                if let Some(file) = value.strip_prefix('<') {
                    parts.warnings.push(format!(
                        "-F {key}=<{file}: the field's text comes from a file, add it by hand"
                    ));
                } else if flag == "--form-string" && value.starts_with('@') {
                    // A literal @, written @@ in a .postino multipart body.
                    parts.form.push(KeyValue::new(key, format!("@{value}")));
                } else {
                    // `@path;type=...`: Postino sends the file with its own type.
                    let value = value.split(";type=").next().unwrap_or(value);
                    parts.form.push(KeyValue::new(key, value));
                }
            }
            "-u" | "--user" => {
                let user = value()?;
                let (username, password) = user.split_once(':').unwrap_or((&user, ""));
                parts.auth = Auth::Basic {
                    username: username.to_string(),
                    password: password.to_string(),
                };
            }
            "--oauth2-bearer" => parts.auth = Auth::Bearer { token: value()? },
            "-A" | "--user-agent" => {
                let agent = value()?;
                parts.headers.push(KeyValue::new("User-Agent", agent));
            }
            "-e" | "--referer" => {
                let referer = value()?;
                parts.headers.push(KeyValue::new("Referer", referer));
            }
            "-b" | "--cookie" => {
                let cookie = value()?;
                if cookie.contains('=') {
                    parts.headers.push(KeyValue::new("Cookie", cookie));
                } else {
                    parts
                        .warnings
                        .push(format!("-b {cookie}: cookies from a file are not imported"));
                }
            }
            "-G" | "--get" => parts.get = true,
            "-I" | "--head" => parts.head = true,
            "--url" => parts.url = Some(value()?),
            "-k" | "--insecure" => parts.warnings.push(
                "-k: certificates are a setting of the app (Settings, Network), not of a request"
                    .to_string(),
            ),
            flag if SETTINGS_WITH_VALUE.contains(&flag) => {
                let setting = value()?;
                parts.warnings.push(format!(
                    "{flag} {setting}: a setting of the app (Settings, Network), not of a request"
                ));
            }
            flag if SKIPPED_WITH_VALUE.contains(&flag) => {
                value()?;
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                // Flags without a value: -s, -S, -L, -v, -i, --compressed, -sSL, ...
                let known = [
                    "--compressed",
                    "--location",
                    "--silent",
                    "--show-error",
                    "--verbose",
                    "--include",
                    "--fail",
                    "--globoff",
                    "--http1.1",
                    "--http2",
                    "--no-buffer",
                    "--progress-bar",
                    "-#",
                    "--remote-name",
                    "-O",
                ];
                let short_flags =
                    !flag.starts_with("--") && flag[1..].chars().all(|c| "sSLviflgNO#".contains(c));
                if !known.contains(&flag) && !short_flags {
                    parts
                        .warnings
                        .push(format!("{flag}: unknown option, skipped"));
                }
            }
            _ => {
                if parts.url.is_none() {
                    parts.url = Some(word.clone());
                } else {
                    parts
                        .warnings
                        .push(format!("{word}: a second URL, skipped"));
                }
            }
        }
    }
    build(parts)
}

/// The request `parts` describe.
fn build(mut parts: Parts) -> Result<CurlImport, CurlError> {
    let url = parts.url.take().ok_or(CurlError::NoUrl)?;
    let (url, mut query) = match url.split_once('?') {
        Some((base, query)) => (base.to_string(), pairs(query)),
        None => (url, Vec::new()),
    };
    let content_type = parts
        .headers
        .iter()
        .find(|header| header.key.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.to_ascii_lowercase())
        .unwrap_or_default();
    let has_body = !parts.data.is_empty()
        || !parts.urlencoded.is_empty()
        || !parts.form.is_empty()
        || !parts.json.is_empty();
    let data = parts.data.join("&");

    let mut body = Body::None;
    if parts.get {
        query.extend(pairs(&data));
        query.append(&mut parts.urlencoded);
    } else if !parts.json.is_empty() {
        body = Body::Json(parts.json.join(""));
        // `--json` also sets these; the JSON body sets the Content-Type itself.
        if !parts
            .headers
            .iter()
            .any(|header| header.key.eq_ignore_ascii_case("accept"))
        {
            parts
                .headers
                .push(KeyValue::new("Accept", "application/json"));
        }
    } else if !parts.form.is_empty() {
        body = Body::Multipart(std::mem::take(&mut parts.form));
    } else if !parts.urlencoded.is_empty()
        || (content_type.is_empty() || content_type.contains("x-www-form-urlencoded"))
            && looks_like_form(&data)
    {
        let mut fields = pairs(&data);
        fields.append(&mut parts.urlencoded);
        body = Body::Form(fields);
    } else if !data.is_empty() {
        body = if content_type.contains("json") {
            Body::Json(data)
        } else if content_type.contains("xml") {
            Body::Xml(data)
        } else {
            Body::Text(data)
        };
    }
    // The body type sets these Content-Types itself.
    let implied = match &body {
        Body::Json(_) => Some("application/json"),
        Body::Form(_) => Some("application/x-www-form-urlencoded"),
        Body::Multipart(_) => Some("multipart/form-data"),
        _ => None,
    };
    if let Some(implied) = implied {
        parts.headers.retain(|header| {
            !(header.key.eq_ignore_ascii_case("content-type")
                && header.value.to_ascii_lowercase().starts_with(implied))
        });
    }

    let method = match parts.method {
        Some(method) => method.to_ascii_uppercase(),
        None if parts.head => "HEAD".to_string(),
        None if (has_body || parts.file_body) && !parts.get => "POST".to_string(),
        None => "GET".to_string(),
    };
    let request = Request {
        method: method.parse().unwrap_or(Method::Get),
        url,
        headers: parts.headers,
        query,
        auth: parts.auth,
        body,
        ..Request::default()
    };
    Ok(CurlImport {
        request,
        warnings: parts.warnings,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(command: &str) -> CurlImport {
        parse_curl(command).expect("a curl command")
    }

    #[test]
    fn words_split_like_a_shell() {
        assert_eq!(
            split("curl 'a b' \"c \\\"d\\\"\" e\\ f $'g\\nh' \\\n -s").expect("split"),
            ["curl", "a b", "c \"d\"", "e f", "g\nh", "-s"]
        );
        assert_eq!(split("curl 'open"), Err(CurlError::UnclosedQuote));
    }

    #[test]
    fn a_plain_get_keeps_its_query_as_entries() {
        let import = parse("curl https://api.test/users?page=2&q=a%20b");
        assert_eq!(import.request.method, Method::Get);
        assert_eq!(import.request.url, "https://api.test/users");
        assert_eq!(
            import.request.query,
            [KeyValue::new("page", "2"), KeyValue::new("q", "a b")]
        );
        assert!(import.warnings.is_empty());
    }

    #[test]
    fn a_browser_copy_as_curl_becomes_a_json_post() {
        let import = parse(
            "curl 'https://api.test/login' \\\n  -H 'accept: */*' \\\n  -H 'content-type: application/json' \\\n  --data-raw '{\"user\":\"ada\"}' \\\n  --compressed",
        );
        let request = import.request;
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.headers, [KeyValue::new("accept", "*/*")]);
        assert_eq!(request.body, Body::Json("{\"user\":\"ada\"}".into()));
        assert!(import.warnings.is_empty());
    }

    #[test]
    fn data_without_a_type_is_a_form_when_it_reads_as_one() {
        let form = parse("curl -d 'name=Ada%20L&lang=en' -d x=1 https://api.test/f").request;
        assert_eq!(
            form.body,
            Body::Form(vec![
                KeyValue::new("name", "Ada L"),
                KeyValue::new("lang", "en"),
                KeyValue::new("x", "1")
            ])
        );
        let text = parse("curl -XPUT -d 'hello there' https://api.test/t").request;
        assert_eq!(text.method, Method::Put);
        assert_eq!(text.body, Body::Text("hello there".into()));
    }

    #[test]
    fn get_moves_the_data_to_the_query() {
        let request =
            parse("curl -G -d q=rust --data-urlencode 'tag=a b' https://api.test/s").request;
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.body, Body::None);
        assert_eq!(
            request.query,
            [KeyValue::new("q", "rust"), KeyValue::new("tag", "a b")]
        );
    }

    #[test]
    fn auth_forms_and_json_are_converted() {
        let basic = parse("curl -u ada:s3cret https://api.test").request;
        assert_eq!(
            basic.auth,
            Auth::Basic {
                username: "ada".into(),
                password: "s3cret".into()
            }
        );
        let multipart =
            parse("curl -F name=ada -F 'photo=@me.png;type=image/png' https://api.test/u").request;
        assert_eq!(
            multipart.body,
            Body::Multipart(vec![
                KeyValue::new("name", "ada"),
                KeyValue::new("photo", "@me.png")
            ])
        );
        let json = parse("curl --json '{\"a\":1}' https://api.test/j").request;
        assert_eq!(json.method, Method::Post);
        assert_eq!(json.body, Body::Json("{\"a\":1}".into()));
        assert_eq!(json.headers, [KeyValue::new("Accept", "application/json")]);
    }

    #[test]
    fn settings_and_files_are_reported() {
        let import = parse("curl -k --max-time 5 -d @body.json -sSL --frobnicate https://x.test");
        assert_eq!(import.warnings.len(), 4, "{:?}", import.warnings);
        assert_eq!(import.request.body, Body::None);
        // The body left out still makes it a POST, as curl sends it.
        assert_eq!(import.request.method, Method::Post);
        let json = parse("curl --json @body.json https://x.test");
        assert_eq!(json.request.method, Method::Post);
    }

    #[test]
    fn bad_commands_say_why() {
        assert_eq!(parse_curl("wget https://x"), Err(CurlError::NotCurl));
        assert_eq!(parse_curl("curl -s"), Err(CurlError::NoUrl));
        assert_eq!(
            parse_curl("curl https://x -H"),
            Err(CurlError::MissingValue("-H".into()))
        );
    }
}
