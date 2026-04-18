use anyhow::Result;
use ego_tree::NodeRef;
use scraper::{node::Node, ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ParsedPage {
    pub url: String,
    pub title: String,
    pub meta_description: String,
    pub content: String,
    pub content_length: usize,
    pub headings: Vec<String>,
    pub links: Vec<String>,
    pub word_count: usize,
}

pub struct Parser {
    title_selector: Selector,
    meta_desc_selector: Selector,
    body_selector: Selector,
    main_selector: Selector,
    article_selector: Selector,
    h1_selector: Selector,
    h2_selector: Selector,
    h3_selector: Selector,
    link_selector: Selector,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            title_selector: Selector::parse("title").unwrap(),
            meta_desc_selector: Selector::parse("meta[name='description']").unwrap(),
            body_selector: Selector::parse("body").unwrap(),
            main_selector: Selector::parse("main").unwrap(),
            article_selector: Selector::parse("article").unwrap(),
            h1_selector: Selector::parse("h1").unwrap(),
            h2_selector: Selector::parse("h2").unwrap(),
            h3_selector: Selector::parse("h3").unwrap(),
            link_selector: Selector::parse("a[href]").unwrap(),
        }
    }

    pub fn parse(&self, url: &str, html: &str) -> Result<ParsedPage> {
        let document = Html::parse_document(html);

        let title = document
            .select(&self.title_selector)
            .next()
            .map(|el| normalize_whitespace(&el.text().collect::<String>()))
            .unwrap_or_default();

        let meta_description = document
            .select(&self.meta_desc_selector)
            .next()
            .and_then(|el| el.value().attr("content"))
            .map(normalize_whitespace)
            .unwrap_or_default();

        let root = self.content_root(&document);

        let mut headings = Vec::new();
        for selector in [&self.h1_selector, &self.h2_selector, &self.h3_selector] {
            for heading in root.select(selector) {
                let text = normalize_whitespace(&heading.text().collect::<Vec<_>>().join(" "));
                if !text.is_empty() && headings.len() < 30 {
                    headings.push(text);
                }
            }
        }

        let links: Vec<String> = root
            .select(&self.link_selector)
            .filter_map(|el| el.value().attr("href"))
            .filter(|href| href.starts_with("http"))
            .take(100)
            .map(|href| href.to_string())
            .collect();

        let content = self.extract_clean_text(&document);
        let content_length = content.len();
        let word_count = content.split_whitespace().count();

        Ok(ParsedPage {
            url: url.to_string(),
            title,
            meta_description,
            content,
            content_length,
            headings,
            links,
            word_count,
        })
    }

    fn extract_clean_text(&self, document: &Html) -> String {
        let root = self.content_root(document);

        let mut chunks = Vec::new();
        collect_text(root, &mut chunks, false);
        normalize_whitespace(&chunks.join(" "))
    }

    fn content_root<'a>(&self, document: &'a Html) -> ElementRef<'a> {
        document
            .select(&self.main_selector)
            .next()
            .or_else(|| document.select(&self.article_selector).next())
            .or_else(|| document.select(&self.body_selector).next())
            .unwrap_or_else(|| document.root_element())
    }
}

fn collect_text(element: ElementRef<'_>, chunks: &mut Vec<String>, skip_subtree: bool) {
    let should_skip = skip_subtree || should_skip_element(element);

    for child in element.children() {
        collect_node(child, chunks, should_skip);
    }
}

fn collect_node(node: NodeRef<'_, Node>, chunks: &mut Vec<String>, skip_subtree: bool) {
    match node.value() {
        Node::Text(text) if !skip_subtree => {
            let normalized = normalize_whitespace(text);
            if !normalized.is_empty() {
                chunks.push(normalized);
            }
        }
        Node::Element(_) => {
            if let Some(element) = ElementRef::wrap(node) {
                collect_text(element, chunks, skip_subtree);
            }
        }
        _ => {}
    }
}

fn should_skip_element(element: ElementRef<'_>) -> bool {
    let name = element.value().name();
    if matches!(
        name,
        "script"
            | "style"
            | "nav"
            | "header"
            | "footer"
            | "aside"
            | "iframe"
            | "noscript"
            | "svg"
            | "canvas"
            | "template"
    ) {
        return true;
    }

    if matches!(
        element.attr("role"),
        Some("navigation" | "banner" | "contentinfo" | "complementary")
    ) {
        return true;
    }

    if element.attr("hidden").is_some() || matches!(element.attr("aria-hidden"), Some("true")) {
        return true;
    }

    if let Some(style) = element.attr("style") {
        let style = style.to_ascii_lowercase();
        if style.contains("display:none")
            || style.contains("display: none")
            || style.contains("visibility:hidden")
            || style.contains("visibility: hidden")
        {
            return true;
        }
    }

    if let Some(class_attr) = element.attr("class") {
        let lowered = class_attr.to_ascii_lowercase();
        for token in [
            "nav",
            "menu",
            "sidebar",
            "footer",
            "header",
            "advert",
            "ads",
            "promo",
            "breadcrumb",
            "cookie",
            "modal",
            "popup",
        ] {
            if lowered.contains(token) {
                return true;
            }
        }
    }

    if let Some(id_attr) = element.attr("id") {
        let lowered = id_attr.to_ascii_lowercase();
        for token in [
            "nav", "menu", "sidebar", "footer", "header", "cookie", "popup",
        ] {
            if lowered.contains(token) {
                return true;
            }
        }
    }

    false
}

fn normalize_whitespace(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

impl ParsedPage {
    pub fn to_jsonl(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_jsonl(line: &str) -> Result<Self> {
        Ok(serde_json::from_str(line)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_parsing_prefers_main_content() {
        let parser = Parser::new();
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test Page</title>
                <meta name="description" content="This is a test page">
            </head>
            <body>
                <header>Header content</header>
                <nav>Navigation</nav>
                <main>
                    <h1>Main Content</h1>
                    <p>This is the main content of the page.</p>
                </main>
                <footer>Footer content</footer>
                <script>console.log('test');</script>
            </body>
            </html>
        "#;

        let result = parser.parse("https://example.com", html).unwrap();

        assert_eq!(result.title, "Test Page");
        assert_eq!(result.meta_description, "This is a test page");
        assert!(result.content.contains("Main Content"));
        assert!(result.content.contains("main content of the page"));
        assert!(!result.content.contains("Header content"));
        assert!(!result.content.contains("Footer content"));
        assert!(!result.content.contains("Navigation"));
    }

    #[test]
    fn test_hidden_and_ad_content_is_removed() {
        let parser = Parser::new();
        let html = r#"
            <html>
                <body>
                    <article>
                        <p>Visible content</p>
                        <div style="display: none">Hidden text</div>
                        <div class="sidebar-ad">Buy now</div>
                        <section aria-hidden="true">Screen reader hidden</section>
                        <p>More visible content</p>
                    </article>
                </body>
            </html>
        "#;

        let result = parser.parse("https://example.com/article", html).unwrap();

        assert!(result.content.contains("Visible content"));
        assert!(result.content.contains("More visible content"));
        assert!(!result.content.contains("Hidden text"));
        assert!(!result.content.contains("Buy now"));
        assert!(!result.content.contains("Screen reader hidden"));
    }

    #[test]
    fn test_jsonl_serialization() {
        let parser = Parser::new();
        let html = "<html><head><title>Test</title></head><body><p>Content</p></body></html>";
        let parsed = parser.parse("https://example.com", html).unwrap();

        let jsonl = parsed.to_jsonl().unwrap();
        assert!(!jsonl.contains('\n'));

        let restored = ParsedPage::from_jsonl(&jsonl).unwrap();
        assert_eq!(restored.url, parsed.url);
        assert_eq!(restored.title, parsed.title);
        assert_eq!(restored.word_count, parsed.word_count);
    }
}
