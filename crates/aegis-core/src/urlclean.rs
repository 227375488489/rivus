//! Removal of tracking parameters, applied before a request is made and when
//! a link is copied out of the browser.

use url::Url;

const PARAMS: &[&str] = &[
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "twclid", "ttclid",
    "li_fat_id", "igshid", "igsh", "mc_cid", "mc_eid", "mkt_tok", "_hsenc", "_hsmi", "hsCtaTracking",
    "vero_id", "vero_conv", "oly_anon_id", "oly_enc_id", "rb_clickid", "s_cid", "wickedid",
    "epik", "irclickid", "sc_cid", "sc_campaign", "sc_channel", "sc_content", "sc_medium",
    "sc_outcome", "sc_geo", "sc_country", "at_medium", "at_campaign", "at_custom1", "at_custom2",
    "ref_src", "ref_url", "spm", "scm", "share_id", "trk", "trkcampaign", "trk_contact",
    "trk_module", "trk_sid", "trk_msg", "ito", "cid_source", "campaign_id", "ad_id", "adset_id",
    "creative_id", "placement_id", "gad_source", "srsltid", "ppi", "pk_campaign", "pk_kwd",
    "pk_source", "pk_medium", "mtm_campaign", "mtm_source", "mtm_medium", "mtm_content",
    "mtm_cid", "mtm_group", "mtm_placement", "hsa_cam", "hsa_grp", "hsa_mt", "hsa_src",
    "hsa_ad", "hsa_acc", "hsa_net", "hsa_kw", "hsa_tgt", "hsa_ver", "hsa_la", "hsa_ol",
    "hsa_plc", "gclsrc", "dclsrc", "_ga", "_gl", "_gac", "ga_source", "ga_medium", "ga_term",
    "ga_content", "ga_campaign", "ga_place", "ga_id", "ga_sourceid", "ga_mediumid",
];

const PREFIXES: &[&str] = &["utm_", "pk_", "mtm_", "hsa_", "vero_", "oly_", "at_", "sc_"];

pub fn is_tracking_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PARAMS.iter().any(|p| *p == lower) || PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Returns the cleaned URL and the names of the parameters that were removed.
pub fn clean(url: &Url) -> (Url, Vec<String>) {
    let mut cleaned = url.clone();
    let mut removed = Vec::new();
    let pairs: Vec<(String, String)> = cleaned
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if pairs.is_empty() {
        return (cleaned, removed);
    }
    let kept: Vec<(String, String)> = pairs
        .into_iter()
        .filter(|(k, _)| {
            if is_tracking_param(k) {
                removed.push(k.clone());
                false
            } else {
                true
            }
        })
        .collect();
    if removed.is_empty() {
        return (cleaned, removed);
    }
    if kept.is_empty() {
        cleaned.set_query(None);
    } else {
        let mut ser = cleaned.query_pairs_mut();
        ser.clear();
        for (k, v) in &kept {
            ser.append_pair(k, v);
        }
    }
    (cleaned, removed)
}

/// Unwrap known redirectors (`l.facebook.com/l.php?u=…`, `google.com/url?q=…`)
/// so the tracker never sees the click.
pub fn unwrap_redirect(url: &Url) -> Option<Url> {
    let host = url.host_str()?.to_ascii_lowercase();
    let param = match host.as_str() {
        "l.facebook.com" | "lm.facebook.com" => "u",
        "www.google.com" | "google.com" => "q",
        "www.youtube.com" if url.path() == "/redirect" => "q",
        "t.umblr.com" => "z",
        "out.reddit.com" => "url",
        _ => return None,
    };
    if host == "www.google.com" || host == "google.com" {
        if url.path() != "/url" {
            return None;
        }
    }
    let raw = url
        .query_pairs()
        .find(|(k, _)| k == param)
        .map(|(_, v)| v.to_string())?;
    let inner = Url::parse(&raw).ok()?;
    if matches!(inner.scheme(), "http" | "https") {
        Some(inner)
    } else {
        None
    }
}

pub fn clean_for_navigation(url: &Url) -> (Url, Vec<String>) {
    let mut current = url.clone();
    let mut removed = Vec::new();
    for _ in 0..3 {
        if let Some(inner) = unwrap_redirect(&current) {
            current = inner;
            continue;
        }
        break;
    }
    let (cleaned, names) = clean(&current);
    removed.extend(names);
    (cleaned, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_common_trackers() {
        let u = Url::parse("https://shop.example/item?id=7&utm_source=news&fbclid=abc&gclid=1#top")
            .unwrap();
        let (c, removed) = clean(&u);
        assert_eq!(c.as_str(), "https://shop.example/item?id=7#top");
        assert_eq!(removed.len(), 3);
    }

    #[test]
    fn keeps_functional_params() {
        let u = Url::parse("https://example.com/?page=2&q=rust&ref=nav").unwrap();
        let (c, removed) = clean(&u);
        assert_eq!(c.as_str(), u.as_str());
        assert!(removed.is_empty());
    }

    #[test]
    fn drops_empty_query() {
        let u = Url::parse("https://example.com/?utm_campaign=x").unwrap();
        let (c, _) = clean(&u);
        assert_eq!(c.as_str(), "https://example.com/");
    }

    #[test]
    fn unwraps_facebook() {
        let u = Url::parse(
            "https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.org%2Fpost%3Fid%3D2&h=AT0",
        )
        .unwrap();
        let (c, _) = clean_for_navigation(&u);
        assert_eq!(c.as_str(), "https://example.org/post?id=2");
    }
}
