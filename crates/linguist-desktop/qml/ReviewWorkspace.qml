import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: workspace
    required property var backend
    required property color backgroundColor
    required property color surfaceColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    signal applyRequested()
    color: backgroundColor

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth

        ColumnLayout {
            width: workspace.width
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 22
                TextField {
                    id: expressionEditor
                    Layout.preferredWidth: 260
                    text: backend.draftExpression
                    placeholderText: qsTr("Select a card")
                    font.pixelSize: 30
                    onTextChanged: if (activeFocus && text !== backend.draftExpression) backend.editDraftExpression(text)
                }
                Label { text: backend.draftDirty ? qsTr("Unsaved draft") : qsTr("Saved draft"); color: mutedColor; font.pixelSize: 17 }
                Item { Layout.fillWidth: true }
                Button { text: qsTr("Undo"); onClicked: backend.undoDraft() }
                Button { text: qsTr("Redo"); onClicked: backend.redoDraft() }
                Button { text: qsTr("Regenerate"); onClicked: backend.regenerateDraft() }
                Button { text: qsTr("Apply to Anki"); highlighted: true; onClicked: workspace.applyRequested() }
            }

            TabBar {
                id: tabs
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                TabButton { text: qsTr("Fields") }
                TabButton { text: qsTr("Card preview") }
                TabButton { text: qsTr("Sources") }
                TabButton { text: qsTr("History") }
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
                    Label { text: qsTr("MEANING"); color: mutedColor; font.pixelSize: 11; font.letterSpacing: 1.2 }
                    TextArea {
                        id: meaningEditor
                        Layout.fillWidth: true
                        text: backend.draftMeaning
                        onTextChanged: if (activeFocus && text !== backend.draftMeaning) backend.editDraftMeaning(text)
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        background: Rectangle { color: "transparent" }
                    }
                    Label { text: backend.draftProvenance; color: mutedColor; font.pixelSize: 11 }
                    Label { text: qsTr("KANJI"); color: mutedColor; font.pixelSize: 11; font.letterSpacing: 1.2 }
                    TextArea {
                        id: kanjiEditor
                        Layout.fillWidth: true
                        text: backend.draftKanji
                        onTextChanged: if (activeFocus && text !== backend.draftKanji) backend.editDraftKanji(text)
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        background: Rectangle { color: "transparent" }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 22
                Layout.rightMargin: 22
                spacing: 14

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 260
                    radius: 10
                    color: surfaceColor
                    border.color: Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 15
                        Label { text: qsTr("COMPREHENSION · FRONT"); color: mutedColor; font.pixelSize: 11 }
                        Item { Layout.fillHeight: true }
                        Label { Layout.alignment: Qt.AlignHCenter; text: backend.draftExpression; color: foregroundColor; font.pixelSize: 40 }
                        Label { Layout.alignment: Qt.AlignHCenter; text: backend.draftAudio; color: accentColor }
                        Item { Layout.fillHeight: true }
                    }
                }
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 260
                    radius: 10
                    color: surfaceColor
                    border.color: Qt.alpha(mutedColor, 0.35)
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 15
                        Label { text: qsTr("COMPREHENSION · BACK"); color: mutedColor; font.pixelSize: 11 }
                        Item { Layout.fillHeight: true }
                        Label { Layout.alignment: Qt.AlignHCenter; text: backend.draftKanji; color: foregroundColor; font.pixelSize: 27 }
                        Label { Layout.alignment: Qt.AlignHCenter; text: backend.draftImages; color: foregroundColor; font.pixelSize: 19 }
                        Item { Layout.fillHeight: true }
                    }
                }
            }

            Label {
                Layout.leftMargin: 22
                Layout.bottomMargin: 22
                text: backend.draftIssues.length > 0 ? backend.draftIssues : qsTr("No blocking issues")
                color: mutedColor
            }
        }
    }
}
