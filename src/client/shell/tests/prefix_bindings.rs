use super::*;

fn backtick_prefix_state(extra: &str) -> ClientShellState {
    let config = toml::from_str::<Config>(&format!("[keys]\nprefix = \"backtick\"\n{extra}"))
        .expect("configured keybinds");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state
}

fn single_pane_key(input: &ClientShellInput) -> (crate::protocol::ClientKeyCode, u8) {
    let [ClientMessage::ClientShellPaneInput { pane_id, events }] = &input.requests[..] else {
        panic!("expected one pane input request: {:?}", input.requests);
    };
    assert_eq!(pane_id, "pane_1");
    let [ClientPaneInputEvent::Key {
        code, modifiers, ..
    }] = &events[..]
    else {
        panic!("expected one key event: {events:?}");
    };
    (code.clone(), *modifiers)
}

#[test]
fn unbound_double_prefix_press_sends_literal_prefix() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));

    assert!(state.handle_input_bytes(&[0x02]).requests.is_empty());
    let literal = state.handle_input_bytes(&[0x02]);
    assert_eq!(
        single_pane_key(&literal),
        (
            crate::protocol::ClientKeyCode::Char('b'),
            KeyModifiers::CONTROL.bits()
        )
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn user_binding_on_prefix_rhs_overrides_literal_passthrough() {
    let mut state = backtick_prefix_state("toggle_sidebar = \"prefix+backtick\"\n");
    let collapsed = state.sidebar_collapsed;

    assert!(state.handle_input_bytes(b"`").requests.is_empty());
    let toggled = state.handle_input_bytes(b"`");

    assert!(
        toggled.requests.is_empty(),
        "prefix must not reach the pane"
    );
    assert_eq!(state.sidebar_collapsed, !collapsed);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn send_prefix_binding_forwards_the_prefix_key_to_the_pane() {
    let mut state = backtick_prefix_state(
        "last_tab = \"prefix+backtick\"\nsend_prefix = \"prefix+e\"\nedit_scrollback = \"prefix+shift+e\"\n",
    );

    assert!(state.handle_input_bytes(b"`").requests.is_empty());
    let sent = state.handle_input_bytes(b"e");

    assert_eq!(
        single_pane_key(&sent),
        (crate::protocol::ClientKeyCode::Char('`'), 0)
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn send_prefix_hint_tracks_how_the_literal_prefix_is_reachable() {
    let default = ClientShellConfig::from_config(&Config::default());
    assert_eq!(
        crate::input::send_prefix_hint(&default.keybinds).as_deref(),
        Some("ctrl+b")
    );

    let rebound = backtick_prefix_state("last_tab = \"prefix+backtick\"\n");
    assert_eq!(
        crate::input::send_prefix_hint(&rebound.config.keybinds),
        None
    );

    let explicit = backtick_prefix_state(
        "last_tab = \"prefix+backtick\"\nsend_prefix = \"prefix+e\"\nedit_scrollback = \"prefix+shift+e\"\n",
    );
    assert_eq!(
        crate::input::send_prefix_hint(&explicit.config.keybinds).as_deref(),
        Some("e")
    );
}

fn two_tab_snapshot(revision: u64, focused_tab: &str) -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.revision = revision;
    let mut second = projected.tabs[0].clone();
    second.tab_id = "tab_2".into();
    second.number = 2;
    second.label = "2".into();
    projected.tabs.push(second);
    for tab in &mut projected.tabs {
        tab.focused = tab.tab_id == focused_tab;
    }
    projected.workspaces[0].active_tab_id = focused_tab.into();
    projected.focused_tab_id = Some(focused_tab.into());
    projected
}

fn last_tab_target(state: &mut ClientShellState) -> Option<String> {
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::LastTab),
        &mut outcome,
    );
    match &outcome.actions[..] {
        // Without usable history the action falls through as a no-op, like `last_pane`.
        [ClientShellAction::Keybind(crate::input::KeybindAction::LastTab)] => None,
        [ClientShellAction::Endpoint { request, .. }] => match &request.method {
            crate::api::schema::Method::TabFocus(target) => Some(target.tab_id.clone()),
            other => panic!("last tab should focus a tab, got {other:?}"),
        },
        other => panic!("unexpected actions: {other:?}"),
    }
}

#[test]
fn last_tab_toggles_to_previously_focused_tab_in_the_workspace() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_tab_snapshot(1, "tab_1")));
    assert_eq!(last_tab_target(&mut state), None, "no history yet");

    state.set_snapshot(Box::new(two_tab_snapshot(2, "tab_2")));
    assert_eq!(last_tab_target(&mut state).as_deref(), Some("tab_1"));

    state.set_snapshot(Box::new(two_tab_snapshot(3, "tab_1")));
    assert_eq!(last_tab_target(&mut state).as_deref(), Some("tab_2"));
}

#[test]
fn last_tab_ignores_closed_tabs() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_tab_snapshot(1, "tab_1")));
    state.set_snapshot(Box::new(two_tab_snapshot(2, "tab_2")));

    let mut closed = two_tab_snapshot(3, "tab_2");
    closed.tabs.retain(|tab| tab.tab_id != "tab_1");
    state.set_snapshot(Box::new(closed));

    assert_eq!(last_tab_target(&mut state), None);
}
