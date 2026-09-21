const MAIN: &str = include_str!("../qml/Main.qml");
const REVIEW: &str = include_str!("../qml/ReviewWorkspace.qml");
const QUEUE: &str = include_str!("../qml/ReviewQueue.qml");
const BATCH: &str = include_str!("../qml/BatchWorkspace.qml");

#[test]
fn text_editors_keep_japanese_ime_available() {
    for (name, qml) in [("main", MAIN), ("review", REVIEW), ("batch", BATCH)] {
        assert!(
            !qml.contains("ImhNoPredictiveText"),
            "{name} must not suppress IME composition"
        );
    }
    assert!(MAIN.matches("inputMethodHints: Qt.ImhNone").count() >= 2);
    assert!(REVIEW.matches("inputMethodHints: Qt.ImhNone").count() >= 3);
}

#[test]
fn pointer_only_surfaces_have_keyboard_and_screen_reader_actions() {
    assert_eq!(
        REVIEW.matches("Accessible.role: Accessible.Button").count(),
        2
    );
    assert_eq!(
        REVIEW
            .matches("Accessible.onPressAction: openZoom()")
            .count(),
        2
    );
    assert_eq!(REVIEW.matches("Keys.onSpacePressed: openZoom()").count(), 2);
    assert!(QUEUE.contains("activeFocusOnTab: true"));
    assert!(BATCH.contains("Accessible.name: qsTr(\"Batch selector preview\")"));
}

#[test]
fn keyboard_contract_has_edit_commit_and_navigation_shortcuts() {
    for shortcut in [
        "StandardKey.Undo",
        "StandardKey.Redo",
        "Ctrl+G",
        "Ctrl+Shift+Return",
    ] {
        assert!(REVIEW.contains(shortcut), "missing shortcut {shortcut}");
    }
    for shortcut in ["Ctrl+K", "Ctrl+Return", "Alt+Down", "Alt+Up"] {
        assert!(MAIN.contains(shortcut), "missing shortcut {shortcut}");
    }
}

#[test]
fn layout_scales_without_automatic_motion() {
    assert!(MAIN.contains("narrowMode"));
    assert!(BATCH.contains("Math.min(workspace.width - 32, 620)"));
    for qml in [MAIN, REVIEW, QUEUE, BATCH] {
        assert!(!qml.contains("Animation {"));
        assert!(!qml.contains("Behavior on"));
    }
}
