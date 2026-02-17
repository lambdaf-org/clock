use regex::Regex;
use once_cell::sync::Lazy;

static RE_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static RE_HYPHENS: Lazy<Regex> = Lazy::new(|| Regex::new(r"-+").unwrap());

/// Normalize an activity name:
/// 1. Collapse excessive character repetition:
///    - Exactly 3 consecutive identical characters → keep 2
///    - 4+ consecutive identical characters → keep 1
/// 2. Split PascalCase/camelCase into hyphenated lowercase (e.g., "WorkSchool" → "work-school")
/// 3. Lowercase everything
pub fn normalize_activity(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Step 1: Collapse 3+ consecutive identical characters to 1
    let collapsed = collapse_repeated_chars(trimmed);

    // Step 2: Detect and split PascalCase/camelCase boundaries with hyphens
    let hyphenated = split_camel_case(&collapsed);

    // Step 3: Lowercase and normalize whitespace/hyphens
    let lowercased = hyphenated.to_lowercase();
    
    // Normalize multiple spaces to single space
    let normalized_spaces = RE_SPACES.replace_all(&lowercased, " ");
    
    // Normalize multiple hyphens to single hyphen
    let normalized_hyphens = RE_HYPHENS.replace_all(&normalized_spaces, "-");
    
    // Trim any leading/trailing spaces or hyphens
    normalized_hyphens.trim_matches(|c| c == ' ' || c == '-').to_string()
}

/// Collapse 3+ consecutive identical characters
/// - Exactly 3 consecutive: keep 2
/// - 4+ consecutive: keep 1
fn collapse_repeated_chars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let current = chars[i];

        // Count consecutive identical characters
        let mut count = 1;
        while i + count < chars.len() && chars[i + count] == current {
            count += 1;
        }

        // Apply collapsing rules:
        // - 1-2 consecutive: keep all
        // - Exactly 3: keep 2
        // - 4+: keep 1
        if count < 3 {
            for _ in 0..count {
                result.push(current);
            }
        } else if count == 3 {
            result.push(current);
            result.push(current);
        } else {
            result.push(current);
        }

        i += count;
    }

    result
}

/// Split camelCase/PascalCase into hyphenated words
fn split_camel_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 10);
    let chars: Vec<char> = s.chars().collect();

    for i in 0..chars.len() {
        let current = chars[i];

        // Insert hyphen before uppercase letter if:
        // 1. Previous char is lowercase (e.g., "workSchool" -> "work-School")
        // 2. Previous char is uppercase and next char is lowercase (e.g., "MyApp" -> "My-App")
        if i > 0 && current.is_uppercase() {
            let prev = chars[i - 1];
            let next = if i + 1 < chars.len() {
                Some(chars[i + 1])
            } else {
                None
            };

            // Case 1: lowercase followed by uppercase
            if prev.is_lowercase() {
                result.push('-');
            }
            // Case 2: uppercase followed by uppercase then lowercase (e.g., "MyApp" -> "My-App")
            else if prev.is_uppercase() && next.map_or(false, |n| n.is_lowercase()) {
                result.push('-');
            }
        }

        result.push(current);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_repeated_chars() {
        assert_eq!(collapse_repeated_chars("workkkkkkk"), "work");
        assert_eq!(collapse_repeated_chars("schoool"), "school");
        assert_eq!(collapse_repeated_chars("boring workkkk"), "boring work");
        assert_eq!(collapse_repeated_chars("work"), "work");
        assert_eq!(collapse_repeated_chars("school"), "school");
        assert_eq!(collapse_repeated_chars("meeting"), "meeting");
        assert_eq!(collapse_repeated_chars("booooring"), "boring");
        // Demonstrate that 3 consecutive becomes 2
        assert_eq!(collapse_repeated_chars("workkk"), "workk");
    }

    #[test]
    fn test_split_camel_case() {
        assert_eq!(split_camel_case("WorkSchool"), "Work-School");
        assert_eq!(split_camel_case("workSchool"), "work-School");
        assert_eq!(split_camel_case("MyAppDev"), "My-App-Dev");
        assert_eq!(split_camel_case("work"), "work");
        assert_eq!(split_camel_case("WORK"), "WORK");
        assert_eq!(split_camel_case("school"), "school");
    }

    #[test]
    fn test_normalize_activity() {
        assert_eq!(normalize_activity("workkkkkkk"), "work");
        assert_eq!(normalize_activity("schoool"), "school");
        assert_eq!(normalize_activity("boring workkkk"), "boring work");
        assert_eq!(normalize_activity("WorkSchool"), "work-school");
        assert_eq!(normalize_activity("workSchool"), "work-school");
        assert_eq!(normalize_activity("MyAppDev"), "my-app-dev");
        assert_eq!(normalize_activity("work"), "work");
        assert_eq!(normalize_activity("school"), "school");
        assert_eq!(normalize_activity("meeting"), "meeting");
        assert_eq!(normalize_activity("WORK"), "work");
        assert_eq!(normalize_activity("  work  "), "work");
        assert_eq!(normalize_activity("work-School"), "work-school");
    }

    #[test]
    fn test_edge_cases() {
        assert_eq!(normalize_activity(""), "");
        assert_eq!(normalize_activity("   "), "");
        assert_eq!(normalize_activity("a"), "a");
        assert_eq!(normalize_activity("ab"), "ab");
        assert_eq!(normalize_activity("aaa"), "aa");  // 3 consecutive → 2
        assert_eq!(normalize_activity("aabbcc"), "aabbcc");  // all doubles, no change
        assert_eq!(normalize_activity("aaabbbccc"), "aabbcc");  // 3 of each → 2 of each
    }
}
