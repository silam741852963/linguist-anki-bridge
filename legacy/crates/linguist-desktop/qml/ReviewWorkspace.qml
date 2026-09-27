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
    property int previewTemplateIndex: 0
    property string zoomHtml: ""
    color: backgroundColor

    Shortcut { sequence: StandardKey.Undo; enabled: backend.draft_can_undo; onActivated: backend.undoDraft() }
    Shortcut { sequence: StandardKey.Redo; enabled: backend.draft_can_redo; onActivated: backend.redoDraft() }
    Shortcut { sequence: "Ctrl+G"; enabled: backend.draft_available; onActivated: backend.regenerateDraft() }
    Shortcut { sequence: "Ctrl+Shift+Return"; enabled: backend.commit_preview_ready; onActivated: backend.applyCommit() }

    MediaPlayer {
        id: previewAudio
        audioOutput: AudioOutput { volume: 1.0 }
    }

    Dialog {
        id: zoomDialog
        modal: true
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
            spacing: 14

            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                spacing: 6
                RowLayout {
                    Layout.fillWidth: true
                    TextField {
                        id: expressionEditor
                        Accessible.name: qsTr("Expression")
                        Accessible.description: qsTr("Edit the card expression")
                        inputMethodHints: Qt.ImhNone
                        Layout.fillWidth: true
                        text: backend.draft_expression
                        placeholderText: qsTr("Select a card")
                        font.pointSize: 22
                        onTextChanged: if (activeFocus && text !== backend.draft_expression) backend.editDraftExpression(text)
                    }
                    Label { text: backend.draft_dirty ? qsTr("Unsaved draft") : qsTr("Saved draft"); color: mutedColor; font.pointSize: 12 }
                }
                Flow {
                    Layout.fillWidth: true
                    Layout.preferredHeight: childrenRect.height
                    spacing: 6
                    ToolButton { text: qsTr("Undo"); icon.source: "icons/undo.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_can_undo; onClicked: backend.undoDraft() }
                    ToolButton { text: qsTr("Redo"); icon.source: "icons/redo.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_can_redo; onClicked: backend.redoDraft() }
                    ToolButton { text: qsTr("Regenerate"); icon.source: "icons/refresh.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_available; onClicked: backend.regenerateDraft() }
                    ToolButton { text: qsTr("Preview changes"); icon.source: "icons/preview.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.draft_available; onClicked: backend.previewCommit() }
                    Button {
                        text: qsTr("Apply to Anki")
                        Accessible.name: text
                        Accessible.description: qsTr("Apply the previewed changes to Anki")
                        highlighted: true
                        enabled: backend.commit_preview_ready
                        onClicked: backend.applyCommit()
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                implicitHeight: editor.implicitHeight + 30
                radius: 10
                color: surfaceColor
                border.color: Qt.alpha(mutedColor, 0.35)

                ColumnLayout {
                    id: editor
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 15
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
                            color: "transparent"
                            border.width: meaningEditor.activeFocus ? 2 : 0
                            border.color: accentColor
                            radius: 4
                        }
                    }
                    Label { text: backend.draft_provenance; color: mutedColor; font.pointSize: 8 }
                    Repeater {
                        model: backend.draft_pending_count
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            Label { Layout.fillWidth: true; text: backend.draftChangeValue(index); color: accentColor; elide: Text.ElideRight }
                            Button { text: qsTr("Accept"); Accessible.name: qsTr("Accept pending change %1").arg(index + 1); onClicked: backend.acceptDraftChange(index) }
                            Button { text: qsTr("Reject"); Accessible.name: qsTr("Reject pending change %1").arg(index + 1); onClicked: backend.rejectDraftChange(index) }
                        }
                    }
                    Label { text: qsTr("KANJI"); color: mutedColor; font.pointSize: 8; font.letterSpacing: 1.2 }
                    TextArea {
                        id: kanjiEditor
                        Accessible.name: qsTr("Kanji construction")
                        Layout.fillWidth: true
                        text: backend.draft_kanji
                        onTextChanged: if (activeFocus && text !== backend.draft_kanji) backend.editDraftKanji(text)
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        inputMethodHints: Qt.ImhNone
                        background: Rectangle {
                            color: "transparent"
                            border.width: kanjiEditor.activeFocus ? 2 : 0
                            border.color: accentColor
                            radius: 4
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                implicitHeight: commitPlan.implicitHeight + 30
                radius: 10
                color: surfaceColor
                border.color: Qt.alpha(mutedColor, 0.35)
                ColumnLayout {
                    id: commitPlan
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 15
                    Label {
                        text: backend.commit_preview_ready
                            ? qsTr("PLANNED CHANGES · dry run")
                            : qsTr("PREVIEW REQUIRED BEFORE APPLY")
                        color: backend.commit_preview_ready ? accentColor : mutedColor
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
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                spacing: 10

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
                    radius: 10
                    color: surfaceColor
                    border.width: activeFocus ? 2 : 1
                    border.color: activeFocus ? accentColor : Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 15
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
                    radius: 10
                    color: surfaceColor
                    border.width: activeFocus ? 2 : 1
                    border.color: activeFocus ? accentColor : Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 15
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

            Label {
                Layout.leftMargin: 22
                Layout.bottomMargin: 22
                text: backend.draft_issues.length > 0 ? backend.draft_issues : qsTr("No blocking issues")
                color: mutedColor
            }
        }
    }
}
