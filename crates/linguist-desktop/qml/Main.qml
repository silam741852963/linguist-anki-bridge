import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
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
    Accessible.name: title
    Accessible.description: qsTr("Language-card review workspace")

    AppBackend { id: backend }
    Component.onCompleted: backend.refreshState()
    Timer {
        interval: 1500
        running: root.visible
        repeat: true
        onTriggered: backend.reloadTheme()
    }
    Timer {
        interval: 500
        running: root.visible
        repeat: true
        onTriggered: backend.runBatchTick()
    }

    readonly property color background: backend.themeBackground
    readonly property color surface: backend.themeSurface
    readonly property color foreground: backend.themeForeground
    readonly property color muted: backend.themeMuted
    readonly property color accent: backend.themeAccent
    // Qt Quick units follow display scale. Keep motion absent unless user-triggered.
    readonly property bool reducedMotion: Qt.application.arguments.indexOf("--reduce-motion") >= 0

    Shortcut { sequence: "Ctrl+K"; onActivated: search.forceActiveFocus() }
    Shortcut { sequence: "Ctrl+Return"; onActivated: backend.previewCommit() }
    Shortcut { sequence: "Ctrl+Shift+T"; onActivated: backend.reloadTheme() }
    Shortcut { sequence: "Ctrl+Shift+B"; onActivated: { backend.refreshBatches(); batchDialog.open() } }
    Shortcut { sequence: "Ctrl+Shift+I"; onActivated: manualDialog.open() }
    Shortcut { sequence: "Ctrl+,"; onActivated: settingsDialog.open() }
    Shortcut {
        sequence: "Alt+Down"
        enabled: backend.reviewRowCount > 0
        onActivated: backend.selectReviewIndex(Math.min(backend.reviewRowCount - 1, backend.selectedReviewIndex + 1))
    }
    Shortcut {
        sequence: "Alt+Up"
        enabled: backend.reviewRowCount > 0
        onActivated: backend.selectReviewIndex(Math.max(0, backend.selectedReviewIndex - 1))
    }

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
                font.pointSize: 14
                font.weight: Font.DemiBold
            }
            TextField {
                id: search
                Accessible.name: qsTr("Search cards, decks, or commands")
                Accessible.description: qsTr("Press Ctrl K to focus search")
                focusPolicy: Qt.StrongFocus
                Layout.fillWidth: true
                Layout.maximumWidth: 520
                placeholderText: qsTr("Search cards, decks, or commands   Ctrl K")
                KeyNavigation.tab: refreshButton
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
                id: refreshButton
                text: qsTr("Refresh state")
                Accessible.name: text
                Accessible.description: qsTr("Refresh Anki and Ollama connection status")
                onClicked: backend.refreshState()
            }
            ToolButton {
                id: batchButton
                text: qsTr("Batch jobs")
                Accessible.name: text
                Accessible.description: qsTr("Open batch management. Shortcut Ctrl Shift B")
                onClicked: { backend.refreshBatches(); batchDialog.open() }
            }
            ToolButton {
                id: importButton
                text: qsTr("Import words")
                Accessible.name: text
                Accessible.description: qsTr("Preview pasted words. Shortcut Ctrl Shift I")
                onClicked: manualDialog.open()
            }
            ToolButton {
                id: settingsButton
                text: qsTr("Settings")
                Accessible.name: text
                Accessible.description: qsTr("Open native settings. Shortcut Ctrl comma")
                onClicked: settingsDialog.open()
                KeyNavigation.tab: search
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
            backend: backend
            backgroundColor: root.background
            surfaceColor: root.surface
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
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

    Dialog {
        id: batchDialog
        modal: true
        title: qsTr("Batch management")
        width: Math.min(root.width * 0.82, 980)
        height: Math.min(root.height * 0.8, 700)
        standardButtons: Dialog.Close
        contentItem: BatchWorkspace {
            backend: backend
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
    }

    Dialog {
        id: manualDialog
        modal: true
        title: qsTr("Import words")
        width: Math.min(root.width * 0.76, 900)
        height: Math.min(root.height * 0.8, 700)
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            spacing: 10
            RowLayout {
                TextField { id: importDeck; Accessible.name: qsTr("Target deck"); Layout.fillWidth: true; text: backend.activeDeck; placeholderText: qsTr("Deck") }
                TextField { id: importLanguage; Accessible.name: qsTr("Language key"); Layout.fillWidth: true; text: "japanese_vocab"; placeholderText: qsTr("Language") }
                TextField { id: importType; Accessible.name: qsTr("Card type"); Layout.fillWidth: true; text: "vocab"; placeholderText: qsTr("Type") }
            }
            TextArea {
                id: manualRows
                Accessible.name: qsTr("Words and optional context")
                Accessible.description: qsTr("One expression per line. Add context after a tab.")
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                placeholderText: qsTr("食べる<Tab>meal verb\n新語<Tab>optional context")
                wrapMode: TextEdit.Wrap
                inputMethodHints: Qt.ImhNoPredictiveText
            }
            Label { text: qsTr("CSV input"); color: root.muted }
            TextArea {
                id: csvRows
                Accessible.name: qsTr("CSV content or drop target")
                Accessible.description: qsTr("Paste CSV with a header row, or drop a local CSV file")
                Layout.fillWidth: true
                Layout.preferredHeight: 100
                placeholderText: qsTr("Word,Language,Type,Context")
                inputMethodHints: Qt.ImhNoPredictiveText
                DropArea {
                    anchors.fill: parent
                    onDropped: function(drop) {
                        if (drop.urls.length > 0)
                            backend.previewCsvFile(drop.urls[0], importDeck.text, importLanguage.text, importType.text)
                        else if (drop.text.length > 0) {
                            csvRows.text = drop.text
                            backend.previewCsvInput(csvRows.text, importDeck.text, importLanguage.text, importType.text)
                        }
                    }
                }
            }
            RowLayout {
                Button {
                    text: qsTr("Preview CSV")
                    Accessible.name: text
                    enabled: csvRows.text.trim().length > 0 && importDeck.text.trim().length > 0
                    onClicked: backend.previewCsvInput(csvRows.text, importDeck.text, importLanguage.text, importType.text)
                }
                Button { text: qsTr("Choose CSV file"); Accessible.name: text; onClicked: csvFileDialog.open() }
                Label { Layout.fillWidth: true; text: backend.csvMapping; color: root.muted; elide: Text.ElideRight }
            }
            RowLayout {
                Button {
                    text: qsTr("Preview import")
                    Accessible.name: text
                    enabled: manualRows.text.trim().length > 0 && importDeck.text.trim().length > 0
                    onClicked: backend.previewManualInput(manualRows.text, importDeck.text, importLanguage.text, importType.text)
                }
                Button {
                    text: qsTr("Enqueue preview")
                    Accessible.name: text
                    Accessible.description: qsTr("Add resolved imports to the review queue without writing Anki")
                    highlighted: true
                    enabled: backend.manualPreviewCount > 0
                    onClicked: { backend.enqueueManualInput(); manualDialog.close() }
                }
            }
            Label { text: qsTr("%1 rows · %2 issues").arg(backend.manualPreviewCount).arg(backend.manualIssueCount); color: root.muted }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: backend.manualPreviewCount
                        delegate: Label { required property int index; Layout.fillWidth: true; text: backend.manualPreviewRow(index); color: root.foreground; wrapMode: Text.Wrap }
                    }
                    Repeater {
                        model: backend.manualIssueCount
                        delegate: Label { required property int index; Layout.fillWidth: true; text: backend.manualPreviewIssue(index); color: root.accent; wrapMode: Text.Wrap }
                    }
                }
            }
        }
    }

    Dialog {
        id: settingsDialog
        modal: true
        title: qsTr("Native settings")
        width: Math.min(root.width * 0.62, 720)
        standardButtons: Dialog.Close
        onOpened: {
            settingsAnki.text = backend.settingsAnkiUrl
            settingsOllama.text = backend.settingsOllamaUrl
            settingsModel.text = backend.settingsOllamaModel
            settingsDictionary.text = backend.settingsDictionaryPreset
            settingsDryRun.checked = backend.settingsDryRun
            settingsAnki.forceActiveFocus()
        }
        contentItem: ColumnLayout {
            spacing: 10
            Label { text: qsTr("AnkiConnect URL"); color: root.foreground }
            TextField { id: settingsAnki; Accessible.name: qsTr("AnkiConnect URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:8765" }
            Label { text: qsTr("Ollama URL"); color: root.foreground }
            TextField { id: settingsOllama; Accessible.name: qsTr("Ollama URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:11434" }
            Label { text: qsTr("Ollama model"); color: root.foreground }
            TextField { id: settingsModel; Accessible.name: qsTr("Ollama model"); Layout.fillWidth: true; placeholderText: qsTr("Optional model override") }
            Label { text: qsTr("Dictionary preset"); color: root.foreground }
            TextField { id: settingsDictionary; Accessible.name: qsTr("Dictionary preset"); Layout.fillWidth: true; placeholderText: qsTr("Dictionary preset") }
            CheckBox { id: settingsDryRun; text: qsTr("Default to dry run"); Accessible.name: text }
            RowLayout {
                Button {
                    text: qsTr("Save native settings")
                    Accessible.name: text
                    highlighted: true
                    onClicked: backend.saveSettings(settingsAnki.text, settingsOllama.text, settingsModel.text, settingsDictionary.text, settingsDryRun.checked)
                }
                Button {
                    text: qsTr("Import legacy YAML")
                    Accessible.name: text
                    Accessible.description: qsTr("One-time read-only import. Existing native settings are never overwritten.")
                    onClicked: legacyConfigDialog.open()
                }
            }
            Label {
                Layout.fillWidth: true
                text: backend.settingsMessage
                color: root.muted
                wrapMode: Text.Wrap
                Accessible.name: text
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("Environment variables still override these values. Legacy YAML is read only when explicitly imported.")
                color: root.muted
                wrapMode: Text.Wrap
            }
        }
    }

    FileDialog {
        id: csvFileDialog
        title: qsTr("Choose CSV file")
        nameFilters: [qsTr("CSV files (*.csv)"), qsTr("All files (*)")]
        onAccepted: backend.previewCsvFile(selectedFile, importDeck.text, importLanguage.text, importType.text)
    }
    FileDialog {
        id: legacyConfigDialog
        title: qsTr("Import legacy YAML config")
        nameFilters: [qsTr("YAML files (*.yaml *.yml)"), qsTr("All files (*)")]
        onAccepted: backend.importLegacyConfig(selectedFile)
    }
}
