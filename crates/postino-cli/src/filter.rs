//! `--filter` patterns: `*` matches any run of characters, `/` included, and `?` one character.
//! A pattern matches a request id with or without its `.postino` extension, so `auth/*` and
//! `auth/login` both work.

/// Whether `id` (a workspace relative path such as `auth/login.postino`) matches `pattern`.
pub fn matches(pattern: &str, id: &str) -> bool {
    let stem = id.strip_suffix(".postino").unwrap_or(id);
    glob(pattern.as_bytes(), id.as_bytes()) || glob(pattern.as_bytes(), stem.as_bytes())
}

/// Whether `id` matches any of `patterns`, or there are none.
pub fn matches_any(patterns: &[String], id: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| matches(pattern, id))
}

/// Iterative wildcard matching with backtracking to the last `*`.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == b'?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((star_p, star_t)) => {
                    p = star_p + 1;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_match_across_folders() {
        assert!(matches("auth/*", "auth/login.postino"));
        assert!(matches("auth/*", "auth/oauth/token.postino"));
        assert!(matches("*login*", "auth/login.postino"));
        assert!(!matches("auth/*", "crud/list.postino"));
    }

    #[test]
    fn the_extension_is_optional() {
        assert!(matches("auth/login", "auth/login.postino"));
        assert!(matches("auth/login.postino", "auth/login.postino"));
        assert!(!matches("auth/log", "auth/login.postino"));
    }

    #[test]
    fn question_marks_match_one_character() {
        assert!(matches("crud/item-?", "crud/item-1.postino"));
        assert!(!matches("crud/item-?", "crud/item-10.postino"));
    }

    #[test]
    fn no_pattern_matches_everything() {
        assert!(matches_any(&[], "x.postino"));
        assert!(matches_any(
            &["a/*".to_string(), "b/*".to_string()],
            "b/x.postino"
        ));
    }
}
