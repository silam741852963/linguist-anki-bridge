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
    Component.onCompleted: backend.refreshState()

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
            Label {
                text: qsTr("● Anki · %1").arg(backend.ankiStatus)
                color: backend.ankiStatus === "Ready" ? "#a6e3a1" : root.muted
            }
            Label {
                text: qsTr("● Ollama · %1").arg(backend.ollamaStatus)
                color: backend.ollamaStatus === "Ready" ? "#a6e3a1" : root.muted
            }
            ToolButton {
                text: qsTr("Refresh state")
                onClicked: backend.refreshState()
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
            backend: backend
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
            onApplyRequested: backend.reportError(qsTr("Commit adapter is not connected yet"))
        }
    }

    footer: Rectangle {
        implicitHeight: 34
        color: root.surface
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 16
            anchors.rightMargin: 16
            Label {
                text: backend.errorMessage.length > 0
                    ? backend.errorMessage
                    : (backend.activeDeck.length > 0 ? qsTr("Deck · %1").arg(backend.activeDeck) : qsTr("No active deck"))
                color: backend.errorMessage.length > 0 ? root.accent : root.muted
            }
            Item { Layout.fillWidth: true }
            Label { text: qsTr("Tab navigate  ·  Ctrl Enter apply"); color: root.muted }
        }
    }
}
