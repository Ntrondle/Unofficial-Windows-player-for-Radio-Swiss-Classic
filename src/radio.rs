//! Station metadata shared by the two native front ends.
use serde::Deserialize;

pub const ENDPOINT: &str = "https://ssatr.playlist-api.deliver.media/graphql";
pub const CHANNELS: [(&str, &str, &str); 3] = [
    ("de", "0191e9e4-ffc8-782b-8ace-6604e0d6f2dc", "TitleDE"),
    ("fr", "0191e9e5-213d-705e-b520-cee967358e6f", "TitleFR"),
    ("it", "0191e9e5-3db3-7deb-ae10-48c24845852b", "TitleIT"),
];
pub const MAX_METADATA_BYTES: usize = 256 * 1024;

#[derive(Debug, Default, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
}

pub fn request_body(language: usize) -> Result<Vec<u8>, String> {
    let (_, channel, field) = CHANNELS.get(language).ok_or("Invalid language")?;
    let query = format!("query ($chan: String) {{ channel(id: $chan) {{ playingnow {{ current {{ metadata {{ artist title: {field} }} }} }} }} }}");
    serde_json::to_vec(&serde_json::json!({"query": query, "variables": {"chan": channel}}))
        .map_err(|error| error.to_string())
}

pub fn parse_track(response: &[u8]) -> Result<Track, String> {
    if response.len() > MAX_METADATA_BYTES {
        return Err("Metadata response too large".into());
    }
    let json: serde_json::Value = serde_json::from_slice(response).map_err(|e| e.to_string())?;
    let metadata = json
        .pointer("/data/channel/playingnow/current/metadata")
        .filter(|value| value.is_object())
        .ok_or("Track information unavailable")?;
    serde_json::from_value(metadata.clone()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_requests_use_the_corresponding_channel_and_title() {
        for (i, (_, id, field)) in CHANNELS.iter().enumerate() {
            let body: serde_json::Value =
                serde_json::from_slice(&request_body(i).unwrap()).unwrap();
            assert_eq!(body["variables"]["chan"], *id);
            assert!(body["query"].as_str().unwrap().contains(field));
        }
        assert!(request_body(3).is_err());
    }

    #[test]
    fn nullable_fields_and_unicode_are_supported() {
        let data = serde_json::json!({"data":{"channel":{"playingnow":{"current":{"metadata":{"title":"Hilarité", "artist":null}}}}}});
        let track = parse_track(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(track.title.as_deref(), Some("Hilarité"));
        assert!(track.artist.is_none());
    }

    #[test]
    fn upstream_errors_and_oversized_bodies_are_rejected() {
        assert!(parse_track(br#"{"errors":[{"message":"unavailable"}],"data":null}"#).is_err());
        assert!(parse_track(b"not JSON").is_err());
        assert!(parse_track(&vec![b' '; MAX_METADATA_BYTES + 1]).is_err());
    }
}
