//! The player's names and the `$` placeholders that show them.
//!
//! At a new game (`sys 17`) the original asks for the hero's surname and
//! given name, each with its reading, and a chat handle (dialog
//! `IDD_NAMEIN`; defaults at `0x443d90`). Texts refer to them, and to the
//! handles of the other chat users, as `$<letter><n>`: `n = 0` is the
//! whole name, `n = k` its k-th character (`0x41b200`).
//!
//! | Letter | Name | Default |
//! |---|---|---|
//! | `M` / `m` | surname / its reading | 緒方 / おがた |
//! | `N` / `n` | given name / its reading | 圭 / ケイ |
//! | `H`, `h` | chat handle | Ａ，Ｗ |
//! | `L`, `l` | EVE's cousin | 歩 |
//! | `A`–`E`, `a`–`e` | other chat users | 稲本店長, ￥２, ツカサ, 和泉, もっちー |
//!
//! Voices: a line can have two recordings, the second one saying the
//! hero's default name. The original plays it only while the readings
//! and the handle are the defaults (flag `0xf866ea`).

use serde::{Deserialize, Serialize};

/// Handles of the other chat users, in the engine's slot order
/// (`0x443da8`); placeholders `$A`–`$E` are slots 3–6 and 8.
pub const CHAT_USERS: [&str; 12] = [
    "ありす",
    "ＮＥＭ",
    "キツネ",
    "稲本店長",
    "￥２",
    "ツカサ",
    "和泉",
    "ＧＯＮ",
    "もっちー",
    "ぱとら",
    "風",
    "めたる",
];

/// What a chat user is called when the player takes their handle.
const DEFAULT_HANDLE: &str = "Ａ，Ｗ";

/// The hero's names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Names {
    pub surname: String,
    pub surname_reading: String,
    pub given: String,
    pub given_reading: String,
    pub handle: String,
}

impl Default for Names {
    fn default() -> Self {
        Self {
            surname: "緒方".into(),
            surname_reading: "おがた".into(),
            given: "圭".into(),
            given_reading: "ケイ".into(),
            handle: DEFAULT_HANDLE.into(),
        }
    }
}

impl Names {
    /// Whether voices may say the hero's name: the readings and the
    /// handle are the defaults (what `sys 17` compares).
    pub fn voiced(&self) -> bool {
        let d = Names::default();
        self.surname_reading == d.surname_reading
            && self.given_reading == d.given_reading
            && self.handle == d.handle
    }

    /// The handle of chat user `slot`; a user whose handle the player
    /// took is called by the default handle instead.
    pub fn chat_user(&self, slot: usize) -> &str {
        match CHAT_USERS.get(slot) {
            Some(&user) if user == self.handle => DEFAULT_HANDLE,
            Some(&user) => user,
            None => "",
        }
    }

    fn field(&self, letter: char) -> Option<&str> {
        Some(match letter {
            'M' => &self.surname,
            'm' => &self.surname_reading,
            'N' => &self.given,
            'n' => &self.given_reading,
            'H' | 'h' => &self.handle,
            'L' | 'l' => "歩",
            'A' | 'a' => self.chat_user(3),
            'B' | 'b' => self.chat_user(4),
            'C' | 'c' => self.chat_user(5),
            'D' | 'd' => self.chat_user(6),
            'E' | 'e' => self.chat_user(8),
            _ => return None,
        })
    }

    /// Replaces the placeholders in `text` and turns the escaped line
    /// breaks (`\n`) into real ones.
    pub fn expand(&self, text: &str) -> String {
        let text = text.replace("\\n", "\n");
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let tail = &rest[at + 1..];
            let mut chars = tail.chars();
            let letter = chars.next();
            let digits: String = chars.take_while(char::is_ascii_digit).collect();
            match (letter.and_then(|l| self.field(l)), digits.parse::<usize>()) {
                (Some(field), Ok(n)) => {
                    if n == 0 {
                        out.push_str(field);
                    } else if let Some(c) = field.chars().nth(n - 1) {
                        out.push(c);
                    }
                    rest = &tail[letter.map_or(0, char::len_utf8) + digits.len()..];
                }
                _ => {
                    out.push('$');
                    rest = tail;
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Which of a line's two recordings to play.
    pub fn pick_voice(&self, plain: Option<String>, named: Option<String>) -> Option<String> {
        let named = named.filter(|v| !v.is_empty());
        let plain = plain.filter(|v| !v.is_empty());
        if self.voiced() {
            named.or(plain)
        } else {
            plain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_names_and_characters() {
        let names = Names::default();
        assert_eq!(names.expand("【$N0】"), "【圭】");
        assert_eq!(names.expand("$M0$N0、$H0"), "緒方圭、Ａ，Ｗ");
        assert_eq!(names.expand("$M2"), "方");
        assert_eq!(names.expand("$M9"), "");
        assert_eq!(names.expand("$L0さん"), "歩さん");
        assert_eq!(names.expand("$A0：$C0"), "稲本店長：ツカサ");
        assert_eq!(names.expand("100$ and $Z0"), "100$ and $Z0");
        assert_eq!(names.expand("a\\nb"), "a\nb");
    }

    #[test]
    fn taking_a_users_handle_renames_them() {
        let names = Names {
            handle: "ツカサ".into(),
            ..Names::default()
        };
        assert_eq!(names.expand("$C0"), DEFAULT_HANDLE);
        assert!(!names.voiced());
    }

    #[test]
    fn named_voices_need_the_default_names() {
        let v = |s: &str| Some(s.to_string());
        let names = Names::default();
        assert_eq!(names.pick_voice(v("A"), v("B")), v("B"));
        assert_eq!(names.pick_voice(v("A"), v("")), v("A"));
        let custom = Names {
            given_reading: "ユウ".into(),
            ..Names::default()
        };
        assert_eq!(custom.pick_voice(v("A"), v("B")), v("A"));
        // The kanji alone does not change what is said.
        let kanji = Names {
            given: "優".into(),
            ..Names::default()
        };
        assert!(kanji.voiced());
    }
}
