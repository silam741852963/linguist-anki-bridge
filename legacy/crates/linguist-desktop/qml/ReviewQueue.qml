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
    required property color successColor
    required property color warningColor
    required property color dangerColor
    property bool searchOpen: false
    function focusSearch() {
        searchOpen = true
        Qt.callLater(function() { cardSearch.forceActiveFocus(); cardSearch.selectAll() })
    }
    color: backgroundColor

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("Review queue"); color: foregroundColor; font.pointSize: 13; font.weight: Font.DemiBold }
            Item { Layout.fillWidth: true }
            Label {
                text: backend.review_total > 0
                    ? qsTr("%1 cards").arg(backend.review_total)
                    : qsTr("0 cards")
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
            placeholderText: qsTr("Search every card in this deck")
            Keys.onReturnPressed: function(event) {
                backend.searchReview(text, event.modifiers & Qt.ShiftModifier ? -1 : 1)
                event.accepted = true
            }
            Keys.onEnterPressed: function(event) {
                backend.searchReview(text, event.modifiers & Qt.ShiftModifier ? -1 : 1)
                event.accepted = true
            }
            Keys.onEscapePressed: queue.searchOpen = false
        }
        Label {
            visible: queue.searchOpen && cardSearch.text.length > 0
            Layout.fillWidth: true
            text: backend.review_search_count > 0
                ? qsTr("Match %1 of %2 · Enter next · Shift Enter previous")
                    .arg(backend.review_search_match).arg(backend.review_search_count)
                : qsTr("Enter to search all cards")
            color: mutedColor
            font.pointSize: 8
            elide: Text.ElideRight
        }
        ComboBox {
            id: deckPicker
            Accessible.name: qsTr("Active Anki deck")
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
                spacing: 8
                model: backend.review_row_count
                currentIndex: backend.selected_review_index
                focus: true
                activeFocusOnTab: true
                Accessible.name: qsTr("Review queue")
                Accessible.description: qsTr("Use arrow keys to select a card")
                Accessible.role: Accessible.List
                keyNavigationEnabled: true
                highlightFollowsCurrentItem: true
                reuseItems: true
                cacheBuffer: height * 2
                boundsBehavior: Flickable.StopAtBounds
                flickDeceleration: 6500
                maximumFlickVelocity: 9000
                function animateToCurrent() {
                    if (currentIndex < 0 || count === 0) return
                    let target = Math.max(0, Math.min(contentHeight - height,
                        currentIndex * 84 - (height - 78) / 2))
                    searchScroll.stop()
                    searchScroll.from = contentY
                    searchScroll.to = target
                    searchScroll.start()
                }
                NumberAnimation {
                    id: searchScroll
                    target: list
                    property: "contentY"
                    duration: 280
                    easing.type: Easing.OutCubic
                }
                Connections {
                    target: backend
                    function onReview_navigation_serialChanged() {
                        Qt.callLater(function() { list.animateToCurrent() })
                    }
                }
                onCurrentIndexChanged: {
                    if (currentIndex >= 0)
                        backend.selectReviewIndex(currentIndex)
                }
                Keys.onReturnPressed: queue.cardSelected()
                Keys.onEnterPressed: queue.cardSelected()
                delegate: Rectangle {
                    required property int index
                    readonly property int contentRevision: backend.review_content_serial
                    readonly property string rowState: contentRevision >= 0 ? backend.reviewState(index) : ""
                    width: ListView.view.width
                    height: 78
                    radius: 12
                    color: ListView.isCurrentItem ? Qt.alpha(accentColor, 0.22) : Qt.alpha(backgroundColor, 0)
                    border.width: ListView.isCurrentItem && ListView.view.activeFocus ? 2 : 1
                    border.color: ListView.isCurrentItem ? accentColor : Qt.alpha(mutedColor, 0.35)
                    focus: ListView.isCurrentItem
                    Accessible.name: qsTr("Review item %1: %2").arg(index + 1)
                        .arg(contentRevision >= 0 ? backend.reviewExpression(index) : "")
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
                        anchors.margins: 12
                        spacing: 4
                        Label { text: contentRevision >= 0 ? backend.reviewExpression(index) : ""; color: foregroundColor; font.pointSize: 12 }
                        Label { text: contentRevision >= 0 ? backend.reviewDetail(index) : ""; color: mutedColor }
                        Label {
                            text: rowState
                            color: rowState === "Ready" ? successColor
                                : rowState === "Failed" ? dangerColor : warningColor
                            font.pointSize: 8
                        }
                    }
                }
            }
        }
        RowLayout {
            Layout.fillWidth: true
            visible: backend.review_page_count > 0
            ToolButton {
                text: qsTr("Previous page")
                icon.source: "icons/chevron-left.svg"
                icon.color: foregroundColor
                display: AbstractButton.IconOnly
                enabled: backend.review_page > 1
                Accessible.name: text
                ToolTip.visible: hovered
                ToolTip.text: text
                onClicked: backend.previousReviewPage()
            }
            Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                color: mutedColor
                text: qsTr("%1–%2 of %3 · page %4/%5")
                    .arg(backend.review_page_start).arg(backend.review_page_end)
                    .arg(backend.review_total).arg(backend.review_page)
                    .arg(backend.review_page_count)
            }
            ToolButton {
                text: qsTr("Next page")
                icon.source: "icons/chevron-right.svg"
                icon.color: foregroundColor
                display: AbstractButton.IconOnly
                enabled: backend.review_page < backend.review_page_count
                Accessible.name: text
                ToolTip.visible: hovered
                ToolTip.text: text
                onClicked: backend.nextReviewPage()
            }
        }
    }
}
