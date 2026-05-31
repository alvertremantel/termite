use crossterm::event::{KeyCode, KeyEvent};
use jones_state::CoreState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAction {
    Dismissed,
    Selected(usize),
    Updated,
    None,
}

pub fn update_search_results<'a>(
    query: &str,
    items: impl Iterator<Item = (usize, &'a str)>,
) -> Vec<usize> {
    let query = query.to_lowercase();
    items
        .filter(|(_, name)| name.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect()
}

pub fn handle_search_key<C>(state: &mut CoreState<C>, key: KeyEvent) -> SearchAction {
    match key.code {
        KeyCode::Esc => {
            state.searching = false;
            SearchAction::Dismissed
        }
        KeyCode::Enter => {
            let result = state
                .search_results
                .get(state.search_index)
                .copied()
                .map(SearchAction::Selected)
                .unwrap_or(SearchAction::Dismissed);
            state.searching = false;
            result
        }
        KeyCode::Backspace => {
            state.search_query.pop();
            SearchAction::Updated
        }
        KeyCode::Char(c) => {
            state.search_query.push(c);
            SearchAction::Updated
        }
        KeyCode::Up => {
            state.search_index = state.search_index.saturating_sub(1);
            SearchAction::None
        }
        KeyCode::Down => {
            if !state.search_results.is_empty() {
                state.search_index = (state.search_index + 1).min(state.search_results.len() - 1);
            }
            SearchAction::None
        }
        _ => SearchAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn search_state() -> CoreState<()> {
        let mut state = CoreState::new(());
        state.searching = true;
        state
    }

    #[test]
    fn update_search_results_matches_case_insensitively() {
        let results = update_search_results(
            "alp",
            [(0, "Alpha"), (1, "beta"), (2, "alphabet")].into_iter(),
        );

        assert_eq!(results, vec![0, 2]);
    }

    #[test]
    fn handle_search_key_updates_query_and_selection() {
        let mut state = search_state();
        state.search_results = vec![2, 4, 8];

        assert_eq!(
            handle_search_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)
            ),
            SearchAction::Updated
        );
        assert_eq!(state.search_query, "a");

        assert_eq!(
            handle_search_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            SearchAction::None
        );
        assert_eq!(state.search_index, 1);

        assert_eq!(
            handle_search_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            SearchAction::None
        );
        assert_eq!(state.search_index, 0);
    }

    #[test]
    fn handle_search_key_handles_backspace_enter_and_escape() {
        let mut state = search_state();
        state.search_query = "abc".into();
        state.search_results = vec![11, 22];
        state.search_index = 1;

        assert_eq!(
            handle_search_key(
                &mut state,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
            ),
            SearchAction::Updated
        );
        assert_eq!(state.search_query, "ab");

        assert_eq!(
            handle_search_key(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            SearchAction::Selected(22)
        );
        assert!(!state.searching);

        state.searching = true;
        assert_eq!(
            handle_search_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            SearchAction::Dismissed
        );
        assert!(!state.searching);
    }

    #[test]
    fn handle_search_key_dismisses_empty_enter_and_keeps_down_bounded() {
        let mut state = search_state();

        assert_eq!(
            handle_search_key(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            SearchAction::Dismissed
        );
        assert!(!state.searching);

        state.searching = true;
        state.search_results = vec![3];
        assert_eq!(
            handle_search_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            SearchAction::None
        );
        assert_eq!(state.search_index, 0);
    }
}
