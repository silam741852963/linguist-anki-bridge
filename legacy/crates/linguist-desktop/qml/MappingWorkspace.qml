import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: mapping
    required property var backend
    required property color backgroundColor
    required property color surfaceColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    required property color infoColor
    required property color generatedColor
    property bool showPurposePicker: true
    property int pendingSource: -1
    implicitHeight: mappingContent.implicitHeight + 32
    radius: 12
    color: surfaceColor
    border.color: Qt.alpha(mutedColor, 0.38)

    function purposeIndex(key) {
        for (let index = 0; index < purposePicker.model.length; index++)
            if (purposePicker.model[index].key === key) return index
        return 0
    }

    function sourceName(index) {
        let revision = backend.mapping_render_serial
        return revision >= 0 ? backend.mappingSourceName(index) : ""
    }

    function targetLabel(index) {
        let revision = backend.mapping_render_serial
        return revision >= 0 ? backend.mappingTargetLabel(index) : ""
    }

    onPendingSourceChanged: connectionCanvas.requestPaint()

    ColumnLayout {
        id: mappingContent
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    text: qsTr("FIELD MAPPING")
                    color: generatedColor
                    font.pointSize: 9
                    font.weight: Font.DemiBold
                    font.letterSpacing: 1.1
                }
                Label {
                    Layout.fillWidth: true
                    text: backend.mapping_deck.length > 0
                        ? (backend.mapping_tags.length > 0
                            ? qsTr("%1 · %2 · %3").arg(backend.mapping_deck).arg(backend.mapping_model).arg(backend.mapping_tags)
                            : qsTr("%1 · %2").arg(backend.mapping_deck).arg(backend.mapping_model))
                        : qsTr("Select a card to inspect its fields")
                    color: mutedColor
                    elide: Text.ElideRight
                }
            }
            ComboBox {
                id: purposePicker
                visible: mapping.showPurposePicker
                Accessible.name: qsTr("Purpose for reusable mapping")
                textRole: "label"
                valueRole: "key"
                model: [
                    { label: qsTr("Choose purpose"), key: "" },
                    { label: qsTr("Japanese vocabulary"), key: "japanese_vocab" },
                    { label: qsTr("Japanese grammar"), key: "japanese_grammar" },
                    { label: qsTr("English vocabulary"), key: "english_vocab" },
                    { label: qsTr("English grammar"), key: "english_grammar" },
                    { label: qsTr("Taiwanese vocabulary"), key: "taiwanese_vocab" },
                    { label: qsTr("Taiwanese grammar"), key: "taiwanese_grammar" },
                    { label: qsTr("German vocabulary"), key: "german_vocab" },
                    { label: qsTr("German grammar"), key: "german_grammar" }
                ]
                currentIndex: mapping.purposeIndex(backend.mapping_purpose)
                onActivated: if (currentValue.length > 0) backend.setMappingPurpose(currentValue)
            }
            ToolButton {
                text: qsTr("Suggest mapping with Ollama")
                icon.source: "icons/brain-circuit.svg"
                icon.color: backend.mapping_busy ? mutedColor : accentColor
                display: AbstractButton.IconOnly
                enabled: backend.mapping_source_count > 0 && !backend.mapping_busy
                Accessible.name: text
                ToolTip.visible: hovered
                ToolTip.text: text
                onClicked: backend.suggestFieldMapping()
            }
            BusyIndicator {
                visible: backend.mapping_busy
                running: visible
                Layout.preferredWidth: 28
                Layout.preferredHeight: 28
            }
            Button {
                text: backend.mapping_dirty ? qsTr("Save mapping*") : qsTr("Save mapping")
                Accessible.name: text
                highlighted: backend.mapping_dirty
                enabled: backend.mapping_source_count > 0
                onClicked: backend.saveFieldMapping()
            }
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("Choose a source point, then a schema point. Connections are used by generation and can be saved for every card with this purpose.")
            color: mutedColor
            wrapMode: Text.Wrap
        }

        Item {
            id: board
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(sourceColumn.implicitHeight, targetColumn.implicitHeight)

            RowLayout {
                anchors.fill: parent
                spacing: 34

                ColumnLayout {
                    id: sourceColumn
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    spacing: 8
                    Label { text: qsTr("CURRENT ANKI FIELDS"); color: infoColor; font.weight: Font.DemiBold }
                    Repeater {
                        id: sourceRepeater
                        model: backend.mapping_source_count
                        delegate: Rectangle {
                            id: sourceCard
                            required property int index
                            Layout.fillWidth: true
                            readonly property string renderedHtml: backend.mapping_render_serial >= 0
                                ? backend.mappingSourceHtml(index) : ""
                            readonly property string imageSource: backend.mapping_render_serial >= 0
                                ? backend.mappingSourceImage(index) : ""
                            readonly property bool containsImage: imageSource.length > 0
                            implicitHeight: Math.max(containsImage ? 286 : 138, sourceContent.implicitHeight + 20)
                            radius: 9
                            color: backgroundColor
                            border.width: mapping.pendingSource === index ? 2 : 1
                            border.color: mapping.pendingSource === index ? accentColor : Qt.alpha(mutedColor, 0.34)
                            ColumnLayout {
                                id: sourceContent
                                anchors.left: parent.left
                                anchors.right: sourcePoint.left
                                anchors.leftMargin: 12
                                anchors.rightMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 4
                                Label {
                                    Layout.fillWidth: true
                                    text: mapping.sourceName(index)
                                    color: foregroundColor
                                    font.weight: Font.DemiBold
                                    elide: Text.ElideRight
                                }
                                Image {
                                    visible: sourceCard.containsImage
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: visible ? 190 : 0
                                    source: sourceCard.imageSource
                                    fillMode: Image.PreserveAspectFit
                                    asynchronous: true
                                    cache: true
                                }
                                Text {
                                    id: fieldRenderer
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: sourceCard.containsImage ? 52 : 92
                                    readonly property int renderRevision: backend.mapping_render_serial
                                    text: renderRevision >= 0 ? sourceCard.renderedHtml : ""
                                    textFormat: Text.RichText
                                    color: foregroundColor
                                    wrapMode: Text.Wrap
                                    clip: true
                                    Accessible.name: qsTr("Rendered contents of %1").arg(mapping.sourceName(index))
                                }
                            }
                            RoundButton {
                                id: sourcePoint
                                anchors.right: parent.right
                                anchors.rightMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                width: 22
                                height: 22
                                text: ""
                                Accessible.name: qsTr("Connect source field %1").arg(mapping.sourceName(index))
                                ToolTip.visible: hovered
                                ToolTip.text: Accessible.name
                                background: Rectangle {
                                    radius: width / 2
                                    color: mapping.pendingSource === index ? accentColor : Qt.alpha(accentColor, 0.28)
                                    border.color: accentColor
                                }
                                onClicked: mapping.pendingSource = index
                            }
                        }
                    }
                }

                ColumnLayout {
                    id: targetColumn
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    spacing: 8
                    Label { text: qsTr("NEW CARD SCHEMA"); color: generatedColor; font.weight: Font.DemiBold }
                    Repeater {
                        id: targetRepeater
                        model: backend.mapping_target_count
                        delegate: Rectangle {
                            id: targetCard
                            required property int index
                            readonly property int sourceIndex: backend.mapping_render_serial >= 0
                                ? backend.mappedSourceIndex(index) : -1
                            readonly property string targetHint: backend.mapping_message.length >= 0
                                ? backend.mappingTargetHint(index) : ""
                            Layout.fillWidth: true
                            readonly property string renderedHtml: backend.mapping_render_serial >= 0 ? backend.mappingTargetHtml(index) : ""
                            readonly property string imageSource: backend.mapping_render_serial >= 0 ? backend.mappingTargetImage(index) : ""
                            readonly property var sourceCard: sourceIndex >= 0 ? sourceRepeater.itemAt(sourceIndex) : null
                            implicitHeight: Math.max(imageSource.length > 0 ? 286 : 138,
                                sourceCard ? sourceCard.height : 0, targetContent.implicitHeight + 24)
                            radius: 9
                            color: backgroundColor
                            border.width: sourceIndex >= 0 ? 2 : 1
                            border.color: sourceIndex >= 0 ? accentColor : Qt.alpha(mutedColor, 0.34)
                            RoundButton {
                                id: targetPoint
                                anchors.left: parent.left
                                anchors.leftMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                width: 22
                                height: 22
                                text: ""
                                Accessible.name: mapping.pendingSource >= 0
                                    ? qsTr("Connect selected source to %1").arg(mapping.targetLabel(index))
                                    : qsTr("Schema field %1 connection point").arg(mapping.targetLabel(index))
                                ToolTip.visible: hovered
                                ToolTip.text: Accessible.name
                                background: Rectangle {
                                    radius: width / 2
                                    color: targetCard.sourceIndex >= 0 ? accentColor : Qt.alpha(accentColor, 0.28)
                                    border.color: accentColor
                                }
                                onClicked: {
                                    if (mapping.pendingSource >= 0) {
                                        backend.connectMappingField(mapping.pendingSource, index)
                                        mapping.pendingSource = -1
                                        connectionCanvas.requestPaint()
                                    }
                                }
                            }
                            ColumnLayout {
                                id: targetContent
                                anchors.left: targetPoint.right
                                anchors.right: clearButton.left
                                anchors.leftMargin: 9
                                anchors.rightMargin: 6
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 3
                                Label { Layout.fillWidth: true; text: mapping.targetLabel(index); color: foregroundColor; font.weight: Font.DemiBold }
                                Image {
                                    visible: targetCard.imageSource.length > 0
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: visible ? 190 : 0
                                    source: targetCard.imageSource
                                    fillMode: Image.PreserveAspectFit
                                    asynchronous: true
                                }
                                Text {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: targetCard.imageSource.length > 0 ? 52 : 92
                                    text: targetCard.renderedHtml.length > 0 ? targetCard.renderedHtml : qsTr("Generate to see the target value")
                                    textFormat: Text.RichText
                                    wrapMode: Text.Wrap
                                    color: foregroundColor
                                    clip: true
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: targetCard.sourceIndex >= 0
                                        ? qsTr("From %1").arg(mapping.sourceName(targetCard.sourceIndex))
                                        : qsTr("Empty · connect a source field")
                                    color: targetCard.sourceIndex >= 0 ? accentColor : mutedColor
                                    elide: Text.ElideRight
                                }
                                Label {
                                    visible: targetCard.targetHint.length > 0
                                    Layout.fillWidth: true
                                    text: targetCard.targetHint
                                    color: mutedColor
                                    font.pointSize: 8
                                    elide: Text.ElideRight
                                    ToolTip.visible: truncated && hoverHandler.hovered
                                    ToolTip.text: text
                                    HoverHandler { id: hoverHandler }
                                }
                            }
                            ToolButton {
                                id: clearButton
                                anchors.right: parent.right
                                anchors.rightMargin: 5
                                anchors.verticalCenter: parent.verticalCenter
                                text: qsTr("Clear connection")
                                icon.source: "icons/unlink.svg"
                                icon.color: mutedColor
                                display: AbstractButton.IconOnly
                                visible: targetCard.sourceIndex >= 0
                                Accessible.name: qsTr("Clear %1 connection").arg(mapping.targetLabel(index))
                                ToolTip.visible: hovered
                                ToolTip.text: Accessible.name
                                onClicked: { backend.clearMappingField(index); connectionCanvas.requestPaint() }
                            }
                        }
                    }
                }
            }

            Canvas {
                id: connectionCanvas
                anchors.fill: parent
                z: 4
                enabled: false
                onPaint: {
                    let context = getContext("2d")
                    context.reset()
                    context.lineWidth = 2
                    context.strokeStyle = accentColor
                    for (let target = 0; target < backend.mapping_target_count; target++) {
                        let source = backend.mappedSourceIndex(target)
                        if (source < 0) continue
                        let sourceItem = sourceRepeater.itemAt(source)
                        let targetItem = targetRepeater.itemAt(target)
                        if (!sourceItem || !targetItem) continue
                        let start = sourceItem.mapToItem(connectionCanvas, sourceItem.width, sourceItem.height / 2)
                        let end = targetItem.mapToItem(connectionCanvas, 0, targetItem.height / 2)
                        let bend = (start.x + end.x) / 2
                        context.beginPath()
                        context.moveTo(start.x, start.y)
                        context.bezierCurveTo(bend, start.y, bend, end.y, end.x, end.y)
                        context.stroke()
                    }
                }
                Connections {
                    target: backend
                    function onMapping_dirtyChanged() { connectionCanvas.requestPaint() }
                    function onMapping_source_countChanged() { Qt.callLater(function() { connectionCanvas.requestPaint() }) }
                    function onMapping_render_serialChanged() { Qt.callLater(function() { connectionCanvas.requestPaint() }) }
                }
            }
        }

        Label {
            visible: backend.mapping_message.length > 0
            Layout.fillWidth: true
            text: backend.mapping_message
            color: backend.mapping_dirty ? accentColor : mutedColor
            wrapMode: Text.Wrap
        }
    }
}
