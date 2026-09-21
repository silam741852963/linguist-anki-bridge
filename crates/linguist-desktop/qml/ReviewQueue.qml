import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: queue
    signal cardSelected()
    required property var backend
    required property color backgroundColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    property bool searchOpen: false
    function focusSearch() {
        searchOpen = true
        Qt.callLater(function() { cardSearch.forceActiveFocus(); cardSearch.selectAll() })
    }
    color: backgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 14
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("Review queue"); color: foregroundColor; font.pointSize: 13; font.weight: Font.DemiBold }
            Item { Layout.fillWidth: true }
            Label {
                text: qsTr("%1 cards").arg(backend.review_row_count)
                color: mutedColor
            }
            ToolButton {
                text: qsTr("Find card or deck")
                icon.source: "icons/search.svg"
                icon.color: foregroundColor
                display: AbstractButton.IconOnly
                Accessible.name: text
                ToolTip.visible: hovered
                ToolTip.text: text + qsTr(" · Ctrl K")
                onClicked: queue.searchOpen ? queue.searchOpen = false : queue.focusSearch()
            }
        }
        TextField {
            id: cardSearch
            visible: queue.searchOpen
            Layout.fillWidth: true
            Accessible.name: qsTr("Find card or deck")
            placeholderText: qsTr("Find card or deck · Enter")
            onAccepted: { backend.searchReview(text); queue.cardSelected() }
            Keys.onEscapePressed: queue.searchOpen = false
        }
        ComboBox {
            id: deckPicker
            Layout.fillWidth: true
            model: backend.deck_count
            currentIndex: backend.selected_deck_index
            enabled: backend.deck_count > 0
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
            currentIndex: backend.review_row_count > 0 ? 1 : 0

            Item {
                ColumnLayout {
                    anchors.centerIn: parent
                    width: Math.min(parent.width - 30, 250)
                    spacing: 8
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        text: backend.queue_state
                        color: foregroundColor
                        font.pointSize: 12
                        font.weight: Font.DemiBold
                    }
                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                        text: backend.queue_message
                        color: mutedColor
                    }
                }
            }

            ListView {
                id: list
                clip: true
                spacing: 6
                model: backend.review_row_count
                currentIndex: backend.selected_review_index
                focus: true
                Accessible.name: qsTr("Review queue")
                Accessible.description: qsTr("Use arrow keys to select a card")
                Accessible.role: Accessible.List
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                onCurrentIndexChanged: {
                    if (currentIndex >= 0)
                        backend.selectReviewIndex(currentIndex)
                }
                Keys.onReturnPressed: queue.cardSelected()
                Keys.onEnterPressed: queue.cardSelected()
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
                    Accessible.description: qsTr("%1. State: %2").arg(backend.reviewDetail(index)).arg(rowState)
                    Accessible.role: Accessible.ListItem
                    Accessible.selected: ListView.isCurrentItem

                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            if (list.currentIndex === index)
                                backend.selectReviewIndex(index)
                            else
                                list.currentIndex = index
                            queue.cardSelected()
                        }
                    }
                    Column {
                        anchors.fill: parent
                        anchors.margins: 11
                        spacing: 4
                        Label { text: backend.reviewExpression(index); color: foregroundColor; font.pointSize: 12 }
                        Label { text: backend.reviewDetail(index); color: mutedColor }
                        Label {
                            text: rowState
                            color: rowState === "Ready" ? accentColor : mutedColor
                            font.pointSize: 8
                        }
                    }
                }
            }
        }
    }
}
