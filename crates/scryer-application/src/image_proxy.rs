use crate::{AppUseCase, ImageProxyKind, ImageProxyRegistration, ImageProxyRepository};
use std::sync::Arc;

pub fn image_proxy_source_token(
    normalized_upstream_url: Option<&str>,
    owner_type: Option<&str>,
    owner_id: Option<&str>,
    image_kind: ImageProxyKind,
) -> String {
    fn update_part(hasher: &mut blake3::Hasher, value: Option<&str>) {
        match value {
            Some(value) => {
                hasher.update(&[1]);
                hasher.update(&(value.len() as u64).to_be_bytes());
                hasher.update(value.as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"scryer:image-proxy-source:v1");
    update_part(&mut hasher, normalized_upstream_url);
    update_part(&mut hasher, owner_type);
    update_part(&mut hasher, owner_id);
    update_part(&mut hasher, Some(image_kind.as_str()));
    hasher.finalize().to_hex().to_string()
}

/// The address to fetch an image from at a display size. TMDB's image CDN
/// serves each asset at fixed widths, so its address is rewritten to the width
/// that covers the variant; a host without sized renditions keeps its address.
/// None when the address is not a URL or the kind has no such variant.
pub fn image_upstream_variant_url(source_url: &str, kind: &str, variant: &str) -> Option<String> {
    let mut parsed = url::Url::parse(source_url).ok()?;
    if parsed.host_str()?.eq_ignore_ascii_case("image.tmdb.org") {
        let size = match (kind, variant) {
            ("poster", "w70") => "w92",
            ("poster", "w250") => "w300",
            ("poster", "original") | ("fanart", "original") => "original",
            ("fanart", "w1280") => "w1280",
            ("person", "w185") => "w185",
            ("person", "original") => "original",
            ("episode_still", "w300") => "w300",
            ("episode_still", "original") => "original",
            _ => return None,
        };
        let path = parsed.path();
        let prefix = "/t/p/";
        let rest = path.strip_prefix(prefix)?;
        let (_, asset) = rest.split_once('/')?;
        parsed.set_path(&format!("{prefix}{size}/{asset}"));
    }
    Some(parsed.to_string())
}

impl AppUseCase {
    pub fn media_image_url(
        &self,
        upstream_url: Option<&str>,
        owner_type: Option<&str>,
        owner_id: Option<&str>,
        image_kind: ImageProxyKind,
        default_variant: &str,
    ) -> Option<String> {
        let upstream_url = upstream_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if upstream_url.is_none() && owner_id.is_none() {
            return None;
        }

        let fallback_class = match image_kind {
            ImageProxyKind::Poster | ImageProxyKind::Person => "portrait",
            ImageProxyKind::Fanart | ImageProxyKind::EpisodeStill => "landscape",
        };
        Some(
            self.services
                .library
                .image_proxy
                .register_image_source(ImageProxyRegistration {
                    upstream_url,
                    owner_type: owner_type.map(str::to_string),
                    owner_id: owner_id.map(str::to_string),
                    image_kind,
                    fallback_class: fallback_class.to_string(),
                    default_variant: default_variant.to_string(),
                }),
        )
    }

    pub fn image_proxy_repository(&self) -> Arc<dyn ImageProxyRepository> {
        self.services.library.image_proxy.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::image_proxy_source_token;
    use crate::ImageProxyKind;

    #[test]
    fn source_tokens_use_unambiguous_versioned_context() {
        let left = image_proxy_source_token(
            Some("https://image.tmdb.org/t/p/original/a.jpg"),
            Some("title"),
            Some("a\0b"),
            ImageProxyKind::Poster,
        );
        let right = image_proxy_source_token(
            Some("https://image.tmdb.org/t/p/original/a.jpg"),
            Some("title\0a"),
            Some("b"),
            ImageProxyKind::Poster,
        );
        assert_ne!(left, right);
    }
}
