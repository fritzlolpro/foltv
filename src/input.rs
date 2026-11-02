use crate::engine::Command;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;

pub fn key_to_command(key: KeyEvent) -> Option<Command> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Command::Quit),
        (KeyCode::Char('n'), KeyModifiers::NONE) => Some(Command::Next),
        (KeyCode::Right | KeyCode::Up, KeyModifiers::NONE) => Some(Command::ChannelUp),
        (KeyCode::Left | KeyCode::Down, KeyModifiers::NONE) => Some(Command::ChannelDown),
        (KeyCode::Char(' ') | KeyCode::Char('p'), KeyModifiers::NONE) => Some(Command::PauseToggle),
        (KeyCode::Char('+') | KeyCode::Char('='), KeyModifiers::NONE) => Some(Command::VolumeUp),
        (KeyCode::Char('-'), KeyModifiers::NONE) => Some(Command::VolumeDown),
        (KeyCode::Char('m'), KeyModifiers::NONE) => Some(Command::MuteToggle),
        (KeyCode::Char('q') | KeyCode::Esc, KeyModifiers::NONE) => Some(Command::Quit),
        _ => None,
    }
}

pub fn read_commands(tx: std::sync::mpsc::Sender<Command>) -> Result<()> {
    terminal::enable_raw_mode()?;
    let result = read_loop(&tx);
    let _ = terminal::disable_raw_mode();
    result
}

fn read_loop(tx: &std::sync::mpsc::Sender<Command>) -> Result<()> {
    loop {
        let Event::Key(key) = event::read()? else { continue };
        let Some(command) = key_to_command(key) else { continue };
        let quit = matches!(command, Command::Quit);
        tx.send(command)?;
        if quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    #[test]
    fn test_key_mapping() {
        assert_eq!(key_to_command(ch('n')), Some(Command::Next));
        assert_eq!(key_to_command(key(KeyCode::Right)), Some(Command::ChannelUp));
        assert_eq!(key_to_command(key(KeyCode::Up)), Some(Command::ChannelUp));
        assert_eq!(key_to_command(key(KeyCode::Left)), Some(Command::ChannelDown));
        assert_eq!(key_to_command(key(KeyCode::Down)), Some(Command::ChannelDown));
        assert_eq!(key_to_command(ch(' ')), Some(Command::PauseToggle));
        assert_eq!(key_to_command(ch('p')), Some(Command::PauseToggle));
        assert_eq!(key_to_command(ch('+')), Some(Command::VolumeUp));
        assert_eq!(key_to_command(ch('=')), Some(Command::VolumeUp));
        assert_eq!(key_to_command(ch('-')), Some(Command::VolumeDown));
        assert_eq!(key_to_command(ch('m')), Some(Command::MuteToggle));
        assert_eq!(key_to_command(ch('q')), Some(Command::Quit));
        assert_eq!(key_to_command(key(KeyCode::Esc)), Some(Command::Quit));
        assert_eq!(
            key_to_command(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
        assert_eq!(key_to_command(ch('x')), None);
        assert_eq!(key_to_command(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::SHIFT)), None);
        assert_eq!(key_to_command(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)), None);
    }
}
