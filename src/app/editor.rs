//! Однорядковий редактор для полів налаштувань.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Простий однорядковий редактор для полів налаштувань.
pub struct Editor {
    pub chars: Vec<char>,
    pub cursor: usize,
}

impl Editor {
    pub(super) fn new(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub(super) fn key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Char('u') => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn editor_basics() {
        let mut e = Editor::new("33.0");
        e.key(key(KeyCode::Char('.'), KeyModifiers::NONE));
        e.key(key(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(e.text(), "33.0.1");
        e.key(key(KeyCode::Home, KeyModifiers::NONE));
        e.key(key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(e.text(), "3.0.1");
        e.key(key(KeyCode::End, KeyModifiers::NONE));
        e.key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text(), "3.0.");
    }
}
