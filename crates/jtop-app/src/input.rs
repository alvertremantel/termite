use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    Quit,
    ToggleHelp,
    Refresh,
    CyclePowerAction,
    PreparePowertopReport,
    ConfirmOrExecute,
    CancelConfirmation,
    None,
}

pub fn map_key_event(key: KeyEvent) -> AppEvent {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => AppEvent::Quit,
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => AppEvent::Quit,
        (KeyCode::Char('?'), _) => AppEvent::ToggleHelp,
        (KeyCode::Char('r'), _) => AppEvent::Refresh,
        (KeyCode::Char('p'), _) => AppEvent::CyclePowerAction,
        (KeyCode::Char('t'), _) => AppEvent::PreparePowertopReport,
        (KeyCode::Char('!'), _) | (KeyCode::Enter, _) => AppEvent::ConfirmOrExecute,
        (KeyCode::Char('n'), _) => AppEvent::CancelConfirmation,
        _ => AppEvent::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_power_and_refresh_keys() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            AppEvent::CyclePowerAction
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            AppEvent::Refresh
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppEvent::ConfirmOrExecute
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            AppEvent::CancelConfirmation
        );
    }
}
