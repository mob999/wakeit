use pulldown_cmark::{Options, Parser};
use scraper::{Html, Selector};
use serde_json::Value;
pub fn report_text(result: &Value) -> Option<&str> {
    result.as_str().or_else(|| {
        (result["truncated"] == true)
            .then(|| result["preview"].as_str())
            .flatten()
    })
}
fn link_key(href: &str) -> std::borrow::Cow<'_, str> {
    percent_encoding::percent_decode_str(href.split(['#', '?']).next().unwrap_or(""))
        .decode_utf8_lossy()
}
pub fn linked(text: &str, href: &str) -> bool {
    authored_target(text, href, "a[href]", "href")
}
pub fn image_linked(text: &str, href: &str) -> bool {
    authored_target(text, href, "img[src]", "src")
}
fn authored_target(text: &str, href: &str, selector: &str, attribute: &str) -> bool {
    let mut rendered = String::new();
    // Parsing to HTML preserves CommonMark's link/code distinction; HTML parsing
    // keeps comments, raw script text and escaped anchors out of the link set.
    pulldown_cmark::html::push_html(
        &mut rendered,
        Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH),
    );
    Html::parse_fragment(&rendered)
        .select(&Selector::parse(selector).expect("static selector"))
        .any(|a| {
            a.value()
                .attr(attribute)
                .is_some_and(|value| link_key(value) == link_key(href))
                && !a
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .any(|e| {
                        matches!(
                            e.value().name(),
                            "script" | "style" | "iframe" | "object" | "template"
                        )
                    })
        })
}

/// The canonical path comes from the Runner, whose OS can differ from Gateway.
/// Keep the final separator so POSIX and Windows drive roots remain absolute.
pub fn document_directory(path: &str) -> Option<&str> {
    path.rfind(['/', '\\']).map(|end| &path[..=end])
}

pub fn local_link(href: &str) -> bool {
    if href.is_empty() || href.starts_with('#') || href.starts_with("//") {
        return false;
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return true;
    }
    if href.as_bytes().get(1) == Some(&b':') && href.as_bytes()[0].is_ascii_alphabetic() {
        return true;
    }
    if href
        .split(['/', '\\', '?', '#'])
        .next()
        .is_some_and(|part| part.contains(':'))
    {
        return false;
    }
    ![
        "/",
        "/w",
        "/tasks",
        "/runs",
        "/runners",
        "/settings",
        "/login",
        "/authorize",
        "/api",
        "/assets",
    ]
    .iter()
    .any(|route| {
        href == *route
            || (*route != "/" && href.starts_with(&format!("{route}/")))
            || href.starts_with(&format!("{route}?"))
    })
}
