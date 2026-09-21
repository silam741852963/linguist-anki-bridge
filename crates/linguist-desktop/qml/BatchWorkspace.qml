import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: workspace
    required property var backend
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor

    ColumnLayout {
        anchors.fill: parent
        spacing: 10
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

    Dialog {
        id: confirm
        modal: true
        visible: backend.batch_confirmation.length > 0
        title: qsTr("Confirm batch action")
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: backend.confirmBatchAction()
        onRejected: backend.cancelBatchAction()
        Label { text: backend.batch_confirmation; color: foregroundColor }
    }

    Dialog {
        id: createJob
        modal: true
        title: qsTr("Create batch job")
        standardButtons: Dialog.Cancel
        ColumnLayout {
            width: 620
            TextField { id: deckName; Layout.fillWidth: true; placeholderText: qsTr("Deck name"); Accessible.name: qsTr("Batch deck name") }
            TextArea {
                id: rows
                Accessible.name: qsTr("Batch rows")
                Accessible.description: qsTr("One note id and expression per line, separated by a tab")
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                placeholderText: qsTr("Note ID<Tab>Expression\n42<Tab>食べる")
                wrapMode: TextEdit.NoWrap
                inputMethodHints: Qt.ImhNoPredictiveText
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
                enabled: deckName.text.trim().length > 0 && rows.text.trim().length > 0
                onClicked: { backend.createBatch(deckName.text, rows.text, batchDryRun.checked); backend.refreshBatches(); createJob.close() }
            }
            Label { text: qsTr("OR SELECT FROM ANKI"); color: mutedColor; font.letterSpacing: 1.2 }
            GridLayout {
                columns: 2
                Layout.fillWidth: true
                TextField { id: selectorModel; Layout.fillWidth: true; placeholderText: qsTr("Model (optional)"); Accessible.name: qsTr("Selector model") }
                TextField { id: selectorTemplate; Layout.fillWidth: true; placeholderText: qsTr("Template (optional)"); Accessible.name: qsTr("Selector template") }
                TextField { id: selectorAfter; Layout.fillWidth: true; placeholderText: qsTr("Created from, YYYY-MM-DD"); Accessible.name: qsTr("Created from date") }
                TextField { id: selectorBefore; Layout.fillWidth: true; placeholderText: qsTr("Created through, YYYY-MM-DD"); Accessible.name: qsTr("Created through date") }
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
                        deck: deckName.text.trim().length > 0 ? deckName.text.trim() : null,
                        created_after: selectorAfter.text.trim().length > 0 ? selectorAfter.text.trim() : null,
                        created_before: selectorBefore.text.trim().length > 0 ? selectorBefore.text.trim() : null,
                        model: selectorModel.text.trim().length > 0 ? selectorModel.text.trim() : null,
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
                    enabled: backend.selector_preview_count > 0 && deckName.text.trim().length > 0
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
                Layout.fillWidth: true
                Layout.preferredHeight: 120
                model: backend.selector_preview_count
                clip: true
                delegate: Label {
                    required property int index
                    width: ListView.view.width
                    text: backend.selectorPreviewRow(index)
                    color: foregroundColor
                    elide: Text.ElideRight
                }
            }
        }
    }
}
