use std::mem::take;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zeroize::Zeroizing;

const ROOM_FOR_TYPING: usize = 512;

#[must_use]
pub fn empty() -> Zeroizing<String> {
    Zeroizing::new(String::with_capacity(ROOM_FOR_TYPING))
}

#[must_use]
pub fn masked(typed: &str) -> String {
    "*".repeat(typed.chars().count())
}

pub fn type_key(typed: &mut Zeroizing<String>, event: &Event) -> Option<Zeroizing<String>> {
    let Event::Key(KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        ..
    }) = event
    else {
        return None;
    };
    match code {
        KeyCode::Enter => return Some(Zeroizing::new(take(&mut **typed))),
        KeyCode::Backspace => {
            typed.pop();
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => typed.clear(),
        KeyCode::Char(_) if modifiers.contains(KeyModifiers::CONTROL) => {}
        KeyCode::Char(character) if typed.len() + character.len_utf8() <= ROOM_FOR_TYPING => {
            typed.push(*character);
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::from(code))
    }

    fn typed(text: &str) -> Zeroizing<String> {
        let mut typed = empty();
        text.chars()
            .map(KeyCode::Char)
            .for_each(|code| assert_eq!(type_key(&mut typed, &press(code)), None));
        typed
    }

    #[test]
    fn hides_what_is_typed() {
        let mut typed = typed("secret");
        type_key(&mut typed, &press(KeyCode::Backspace));
        assert_eq!(masked(&typed), "*****");
    }

    #[test]
    fn control_u_clears() {
        let mut typed = typed("secret");
        let control_u = Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        type_key(&mut typed, &control_u);
        assert_eq!(typed.as_str(), "");
    }

    #[test]
    fn enter_hands_over_what_was_typed() {
        let mut typed = typed("secret");
        let submitted = type_key(&mut typed, &press(KeyCode::Enter));
        assert_eq!(submitted.as_deref().map(String::as_str), Some("secret"));
        assert_eq!(typed.as_str(), "");
    }
}
