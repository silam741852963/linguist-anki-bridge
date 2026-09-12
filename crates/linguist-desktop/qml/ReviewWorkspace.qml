import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: workspace
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
                Label { text: "食べる"; color: foregroundColor; font.pixelSize: 30; font.weight: Font.DemiBold }
                Label { text: "たべる"; color: mutedColor; font.pixelSize: 17 }
                Item { Layout.fillWidth: true }
                Button { text: qsTr("Regenerate") }
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
                        Layout.fillWidth: true
                        text: qsTr("to eat; to consume\n\nUsed for ordinary eating and consuming food.")
                        color: foregroundColor
                        wrapMode: TextEdit.Wrap
                        background: Rectangle { color: "transparent" }
                    }
                    Label { text: qsTr("Dictionary · Jisho    Generated nuance · Ollama"); color: mutedColor; font.pixelSize: 11 }
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
                        Label { Layout.alignment: Qt.AlignHCenter; text: "食べる"; color: foregroundColor; font.pixelSize: 40 }
                        Label { Layout.alignment: Qt.AlignHCenter; text: qsTr("▶  Play audio"); color: accentColor }
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
                        Label { Layout.alignment: Qt.AlignHCenter; text: "たべる"; color: foregroundColor; font.pixelSize: 27 }
                        Label { Layout.alignment: Qt.AlignHCenter; text: qsTr("to eat; to consume"); color: foregroundColor; font.pixelSize: 19 }
                        Item { Layout.fillHeight: true }
                    }
                }
            }

            Label {
                Layout.leftMargin: 22
                Layout.bottomMargin: 22
                text: qsTr("No blocking issues · 2 generated values · 1 dictionary source")
                color: mutedColor
            }
        }
    }
}
