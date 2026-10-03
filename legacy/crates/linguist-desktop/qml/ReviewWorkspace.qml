import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtMultimedia

Rectangle {
    id: workspace
    required property var backend
    required property color backgroundColor
    required property color surfaceColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    required property color successColor
    required property color warningColor
    required property color dangerColor
    required property color infoColor
    required property color linkColor
    required property color generatedColor
    property int previewTemplateIndex: 0
    property string zoomHtml: ""
    property bool mappingScreen: true
    Connections {
        target: backend
        function onDraft_generatedChanged() {
            if (!backend.draft_generated) workspace.mappingScreen = true
        }
    }
    color: backgroundColor

    Shortcut { sequences: [StandardKey.Undo]; enabled: backend.draft_can_undo; onActivated: backend.undoDraft() }
    Shortcut { sequences: [StandardKey.Redo]; enabled: backend.draft_can_redo; onActivated: backend.redoDraft() }
    Shortcut { sequence: "Ctrl+G"; enabled: backend.draft_available; onActivated: backend.regenerateDraft() }
    Shortcut { sequence: "Ctrl+Shift+Return"; enabled: backend.commit_preview_ready; onActivated: backend.applyCommit() }

    MediaPlayer {
        id: previewAudio
        audioOutput: AudioOutput { volume: 1.0 }
    }

    ThemedDialog {
        id: zoomDialog
        backgroundColor: workspace.backgroundColor
        surfaceColor: workspace.surfaceColor
        foregroundColor: workspace.foregroundColor
        mutedColor: workspace.mutedColor
        accentColor: workspace.accentColor
        title: qsTr("Card preview")
        width: Math.min(workspace.width * 0.92, 900)
        height: Math.min(workspace.height * 0.9, 720)
        standardButtons: Dialog.Close
        contentItem: ScrollView {
            contentWidth: availableWidth
            Text {
                width: parent.width
                text: workspace.zoomHtml
                textFormat: Text.RichText
                wrapMode: Text.Wrap
                color: workspace.foregroundColor
            }
        }
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth

        ColumnLayout {
            width: workspace.width
            spacing: 12

            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                spacing: 8
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        id: expressionEditor
                        Accessible.name: qsTr("Expression")
                        Accessible.description: qsTr("Selected card expression")
                        Layout.fillWidth: true
                        text: backend.draft_expression
                        textFormat: Text.PlainText
                        font.pointSize: 22
                        font.weight: Font.DemiBold
                        color: foregroundColor
                        elide: Text.ElideRight
                    }
                    ToolButton {
                        text: qsTr("Transform and generate")
                        icon.source: "icons/database.svg"
                        icon.color: workspace.mappingScreen ? accentColor : mutedColor
                        display: AbstractButton.IconOnly
                        Accessible.name: text
                        ToolTip.visible: hovered
                        ToolTip.text: text
                        onClicked: workspace.mappingScreen = true
                    }
                    ToolButton {
                        text: qsTr("Card preview")
                        enabled: backend.draft_generated
                        icon.source: "icons/card.svg"
                        icon.color: workspace.mappingScreen ? mutedColor : accentColor
                        display: AbstractButton.IconOnly
                        Accessible.name: text
                        ToolTip.visible: hovered
                        ToolTip.text: text
                        onClicked: workspace.mappingScreen = false
                    }
                    Label { text: backend.draft_dirty ? qsTr("Unsaved draft") : qsTr("Saved draft"); color: mutedColor; font.pointSize: 12 }
                }
                Flow {
                    visible: workspace.mappingScreen
                    Layout.fillWidth: true
                    Layout.preferredHeight: childrenRect.height
                    spacing: 6
                    ToolButton { text: qsTr("Undo"); icon.source: "icons/undo.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_can_undo; onClicked: backend.undoDraft() }
                    ToolButton { text: qsTr("Redo"); icon.source: "icons/redo.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_can_redo; onClicked: backend.redoDraft() }
                    ToolButton { text: qsTr("Regenerate"); icon.source: "icons/refresh.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_available; onClicked: backend.regenerateDraft() }
                }
            }

            MappingWorkspace {
                visible: workspace.mappingScreen
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.preferredHeight: Math.max(430, implicitHeight)
                backend: workspace.backend
                backgroundColor: workspace.backgroundColor
                surfaceColor: workspace.surfaceColor
                foregroundColor: workspace.foregroundColor
                mutedColor: workspace.mutedColor
                accentColor: workspace.accentColor
                infoColor: workspace.infoColor
                generatedColor: workspace.generatedColor
            }

            Rectangle {
                visible: workspace.mappingScreen
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                implicitHeight: editor.implicitHeight + 30
                radius: 12
                color: surfaceColor
                border.color: Qt.alpha(mutedColor, 0.35)

                ColumnLayout {
                    id: editor
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 16
                    anchors.verticalCenter: parent.verticalCenter
                    RowLayout {
                        Label { text: qsTr("MEANING"); color: mutedColor; font.pointSize: 8; font.letterSpacing: 1.2 }
                        Item { Layout.fillWidth: true }
                        CheckBox { text: qsTr("Lock"); Accessible.name: qsTr("Lock meaning field"); checked: backend.draft_meaning_locked; onToggled: backend.toggleDraftMeaningLock(checked) }
                    }
                    TextArea {
                        id: meaningEditor
                        Accessible.name: qsTr("Meaning")
                        Accessible.description: qsTr("Editable card meaning")
                        Layout.fillWidth: true
                        text: backend.draft_meaning
                        onTextChanged: if (activeFocus && text !== backend.draft_meaning) backend.editDraftMeaning(text)
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        inputMethodHints: Qt.ImhNone
                        background: Rectangle {
                            color: Qt.alpha(surfaceColor, 0)
                            border.width: meaningEditor.activeFocus ? 2 : 0
                            border.color: accentColor
                            radius: 8
                        }
                    }
                    Label { text: backend.draft_provenance; color: mutedColor; font.pointSize: 8 }
                    Label {
                        text: qsTr("EXAMPLES")
                        color: mutedColor
                        visible: backend.mapping_purpose === "japanese_grammar"
                    }
                    TextArea {
                        visible: backend.mapping_purpose === "japanese_grammar"
                        Accessible.name: qsTr("Grammar examples")
                        Layout.fillWidth: true
                        text: backend.draft_examples
                        onTextChanged: if (activeFocus && text !== backend.draft_examples) backend.editDraftExamples(text)
                        wrapMode: TextEdit.Wrap
                        inputMethodHints: Qt.ImhNone
                        color: foregroundColor
                    }
                    Repeater {
                        model: backend.draft_pending_count
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            Label { Layout.fillWidth: true; text: backend.draftChangeValue(index); color: generatedColor; elide: Text.ElideRight }
                            Button { text: qsTr("Accept"); Accessible.name: qsTr("Accept pending change %1").arg(index + 1); onClicked: backend.acceptDraftChange(index) }
                            Button { text: qsTr("Reject"); Accessible.name: qsTr("Reject pending change %1").arg(index + 1); onClicked: backend.rejectDraftChange(index) }
                        }
                    }
                    Label { visible: backend.mapping_purpose === "japanese_vocab"; text: qsTr("KANJI"); color: mutedColor; font.pointSize: 8; font.letterSpacing: 1.2 }
                    TextArea {
                        id: kanjiEditor
                        visible: backend.mapping_purpose === "japanese_vocab"
                        Accessible.name: qsTr("Kanji construction")
                        Layout.fillWidth: true
                        text: backend.draft_kanji
                        onTextChanged: if (activeFocus && text !== backend.draft_kanji) backend.editDraftKanji(text)
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        inputMethodHints: Qt.ImhNone
                        background: Rectangle {
                            color: Qt.alpha(surfaceColor, 0)
                            border.width: kanjiEditor.activeFocus ? 2 : 0
                            border.color: accentColor
                            radius: 8
                        }
                    }
                }
            }

            Rectangle {
                visible: !workspace.mappingScreen
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                implicitHeight: commitPlan.implicitHeight + 30
                radius: 12
                color: surfaceColor
                border.color: Qt.alpha(mutedColor, 0.35)
                ColumnLayout {
                    id: commitPlan
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 16
                    Label {
                        text: backend.commit_preview_ready
                            ? qsTr("PLANNED CHANGES · dry run")
                            : qsTr("PREVIEW REQUIRED BEFORE APPLY")
                        color: backend.commit_preview_ready ? infoColor : mutedColor
                        font.pointSize: 8
                        font.letterSpacing: 1.2
                    }
                    Repeater {
                        model: backend.commit_field_count
                        delegate: Label {
                            required property int index
                            text: backend.commitField(index)
                            color: foregroundColor
                            wrapMode: Text.Wrap
                        }
                    }
                    Label {
                        visible: backend.commit_preview_ready && backend.commit_field_count === 0
                        text: qsTr("No field changes")
                        color: mutedColor
                    }
                    Label {
                        visible: backend.commit_preview_ready
                        text: qsTr("Media: %1 · Model: %2").arg(backend.commit_media_count)
                            .arg(backend.commit_model_changed ? qsTr("change") : qsTr("unchanged"))
                        color: mutedColor
                    }
                    Repeater {
                        model: backend.commit_snapshot_count
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            Label { Layout.fillWidth: true; text: backend.commitSnapshot(index); color: mutedColor }
                            Button { text: qsTr("Restore"); Accessible.name: qsTr("Restore snapshot %1").arg(index + 1); onClicked: backend.restoreSnapshot(index) }
                        }
                    }
                }
            }

            ColumnLayout {
                visible: !workspace.mappingScreen
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                spacing: 12

                RowLayout {
                    Layout.fillWidth: true
                    ComboBox {
                        id: previewTemplate
                        Accessible.name: qsTr("Card preview template")
                        model: [qsTr("Comprehension"), qsTr("Spelling"), qsTr("Production")]
                        currentIndex: workspace.previewTemplateIndex
                        onActivated: workspace.previewTemplateIndex = currentIndex
                    }
                    ToolButton {
                        text: qsTr("Play audio")
                        icon.source: "icons/play.svg"
                        display: AbstractButton.IconOnly
                        Accessible.name: text
                        ToolTip.visible: hovered
                        ToolTip.text: text
                        enabled: backend.previewAudioUrl(0).length > 0
                        onClicked: {
                            previewAudio.source = backend.previewAudioUrl(0)
                            previewAudio.play()
                        }
                    }
                    Item { Layout.fillWidth: true }
                }
                GridLayout {
                    Layout.fillWidth: true
                    columns: workspace.width < 720 ? 1 : 2
                    columnSpacing: 12
                    rowSpacing: 12
                Rectangle {
                    id: previewFront
                    function openZoom() {
                        workspace.zoomHtml = backend.cardPreview(workspace.previewTemplateIndex, false)
                        zoomDialog.open()
                    }
                    Layout.fillWidth: true
                    Layout.preferredHeight: 260
                    activeFocusOnTab: true
                    Accessible.name: qsTr("Open %1 front preview").arg(previewTemplate.currentText)
                    Accessible.description: qsTr("Press Enter or Space to enlarge this card face")
                    Accessible.role: Accessible.Button
                    Accessible.onPressAction: openZoom()
                    Keys.onReturnPressed: openZoom()
                    Keys.onEnterPressed: openZoom()
                    Keys.onSpacePressed: openZoom()
                    radius: 12
                    color: surfaceColor
                    border.width: activeFocus ? 2 : 1
                    border.color: activeFocus ? accentColor : Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 16
                        Label { text: qsTr("%1 · FRONT").arg(previewTemplate.currentText); color: mutedColor; font.pointSize: 8 }
                        Text {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            text: backend.cardPreview(workspace.previewTemplateIndex, false)
                            textFormat: Text.RichText
                            wrapMode: Text.Wrap
                            color: foregroundColor
                            clip: true
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: previewFront.openZoom()
                            }
                        }
                    }
                }
                Rectangle {
                    id: previewBack
                    function openZoom() {
                        workspace.zoomHtml = backend.cardPreview(workspace.previewTemplateIndex, true)
                        zoomDialog.open()
                    }
                    Layout.fillWidth: true
                    Layout.preferredHeight: 260
                    activeFocusOnTab: true
                    Accessible.name: qsTr("Open %1 back preview").arg(previewTemplate.currentText)
                    Accessible.description: qsTr("Press Enter or Space to enlarge this card face")
                    Accessible.role: Accessible.Button
                    Accessible.onPressAction: openZoom()
                    Keys.onReturnPressed: openZoom()
                    Keys.onEnterPressed: openZoom()
                    Keys.onSpacePressed: openZoom()
                    radius: 12
                    color: surfaceColor
                    border.width: activeFocus ? 2 : 1
                    border.color: activeFocus ? accentColor : Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 16
                        Label { text: qsTr("%1 · BACK").arg(previewTemplate.currentText); color: mutedColor; font.pointSize: 8 }
                        Text {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            text: backend.cardPreview(workspace.previewTemplateIndex, true)
                            textFormat: Text.RichText
                            wrapMode: Text.Wrap
                            color: foregroundColor
                            clip: true
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: previewBack.openZoom()
                            }
                        }
                    }
                }
                }
            }

            ColumnLayout {
                visible: !workspace.mappingScreen && backend.draft_generated
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Dry run checks the proposed changes without writing to Anki. Apply writes the reviewed changes and creates a recovery snapshot.")
                    color: mutedColor
                    wrapMode: Text.Wrap
                }
                RowLayout {
                    Button {
                        text: qsTr("Dry run")
                        Accessible.name: text
                        onClicked: backend.previewCommit()
                    }
                    Button {
                        text: qsTr("Apply to Anki")
                        Accessible.name: text
                        highlighted: true
                        enabled: backend.commit_preview_ready
                        onClicked: backend.applyCommit()
                    }
                }
            }

            Label {
                visible: !workspace.mappingScreen
                Layout.leftMargin: 16
                Layout.bottomMargin: 16
                text: backend.draft_issues.length > 0 ? backend.draft_issues : qsTr("No blocking issues")
                color: backend.draft_issues.length > 0 ? dangerColor : successColor
            }
        }
    }
}
