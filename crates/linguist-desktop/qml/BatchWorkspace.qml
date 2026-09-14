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
            Layout.fillWidth: true
            Layout.preferredHeight: 160
            model: backend.batchJobCount
            clip: true
            delegate: ItemDelegate {
                required property int index
                width: jobs.width
                text: backend.batchJob(index)
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
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: backend.batchItemCount
            clip: true
            delegate: Label {
                required property int index
                width: items.width
                text: backend.batchItem(index)
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
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: { backend.createBatch(deckName.text, rows.text); backend.refreshBatches() }
        ColumnLayout {
            width: 460
            TextField { id: deckName; Layout.fillWidth: true; placeholderText: qsTr("Deck name"); Accessible.name: qsTr("Batch deck name") }
            TextArea {
                id: rows
                Accessible.name: qsTr("Batch rows")
                Accessible.description: qsTr("One note id and expression per line, separated by a tab")
                Layout.fillWidth: true
                Layout.preferredHeight: 180
                placeholderText: qsTr("Note ID<Tab>Expression\n42<Tab>食べる")
                wrapMode: TextEdit.NoWrap
            }
        }
    }
}
