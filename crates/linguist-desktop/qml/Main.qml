import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import io.github.lam.linguist_anki_bridge

ApplicationWindow {
    id: root
    visible: true
    width: 1440
    height: 880
    minimumWidth: 980
    minimumHeight: 640
    title: qsTr("Linguist Anki Bridge")
    color: backend.themeBackground

    AppBackend { id: backend }

    readonly property color background: backend.themeBackground
    readonly property color surface: backend.themeSurface
    readonly property color foreground: backend.themeForeground
    readonly property color muted: backend.themeMuted
    readonly property color accent: backend.themeAccent

    Shortcut { sequence: "Ctrl+K"; onActivated: search.forceActiveFocus() }
    Shortcut { sequence: "Ctrl+Return"; onActivated: reviewWorkspace.applyRequested() }
    Shortcut { sequence: "Ctrl+Shift+T"; onActivated: backend.reloadTheme() }

    header: ToolBar {
        background: Rectangle { color: root.background }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 18
            anchors.rightMargin: 18
            spacing: 16

            Label {
                text: qsTr("Linguist")
                color: root.foreground
                font.pixelSize: 19
                font.weight: Font.DemiBold
            }
            TextField {
                id: search
                Layout.fillWidth: true
                Layout.maximumWidth: 520
                placeholderText: qsTr("Search cards, decks, or commands   Ctrl K")
            }
            Item { Layout.fillWidth: true }
            Label { text: qsTr("● Anki"); color: "#a6e3a1" }
            Label { text: qsTr("● Ollama"); color: "#a6e3a1" }
            ToolButton {
                text: qsTr("Reload theme")
                onClicked: backend.reloadTheme()
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 1

        NavigationRail {
            Layout.preferredWidth: 220
            Layout.fillHeight: true
            backgroundColor: root.surface
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
        ReviewQueue {
            Layout.preferredWidth: 360
            Layout.fillHeight: true
            backgroundColor: Qt.darker(root.surface, 1.08)
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
        ReviewWorkspace {
            id: reviewWorkspace
            Layout.fillWidth: true
            Layout.fillHeight: true
            backgroundColor: root.background
            surfaceColor: root.surface
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
            onApplyRequested: statusText.text = qsTr("Native commit adapter is the next migration slice")
        }
    }

    footer: Rectangle {
        implicitHeight: 34
        color: root.surface
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 16
            anchors.rightMargin: 16
            Label { id: statusText; text: qsTr("Review-first native foundation"); color: root.muted }
            Item { Layout.fillWidth: true }
            Label { text: qsTr("Tab navigate  ·  Ctrl Enter apply"); color: root.muted }
        }
    }
}
