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

/// basic-pitch: the notes played, from audio (`crate::notes`).
pub static NOTES: Model = Model {
    id: "notes",
    name: "basic-pitch (Spotify)",
    does: "the notes an instrument or a voice played: pitch, start, end and velocity",
    licence: "Apache-2.0 (code and model), checked 2026-11-11",
    source: "https://github.com/spotify/basic-pitch",
    files: &[ModelFile {
        name: "nmp.onnx",
        // The commit that added the ONNX export; the file is in Git, not LFS.
        url: "https://raw.githubusercontent.com/spotify/basic-pitch/dfb20ef559dff1792e11e022f3f0c7008c1dee6d/basic_pitch/saved_models/icassp_2022/nmp.onnx",
        sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
        bytes: 230_444,
    }],
};

/// Every model that can be installed.
pub static MODELS: &[&Model] = &[&NOTES, &TAGS];

/// Xiaomi's CED-tiny, its official ONNX export (int8 weights, its own mel
/// front end inside the graph) and the AudioSet label names from the same
/// repository's `config.json`.
pub static TAGS: Model = Model {
    id: "tags",
    name: "CED-tiny",
    does: "what a sound or a recording contains, from AudioSet's 527 labels",
    licence: "weights Apache-2.0 (model card, checked 2026-11-11); the CED training code \
              is GPL-3.0 and none of it is used; label names from AudioSet's ontology \
              (Google, CC BY-SA 4.0)",
    source: "https://huggingface.co/mispeech/ced-tiny",
    files: &[
        ModelFile {
            name: "model.onnx",
            url: "https://huggingface.co/mispeech/ced-tiny/resolve/ace276d29dd0bb3f3517b0fa8cf300738c409019/model.onnx",
            sha256: "3890f227660be71886ff48633d11a731993e0206db8a486781412c28a0873435",
            bytes: 6_681_767,
        },
        ModelFile {
            name: "config.json",
            url: "https://huggingface.co/mispeech/ced-tiny/resolve/ace276d29dd0bb3f3517b0fa8cf300738c409019/config.json",
            sha256: "3a060c57c28b8138cf66bbab2c0dcab79f07c871b600f52a06f2652bf3cad2bd",
            bytes: 25_058,
        },
    ],
};

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
