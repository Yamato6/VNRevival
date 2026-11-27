//! Texts of the interface in English, Spanish and Japanese.
//!
//! Each language can be changed or completed with a JSON file of
//! `{"key": "text"}` pairs in `<user data>/lang/<code>.json` (`en`, `es`,
//! `ja`); missing keys fall back to the built-in text, then to English.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Language of the interface (not of the games).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    Spanish,
    Japanese,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::Spanish, Language::Japanese];

    /// ISO 639-1 code, used for the override files.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
            Language::Japanese => "ja",
        }
    }

    /// The language's own name.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
            Language::Japanese => "日本語",
        }
    }
}

/// `(key, English, Spanish, Japanese)`.
const TEXTS: &[(&str, &str, &str, &str)] = &[
    ("menu.start", "New game", "Nueva partida", "はじめから"),
    ("menu.load", "Load", "Cargar", "つづきから"),
    ("menu.extras", "Extras", "Extras", "おまけ"),
    ("menu.settings", "Settings", "Ajustes", "設定"),
    (
        "menu.library",
        "Change game",
        "Cambiar juego",
        "ゲームを変更",
    ),
    ("menu.quit", "Quit", "Salir", "終了"),
    ("pause.title", "Menu", "Menú", "メニュー"),
    (
        "pause.resume",
        "Back to the game",
        "Volver al juego",
        "ゲームに戻る",
    ),
    ("pause.save", "Save", "Guardar", "セーブ"),
    ("pause.load", "Load", "Cargar", "ロード"),
    ("pause.history", "History", "Historial", "履歴"),
    ("pause.settings", "Settings", "Ajustes", "設定"),
    ("pause.auto", "Auto mode", "Modo auto", "オートモード"),
    ("game.auto", "AUTO", "AUTO", "AUTO"),
    ("pause.menu", "Main menu", "Menú principal", "タイトルへ"),
    ("pause.quit", "Quit", "Salir", "終了"),
    ("slots.save", "Save", "Guardar partida", "セーブ"),
    ("slots.load", "Load", "Cargar partida", "ロード"),
    ("slots.empty", "Empty", "Vacío", "空き"),
    (
        "slots.disabled",
        "Saving is disabled",
        "Las partidas están desactivadas",
        "セーブできません",
    ),
    ("slots.saved", "Saved", "Guardada", "セーブしました"),
    ("history.title", "History", "Historial", "履歴"),
    (
        "history.empty",
        "Nothing yet",
        "Todavía no hay nada",
        "まだありません",
    ),
    (
        "history.hint",
        "Click a line with a voice to hear it again",
        "Haz clic en una línea con voz para oírla de nuevo",
        "ボイスのある行をクリックすると再生します",
    ),
    ("settings.title", "Settings", "Ajustes", "設定"),
    ("settings.tab.system", "System", "Sistema", "システム"),
    ("settings.tab.sound", "Sound", "Sonido", "サウンド"),
    ("settings.tab.screen", "Text", "Texto", "テキスト"),
    ("settings.language", "Language", "Idioma", "言語"),
    ("settings.start", "At start", "Al empezar", "起動時"),
    (
        "settings.start.universal",
        "VNRevival menu",
        "Menú de VNRevival",
        "VNRevival のメニュー",
    ),
    (
        "settings.start.original",
        "Game's title screen",
        "Título del juego",
        "ゲームのタイトル",
    ),
    (
        "settings.fullscreen",
        "Full screen",
        "Pantalla completa",
        "フルスクリーン",
    ),
    (
        "settings.unlock_all",
        "Unlock all extras",
        "Desbloquear todos los extras",
        "おまけをすべて開放",
    ),
    (
        "settings.master",
        "Master volume",
        "Volumen general",
        "マスター音量",
    ),
    ("settings.music", "Music", "Música", "BGM"),
    ("settings.sound", "Sound effects", "Efectos", "効果音"),
    ("settings.voice", "Voices", "Voces", "ボイス"),
    (
        "settings.text_speed",
        "Text speed",
        "Velocidad del texto",
        "文字速度",
    ),
    (
        "settings.window",
        "Window opacity",
        "Opacidad de la ventana",
        "ウィンドウの濃さ",
    ),
    ("settings.tab.reading", "Reading", "Lectura", "読み進め"),
    ("settings.tab.game", "Game", "Juego", "ゲーム"),
    ("options.ask", "New game", "Nueva partida", "はじめから"),
    (
        "options.ask.note",
        "Enter the hero's name. You can change it later in Settings → Game.",
        "Escribe el nombre del protagonista. Puedes cambiarlo en Ajustes → Juego.",
        "主人公の名前を入力してください。あとで「設定 → ゲーム」で変更できます。",
    ),
    ("options.start", "Start", "Empezar", "開始"),
    (
        "options.defaults",
        "Restore defaults",
        "Valores originales",
        "初期値に戻す",
    ),
    (
        "settings.font",
        "Interface font",
        "Fuente de la interfaz",
        "メニューのフォント",
    ),
    (
        "settings.game_font",
        "Game text font",
        "Fuente del juego",
        "本文のフォント",
    ),
    (
        "settings.font.original",
        "Original (MS P Gothic / Mincho)",
        "Original (MS P Gothic / Mincho)",
        "オリジナル（ＭＳ Ｐ書体）",
    ),
    (
        "settings.text_size",
        "Text size",
        "Tamaño del texto",
        "文字サイズ",
    ),
    (
        "settings.line_spacing",
        "Line spacing",
        "Interlineado",
        "行間",
    ),
    (
        "settings.outline",
        "Text edge",
        "Borde del texto",
        "文字の縁",
    ),
    ("settings.outline.off", "None", "Ninguno", "なし"),
    ("settings.outline.shadow", "Shadow", "Sombra", "影"),
    ("settings.outline.outline", "Outline", "Contorno", "縁取り"),
    (
        "settings.high_contrast",
        "High contrast",
        "Alto contraste",
        "ハイコントラスト",
    ),
    (
        "settings.auto_speed",
        "Auto mode speed",
        "Velocidad del modo auto",
        "オート速度",
    ),
    (
        "settings.wait_voice",
        "Auto mode waits for voices",
        "El modo auto espera a la voz",
        "オートはボイスを待つ",
    ),
    (
        "settings.skip_unread",
        "Skip unread text",
        "Saltar texto no leído",
        "未読もスキップ",
    ),
    (
        "settings.preview",
        "「メール、届いてる？」　This is how the game text will look.",
        "「メール、届いてる？」　Así se verá el texto del juego.",
        "「メール、届いてる？」　ゲームの文章はこのように表示されます。",
    ),
    ("library.title", "Library", "Biblioteca", "ライブラリ"),
    ("library.add", "Add game…", "Añadir juego…", "ゲームを追加…"),
    ("library.new", "not played yet", "sin jugar", "未プレイ"),
    (
        "library.empty",
        "No games yet. Add the folder of a game you own with \"Add game…\".",
        "Aún no hay juegos. Añade la carpeta de un juego tuyo con «Añadir juego…».",
        "ゲームがありません。「ゲームを追加…」でお持ちのゲームのフォルダを選んでください。",
    ),
    (
        "picker.title",
        "Choose the folder of a game",
        "Elige la carpeta de un juego",
        "ゲームのフォルダを選択",
    ),
    ("picker.up", "↑ Up", "↑ Subir", "↑ 上へ"),
    (
        "picker.use",
        "Use this folder",
        "Usar esta carpeta",
        "このフォルダを使う",
    ),
    (
        "picker.found",
        "Found: {}",
        "Encontrado: {}",
        "見つかりました：{}",
    ),
    (
        "picker.none",
        "No supported game in this folder",
        "No hay ningún juego compatible en esta carpeta",
        "対応ゲームが見つかりません",
    ),
    (
        "picker.unreadable",
        "This folder cannot be read",
        "No se puede leer esta carpeta",
        "このフォルダは読めません",
    ),
    ("extras.gallery", "CG gallery", "Galería", "CG ギャラリー"),
    ("extras.music", "Music", "Música", "音楽"),
    ("extras.scenes", "Scenes", "Escenas", "シーン回想"),
    (
        "extras.empty",
        "Nothing here",
        "No hay nada aquí",
        "ありません",
    ),
    ("extras.locked", "？", "？", "？"),
    ("common.on", "On", "Sí", "オン"),
    ("common.off", "Off", "No", "オフ"),
    ("common.instant", "Instant", "Instantáneo", "瞬間"),
    ("common.back", "Back", "Volver", "戻る"),
];

/// The interface texts of one language.
#[derive(Debug, Clone)]
pub struct Strings {
    language: Language,
    texts: HashMap<String, String>,
}

impl Strings {
    /// The built-in texts of `language`, changed by
    /// `<dir>/<code>.json` when `dir` is given and the file exists.
    pub fn new(language: Language, dir: Option<&Path>) -> Self {
        let mut texts: HashMap<String, String> = TEXTS
            .iter()
            .map(|&(key, en, es, ja)| {
                let text = match language {
                    Language::English => en,
                    Language::Spanish => es,
                    Language::Japanese => ja,
                };
                (key.to_string(), text.to_string())
            })
            .collect();
        if let Some(dir) = dir {
            let path = dir.join(format!("{}.json", language.code()));
            if let Ok(data) = std::fs::read(&path) {
                match serde_json::from_slice::<HashMap<String, String>>(&data) {
                    Ok(extra) => texts.extend(extra),
                    Err(e) => eprintln!("{}: {e}", path.display()),
                }
            }
        }
        Self { language, texts }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// The text for `key`; the key itself if there is none.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.texts.get(key).map_or(key, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_unique() {
        let mut keys: Vec<_> = TEXTS.iter().map(|t| t.0).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len());
    }

    #[test]
    fn overrides_replace_built_in_texts() {
        let dir = std::env::temp_dir().join(format!("vnr-lang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("es.json"),
            r#"{"menu.quit": "Cerrar", "x.new": "Nuevo"}"#,
        )
        .unwrap();
        let es = Strings::new(Language::Spanish, Some(&dir));
        assert_eq!(es.get("menu.quit"), "Cerrar");
        assert_eq!(es.get("x.new"), "Nuevo");
        assert_eq!(es.get("menu.load"), "Cargar");
        assert_eq!(es.get("missing"), "missing");
        let ja = Strings::new(Language::Japanese, Some(&dir));
        assert_eq!(ja.get("menu.quit"), "終了");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
