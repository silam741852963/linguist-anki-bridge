const MAIN: &str = include_str!("../qml/Main.qml");
const REVIEW: &str = include_str!("../qml/ReviewWorkspace.qml");
const QUEUE: &str = include_str!("../qml/ReviewQueue.qml");
const BATCH: &str = include_str!("../qml/BatchWorkspace.qml");
const MAPPING: &str = include_str!("../qml/MappingWorkspace.qml");
const DIALOG: &str = include_str!("../qml/ThemedDialog.qml");

const ICONS: &[(&str, &str)] = &[
    ("add", include_str!("../qml/icons/add.svg")),
    (
        "brain-circuit",
        include_str!("../qml/icons/brain-circuit.svg"),
    ),
    ("card", include_str!("../qml/icons/card.svg")),
    (
        "chevron-left",
        include_str!("../qml/icons/chevron-left.svg"),
    ),
    (
        "chevron-right",
        include_str!("../qml/icons/chevron-right.svg"),
    ),
    ("database", include_str!("../qml/icons/database.svg")),
    ("list", include_str!("../qml/icons/list.svg")),
    ("pause", include_str!("../qml/icons/pause.svg")),
    ("play", include_str!("../qml/icons/play.svg")),
    ("preview", include_str!("../qml/icons/preview.svg")),
    ("redo", include_str!("../qml/icons/redo.svg")),
    ("refresh", include_str!("../qml/icons/refresh.svg")),
    ("search", include_str!("../qml/icons/search.svg")),
    ("settings", include_str!("../qml/icons/settings.svg")),
    (
        "triangle-alert",
        include_str!("../qml/icons/triangle-alert.svg"),
    ),
    ("undo", include_str!("../qml/icons/undo.svg")),
    ("unlink", include_str!("../qml/icons/unlink.svg")),
];

#[test]
fn text_editors_keep_japanese_ime_available() {
    for (name, qml) in [("main", MAIN), ("review", REVIEW), ("batch", BATCH)] {
        assert!(
            !qml.contains("ImhNoPredictiveText"),
            "{name} must not suppress IME composition"
        );
    }
    assert!(MAIN.matches("inputMethodHints: Qt.ImhNone").count() >= 2);
    assert!(REVIEW.matches("inputMethodHints: Qt.ImhNone").count() >= 2);
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
fn layout_scales_with_only_requested_navigation_motion() {
    assert!(MAIN.contains("narrowMode"));
    assert!(BATCH.contains("width: Math.min(workspace.width * 0.9, 680)"));
    assert!(BATCH.contains("contentItem: ScrollView"));
    for qml in [MAIN, REVIEW, BATCH] {
        assert!(!qml.contains("Animation {"));
        assert!(!qml.contains("Behavior on"));
    }
    assert!(QUEUE.contains("easing.type: Easing.OutCubic"));
    assert!(!QUEUE.contains("Behavior on"));
}

#[test]
fn native_ui_colors_only_come_from_theme_properties() {
    for (name, qml) in [
        ("main", MAIN),
        ("review", REVIEW),
        ("queue", QUEUE),
        ("batch", BATCH),
        ("mapping", MAPPING),
        ("dialog", DIALOG),
    ] {
        assert!(!qml.contains('#'), "{name} contains a hardcoded hex color");
        for literal in ["\"white\"", "\"black\"", "\"transparent\""] {
            assert!(
                !qml.contains(literal),
                "{name} contains hardcoded color {literal}"
            );
        }
    }

    for (name, icon) in ICONS {
        assert!(
            icon.contains("stroke=\"currentColor\""),
            "{name} is not theme-tinted"
        );
        assert!(icon.contains("fill=\"none\""), "{name} has a fixed fill");
        assert!(!icon.contains('#'), "{name} contains a hardcoded hex color");
    }
}
