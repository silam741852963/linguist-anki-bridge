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
    color: appBackground
    palette.window: appBackground
    palette.windowText: foreground
    palette.base: surface
    palette.text: foreground
    palette.button: surface
    palette.buttonText: foreground
    palette.highlight: selection
    palette.highlightedText: appBackground

    SystemPalette { id: systemColors }
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
    Timer {
        interval: 40
        running: root.visible
        repeat: true
        onTriggered: backend.pollReviewLoad()
    }

    readonly property color appBackground: backend.theme_background.length > 0 ? backend.theme_background : systemColors.window
    readonly property color surface: backend.theme_surface.length > 0 ? backend.theme_surface : systemColors.base
    readonly property color foreground: backend.theme_foreground.length > 0 ? backend.theme_foreground : systemColors.text
    readonly property color muted: backend.theme_muted.length > 0 ? backend.theme_muted : systemColors.mid
    readonly property color accent: backend.theme_accent.length > 0 ? backend.theme_accent : systemColors.highlight
    readonly property color selection: backend.theme_selection.length > 0 ? backend.theme_selection : systemColors.highlight
    readonly property color danger: backend.theme_red.length > 0 ? backend.theme_red : accent
    readonly property color warning: backend.theme_yellow.length > 0 ? backend.theme_yellow : accent
    readonly property color success: backend.theme_green.length > 0 ? backend.theme_green : accent
    readonly property color info: backend.theme_cyan.length > 0 ? backend.theme_cyan : accent
    readonly property color link: backend.theme_blue.length > 0 ? backend.theme_blue : accent
    readonly property color generated: backend.theme_magenta.length > 0 ? backend.theme_magenta : accent
    readonly property bool narrowMode: width < 1000
    property bool showQueue: true
    readonly property string connectionWarning: [
        backend.anki_status === "Ready" ? "" : qsTr("Anki: %1").arg(backend.anki_status),
        backend.ollama_status === "Ready" ? "" : qsTr("Ollama: %1").arg(backend.ollama_status)
    ].filter(Boolean).join(" · ")
    readonly property string headerWarning: backend.error_message.length > 0 ? backend.error_message : connectionWarning
    readonly property color headerWarningColor: backend.error_message.length > 0 ? root.danger : root.warning
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
            anchors.leftMargin: root.narrowMode ? 8 : 16
            anchors.rightMargin: root.narrowMode ? 8 : 16
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
                color: root.headerWarningColor
                elide: Text.ElideRight
                Accessible.name: text
            }
            ToolButton {
                visible: root.headerWarning.length > 0
                text: qsTr("Warnings and errors")
                icon.source: "icons/triangle-alert.svg"
                icon.color: root.headerWarningColor
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
            successColor: root.success
            warningColor: root.warning
            dangerColor: root.danger
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
            successColor: root.success
            warningColor: root.warning
            dangerColor: root.danger
            infoColor: root.info
            linkColor: root.link
            generatedColor: root.generated
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

    ThemedDialog {
        id: batchDialog
        backgroundColor: root.appBackground
        surfaceColor: root.surface
        foregroundColor: root.foreground
        mutedColor: root.muted
        accentColor: root.accent
        title: qsTr("Batch management")
        width: Math.min(root.width * 0.82, 980)
        height: Math.min(root.height * 0.8, 700)
        standardButtons: Dialog.Close
        contentItem: BatchWorkspace {
            backend: backend
            backgroundColor: root.appBackground
            surfaceColor: root.surface
            foregroundColor: root.foreground
            mutedColor: root.muted
            accentColor: root.accent
        }
    }

    ThemedDialog {
        id: manualDialog
        backgroundColor: root.appBackground
        surfaceColor: root.surface
        foregroundColor: root.foreground
        mutedColor: root.muted
        accentColor: root.accent
        function remapCsv() {
            backend.remapCsvInput(csvExpression.currentIndex, csvLanguage.currentIndex - 1,
                                  csvType.currentIndex - 1, csvContext.currentIndex - 1)
        }
        title: qsTr("Import words")
        width: Math.min(root.width * 0.76, 900)
        height: Math.min(root.height * 0.8, 700)
        standardButtons: Dialog.Close
        onOpened: Qt.callLater(function() {
            importDeck.editText = backend.active_deck
            importDeck.forceActiveFocus()
        })
        contentItem: ColumnLayout {
            spacing: 12
            RowLayout {
                ComboBox {
                    id: importDeck
                    Accessible.name: qsTr("Target deck")
                    Layout.fillWidth: true
                    editable: true
                    model: {
                        let names = []
                        for (let index = 0; index < backend.deck_count; index++) names.push(backend.deckName(index))
                        return names
                    }
                }
                ComboBox { id: importLanguage; Accessible.name: qsTr("Language key"); Layout.fillWidth: true; editable: true; model: ["japanese_vocab", "japanese_grammar", "english_vocab", "english_grammar", "taiwanese_vocab", "taiwanese_grammar", "german_vocab", "german_grammar"]; currentIndex: 0 }
                ComboBox { id: importType; Accessible.name: qsTr("Card type"); Layout.fillWidth: true; editable: true; model: ["vocab", "grammar"]; currentIndex: 0 }
            }
            TextArea {
                id: manualRows
                Accessible.name: qsTr("Words and optional context")
                Accessible.description: qsTr("One expression per line. Add context after a tab.")
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                placeholderText: qsTr("食べる<Tab>meal verb\n新語<Tab>optional context")
                wrapMode: TextEdit.Wrap
                inputMethodHints: Qt.ImhNone
            }
            Label { text: qsTr("CSV input"); color: root.muted }
            TextArea {
                id: csvRows
                Accessible.name: qsTr("CSV content or drop target")
                Accessible.description: qsTr("Paste CSV with a header row, or drop a local CSV file")
                Layout.fillWidth: true
                Layout.preferredHeight: 100
                placeholderText: qsTr("Word,Language,Type,Context")
                inputMethodHints: Qt.ImhNone
                DropArea {
                    anchors.fill: parent
                    onDropped: function(drop) {
                        if (drop.urls.length > 0)
                            backend.previewCsvFile(drop.urls[0], importDeck.editText, importLanguage.editText, importType.editText)
                        else if (drop.text.length > 0) {
                            csvRows.text = drop.text
                            backend.previewCsvInput(csvRows.text, importDeck.editText, importLanguage.editText, importType.editText)
                        }
                    }
                }
            }
            RowLayout {
                Button {
                    text: qsTr("Preview CSV")
                    Accessible.name: text
                    enabled: csvRows.text.trim().length > 0 && importDeck.editText.trim().length > 0
                    onClicked: backend.previewCsvInput(csvRows.text, importDeck.editText, importLanguage.editText, importType.editText)
                }
                Button { text: qsTr("Choose CSV file"); Accessible.name: text; onClicked: csvFileDialog.open() }
                Item { Layout.fillWidth: true }
            }
            GridLayout {
                visible: backend.csv_column_count > 0
                Layout.fillWidth: true
                columns: manualDialog.width < 700 ? 2 : 4
                Label { text: qsTr("Expression"); color: root.muted }
                ComboBox {
                    id: csvExpression
                    Layout.fillWidth: true
                    Accessible.name: qsTr("CSV expression column")
                    model: {
                        let names = []
                        for (let i = 0; i < backend.csv_column_count; i++) names.push(backend.csvColumnName(i))
                        return names
                    }
                    currentIndex: backend.csv_expression_column
                    onActivated: manualDialog.remapCsv()
                }
                Label { text: qsTr("Language"); color: root.muted }
                ComboBox {
                    id: csvLanguage
                    Layout.fillWidth: true
                    Accessible.name: qsTr("CSV language column")
                    model: {
                        let names = [qsTr("None")]
                        for (let i = 0; i < backend.csv_column_count; i++) names.push(backend.csvColumnName(i))
                        return names
                    }
                    currentIndex: backend.csv_language_column + 1
                    onActivated: manualDialog.remapCsv()
                }
                Label { text: qsTr("Type"); color: root.muted }
                ComboBox {
                    id: csvType
                    Layout.fillWidth: true
                    Accessible.name: qsTr("CSV type column")
                    model: {
                        let names = [qsTr("None")]
                        for (let i = 0; i < backend.csv_column_count; i++) names.push(backend.csvColumnName(i))
                        return names
                    }
                    currentIndex: backend.csv_type_column + 1
                    onActivated: manualDialog.remapCsv()
                }
                Label { text: qsTr("Context"); color: root.muted }
                ComboBox {
                    id: csvContext
                    Layout.fillWidth: true
                    Accessible.name: qsTr("CSV context column")
                    model: {
                        let names = [qsTr("None")]
                        for (let i = 0; i < backend.csv_column_count; i++) names.push(backend.csvColumnName(i))
                        return names
                    }
                    currentIndex: backend.csv_context_column + 1
                    onActivated: manualDialog.remapCsv()
                }
            }
            RowLayout {
                Button {
                    text: qsTr("Preview import")
                    Accessible.name: text
                    enabled: manualRows.text.trim().length > 0 && importDeck.editText.trim().length > 0
                    onClicked: backend.previewManualInput(manualRows.text, importDeck.editText, importLanguage.editText, importType.editText)
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

    ThemedDialog {
        id: mappingDialog
        backgroundColor: root.appBackground
        surfaceColor: root.surface
        foregroundColor: root.foreground
        mutedColor: root.muted
        accentColor: root.accent
        title: qsTr("Deck and field mapping")
        width: Math.min(root.width * 0.92, 1180)
        height: Math.min(root.height * 0.9, 820)
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                ComboBox {
                    id: mappingPurpose
                    Layout.preferredWidth: 230
                    Accessible.name: qsTr("Deck purpose")
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
                    onActivated: {
                        backend.setMappingPurpose(currentValue)
                        mappingDeck.editText = backend.mappedDeckName(currentValue)
                    }
                }
                ComboBox {
                    id: mappingDeck
                    Layout.fillWidth: true
                    editable: true
                    Accessible.name: qsTr("Anki deck to inspect")
                    model: backend.deck_count
                    delegate: ItemDelegate {
                        required property int index
                        width: mappingDeck.width
                        text: backend.deckName(index)
                        onClicked: {
                            mappingDeck.editText = text
                            mappingDeck.popup.close()
                        }
                    }
                    onAccepted: backend.inspectDeckMapping(mappingPurpose.currentValue, editText)
                }
                ToolButton {
                    text: qsTr("Inspect selected deck")
                    icon.source: "icons/search.svg"
                    icon.color: root.accent
                    display: AbstractButton.IconOnly
                    enabled: mappingDeck.editText.trim().length > 0 && !backend.mapping_busy
                    Accessible.name: text
                    ToolTip.visible: hovered
                    ToolTip.text: text
                    onClicked: backend.inspectDeckMapping(mappingPurpose.currentValue, mappingDeck.editText)
                }
            }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                MappingWorkspace {
                    width: parent.width
                    backend: backend
                    backgroundColor: root.appBackground
                    surfaceColor: root.surface
                    foregroundColor: root.foreground
                    mutedColor: root.muted
                    accentColor: root.accent
                    infoColor: root.info
                    generatedColor: root.generated
                    showPurposePicker: false
                }
            }
        }
    }

    ThemedDialog {
        id: settingsDialog
        backgroundColor: root.appBackground
        surfaceColor: root.surface
        foregroundColor: root.foreground
        mutedColor: root.muted
        accentColor: root.accent
        title: qsTr("Native settings")
        width: Math.min(root.width * 0.62, 720)
        height: Math.min(root.height * 0.84, 760)
        standardButtons: Dialog.Close
        onOpened: {
            settingsAnki.text = backend.settings_anki_url
            settingsOllama.text = backend.settings_ollama_url
            settingsModel.text = backend.settings_ollama_model
            settingsDictionary.text = backend.settings_dictionary_preset
            settingsDryRun.checked = backend.settings_dry_run
            settingsAnki.forceActiveFocus()
        }
        contentItem: ScrollView {
            clip: true
            contentWidth: availableWidth
        ColumnLayout {
            width: parent.width
            spacing: 12
            Label { text: qsTr("AnkiConnect URL"); color: root.foreground }
            TextField { id: settingsAnki; Accessible.name: qsTr("AnkiConnect URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:8765" }
            Label { text: qsTr("Ollama URL"); color: root.foreground }
            TextField { id: settingsOllama; Accessible.name: qsTr("Ollama URL"); Layout.fillWidth: true; placeholderText: "http://127.0.0.1:11434" }
            Label { text: qsTr("Ollama model"); color: root.foreground }
            TextField { id: settingsModel; Accessible.name: qsTr("Ollama model"); Layout.fillWidth: true; placeholderText: qsTr("Optional model override") }
            Label { text: qsTr("Dictionary preset"); color: root.foreground }
            TextField { id: settingsDictionary; Accessible.name: qsTr("Dictionary preset"); Layout.fillWidth: true; placeholderText: qsTr("Dictionary preset") }
            Label { text: qsTr("Deck and field mapping"); color: root.foreground; font.weight: Font.DemiBold }
            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Inspect a deck, generate an Ollama-assisted mapping, then review every connection before saving.")
                    color: root.muted
                    wrapMode: Text.Wrap
                }
                ToolButton {
                    text: qsTr("Open mapping workspace")
                    icon.source: "icons/database.svg"
                    icon.color: root.accent
                    display: AbstractButton.IconOnly
                    Accessible.name: text
                    ToolTip.visible: hovered
                    ToolTip.text: text
                    onClicked: {
                        settingsDialog.close()
                        mappingDeck.editText = backend.mappedDeckName(mappingPurpose.currentValue)
                        mappingDialog.open()
                    }
                }
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
        onAccepted: backend.previewCsvFile(selectedFile, importDeck.editText, importLanguage.editText, importType.editText)
    }
    FileDialog {
        id: legacyConfigDialog
        title: qsTr("Import legacy YAML config")
        nameFilters: [qsTr("YAML files (*.yaml *.yml)"), qsTr("All files (*)")]
        onAccepted: backend.importLegacyConfig(selectedFile)
    }
}
