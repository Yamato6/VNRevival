//! Access to the game's packs, with decoded pictures cached.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use overflow_adv2::script::Script;
use overflow_adv2::{Pack, PixelFormat, audio, graphic};
use vnr_core::{BackendError, Clip, Color, Sprite};

type PackFile = Pack<BufReader<File>>;

fn open_pack(root: &Path, name: &str) -> Result<Option<PackFile>, BackendError> {
    let Some(path) = vnr_core::resolve_ci(root, &format!("PACK/{name}")) else {
        return Ok(None);
    };
    Ok(Some(
        Pack::open(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    ))
}

/// The four packs of an install.
pub struct Assets {
    script: PackFile,
    graphic: PackFile,
    sound: Option<PackFile>,
    voice: Option<PackFile>,
    pictures: HashMap<String, Option<Arc<Sprite>>>,
}

impl Assets {
    pub fn open(root: &Path) -> Result<Self, BackendError> {
        let required = |name: &str| -> Result<PackFile, BackendError> {
            open_pack(root, name)?.ok_or_else(|| format!("PACK/{name} is missing").into())
        };
        Ok(Self {
            script: required("SCRIPT.DAT")?,
            graphic: required("GRAPHIC.DAT")?,
            sound: open_pack(root, "SOUND.DAT")?,
            voice: open_pack(root, "VOICE.DAT")?,
            pictures: HashMap::new(),
        })
    }

    /// A script by name, with or without `.obj`.
    pub fn script(&mut self, name: &str) -> Result<Script, BackendError> {
        let file = if name.to_ascii_lowercase().ends_with(".obj") {
            name.to_owned()
        } else {
            format!("{name}.obj")
        };
        let entry = self
            .script
            .find(&file)
            .ok_or_else(|| format!("script {file} not found"))?
            .clone();
        let data = self.script.read(&entry)?;
        Ok(Script::parse(&data).map_err(|e| format!("{file}: {e}"))?)
    }

    /// The first picture of `stem` with one of `extensions`, or `None`
    /// if there is no such file.
    pub fn picture(&mut self, stem: &str, extensions: &[&str]) -> Option<Arc<Sprite>> {
        let key = format!("{}|{}", stem.to_ascii_lowercase(), extensions.join(","));
        if let Some(cached) = self.pictures.get(&key) {
            return cached.clone();
        }
        let sprite = extensions.iter().find_map(|ext| {
            let entry = self.graphic.find(&format!("{stem}.{ext}"))?.clone();
            let data = self.graphic.read(&entry).ok()?;
            let picture = graphic::decode(&entry.name, &data)
                .ok()?
                .into_iter()
                .next()?;
            let image = picture.image;
            Some(Arc::new(match image.format {
                PixelFormat::Rgb8 => Sprite::from_rgb(image.width, image.height, &image.pixels),
                PixelFormat::Rgba8 => Sprite::from_rgba(image.width, image.height, image.pixels),
                PixelFormat::Gray8 => {
                    Sprite::from_mask(image.width, image.height, &image.pixels, Color::WHITE)
                }
            }))
        });
        self.pictures.insert(key, sprite.clone());
        sprite
    }

    /// A sound effect or music track from `SOUND.DAT`.
    pub fn sound(&mut self, name: &str) -> Option<Arc<Clip>> {
        load_clip(self.sound.as_mut()?, name)
    }

    /// A voice from `VOICE.DAT`.
    pub fn voice(&mut self, name: &str) -> Option<Arc<Clip>> {
        load_clip(self.voice.as_mut()?, name)
    }
}

/// Decodes `name.adp4` or `name.adps` (music loops after its intro).
fn load_clip(pack: &mut PackFile, name: &str) -> Option<Arc<Clip>> {
    if name.is_empty() {
        return None;
    }
    let entry = pack
        .find(&format!("{name}.adp4"))
        .or_else(|| pack.find(&format!("{name}.adps")))?
        .clone();
    let data = pack.read(&entry).ok()?;
    let clip = if entry.name.to_ascii_lowercase().ends_with(".adps") {
        let music = audio::decode_adps(&data).ok()?;
        let start = music.intro.len() / 2;
        let mut samples = music.intro;
        samples.extend_from_slice(&music.looped);
        Clip::stereo(samples).looping_from(start)
    } else {
        Clip::mono(&audio::decode_adp4(&data).ok()?)
    };
    Some(Arc::new(clip))
}
