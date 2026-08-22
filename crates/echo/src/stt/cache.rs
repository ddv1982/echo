use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelCache {
    dir: PathBuf,
}

impl ModelCache {
    #[must_use]
    pub fn from_env() -> Self {
        let dir = env::var_os("ECHO_MODEL_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("XDG_CACHE_HOME").map(|cache| PathBuf::from(cache).join("echo"))
            })
            .or_else(|| {
                env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("echo"))
            })
            .unwrap_or_else(|| PathBuf::from("/tmp/echo-models"));
        Self { dir }
    }

    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    #[must_use]
    pub fn vad_model(&self) -> Option<PathBuf> {
        ["ggml-silero-v6.2.0.bin", "ggml-silero-v5.1.2.bin"]
            .into_iter()
            .map(|name| self.path(name))
            .find(|path| path.is_file())
    }

    /// The Parakeet model root: the nested directory preferred, the cache
    /// directory itself when the files sit directly in it. `None` unless all
    /// four files sherpa-onnx needs are present, so `available()` can never
    /// promise a model `transcribe` then reports missing.
    #[must_use]
    pub fn parakeet_root(&self) -> Option<PathBuf> {
        let nested = self.path("parakeet-tdt-0.6b-v3");
        if parakeet_model_present(&nested) {
            return Some(nested);
        }
        if parakeet_model_present(self.dir()) {
            return Some(self.dir().to_path_buf());
        }
        None
    }

    /// One scan of the model directory answering "what is installed" for the
    /// whole app: Whisper GGML files, Silero VAD weights, Parakeet ONNX sets.
    #[must_use]
    pub fn inventory(&self) -> ModelInventory {
        let mut inventory = ModelInventory::default();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(_) => return inventory,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if inventory.parakeet.is_none() && parakeet_model_present(&path) {
                    inventory.parakeet = Some(path);
                }
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if is_silero_vad(&file_name) {
                inventory.vad.push(path);
                continue;
            }
            if let Some((name, family, multilingual, quantisation)) =
                parse_whisper_filename(&file_name)
            {
                let size_bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                inventory.whisper.push(InstalledModel {
                    name,
                    path,
                    family,
                    multilingual,
                    quantisation,
                    size_bytes,
                });
            }
        }
        inventory.whisper.sort_by(|a, b| a.name.cmp(&b.name));
        inventory.vad.sort();
        inventory.vad.reverse();
        inventory
    }
}

fn is_silero_vad(file_name: &str) -> bool {
    file_name.starts_with("ggml-silero-v") && file_name.ends_with(".bin")
}

fn parakeet_model_present(dir: &Path) -> bool {
    dir.join("tokens.txt").is_file()
        && first_existing(dir, &["encoder.int8.onnx", "encoder.onnx"]).is_some()
        && first_existing(dir, &["decoder.int8.onnx", "decoder.onnx"]).is_some()
        && first_existing(dir, &["joiner.int8.onnx", "joiner.onnx"]).is_some()
}

fn first_existing(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperFamily {
    Tiny,
    Base,
    Small,
    Medium,
    LargeV1,
    LargeV2,
    LargeV3Turbo,
    LargeV3,
}

impl WhisperFamily {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "tiny" => Some(Self::Tiny),
            "base" => Some(Self::Base),
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            // upstream published plain "large" only for the v1 checkpoint
            "large" | "large-v1" => Some(Self::LargeV1),
            "large-v2" => Some(Self::LargeV2),
            "large-v3-turbo" => Some(Self::LargeV3Turbo),
            "large-v3" => Some(Self::LargeV3),
            _ => None,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::LargeV1 => "large-v1",
            Self::LargeV2 => "large-v2",
            Self::LargeV3Turbo => "large-v3-turbo",
            Self::LargeV3 => "large-v3",
        }
    }

    /// Quality ordering used by `ModelInventory::best_whisper`. Measured WER
    /// places large-v3-turbo (2.01%) between large-v2 (2.65%) and large-v3
    /// (1.82%), per https://github.com/handy-computer/transcribe.cpp/blob/main/docs/models/whisper.md
    fn rank(self) -> u8 {
        match self {
            Self::Tiny => 0,
            Self::Base => 1,
            Self::Small => 2,
            Self::Medium => 3,
            Self::LargeV1 | Self::LargeV2 => 4,
            Self::LargeV3Turbo => 5,
            Self::LargeV3 => 6,
        }
    }
}

/// Less quantization is higher quality, for breaking ties inside one family.
fn quantisation_rank(quantisation: Option<&str>) -> u8 {
    match quantisation {
        Some("q5_0") => 0,
        Some("q5_1") => 1,
        Some("q8_0") => 2,
        None => 3,
        _ => 0,
    }
}

/// One Whisper GGML file on disk. `multilingual` is the filename-derived
/// pre-flight value used to populate pickers and refuse impossible language
/// choices; the authoritative value is `model.multilingual` in the engine's
/// JSON, available only after a run.
#[derive(Debug, Clone)]
pub struct InstalledModel {
    /// Filename stem without the `ggml-` prefix, e.g. `small.en-q5_1`. This is
    /// the name `Config.whisper_model` and `ECHO_WHISPER_MODEL` match against.
    pub name: String,
    pub path: PathBuf,
    pub family: WhisperFamily,
    pub multilingual: bool,
    pub quantisation: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelInventory {
    pub whisper: Vec<InstalledModel>,
    pub vad: Vec<PathBuf>,
    pub parakeet: Option<PathBuf>,
}

impl ModelInventory {
    /// The model Echo runs when nothing is configured: prefer multilingual
    /// over `.en`, then the higher family rung, then less quantization.
    #[must_use]
    pub fn best_whisper(&self) -> Option<&InstalledModel> {
        self.whisper.iter().max_by_key(|model| {
            (
                model.multilingual,
                model.family.rank(),
                quantisation_rank(model.quantisation.as_deref()),
            )
        })
    }
}

/// The GGML naming convention is a table, not a rule: `tiny`, `base`, `small`
/// quantize as q5_1, `medium` and the larges as q5_0, q8_0 exists for all,
/// there is no `.en` turbo, and `-tdrz` tinydiarize builds are ignored because
/// Echo has no diarization path. Unrecognized filenames are ignored, not
/// guessed at.
pub(crate) fn parse_whisper_filename(
    file_name: &str,
) -> Option<(String, WhisperFamily, bool, Option<String>)> {
    let stem = file_name
        .strip_suffix(".bin")
        .or_else(|| file_name.strip_suffix(".gguf"))?;
    let stem = stem.strip_prefix("ggml-").unwrap_or(stem);
    if stem.ends_with("-tdrz") {
        return None;
    }
    let (stem, quantisation) = ["q5_0", "q5_1", "q8_0"]
        .iter()
        .find_map(|quant| {
            stem.strip_suffix(&format!("-{quant}"))
                .map(|rest| (rest, (*quant).to_string()))
        })
        .map_or((stem, None), |(rest, quant)| (rest, Some(quant)));
    let (stem, english_only) = stem.strip_suffix(".en").map_or((stem, false), |rest| (rest, true));
    let family = WhisperFamily::from_name(stem)?;
    Some((stem_name(file_name), family, !english_only, quantisation))
}

fn stem_name(file_name: &str) -> String {
    let stem = file_name
        .strip_suffix(".bin")
        .or_else(|| file_name.strip_suffix(".gguf"))
        .unwrap_or(file_name);
    stem.strip_prefix("ggml-").unwrap_or(stem).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "echo-cache-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn inventory_of(label: &str, files: &[&str]) -> ModelInventory {
        let dir = scratch_dir(label);
        for file in files {
            fs::write(dir.join(file), []).unwrap();
        }
        let inventory = ModelCache::at(&dir).inventory();
        let _ = fs::remove_dir_all(&dir);
        inventory
    }

    #[test]
    fn parses_every_catalog_shape() {
        let cases: &[(&str, WhisperFamily, bool, Option<&str>)] = &[
            ("ggml-tiny.bin", WhisperFamily::Tiny, true, None),
            ("ggml-tiny.en.bin", WhisperFamily::Tiny, false, None),
            ("ggml-base.en.bin", WhisperFamily::Base, false, None),
            ("ggml-base.bin", WhisperFamily::Base, true, None),
            ("ggml-small.en-q5_1.bin", WhisperFamily::Small, false, Some("q5_1")),
            ("ggml-small-q5_1.bin", WhisperFamily::Small, true, Some("q5_1")),
            ("ggml-medium-q5_0.bin", WhisperFamily::Medium, true, Some("q5_0")),
            ("ggml-medium.en-q5_0.bin", WhisperFamily::Medium, false, Some("q5_0")),
            ("ggml-large.bin", WhisperFamily::LargeV1, true, None),
            ("ggml-large-v1.bin", WhisperFamily::LargeV1, true, None),
            ("ggml-large-v2.bin", WhisperFamily::LargeV2, true, None),
            ("ggml-large-v3.bin", WhisperFamily::LargeV3, true, None),
            (
                "ggml-large-v3-turbo-q5_0.bin",
                WhisperFamily::LargeV3Turbo,
                true,
                Some("q5_0"),
            ),
            ("ggml-base-q8_0.bin", WhisperFamily::Base, true, Some("q8_0")),
            (
                "ggml-large-v3-turbo-q8_0.bin",
                WhisperFamily::LargeV3Turbo,
                true,
                Some("q8_0"),
            ),
            ("base.en.bin", WhisperFamily::Base, false, None),
            ("ggml-base.en.gguf", WhisperFamily::Base, false, None),
        ];
        for (file, family, multilingual, quant) in cases {
            let inventory = inventory_of("shapes", &[file]);
            assert_eq!(inventory.whisper.len(), 1, "{file}");
            let model = &inventory.whisper[0];
            assert_eq!(model.family, *family, "{file}");
            assert_eq!(model.multilingual, *multilingual, "{file}");
            assert_eq!(model.quantisation.as_deref(), *quant, "{file}");
            let expected_name = file
                .trim_start_matches("ggml-")
                .trim_end_matches(".bin")
                .trim_end_matches(".gguf");
            assert_eq!(model.name, expected_name, "{file}");
        }
    }

    #[test]
    fn ignores_tdrz_and_unrecognized_files() {
        let inventory = inventory_of(
            "ignored",
            &[
                "ggml-small.en-tdrz.bin",
                "random.bin",
                "ggml-.bin",
                "notes.txt",
                "ggml-silero-v6.2.0.bin",
            ],
        );
        assert!(inventory.whisper.is_empty());
        assert_eq!(inventory.vad.len(), 1);
    }

    #[test]
    fn empty_directory_is_an_empty_inventory() {
        let dir = scratch_dir("empty");
        let inventory = ModelCache::at(&dir).inventory();
        assert!(inventory.whisper.is_empty());
        assert!(inventory.vad.is_empty());
        assert_eq!(inventory.parakeet, None);
        let missing = ModelCache::at(dir.join("not-there")).inventory();
        assert!(missing.whisper.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn best_whisper_prefers_multilingual_then_family_then_precision() {
        let inventory = inventory_of("best", &["ggml-base.en.bin", "ggml-small.bin"]);
        assert_eq!(inventory.best_whisper().map(|m| m.name.as_str()), Some("small"));

        let inventory = inventory_of("best-quant", &["ggml-small-q5_1.bin", "ggml-small.bin"]);
        assert_eq!(inventory.best_whisper().map(|m| m.name.as_str()), Some("small"));

        let inventory = inventory_of("best-en", &["ggml-tiny.en.bin", "ggml-base.en.bin"]);
        assert_eq!(
            inventory.best_whisper().map(|m| m.name.as_str()),
            Some("base.en")
        );

        let inventory = inventory_of(
            "best-turbo",
            &["ggml-large-v2.bin", "ggml-large-v3-turbo-q5_0.bin"],
        );
        assert_eq!(
            inventory.best_whisper().map(|m| m.name.as_str()),
            Some("large-v3-turbo-q5_0")
        );

        let inventory = inventory_of(
            "best-large",
            &["ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3.bin"],
        );
        assert_eq!(
            inventory.best_whisper().map(|m| m.name.as_str()),
            Some("large-v3")
        );

        let inventory = inventory_of("best-none", &["ggml-silero-v6.2.0.bin"]);
        assert!(inventory.best_whisper().is_none());
    }

    #[test]
    fn parakeet_root_needs_all_four_files() {
        let dir = scratch_dir("parakeet");
        let nested = dir.join("parakeet-tdt-0.6b-v3");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("tokens.txt"), []).unwrap();
        assert_eq!(ModelCache::at(&dir).parakeet_root(), None);
        for file in ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx"] {
            fs::write(nested.join(file), []).unwrap();
        }
        assert_eq!(ModelCache::at(&dir).parakeet_root(), Some(nested));
        let _ = fs::remove_dir_all(&dir);
    }
}
