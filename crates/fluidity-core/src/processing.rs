/// Post-processing for transcribed text.
/// Matches FluidVoice's pipeline: filler removal → custom dictionary → spoken punctuation.

/// Remove filler words like "um", "uh", "like", "you know", etc.
pub fn remove_filler_words(text: &str) -> String {
    let fillers = [
        "um", "uh", "er", "ah", "like", "you know", "i mean",
        "sort of", "kind of", "actually", "basically", "literally",
        "honestly", "so yeah", "right",
    ];

    let mut result = text.to_string();
    for filler in fillers {
        // Case-insensitive: replace word-boundary occurrences
        let lower = result.to_lowercase();
        let mut i = 0;
        let mut new_result = String::new();
        let filler_lower = filler.to_lowercase();

        while i < result.len() {
            if lower[i..].starts_with(&filler_lower) {
                // Check word boundary before
                let before = i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric();
                let after = i + filler_lower.len() >= lower.len()
                    || !lower.as_bytes()[i + filler_lower.len()].is_ascii_alphanumeric();
                if before && after {
                    i += filler_lower.len();
                    continue;
                }
            }
            new_result.push(result.as_bytes()[i] as char);
            i += 1;
        }
        result = new_result;
    }

    // Collapse multiple spaces
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_space = false;
    for ch in result.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(ch);
            prev_space = false;
        }
    }

    cleaned.trim().to_string()
}

/// Apply custom dictionary replacements (user-defined word/phrase substitutions).
pub fn apply_custom_dictionary(text: &str, entries: &[(String, String)]) -> String {
    let mut result = text.to_string();
    for (from, to) in entries {
        // Case-insensitive whole-word replacement
        let lower = result.to_lowercase();
        let from_lower = from.to_lowercase();
        let mut i = 0;
        let mut new_result = String::new();

        while i < result.len() {
            if lower[i..].starts_with(&from_lower) {
                let before = i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric();
                let after = i + from_lower.len() >= lower.len()
                    || !lower.as_bytes()[i + from_lower.len()].is_ascii_alphanumeric();
                if before && after {
                    new_result.push_str(to);
                    i += from_lower.len();
                    continue;
                }
            }
            new_result.push(result.as_bytes()[i] as char);
            i += 1;
        }
        result = new_result;
    }
    result
}

/// Convert spoken punctuation words to actual punctuation symbols.
pub fn apply_spoken_punctuation(text: &str) -> String {
    let replacements = [
        ("period", "."),
        ("dot", "."),
        ("point", "."),
        ("decimal", "."),
        ("comma", ","),
        ("question mark", "?"),
        ("exclamation mark", "!"),
        ("exclamation point", "!"),
        ("semicolon", ";"),
        ("colon", ":"),
        ("new line", "\n"),
        ("newline", "\n"),
        ("new paragraph", "\n\n"),
        ("open paren", "("),
        ("open parenthesis", "("),
        ("close paren", ")"),
        ("close parenthesis", ")"),
        ("open bracket", "["),
        ("close bracket", "]"),
        ("open brace", "{"),
        ("close brace", "}"),
        ("dash", "-"),
        ("hyphen", "-"),
        ("underscore", "_"),
        ("star", "*"),
        ("asterisk", "*"),
        ("ampersand", "&"),
        ("at sign", "@"),
        ("percent", "%"),
        ("dollar sign", "$"),
        ("euro", "€"),
        ("pound", "£"),
        ("yen", "¥"),
        ("hash", "#"),
        ("hashtag", "#"),
        ("slash", "/"),
        ("backslash", "\\"),
        ("quote", "\""),
        ("open quote", "\""),
        ("close quote", "\""),
        ("single quote", "'"),
        ("apostrophe", "'"),
        ("tilde", "~"),
        ("caret", "^"),
        ("pipe", "|"),
        ("less than", "<"),
        ("greater than", ">"),
        ("equals", "="),
        ("plus", "+"),
        ("minus", "-"),
        ("multiply by", "×"),
        ("divide by", "÷"),
        ("section", "§"),
        ("paragraph", "¶"),
        ("bullet", "•"),
    ];

    let mut result = text.to_string();
    for (spoken, symbol) in replacements {
        // Case-insensitive whole-word replacement
        let lower = result.to_lowercase();
        let spoken_lower = spoken.to_lowercase();
        let mut i = 0;
        let mut new_result = String::new();

        while i < result.len() {
            if lower[i..].starts_with(&spoken_lower) {
                let before = i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric();
                let after = i + spoken_lower.len() >= lower.len()
                    || !lower.as_bytes()[i + spoken_lower.len()].is_ascii_alphanumeric();
                if before && after {
                    // Preserve capitalization: if first letter was uppercase, capitalize symbol
                    let char_before = if i > 0 { result.as_bytes()[i - 1] as char } else { ' ' };
                    let should_cap = !char_before.is_alphabetic()
                        && result.as_bytes()[i] as char != symbol.chars().next().unwrap_or(' ');
                    new_result.push_str(if should_cap { symbol } else { symbol });
                    i += spoken_lower.len();
                    continue;
                }
            }
            new_result.push(result.as_bytes()[i] as char);
            i += 1;
        }
        result = new_result;
    }

    // Capitalize first word after sentence-ending punctuation
    let mut chars: Vec<char> = result.chars().collect();
    let mut cap_next = true;
    for i in 0..chars.len() {
        if cap_next && chars[i].is_alphabetic() {
            chars[i] = chars[i].to_uppercase().next().unwrap_or(chars[i]);
            cap_next = false;
        } else if chars[i] == '.' || chars[i] == '!' || chars[i] == '?' {
            cap_next = true;
        } else if chars[i].is_whitespace() {
            continue;
        } else {
            cap_next = false;
        }
    }

    chars.into_iter().collect()
}

/// Full post-processing pipeline (matches FluidVoice order).
pub fn full_pipeline(text: &str, dictionary: &[(String, String)]) -> String {
    let text = remove_filler_words(text);
    let text = apply_custom_dictionary(&text, dictionary);
    apply_spoken_punctuation(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_filler_words() {
        assert_eq!(remove_filler_words("um hello world"), "hello world");
        assert_eq!(remove_filler_words("like you know"), "");
        assert_eq!(remove_filler_words("hello um world"), "hello world");
    }

    #[test]
    fn test_spoken_punctuation() {
        let result = apply_spoken_punctuation("hello period world comma how are you question mark");
        assert_eq!(result.trim(), "Hello. World, how are you?");
    }

    #[test]
    fn test_custom_dictionary() {
        let dict = vec![("op code".to_string(), "opcode".to_string())];
        let result = apply_custom_dictionary("write some op code", &dict);
        assert_eq!(result, "write some opcode");
    }
}
