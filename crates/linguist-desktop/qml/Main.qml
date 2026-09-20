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
    minimumWidth: 360
    minimumHeight: 360
    title: qsTr("Linguist Anki Bridge")
    color: backend.theme_background

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

    readonly property color appBackground: backend.theme_background
    readonly property color surface: backend.theme_surface
    readonly property color foreground: backend.theme_foreground
    readonly property color muted: backend.theme_muted
    readonly property color accent: backend.theme_accent
    readonly property bool narrowMode: width < 1000
    property bool showQueue: true
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
        enabled: backend.review_row_count > 0
        onActivated: backend.selectReviewIndex(Math.min(backend.review_row_count - 1, backend.selected_review_index + 1))
    }
    Shortcut {
        sequence: "Alt+Up"
        enabled: backend.review_row_count > 0
        onActivated: backend.selectReviewIndex(Math.max(0, backend.selected_review_index - 1))
    }

    header: ToolBar {
        background: Rectangle { color: root.appBackground }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: root.narrowMode ? 8 : 18
            anchors.rightMargin: root.narrowMode ? 8 : 18
            spacing: root.narrowMode ? 8 : 16

            Label {
                text: qsTr("Linguist")
                visible: root.width >= 1200
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
                Layout.minimumWidth: 80
                Layout.preferredWidth: root.width >= 1200 ? 320 : 160
                Layout.maximumWidth: 320
                placeholderText: root.width < 700 ? qsTr("Search · Ctrl K") : qsTr("Search cards, decks, or commands   Ctrl K")
                KeyNavigation.tab: refreshButton
            }
            ToolButton {
                visible: root.narrowMode
                text: root.showQueue ? qsTr("Card") : qsTr("Queue")
                Accessible.name: root.showQueue ? qsTr("Show card editor") : qsTr("Show review queue")
                onClicked: root.showQueue = !root.showQueue
            }
            Label {
                text: qsTr("● Anki · %1").arg(backend.anki_status)
                visible: root.width >= 1250
                color: backend.anki_status === "Ready" ? "#a6e3a1" : root.muted
            }
            Label {
                text: qsTr("● Ollama · %1").arg(backend.ollama_status)
                visible: root.width >= 1250
                color: backend.ollama_status === "Ready" ? "#a6e3a1" : root.muted
            }
            ToolButton {
                id: refreshButton
                text: root.width >= 1200 ? qsTr("Refresh state") : root.width >= 700 ? qsTr("Refresh") : qsTr("↻")
                Accessible.name: text
                Accessible.description: qsTr("Refresh Anki and Ollama connection status")
                onClicked: backend.refreshState()
            }
            ToolButton {
                id: batchButton
                text: root.width >= 1200 ? qsTr("Batch jobs") : root.width >= 700 ? qsTr("Batch") : qsTr("Jobs")
                Accessible.name: text
                Accessible.description: qsTr("Open batch management. Shortcut Ctrl Shift B")
                onClicked: { backend.refreshBatches(); batchDialog.open() }
            }
            ToolButton {
                id: importButton
                text: root.width >= 1200 ? qsTr("Import words") : root.width >= 700 ? qsTr("Import") : qsTr("Add")
                Accessible.name: text
                Accessible.description: qsTr("Preview pasted words. Shortcut Ctrl Shift I")
                onClicked: manualDialog.open()
            }
            ToolButton {
                id: settingsButton
                text: root.width < 700 ? qsTr("⚙") : qsTr("Settings")
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
        Accessible.name: root.title
        Accessible.description: qsTr("Language-card review workspace")

        NavigationRail {
            visible: !root.narrowMode
            onReviewRequested: reviewWorkspace.forceActiveFocus()
            onAddRequested: manualDialog.open()
            onBatchRequested: { backend.refreshBatches(); batchDialog.open() }
            onHistoryRequested: reviewWorkspace.openHistory()
            onSettingsRequested: settingsDialog.open()
            Layout.preferredWidth: 220
            Layout.fillHeight: true
            backgroundColor: root.surface
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
        ReviewQueue {
            visible: !root.narrowMode || root.showQueue
            Layout.preferredWidth: root.narrowMode ? 0 : 360
            Layout.fillWidth: root.narrowMode
            Layout.fillHeight: true
            backend: backend
            backgroundColor: Qt.darker(root.surface, 1.08)
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
        ReviewWorkspace {
            id: reviewWorkspace
            visible: !root.narrowMode || !root.showQueue
            Layout.fillWidth: true
            Layout.fillHeight: true
            backend: backend
            backgroundColor: root.appBackground
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
                text: backend.error_message.length > 0
                    ? backend.error_message
                    : (backend.active_deck.length > 0 ? qsTr("Deck · %1").arg(backend.active_deck) : qsTr("No active deck"))
                color: backend.error_message.length > 0 ? root.accent : root.muted
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
                TextField { id: importDeck; Accessible.name: qsTr("Target deck"); Layout.fillWidth: true; text: backend.active_deck; placeholderText: qsTr("Deck") }
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
                Label { Layout.fillWidth: true; text: backend.csv_mapping; color: root.muted; elide: Text.ElideRight }
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
                    enabled: backend.manual_preview_count > 0
                    onClicked: { backend.enqueueManualInput(); manualDialog.close() }
                }
            }
            Label { text: qsTr("%1 rows · %2 issues").arg(backend.manual_preview_count).arg(backend.manual_issue_count); color: root.muted }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: backend.manual_preview_count
                        delegate: Label { required property int index; Layout.fillWidth: true; text: backend.manualPreviewRow(index); color: root.foreground; wrapMode: Text.Wrap }
                    }
                    Repeater {
                        model: backend.manual_issue_count
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
            settingsAnki.text = backend.settings_anki_url
            settingsOllama.text = backend.settings_ollama_url
            settingsModel.text = backend.settings_ollama_model
            settingsDictionary.text = backend.settings_dictionary_preset
            settingsDryRun.checked = backend.settings_dry_run
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
                text: backend.settings_message
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
