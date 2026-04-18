use anyhow::Result;
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone)]
pub struct RobotsTxt {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Rule {
    user_agent: String,
    disallowed: Vec<String>,
    allowed: Vec<String>,
}

impl RobotsTxt {
    pub fn parse(content: &str) -> Self {
        let mut rules = Vec::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut current_disallow = Vec::new();
        let mut current_allow = Vec::new();

        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim().to_string();

                match key.as_str() {
                    "user-agent" => {
                        // Save previous rule if exists
                        if !current_agents.is_empty() {
                            for agent in &current_agents {
                                rules.push(Rule {
                                    user_agent: agent.clone(),
                                    disallowed: current_disallow.clone(),
                                    allowed: current_allow.clone(),
                                });
                            }
                        }

                        // Start new rule
                        current_agents = vec![value.to_lowercase()];
                        current_disallow.clear();
                        current_allow.clear();
                    }
                    "disallow" => {
                        if !value.is_empty() {
                            current_disallow.push(value);
                        }
                    }
                    "allow" => {
                        if !value.is_empty() {
                            current_allow.push(value);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Save last rule
        if !current_agents.is_empty() {
            for agent in &current_agents {
                rules.push(Rule {
                    user_agent: agent.clone(),
                    disallowed: current_disallow.clone(),
                    allowed: current_allow.clone(),
                });
            }
        }

        Self { rules }
    }

    pub fn is_allowed(&self, path: &str, user_agent: &str) -> bool {
        let user_agent_lower = user_agent.to_lowercase();

        // Find matching rules - prefer specific agent over *
        let mut specific_rules = Vec::new();
        let mut wildcard_rules = Vec::new();

        for rule in &self.rules {
            if rule.user_agent == "*" {
                wildcard_rules.push(rule);
            } else if user_agent_lower.contains(&rule.user_agent) {
                specific_rules.push(rule);
            }
        }

        // Use specific rules if available, otherwise wildcard
        let rules_to_check = if !specific_rules.is_empty() {
            specific_rules
        } else {
            wildcard_rules
        };

        if rules_to_check.is_empty() {
            return true; // No rules = allowed
        }

        // Check rules
        for rule in rules_to_check {
            // Check Allow rules first (they take precedence)
            for allowed in &rule.allowed {
                if path_matches(path, allowed) {
                    return true;
                }
            }

            // Check Disallow rules
            for disallowed in &rule.disallowed {
                if path_matches(path, disallowed) {
                    return false;
                }
            }
        }

        true // Default to allowed
    }
}

fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern == "/" {
        return true; // Matches everything
    }

    // Handle patterns with * and $
    if pattern.contains('*') {
        if pattern.ends_with('$') {
            // e.g., "/*.pdf$" - must end with .pdf
            let pattern_no_dollar = &pattern[..pattern.len() - 1];
            let parts: Vec<&str> = pattern_no_dollar.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0];
                let suffix = parts[1];
                return path.starts_with(prefix) && path.ends_with(suffix);
            }
        } else {
            // e.g., "/*.php" or "/temp*"
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0];
                let suffix = parts[1];
                if suffix.is_empty() {
                    return path.starts_with(prefix);
                } else {
                    return path.starts_with(prefix) && path.ends_with(suffix);
                }
            }
        }
    }

    if pattern.ends_with('$') {
        let exact = &pattern[..pattern.len() - 1];
        path == exact
    } else {
        path.starts_with(pattern)
    }
}

pub struct RobotsCache {
    cache: HashMap<String, Option<RobotsTxt>>,
}

impl RobotsCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub async fn is_allowed(&mut self, url: &str, user_agent: &str) -> Result<bool> {
        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("No host in URL"))?;

        let robots_url = format!("{}://{}/robots.txt", parsed.scheme(), domain);

        // Check cache
        if !self.cache.contains_key(&robots_url) {
            // Fetch robots.txt
            let robots = self.fetch_robots(&robots_url).await;
            self.cache.insert(robots_url.clone(), robots);
        }

        let path = parsed.path();

        match self.cache.get(&robots_url) {
            Some(Some(robots)) => Ok(robots.is_allowed(path, user_agent)),
            Some(None) => Ok(true), // No robots.txt = allow all
            None => Ok(true),
        }
    }

    async fn fetch_robots(&self, url: &str) -> Option<RobotsTxt> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .ok()?;

        let response = client.get(url).send().await.ok()?;

        if !response.status().is_success() {
            return None; // No robots.txt = allow all
        }

        let content = response.text().await.ok()?;
        Some(RobotsTxt::parse(&content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_robots_txt() {
        let content = r#"
User-agent: *
Disallow: /admin
Disallow: /private/
Allow: /public

User-agent: Googlebot
Disallow: /temp
        "#;

        let robots = RobotsTxt::parse(content);

        // Test for any user agent (uses * rules)
        assert!(robots.is_allowed("/", "MyBot"));
        assert!(robots.is_allowed("/public", "MyBot"));
        assert!(!robots.is_allowed("/admin", "MyBot"));
        assert!(!robots.is_allowed("/private/data", "MyBot"));

        // Test for Googlebot (uses Googlebot-specific rules, not * rules)
        assert!(!robots.is_allowed("/temp", "Googlebot"));
        assert!(robots.is_allowed("/admin", "Googlebot")); // Not blocked because specific rule doesn't mention /admin
    }

    #[test]
    fn test_allow_precedence() {
        let content = r#"
User-agent: *
Disallow: /
Allow: /public
        "#;

        let robots = RobotsTxt::parse(content);

        assert!(robots.is_allowed("/public", "MyBot"));
        assert!(robots.is_allowed("/public/page", "MyBot"));
        assert!(!robots.is_allowed("/private", "MyBot"));
    }

    #[test]
    fn test_wildcard_patterns() {
        let content = r#"
User-agent: *
Disallow: /*.pdf$
Disallow: /temp*
        "#;

        let robots = RobotsTxt::parse(content);

        assert!(!robots.is_allowed("/document.pdf", "MyBot"));
        assert!(!robots.is_allowed("/temp", "MyBot"));
        assert!(!robots.is_allowed("/temporary", "MyBot"));
        assert!(robots.is_allowed("/document.html", "MyBot"));
    }

    #[test]
    fn test_empty_robots() {
        let robots = RobotsTxt::parse("");
        assert!(robots.is_allowed("/anything", "MyBot"));
    }

    #[test]
    fn test_disallow_all() {
        let content = r#"
User-agent: *
Disallow: /
        "#;

        let robots = RobotsTxt::parse(content);
        assert!(!robots.is_allowed("/", "MyBot"));
        assert!(!robots.is_allowed("/anything", "MyBot"));
    }
}
