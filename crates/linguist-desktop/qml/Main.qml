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
    palette.window: appBackground
    palette.windowText: foreground
    palette.base: surface
    palette.text: foreground
    palette.button: surface
    palette.buttonText: foreground
    palette.highlight: accent
    palette.highlightedText: appBackground

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
    readonly property string connectionWarning: [
        backend.anki_status === "Ready" ? "" : qsTr("Anki: %1").arg(backend.anki_status),
        backend.ollama_status === "Ready" ? "" : qsTr("Ollama: %1").arg(backend.ollama_status)
    ].filter(Boolean).join(" · ")
    readonly property string headerWarning: backend.error_message.length > 0 ? backend.error_message : connectionWarning
    // Qt Quick units follow display scale. Keep motion absent unless user-triggered.
    readonly property bool reducedMotion: Qt.application.arguments.indexOf("--reduce-motion") >= 0

    Shortcut { sequence: "Ctrl+K"; onActivated: { root.showQueue = true; reviewQueue.focusSearch() } }
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

            ToolButton {
                visible: root.narrowMode
                text: root.showQueue ? qsTr("Show card editor") : qsTr("Show review queue")
                icon.source: root.showQueue ? "icons/card.svg" : "icons/list.svg"
                icon.color: root.foreground
                display: AbstractButton.IconOnly
                Accessible.name: root.showQueue ? qsTr("Show card editor") : qsTr("Show review queue")
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: root.showQueue = !root.showQueue
            }
            ToolButton {
                text: qsTr("Anki")
                icon.source: "icons/database.svg"
                icon.color: backend.anki_status === "Ready" ? root.accent : root.muted
                display: AbstractButton.IconOnly
                Accessible.name: qsTr("Anki: %1").arg(backend.anki_status)
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: settingsDialog.open()
            }
            ToolButton {
                text: qsTr("Ollama")
                icon.source: "icons/brain-circuit.svg"
                icon.color: backend.ollama_status === "Ready" ? root.accent : root.muted
                display: AbstractButton.IconOnly
                Accessible.name: qsTr("Ollama: %1").arg(backend.ollama_status)
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: { settingsDialog.open(); Qt.callLater(function() { settingsOllama.forceActiveFocus() }) }
            }
            ToolButton {
                id: refreshButton
                text: qsTr("Refresh connections and queue")
                icon.source: "icons/refresh.svg"
                icon.color: root.foreground
                display: AbstractButton.IconOnly
                Accessible.name: text
                Accessible.description: qsTr("Refresh Anki and Ollama connection status")
                ToolTip.visible: hovered
                ToolTip.text: text
                onClicked: backend.refreshState()
            }
            ToolButton {
                id: batchButton
                text: qsTr("Batch jobs")
                icon.source: "icons/list.svg"
                icon.color: root.foreground
                display: AbstractButton.IconOnly
                Accessible.name: text
                Accessible.description: qsTr("Open batch management. Shortcut Ctrl Shift B")
                ToolTip.visible: hovered
                ToolTip.text: text + qsTr(" · Ctrl Shift B")
                onClicked: { backend.refreshBatches(); batchDialog.open() }
            }
            ToolButton {
                id: importButton
                text: qsTr("Import words")
                icon.source: "icons/add.svg"
                icon.color: root.foreground
                display: AbstractButton.IconOnly
                Accessible.name: text
                Accessible.description: qsTr("Preview pasted words. Shortcut Ctrl Shift I")
                ToolTip.visible: hovered
                ToolTip.text: text + qsTr(" · Ctrl Shift I")
                onClicked: manualDialog.open()
            }
            ToolButton {
                id: settingsButton
                text: qsTr("Settings")
                icon.source: "icons/settings.svg"
                icon.color: root.foreground
                display: AbstractButton.IconOnly
                Accessible.name: text
                Accessible.description: qsTr("Open native settings. Shortcut Ctrl comma")
                ToolTip.visible: hovered
                ToolTip.text: text + qsTr(" · Ctrl ,")
                onClicked: settingsDialog.open()
            }
            Item { Layout.fillWidth: true }
            Label {
                visible: root.headerWarning.length > 0 && root.width >= 720
                Layout.maximumWidth: Math.max(100, root.width * 0.32)
                text: root.headerWarning
                color: root.accent
                elide: Text.ElideRight
                Accessible.name: text
            }
            ToolButton {
                visible: root.headerWarning.length > 0
                text: qsTr("Warnings and errors")
                icon.source: "icons/triangle-alert.svg"
                icon.color: root.accent
                display: AbstractButton.IconOnly
                Accessible.name: root.headerWarning
                ToolTip.visible: hovered
                ToolTip.text: root.headerWarning
                onClicked: {
                    if (backend.error_message.length > 0) backend.clearError()
                    else settingsDialog.open()
                }
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 1
        Accessible.name: root.title
        Accessible.description: qsTr("Language-card review workspace")

        ReviewQueue {
            id: reviewQueue
            visible: !root.narrowMode || root.showQueue
            Layout.preferredWidth: root.narrowMode ? 0 : 360
            Layout.fillWidth: root.narrowMode
            Layout.fillHeight: true
            backend: backend
            backgroundColor: Qt.darker(root.surface, 1.08)
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
            onCardSelected: if (root.narrowMode) root.showQueue = false
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
                text: backend.active_deck.length > 0 ? qsTr("Deck · %1").arg(backend.active_deck) : qsTr("No active deck")
                color: root.muted
            }
            Item { Layout.fillWidth: true }
            Label { text: qsTr("Ctrl Enter preview"); color: root.muted }
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
                    enabled: backend.manual_enqueue_count > 0
                    onClicked: { backend.enqueueManualInput(); manualDialog.close() }
                }
            }
            Label { text: qsTr("%1 rows · %2 to enqueue · %3 issues").arg(backend.manual_preview_count).arg(backend.manual_enqueue_count).arg(backend.manual_issue_count); color: root.muted }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: backend.manual_preview_count
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            Label {
                                Layout.fillWidth: true
                                text: backend.manualPreviewRow(index)
                                color: root.foreground
                                wrapMode: Text.Wrap
                            }
                            ComboBox {
                                Layout.preferredWidth: 205
                                model: backend.manualPreviewOptions(index).split("\n")
                                Accessible.name: qsTr("Decision for row %1").arg(index + 1)
                                ToolTip.visible: hovered
                                ToolTip.text: Accessible.name
                                onActivated: backend.selectManualDecision(index, currentIndex)
                            }
                        }
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
        height: Math.min(root.height * 0.84, 760)
        standardButtons: Dialog.Close
        function loadDeckMapping() {
            if (!settingsPurpose || !settingsDeck || !settingsDeckModel || !settingsPurpose.currentValue) return
            settingsDeck.editText = backend.mappedDeckName(settingsPurpose.currentValue)
            settingsDeckModel.text = backend.mappedModelName(settingsPurpose.currentValue)
        }
        onOpened: {
            settingsAnki.text = backend.settings_anki_url
            settingsOllama.text = backend.settings_ollama_url
            settingsModel.text = backend.settings_ollama_model
            settingsDictionary.text = backend.settings_dictionary_preset
            settingsDryRun.checked = backend.settings_dry_run
            loadDeckMapping()
            settingsAnki.forceActiveFocus()
        }
        contentItem: ScrollView {
            clip: true
            contentWidth: availableWidth
        ColumnLayout {
            width: parent.width
            spacing: 10
            Label { text: qsTr("AnkiConnect URL"); color: root.foreground }
            TextField { id: settingsAnki; Accessible.name: qsTr("AnkiConnect URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:8765" }
            Label { text: qsTr("Ollama URL"); color: root.foreground }
            TextField { id: settingsOllama; Accessible.name: qsTr("Ollama URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:11434" }
            Label { text: qsTr("Ollama model"); color: root.foreground }
            TextField { id: settingsModel; Accessible.name: qsTr("Ollama model"); Layout.fillWidth: true; placeholderText: qsTr("Optional model override") }
            Label { text: qsTr("Dictionary preset"); color: root.foreground }
            TextField { id: settingsDictionary; Accessible.name: qsTr("Dictionary preset"); Layout.fillWidth: true; placeholderText: qsTr("Dictionary preset") }
            Label { text: qsTr("Deck purpose mapping"); color: root.foreground; font.weight: Font.DemiBold }
            RowLayout {
                Layout.fillWidth: true
                ComboBox {
                    id: settingsPurpose
                    Accessible.name: qsTr("Deck purpose")
                    Layout.fillWidth: true
                    textRole: "label"
                    valueRole: "key"
                    model: [
                        { label: qsTr("Japanese vocabulary"), key: "japanese_vocab" },
                        { label: qsTr("Japanese grammar"), key: "japanese_grammar" },
                        { label: qsTr("English vocabulary"), key: "english_vocab" },
                        { label: qsTr("English grammar"), key: "english_grammar" },
                        { label: qsTr("Taiwanese vocabulary"), key: "taiwanese_vocab" },
                        { label: qsTr("Taiwanese grammar"), key: "taiwanese_grammar" },
                        { label: qsTr("German vocabulary"), key: "german_vocab" },
                        { label: qsTr("German grammar"), key: "german_grammar" }
                    ]
                    onCurrentIndexChanged: settingsDialog.loadDeckMapping()
                }
                ComboBox {
                    id: settingsDeck
                    Accessible.name: qsTr("Anki deck for selected purpose")
                    Layout.fillWidth: true
                    editable: true
                    model: {
                        let names = []
                        for (let index = 0; index < backend.deck_count; index++)
                            names.push(backend.deckName(index))
                        return names
                    }
                    onActivated: editText = currentText
                }
            }
            TextField {
                id: settingsDeckModel
                Accessible.name: qsTr("Anki note type for mapped deck")
                Layout.fillWidth: true
                placeholderText: qsTr("Note type for new cards (optional)")
            }
            RowLayout {
                Layout.fillWidth: true
                Button {
                    text: qsTr("Save deck mapping")
                    Accessible.name: text
                    onClicked: backend.saveDeckMapping(settingsPurpose.currentValue, settingsDeck.editText, settingsDeckModel.text)
                }
                Label { Layout.fillWidth: true; text: qsTr("Empty deck clears mapping"); color: root.muted }
            }
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
