import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: workspace
    required property var backend
    required property color backgroundColor
    required property color surfaceColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor

    function completeDate(value) {
        return /^\d{4}-\d{2}-\d{2}$/.test(value) ? value : null
    }

    Shortcut { sequence: "Ctrl+N"; enabled: workspace.visible; onActivated: createJob.open() }
    Shortcut { sequence: "Ctrl+R"; enabled: workspace.visible; onActivated: backend.refreshBatches() }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12
        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("BATCH JOBS"); color: mutedColor; font.letterSpacing: 1.2 }
            Item { Layout.fillWidth: true }
            Button { text: qsTr("New job"); Accessible.name: text; onClicked: createJob.open() }
            ToolButton { text: qsTr("Refresh jobs"); icon.source: "icons/refresh.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; onClicked: backend.refreshBatches() }
        }
        ListView {
            id: jobs
            Accessible.name: qsTr("Batch jobs")
            Accessible.role: Accessible.List
            Layout.fillWidth: true
            Layout.preferredHeight: 160
            model: backend.batch_job_count
            currentIndex: backend.batch_selected_index
            clip: true
            activeFocusOnTab: true
            delegate: ItemDelegate {
                required property int index
                width: jobs.width
                text: backend.batchJob(index)
                Accessible.name: text
                Accessible.role: Accessible.ListItem
                onClicked: backend.selectBatch(index)
            }
        }
        RowLayout {
            Layout.fillWidth: true
            ToolButton {
                text: backend.batch_status === "running" ? qsTr("Pause") : qsTr("Resume")
                icon.source: backend.batch_status === "running" ? "icons/pause.svg" : "icons/play.svg"
                display: AbstractButton.IconOnly
                Accessible.name: text
                ToolTip.visible: hovered
                ToolTip.text: text
                enabled: backend.batch_selected_index >= 0 && ["running", "paused", "queued"].indexOf(backend.batch_status) >= 0
                onClicked: backend.batch_status === "running" ? backend.pauseBatch() : backend.resumeBatch()
            }
            ToolButton { text: qsTr("Retry failed job"); icon.source: "icons/refresh.svg"; display: AbstractButton.IconOnly; Accessible.name: text; ToolTip.visible: hovered; ToolTip.text: text; enabled: backend.batch_selected_index >= 0 && backend.batch_status === "failed"; onClicked: backend.retryBatch() }
            Item { Layout.fillWidth: true }
            Button { text: qsTr("Cancel"); Accessible.name: text; enabled: backend.batch_selected_index >= 0; onClicked: backend.requestBatchAction(0) }
            Button { text: qsTr("Rollback"); Accessible.name: text; enabled: backend.batch_selected_index >= 0; onClicked: backend.requestBatchAction(1) }
            Button { text: qsTr("Delete"); Accessible.name: text; enabled: backend.batch_selected_index >= 0; onClicked: backend.requestBatchAction(2) }
        }
        Label { text: qsTr("%1 / %2 items loaded").arg(backend.batch_item_count).arg(backend.batch_item_total); color: mutedColor }
        ListView {
            id: items
            Accessible.name: qsTr("Items in selected batch job")
            Accessible.role: Accessible.List
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: backend.batch_item_count
            clip: true
            delegate: Label {
                required property int index
                width: items.width
                text: backend.batchItem(index)
                Accessible.name: text
                Accessible.role: Accessible.ListItem
                color: foregroundColor
                elide: Text.ElideRight
            }
        }
    }

    ThemedDialog {
        id: confirm
        backgroundColor: workspace.backgroundColor
        surfaceColor: workspace.surfaceColor
        foregroundColor: workspace.foregroundColor
        mutedColor: workspace.mutedColor
        accentColor: workspace.accentColor
        visible: backend.batch_confirmation.length > 0
        title: qsTr("Confirm batch action")
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: Qt.callLater(function() { standardButton(Dialog.Cancel).forceActiveFocus() })
        onAccepted: backend.confirmBatchAction()
        onRejected: backend.cancelBatchAction()
        Label { text: backend.batch_confirmation; color: foregroundColor }
    }

    ThemedDialog {
        id: createJob
        backgroundColor: workspace.backgroundColor
        surfaceColor: workspace.surfaceColor
        foregroundColor: workspace.foregroundColor
        mutedColor: workspace.mutedColor
        accentColor: workspace.accentColor
        title: qsTr("Create batch job")
        width: Math.min(workspace.width * 0.9, 680)
        height: Math.min(workspace.height * 0.9, 720)
        standardButtons: Dialog.Cancel
        onOpened: Qt.callLater(function() { deckName.forceActiveFocus() })
        contentItem: ScrollView {
            clip: true
            contentWidth: availableWidth
        ColumnLayout {
            width: parent.width
            ComboBox {
                id: deckName
                Layout.fillWidth: true
                editable: true
                Accessible.name: qsTr("Batch deck name")
                model: {
                    let names = []
                    for (let index = 0; index < backend.deck_count; index++) names.push(backend.deckName(index))
                    return names
                }
            }
            TextArea {
                id: rows
                Accessible.name: qsTr("Batch rows")
                Accessible.description: qsTr("One note id and expression per line, separated by a tab")
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                placeholderText: qsTr("Note ID<Tab>Expression\n42<Tab>食べる")
                wrapMode: TextEdit.NoWrap
                inputMethodHints: Qt.ImhNone
            }
            CheckBox {
                id: batchDryRun
                text: qsTr("Dry run (no Anki writes)")
                checked: true
                Accessible.name: text
            }
            Button {
                text: qsTr("Create from explicit rows")
                Accessible.name: text
                enabled: deckName.editText.trim().length > 0 && rows.text.trim().length > 0
                onClicked: { backend.createBatch(deckName.editText, rows.text, batchDryRun.checked); backend.refreshBatches(); createJob.close() }
            }
            Label { text: qsTr("OR SELECT FROM ANKI"); color: mutedColor; font.letterSpacing: 1.2 }
            GridLayout {
                columns: 2
                Layout.fillWidth: true
                ComboBox {
                    id: selectorModel
                    Layout.fillWidth: true
                    editable: true
                    Accessible.name: qsTr("Selector model")
                    model: {
                        let names = [""]
                        for (let index = 0; index < backend.model_count; index++) names.push(backend.modelName(index))
                        return names
                    }
                }
                TextField { id: selectorTemplate; Layout.fillWidth: true; placeholderText: qsTr("Template (optional)"); Accessible.name: qsTr("Selector template") }
                TextField { id: selectorAfter; Layout.fillWidth: true; placeholderText: qsTr("Created from, YYYY-MM-DD"); Accessible.name: qsTr("Created from date"); inputMask: "0000-00-00;_" }
                TextField { id: selectorBefore; Layout.fillWidth: true; placeholderText: qsTr("Created through, YYYY-MM-DD"); Accessible.name: qsTr("Created through date"); inputMask: "0000-00-00;_" }
                TextField { id: selectorTags; Layout.fillWidth: true; placeholderText: qsTr("Tags, comma separated"); Accessible.name: qsTr("Selector tags") }
                TextField { id: selectorExcludedTags; Layout.fillWidth: true; placeholderText: qsTr("Excluded tags, comma separated"); Accessible.name: qsTr("Excluded selector tags") }
                TextField { id: selectorQuery; Layout.fillWidth: true; placeholderText: qsTr("Additional Anki query"); Accessible.name: qsTr("Additional selector query") }
                RowLayout {
                    Layout.fillWidth: true
                    Label { text: qsTr("Maximum notes"); color: mutedColor }
                    SpinBox { id: selectorLimit; Layout.fillWidth: true; from: 0; to: 1000000; value: 0; editable: true; Accessible.name: qsTr("Maximum matching notes, zero means unlimited") }
                }
                ComboBox { id: selectorImage; Layout.fillWidth: true; model: [qsTr("Any image"), qsTr("Has image"), qsTr("No image")]; Accessible.name: qsTr("Image filter") }
                ComboBox { id: selectorCompletion; Layout.fillWidth: true; model: [qsTr("Any completion"), qsTr("Incomplete"), qsTr("Complete")]; Accessible.name: qsTr("Completion filter") }
            }
            RowLayout {
                Button {
                    text: qsTr("Preview selector")
                    Accessible.name: text
                    onClicked: backend.previewBatchSelector(JSON.stringify({
                        deck: deckName.editText.trim().length > 0 ? deckName.editText.trim() : null,
                        created_after: workspace.completeDate(selectorAfter.text),
                        created_before: workspace.completeDate(selectorBefore.text),
                        model: selectorModel.editText.trim().length > 0 ? selectorModel.editText.trim() : null,
                        template: selectorTemplate.text.trim().length > 0 ? selectorTemplate.text.trim() : null,
                        query: selectorQuery.text,
                        tags: selectorTags.text.split(",").map(tag => tag.trim()).filter(tag => tag.length > 0),
                        excluded_tags: selectorExcludedTags.text.split(",").map(tag => tag.trim()).filter(tag => tag.length > 0),
                        image: ["any", "has_image", "no_image"][selectorImage.currentIndex],
                        completion: ["any", "incomplete", "complete"][selectorCompletion.currentIndex],
                        limit: selectorLimit.value
                    }))
                }
                Button {
                    text: qsTr("Create selector batch")
                    Accessible.name: text
                    highlighted: true
                    enabled: backend.selector_preview_count > 0 && deckName.editText.trim().length > 0
                    onClicked: { backend.createBatchFromSelector(batchDryRun.checked); backend.refreshBatches(); createJob.close() }
                }
                Label {
                    text: backend.selector_limited
                        ? qsTr("%1 total · first %2 shown").arg(backend.selector_total).arg(backend.selector_preview_count)
                        : qsTr("%1 matches").arg(backend.selector_total)
                    color: mutedColor
                }
            }
            ListView {
                id: selectorResults
                Accessible.name: qsTr("Batch selector preview")
                Accessible.description: qsTr("At most the first 200 matching notes are displayed")
                Accessible.role: Accessible.List
                Layout.fillWidth: true
                Layout.preferredHeight: 120
                model: backend.selector_preview_count
                clip: true
                delegate: Label {
                    required property int index
                    width: selectorResults.width
                    text: backend.selectorPreviewRow(index)
                    Accessible.name: text
                    Accessible.role: Accessible.ListItem
                    color: foregroundColor
                    elide: Text.ElideRight
                }
            }
        }
        }
    }
}
