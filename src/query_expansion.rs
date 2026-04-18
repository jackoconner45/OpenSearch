use std::collections::HashMap;

pub struct QueryExpander {
    synonyms: HashMap<String, Vec<String>>,
}

impl QueryExpander {
    pub fn new() -> Self {
        let mut synonyms = HashMap::new();
        
        // Common synonyms for search terms
        synonyms.insert("internet".to_string(), vec!["web".to_string(), "online".to_string(), "net".to_string()]);
        synonyms.insert("domain".to_string(), vec!["website".to_string(), "site".to_string()]);
        synonyms.insert("security".to_string(), vec!["secure".to_string(), "safety".to_string(), "protection".to_string()]);
        synonyms.insert("register".to_string(), vec!["registration".to_string(), "signup".to_string()]);
        synonyms.insert("manage".to_string(), vec!["management".to_string(), "admin".to_string(), "control".to_string()]);
        synonyms.insert("protocol".to_string(), vec!["standard".to_string(), "specification".to_string()]);
        synonyms.insert("address".to_string(), vec!["ip".to_string(), "location".to_string()]);
        synonyms.insert("server".to_string(), vec!["host".to_string(), "machine".to_string()]);
        synonyms.insert("network".to_string(), vec!["net".to_string(), "connection".to_string()]);
        synonyms.insert("database".to_string(), vec!["db".to_string(), "data".to_string(), "storage".to_string()]);
        
        Self { synonyms }
    }
    
    pub fn expand(&self, query: &str) -> String {
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut expanded_terms = Vec::new();
        
        for word in words {
            let word_lower = word.to_lowercase();
            expanded_terms.push(word.to_string());
            
            // Add synonyms if available
            if let Some(syns) = self.synonyms.get(&word_lower) {
                for syn in syns {
                    expanded_terms.push(format!("OR {}", syn));
                }
            }
        }
        
        expanded_terms.join(" ")
    }
}

pub fn suggest_correction(query: &str, dictionary: &[String]) -> Option<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut corrections = Vec::new();
    let mut has_correction = false;
    
    for word in words {
        let word_lower = word.to_lowercase();
        
        // Find closest match in dictionary
        let mut best_match = word.to_string();
        let mut best_distance = 3; // Max edit distance
        
        for dict_word in dictionary {
            let distance = levenshtein_distance(&word_lower, &dict_word.to_lowercase());
            if distance > 0 && distance < best_distance {
                best_distance = distance;
                best_match = dict_word.clone();
                has_correction = true;
            }
        }
        
        corrections.push(best_match);
    }
    
    if has_correction {
        Some(corrections.join(" "))
    } else {
        None
    }
}

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    
    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }
    
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];
    
    for i in 0..=len1 {
        matrix[i][0] = i;
    }
    for j in 0..=len2 {
        matrix[0][j] = j;
    }
    
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    
    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] { 0 } else { 1 };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(matrix[i - 1][j] + 1, matrix[i][j - 1] + 1),
                matrix[i - 1][j - 1] + cost,
            );
        }
    }
    
    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_query_expansion() {
        let expander = QueryExpander::new();
        let expanded = expander.expand("internet security");
        
        assert!(expanded.contains("internet"));
        assert!(expanded.contains("security"));
        assert!(expanded.contains("OR web") || expanded.contains("OR online"));
        assert!(expanded.contains("OR secure") || expanded.contains("OR safety"));
    }
    
    #[test]
    fn test_spelling_correction() {
        let dictionary = vec![
            "internet".to_string(),
            "domain".to_string(),
            "security".to_string(),
        ];
        
        let correction = suggest_correction("internot", &dictionary);
        assert_eq!(correction, Some("internet".to_string()));
        
        let correction = suggest_correction("domian", &dictionary);
        assert_eq!(correction, Some("domain".to_string()));
    }
    
    #[test]
    fn test_no_correction_needed() {
        let dictionary = vec!["internet".to_string()];
        let correction = suggest_correction("internet", &dictionary);
        assert_eq!(correction, None);
    }

    #[test]
    fn test_no_synonym_passthrough() {
        let expander = QueryExpander::new();
        let expanded = expander.expand("elephant");
        assert_eq!(expanded, "elephant", "unknown term should pass through unchanged");
    }

    #[test]
    fn test_correction_no_close_match() {
        let dictionary = vec!["internet".to_string()];
        // "xyz" is far from "internet" (edit distance > 2)
        let correction = suggest_correction("xyz", &dictionary);
        assert_eq!(correction, None);
    }

    #[test]
    fn test_expansion_preserves_original_terms() {
        let expander = QueryExpander::new();
        let expanded = expander.expand("web domain");
        assert!(expanded.contains("web"), "original term 'web' must be present");
        assert!(expanded.contains("domain"), "original term 'domain' must be present");
    }
}
