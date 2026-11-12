//! What can be installed: each model, its pinned files and its licence.

/// One file of a model, pinned: fetched from `url` and kept only if its
/// SHA-256 is `sha256`.
#[derive(Debug)]
pub struct ModelFile {
    /// The name it is kept under, in the model's directory.
    pub name: &'static str,
    /// Where it comes from: a URL that names a fixed revision.
    pub url: &'static str,
    /// Lowercase hex.
    pub sha256: &'static str,
    pub bytes: u64,
}

/// An installable model.
#[derive(Debug)]
pub struct Model {
    /// What the command line, the app and the tools call it: `stems`,
    /// `notes`, `tags` or `match`. Also its directory's name.
    pub id: &'static str,
    /// The model, by name.
    pub name: &'static str,
    /// What it does for an agent, in a line.
    pub does: &'static str,
    /// The licence of its code and of its weights, and of the conversion
    /// installed when that is not the original's.
    pub licence: &'static str,
    /// Where the original lives.
    pub source: &'static str,
    pub files: &'static [ModelFile],
}

impl Model {
    /// The download, in bytes.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// Every model that can be installed.
pub static MODELS: &[&Model] = &[];

/// The model with this id.
pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().copied().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_is_pinned() {
        let mut ids = std::collections::HashSet::new();
        for model in MODELS {
            assert!(ids.insert(model.id), "{} listed twice", model.id);
            assert!(!model.files.is_empty(), "{} has no files", model.id);
            for file in model.files {
                assert_eq!(file.sha256.len(), 64, "{}: {}", model.id, file.name);
                assert!(
                    file.sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "{}: {} is not lowercase hex",
                    model.id,
                    file.name
                );
                assert!(file.bytes > 0, "{}: {}", model.id, file.name);
                assert!(
                    file.url.starts_with("https://"),
                    "{}: {}",
                    model.id,
                    file.name
                );
                assert!(
                    !file.name.contains('/') && !file.name.contains(".."),
                    "{}: {} must be a plain file name",
                    model.id,
                    file.name
                );
            }
        }
    }
}
