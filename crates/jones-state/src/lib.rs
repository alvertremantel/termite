use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Content,
}

pub struct CoreState<C = ()> {
    pub focus: Focus,
    pub running: bool,
    pub config: C,
    pub sidebar_area: Rect,
    pub content_area: Rect,
    pub sidebar_visible: bool,
    pub searching: bool,
    pub search_query: String,
    pub search_results: Vec<usize>,
    pub search_index: usize,
    pub help_visible: bool,
}

impl<C> CoreState<C> {
    pub fn new(config: C) -> Self {
        Self {
            focus: Focus::Sidebar,
            running: true,
            config,
            sidebar_area: Rect::default(),
            content_area: Rect::default(),
            sidebar_visible: true,
            searching: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: 0,
            help_visible: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_expected_defaults() {
        let state = CoreState::new("config");

        assert_eq!(state.focus, Focus::Sidebar);
        assert!(state.running);
        assert_eq!(state.config, "config");
        assert!(!state.searching);
        assert!(state.search_query.is_empty());
        assert!(state.search_results.is_empty());
    }
}
