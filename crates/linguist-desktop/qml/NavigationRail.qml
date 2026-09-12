import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    required property color backgroundColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    color: backgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 7

        Label { text: qsTr("WORKSPACE"); color: mutedColor; font.pixelSize: 11; font.letterSpacing: 1.3 }
        Repeater {
            model: [qsTr("Review"), qsTr("Add cards"), qsTr("Batch jobs"), qsTr("History")]
            delegate: Button {
                required property string modelData
                Layout.fillWidth: true
                text: modelData
                flat: true
                highlighted: index === 0
            }
        }
        Label {
            Layout.topMargin: 18
            text: qsTr("DECKS")
            color: mutedColor
            font.pixelSize: 11
            font.letterSpacing: 1.3
        }
        Repeater {
            model: [qsTr("Japanese Vocabulary  18"), qsTr("Japanese Grammar  4"), qsTr("English Vocabulary  2")]
            delegate: Button {
                required property string modelData
                Layout.fillWidth: true
                text: modelData
                flat: true
            }
        }
        Item { Layout.fillHeight: true }
        Button { Layout.fillWidth: true; text: qsTr("Settings"); flat: true }
    }
}
