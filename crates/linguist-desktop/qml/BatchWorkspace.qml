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
            Button { text: qsTr("Refresh"); Accessible.name: text; onClicked: backend.refreshBatches() }
        }
        ListView {
            id: jobs
            Accessible.name: qsTr("Batch jobs")
            Accessible.role: Accessible.List
            Layout.fillWidth: true
            Layout.preferredHeight: 160
            model: backend.batchJobCount
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
            Button { text: qsTr("Pause"); Accessible.name: text; onClicked: backend.pauseBatch() }
            Button { text: qsTr("Resume"); Accessible.name: text; onClicked: backend.resumeBatch() }
            Button { text: qsTr("Retry"); Accessible.name: text; onClicked: backend.retryBatch() }
            Item { Layout.fillWidth: true }
            Button { text: qsTr("Cancel"); Accessible.name: text; onClicked: backend.requestBatchAction(0) }
            Button { text: qsTr("Rollback"); Accessible.name: text; onClicked: backend.requestBatchAction(1) }
            Button { text: qsTr("Delete"); Accessible.name: text; onClicked: backend.requestBatchAction(2) }
        }
        Label { text: qsTr("%1 / %2 items loaded").arg(backend.batchItemCount).arg(backend.batchItemTotal); color: mutedColor }
        ListView {
            id: items
            Accessible.name: qsTr("Items in selected batch job")
            Accessible.role: Accessible.List
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: backend.batchItemCount
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
        visible: backend.batchConfirmation.length > 0
        title: qsTr("Confirm batch action")
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: backend.confirmBatchAction()
        onRejected: backend.cancelBatchAction()
        Label { text: backend.batchConfirmation; color: foregroundColor }
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
                TextField { id: selectorAfter; Layout.fillWidth: true; placeholderText: qsTr("Created after, Anki date syntax"); Accessible.name: qsTr("Created after") }
                TextField { id: selectorBefore; Layout.fillWidth: true; placeholderText: qsTr("Created before, Anki date syntax"); Accessible.name: qsTr("Created before") }
                TextField { id: selectorTags; Layout.fillWidth: true; placeholderText: qsTr("Tags, comma separated"); Accessible.name: qsTr("Selector tags") }
                TextField { id: selectorQuery; Layout.fillWidth: true; placeholderText: qsTr("Additional Anki query"); Accessible.name: qsTr("Additional selector query") }
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
                        image: ["any", "has_image", "no_image"][selectorImage.currentIndex],
                        completion: ["any", "incomplete", "complete"][selectorCompletion.currentIndex]
                    }))
                }
                Button {
                    text: qsTr("Create selector batch")
                    Accessible.name: text
                    highlighted: true
                    enabled: backend.selectorPreviewCount > 0
                    onClicked: { backend.createBatchFromSelector(batchDryRun.checked); backend.refreshBatches(); createJob.close() }
                }
                Label {
                    text: backend.selectorLimited
                        ? qsTr("%1 total · first %2 shown").arg(backend.selectorTotal).arg(backend.selectorPreviewCount)
                        : qsTr("%1 matches").arg(backend.selectorTotal)
                    color: mutedColor
                }
            }
            ListView {
                Layout.fillWidth: true
                Layout.preferredHeight: 120
                model: backend.selectorPreviewCount
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
