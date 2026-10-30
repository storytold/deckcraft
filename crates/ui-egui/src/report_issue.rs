//! Help › Report an Issue…: opens a new GitHub issue with the version and system filled in. Nothing
//! is created until the user submits it.

use crate::SlideApp;

/// File types DeckCraft opens; anything else could be private text, so it is not named.
const KNOWN_TYPES: &[&str] = &["deckcraft", "pptx", "potx", "ppsx"];

/// The new-issue address. `version` and `environment` are the bug form's field ids, and `body`
/// fills a blank issue where no form exists. The document is described by its file extension
/// alone, never a name, path or contents.
pub(crate) fn issue_url(version: &str, os: &str, document_ext: Option<&str>) -> String {
    let short = |s: &str| s.chars().filter(|c| !c.is_control()).take(40).collect::<String>();
    let ext = document_ext.filter(|e| KNOWN_TYPES.contains(e)).unwrap_or("none");
    let (version, environment) = (short(version), format!("{}; document: {ext}", short(os)));
    let encode = |s: &str| {
        let keep = |b: &u8| b.is_ascii_alphanumeric() || b"-_.~".contains(b);
        s.bytes().map(|b| if keep(&b) { char::from(b).to_string() } else { format!("%{b:02X}") }).collect::<String>()
    };
    let body = format!("DeckCraft {version}\n{environment}");
    let (v, e, b) = (encode(&version), encode(&environment), encode(&body));
    format!("{}?template=bug_report.yml&version={v}&environment={e}&os={e}&body={b}", deckcraft_engine::links::NEW_ISSUE)
}

/// The address for the open presentation, or for none.
pub(crate) fn url(app: &SlideApp) -> String {
    let ext =
        app.session.active().and_then(|d| d.path.as_deref()).and_then(|p| std::path::Path::new(p).extension()?.to_str()).map(str::to_ascii_lowercase);
    let os = if cfg!(target_arch = "wasm32") { "Web".into() } else { format!("{} {}", std::env::consts::OS, std::env::consts::ARCH) };
    issue_url(crate::dialogs::version_line().trim_start_matches("Version "), &os, ext.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_carries_version_and_system_only_and_stays_short() {
        let url = issue_url("0.1.0", "linux x86_64", Some("pptx"));
        assert!(url.starts_with("https://github.com/storytold/deckcraft/issues/new?template=bug_report.yml&version=0.1.0&environment=linux%20x86_64%3B%20document%3A%20pptx&os="), "{url}");
        // Hostile text and an unusual extension can't add a parameter, leak, or grow the address.
        let url = issue_url(&"\u{1F600}".repeat(99), &"&#=".repeat(99), Some("x.y/Z"));
        assert_eq!(url.matches('?').count() + url.matches('#').count(), 1, "{url}");
        assert!(url.is_ascii() && url.contains("document%3A%20none") && url.len() < 2048, "{} bytes: {url}", url.len());
    }
}
