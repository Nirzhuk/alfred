use url::Url;

const HTTPS_SCHEME: &str = "https";
const APP_SCHEME: &str = "alfred-workflows";
const SHARE_ID_CHARS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedShareOrigin {
    url: Url,
    development: bool,
}

impl TrustedShareOrigin {
    pub fn parse(value: &str, allow_development_http: bool) -> Result<Self, String> {
        let url = Url::parse(value.trim())
            .map_err(|_| "sharing origin is not a valid URL".to_string())?;
        let development = url.scheme() == "http"
            && allow_development_http
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
        if url.scheme() != HTTPS_SCHEME && !development {
            return Err("sharing origin must use HTTPS".into());
        }
        if url.host_str().is_none()
            || url.username() != ""
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || url.port().is_some_and(|port| !development || port == 0)
        {
            return Err("sharing origin contains an unsupported route or credential".into());
        }
        Ok(Self { url, development })
    }
    pub fn is_development(&self) -> bool {
        self.development
    }
    pub fn api_url(&self, share_id: &str) -> Result<Url, String> {
        let mut result = self
            .url
            .join(&format!("api/v1/shares/{share_id}"))
            .map_err(|_| "could not construct sharing API URL".to_string())?;
        result.set_query(None);
        Ok(result)
    }
    pub fn create_url(&self) -> Result<Url, String> {
        self.url
            .join("api/v1/shares")
            .map_err(|_| "could not construct sharing API URL".into())
    }
    pub fn preview_url(&self, share_id: &str) -> Result<Url, String> {
        self.url
            .join(&format!("s/{share_id}"))
            .map_err(|_| "could not construct sharing URL".into())
    }
    pub fn validate_publication_url(&self, value: &str, share_id: &str) -> Result<(), String> {
        if !valid_id(share_id) {
            return Err("sharing service returned a malformed share ID".into());
        }
        let url = Url::parse(value)
            .map_err(|_| "sharing service returned a malformed URL".to_string())?;
        let expected_scheme = if self.development {
            [HTTPS_SCHEME, "http"].as_slice()
        } else {
            [HTTPS_SCHEME].as_slice()
        };
        let expected_path = format!("/s/{share_id}");
        if !expected_scheme.contains(&url.scheme())
            || url.origin() != self.url.origin()
            || url.path() != expected_path
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("sharing service returned an untrusted publication URL".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareLink {
    pub share_id: String,
    pub source: ShareLinkSource,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareLinkSource {
    Https,
    AlfredScheme,
}

fn valid_id(value: &str) -> bool {
    (22..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub fn parse_share_link(value: &str, origin: &TrustedShareOrigin) -> Result<ShareLink, String> {
    let url = Url::parse(value.trim()).map_err(|_| "paste an Alfred share link".to_string())?;
    let (source, id) =
        if url.scheme() == HTTPS_SCHEME || (origin.development && url.scheme() == "http") {
            if url.origin() != origin.url.origin()
                || url
                    .path_segments()
                    .map(|segments| segments.collect::<Vec<_>>())
                    != Some(vec![
                        "s",
                        url.path_segments()
                            .and_then(|mut segments| segments.next_back())
                            .unwrap_or(""),
                    ])
                || url.query().is_some()
                || url.fragment().is_some()
                || url.username() != ""
                || url.password().is_some()
            {
                return Err("the link must be the canonical HTTPS Alfred share URL".into());
            }
            (
                ShareLinkSource::Https,
                url.path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .unwrap_or_default(),
            )
        } else if url.scheme() == APP_SCHEME {
            let segments = url
                .path_segments()
                .map(|segments| segments.collect::<Vec<_>>())
                .unwrap_or_default();
            if url.host_str() != Some("import")
                || url.query().is_some()
                || url.fragment().is_some()
                || segments.len() != 1
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
            {
                return Err("the Alfred import link is malformed".into());
            }
            (ShareLinkSource::AlfredScheme, segments[0])
        } else {
            return Err("use an HTTPS Alfred share link".into());
        };
    if !valid_id(id) || id.len() < SHARE_ID_CHARS {
        return Err("the share ID is malformed".into());
    }
    Ok(ShareLink {
        share_id: id.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_canonical_links() {
        let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
        let id = "a".repeat(32);
        assert_eq!(
            parse_share_link(&format!("https://share.example/s/{id}"), &origin)
                .unwrap()
                .share_id,
            id
        );
        assert_eq!(
            parse_share_link(&format!("alfred-workflows://import/{id}"), &origin)
                .unwrap()
                .source,
            ShareLinkSource::AlfredScheme
        );
        assert!(parse_share_link(&format!("https://share.example/s/{id}?x=1"), &origin).is_err());
    }
    #[test]
    fn development_http_is_explicit() {
        assert!(TrustedShareOrigin::parse("http://localhost:8787/", false).is_err());
        let origin = TrustedShareOrigin::parse("http://localhost:8787/", true).unwrap();
        let id = "a".repeat(32);
        assert!(parse_share_link(&format!("http://localhost:8787/s/{id}"), &origin).is_ok());
        assert!(parse_share_link(&format!("https://localhost:8787/s/{id}"), &origin).is_err());
    }
    #[test]
    fn app_links_reject_credentials_and_ports() {
        let origin = TrustedShareOrigin::parse("https://share.example/", false).unwrap();
        let id = "a".repeat(32);
        assert!(parse_share_link(
            &format!("alfred-workflows://user:pass@import:99/{id}"),
            &origin
        )
        .is_err());
    }
}
