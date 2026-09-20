import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    signal reviewRequested()
    signal addRequested()
    signal batchRequested()
    signal historyRequested()
    signal settingsRequested()
    required property color backgroundColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    color: backgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 7

        Label { text: qsTr("WORKSPACE"); color: mutedColor; font.pointSize: 8; font.letterSpacing: 1.3 }
        Repeater {
            model: [qsTr("Review"), qsTr("Add cards"), qsTr("Batch jobs"), qsTr("History")]
            delegate: Button {
                required property string modelData
                required property int index
                Layout.fillWidth: true
                text: modelData
                Accessible.name: modelData
                flat: true
                highlighted: index === 0
                onClicked: {
                    if (index === 0) reviewRequested()
                    else if (index === 1) addRequested()
                    else if (index === 2) batchRequested()
                    else historyRequested()
                }
            }
        }
        Label {
            Layout.topMargin: 18
            text: qsTr("DECKS")
            color: mutedColor
            font.pointSize: 8
            font.letterSpacing: 1.3
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Choose a deck in the review queue")
            color: mutedColor
            wrapMode: Text.Wrap
        }
        Item { Layout.fillHeight: true }
        Button { Layout.fillWidth: true; text: qsTr("Settings"); flat: true; onClicked: settingsRequested() }
    }
}
