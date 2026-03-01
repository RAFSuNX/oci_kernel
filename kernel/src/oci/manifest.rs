extern crate alloc;
use alloc::{string::String, vec::Vec};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct LayerDescriptor {
    pub media_type: String,
    pub digest:     String,
    pub size:       u64,
}

#[derive(Debug)]
pub struct ImageManifest {
    pub schema_version: u8,
    pub config:         LayerDescriptor,
    pub layers:         Vec<LayerDescriptor>,
}

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    MissingField,
}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self { ParseError::Json(e) }
}

impl ImageManifest {
    pub fn from_json(json: &str) -> Result<Self, ParseError> {
        let v: Value = serde_json::from_str(json)
            .map_err(ParseError::Json)?;

        let config_val = &v["config"];
        let config = LayerDescriptor {
            media_type: config_val["mediaType"].as_str().unwrap_or("").into(),
            digest:     config_val["digest"].as_str().unwrap_or("").into(),
            size:       config_val["size"].as_u64().unwrap_or(0),
        };

        let layers = v["layers"]
            .as_array()
            .ok_or(ParseError::MissingField)?
            .iter()
            .map(|l| LayerDescriptor {
                media_type: l["mediaType"].as_str().unwrap_or("").into(),
                digest:     l["digest"].as_str().unwrap_or("").into(),
                size:       l["size"].as_u64().unwrap_or(0),
            })
            .collect();

        Ok(ImageManifest {
            schema_version: v["schemaVersion"].as_u64().unwrap_or(2) as u8,
            config,
            layers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_JSON: &str = r#"{
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "digest": "sha256:abc123",
            "size": 1234
        },
        "layers": [
            {
                "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                "digest": "sha256:layer1",
                "size": 45000000
            }
        ]
    }"#;

    #[test]
    fn parse_manifest() {
        let m = ImageManifest::from_json(MANIFEST_JSON).unwrap();
        assert_eq!(m.layers.len(), 1);
        assert_eq!(m.layers[0].digest, "sha256:layer1");
        assert_eq!(m.layers[0].size, 45000000);
    }

    #[test]
    fn manifest_layer_media_type() {
        let m = ImageManifest::from_json(MANIFEST_JSON).unwrap();
        assert!(m.layers[0].media_type.contains("gzip") ||
                m.layers[0].media_type.contains("zstd"));
    }

    #[test]
    fn manifest_config_parsed() {
        let m = ImageManifest::from_json(MANIFEST_JSON).unwrap();
        assert_eq!(m.config.digest, "sha256:abc123");
        assert_eq!(m.schema_version, 2);
    }

    #[test]
    fn manifest_missing_layers_errors() {
        let json = r#"{"schemaVersion": 2, "config": {"digest": "sha256:x", "size": 0}}"#;
        assert!(ImageManifest::from_json(json).is_err());
    }
}
