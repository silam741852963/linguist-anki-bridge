import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    required property var backend
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
            Label {
                text: qsTr("%1 cards").arg(backend.reviewRowCount)
                color: mutedColor
            }
        }
        ComboBox {
            id: deckPicker
            Layout.fillWidth: true
            model: backend.deckCount
            currentIndex: backend.selectedDeckIndex
            enabled: backend.deckCount > 0
            delegate: ItemDelegate {
                required property int index
                width: deckPicker.width
                text: backend.deckName(index)
                highlighted: deckPicker.highlightedIndex === index
                onClicked: {
                    deckPicker.currentIndex = index
                    deckPicker.activated(index)
                    deckPicker.popup.close()
                }
            }
            contentItem: Label {
                leftPadding: 10
                rightPadding: 10
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
                color: deckPicker.enabled ? foregroundColor : mutedColor
                text: deckPicker.currentIndex >= 0
                    ? backend.deckName(deckPicker.currentIndex)
                    : qsTr("No decks")
            }
            onActivated: backend.selectDeckIndex(currentIndex)
        }
        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: backend.reviewRowCount > 0 ? 1 : 0

            Item {
                ColumnLayout {
                    anchors.centerIn: parent
                    width: Math.min(parent.width - 30, 250)
                    spacing: 8
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        text: backend.queueState
                        color: foregroundColor
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                    }
                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                        text: backend.queueMessage
                        color: mutedColor
                    }
                }
            }

            ListView {
                id: list
                clip: true
                spacing: 6
                model: backend.reviewRowCount
                currentIndex: backend.selectedReviewIndex
                focus: true
                Accessible.name: qsTr("Review queue")
                Accessible.description: qsTr("Use arrow keys to select a card")
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                onCurrentIndexChanged: {
                    if (currentIndex >= 0)
                        backend.selectReviewIndex(currentIndex)
                }
                delegate: Rectangle {
                    required property int index
                    readonly property string rowState: backend.reviewState(index)
                    width: ListView.view.width
                    height: 78
                    radius: 8
                    color: ListView.isCurrentItem ? Qt.alpha(accentColor, 0.22) : "transparent"
                    border.color: ListView.isCurrentItem ? accentColor : Qt.alpha(mutedColor, 0.35)
                    focus: ListView.isCurrentItem
                    Accessible.name: qsTr("Review item %1: %2").arg(index + 1).arg(backend.reviewExpression(index))

                    MouseArea {
                        anchors.fill: parent
                        onClicked: list.currentIndex = index
                    }
                    Column {
                        anchors.fill: parent
                        anchors.margins: 11
                        spacing: 4
                        Label { text: backend.reviewExpression(index); color: foregroundColor; font.pixelSize: 17 }
                        Label { text: backend.reviewDetail(index); color: mutedColor }
                        Label {
                            text: rowState
                            color: rowState === "Ready" ? "#a6e3a1" : accentColor
                            font.pixelSize: 11
                        }
                    }
                }
            }
        }
    }
}
