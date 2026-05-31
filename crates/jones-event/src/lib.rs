use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
};
use futures_lite::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent<E = ()> {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
    Custom(E),
}

pub struct EventHandler<E = ()> {
    rx: mpsc::UnboundedReceiver<AppEvent<E>>,
}

impl<E: Send + 'static> EventHandler<E> {
    pub fn new(tick_rate: Duration) -> (Self, mpsc::UnboundedSender<AppEvent<E>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_term = tx.clone();

        tokio::spawn(async move {
            let mut stream = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_rate);

            loop {
                tokio::select! {
                    maybe_event = stream.next() => {
                        match maybe_event {
                            Some(Ok(Event::Key(key)))
                                if key.kind == KeyEventKind::Press
                                    && tx_term.send(AppEvent::Key(key)).is_err() =>
                            {
                                return;
                            }
                            Some(Ok(Event::Mouse(mouse)))
                                if tx_term.send(AppEvent::Mouse(mouse)).is_err() =>
                            {
                                return;
                            }
                            Some(Ok(Event::Resize(w, h)))
                                if tx_term.send(AppEvent::Resize(w, h)).is_err() =>
                            {
                                return;
                            }
                            Some(Err(_)) | None => return,
                            _ => {}
                        }
                    }
                    _ = tick_interval.tick() => {
                        if tx_term.send(AppEvent::Tick).is_err() { return; }
                    }
                }
            }
        });

        (Self { rx }, tx)
    }

    pub async fn next(&mut self) -> Option<AppEvent<E>> {
        self.rx.recv().await
    }
}

pub fn is_quit(key: &KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } | KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

pub fn is_nav_up(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Up | KeyCode::Char('k'))
}

pub fn is_nav_down(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Down | KeyCode::Char('j'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_keys_are_detected() {
        assert!(is_quit(&KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE
        )));
        assert!(is_quit(&KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(!is_quit(&KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn navigation_keys_support_arrows_and_vim_bindings() {
        assert!(is_nav_up(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert!(is_nav_up(&KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE
        )));
        assert!(is_nav_down(&KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE
        )));
        assert!(is_nav_down(&KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE
        )));
    }
}
