//! String helpers ported from Godot's `core/string/ustring.cpp` that the
//! scene system relies on.

/// Characters not allowed in node names (`UNIQUE_NODE_PREFIX` is `%`).
pub const INVALID_NODE_NAME_CHARACTERS: [char; 6] = ['.', ':', '@', '/', '"', '%'];

/// `String::validate_node_name()`: replaces invalid characters with `_`.
pub fn validate_node_name(name: &str) -> String {
    if !name.contains(INVALID_NODE_NAME_CHARACTERS) {
        return name.to_owned();
    }
    name.chars().map(|c| if INVALID_NODE_NAME_CHARACTERS.contains(&c) { '_' } else { c }).collect()
}

/// `String::get_invalid_node_name_characters()`.
pub fn invalid_node_name_characters(allow_internal: bool) -> String {
    INVALID_NODE_NAME_CHARACTERS
        .iter()
        .filter(|&&c| !(allow_internal && c == '@'))
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn wildcard(p: &[char], s: &[char], case_sensitive: bool) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => wildcard(&p[1..], s, case_sensitive) || (!s.is_empty() && wildcard(p, &s[1..], case_sensitive)),
        Some('?') => !s.is_empty() && s[0] != '.' && wildcard(&p[1..], &s[1..], case_sensitive),
        Some(&c) => {
            !s.is_empty()
                && if case_sensitive { s[0] == c } else { s[0].to_uppercase().eq(c.to_uppercase()) }
                && wildcard(&p[1..], &s[1..], case_sensitive)
        }
    }
}

/// `String::match()`: `*` matches any sequence, `?` any single char except `.`.
/// Empty pattern or empty string never match (Godot behaviour).
pub fn matches(s: &str, pattern: &str) -> bool {
    if pattern.is_empty() || s.is_empty() {
        return false;
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = s.chars().collect();
    wildcard(&p, &s, true)
}

/// `String::matchn()` - case-insensitive [`matches`].
pub fn matchn(s: &str, pattern: &str) -> bool {
    if pattern.is_empty() || s.is_empty() {
        return false;
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = s.chars().collect();
    wildcard(&p, &s, false)
}

/// `increase_numeric_string()` from `node.cpp`: "9" → "10", "19" → "20".
pub fn increase_numeric_string(s: &str) -> String {
    let mut digits: Vec<u8> = s.bytes().collect();
    let mut carry = !digits.is_empty();
    for d in digits.iter_mut().rev() {
        if !carry {
            break;
        }
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            carry = false;
        }
    }
    let mut out = String::from_utf8(digits).unwrap_or_default();
    if carry {
        out.insert(0, '1');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // "[String] match" from godot/tests/core/string/test_string.cpp.
    #[test]
    fn wildcard_matching_godot_cases() {
        assert!(matches("img1.png", "*.png"));
        assert!(!matches("img1.jpeg", "*.png"));
        assert!(!matches("img1.Png", "*.png"));
        assert!(matchn("img1.Png", "*.png"));
    }

    #[test]
    fn wildcard_matching_extra() {
        assert!(matches("img1.png", "img*"));
        assert!(matches("img1.png", "?mg1.png"));
        assert!(!matches("", "*"));
        assert!(!matches("abc", ""));
        assert!(matches("Node2", "Node?"));
        assert!(!matches("Node.2", "Node??"));
    }

    #[test]
    fn node_names() {
        assert_eq!(validate_node_name("a.b:c@d/e\"f%g"), "a_b_c_d_e_f_g");
        assert_eq!(validate_node_name("Player"), "Player");
        assert_eq!(invalid_node_name_characters(false), ". : @ / \" %");
        assert_eq!(increase_numeric_string("9"), "10");
        assert_eq!(increase_numeric_string("19"), "20");
        assert_eq!(increase_numeric_string("2"), "3");
    }
}
