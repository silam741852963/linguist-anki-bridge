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
        anchors.margins: 14
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("Review queue"); color: foregroundColor; font.pixelSize: 18; font.weight: Font.DemiBold }
            Item { Layout.fillWidth: true }
            Label { text: qsTr("24 cards"); color: mutedColor }
        }
        ComboBox {
            Layout.fillWidth: true
            model: [qsTr("Needs review"), qsTr("Ready"), qsTr("Failed"), qsTr("All cards")]
        }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 6
            currentIndex: 0
            model: ListModel {
                ListElement { expression: "食べる"; reading: "たべる · vocabulary"; issue: "Ready" }
                ListElement { expression: "ということだ"; reading: "grammar · OCR"; issue: "Check OCR" }
                ListElement { expression: "見落とす"; reading: "みおとす · vocabulary"; issue: "Image choice" }
                ListElement { expression: "to reckon"; reading: "English · vocabulary"; issue: "Ready" }
            }
            delegate: Rectangle {
                required property int index
                required property string expression
                required property string reading
                required property string issue
                width: ListView.view.width
                height: 78
                radius: 8
                color: ListView.isCurrentItem ? Qt.alpha(accentColor, 0.22) : "transparent"
                border.color: ListView.isCurrentItem ? accentColor : Qt.alpha(mutedColor, 0.35)
                focus: ListView.isCurrentItem

                MouseArea { anchors.fill: parent; onClicked: list.currentIndex = index }
                Column {
                    anchors.fill: parent
                    anchors.margins: 11
                    spacing: 4
                    Label { text: expression; color: foregroundColor; font.pixelSize: 17 }
                    Label { text: reading; color: mutedColor }
                    Label { text: issue; color: issue === "Ready" ? "#a6e3a1" : accentColor; font.pixelSize: 11 }
                }
            }
        }
    }
}
