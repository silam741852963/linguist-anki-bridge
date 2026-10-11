import QtQuick
import QtQuick.Controls

Dialog {
    id: dialog
    required property color backgroundColor
    required property color surfaceColor
    required property color foregroundColor
    required property color mutedColor
    required property color accentColor
    modal: true
    clip: true
    anchors.centerIn: Overlay.overlay
    padding: 16
    palette.window: backgroundColor
    palette.windowText: foregroundColor
    palette.base: surfaceColor
    palette.text: foregroundColor
    palette.button: surfaceColor
    palette.buttonText: foregroundColor
    palette.highlight: accentColor
    background: Rectangle {
        // A continuous square frame matches the terminal theme and keeps the
        // title/footer surfaces from exposing unpainted rounded corners.
        radius: 0
        color: dialog.surfaceColor
        border.width: 1
        border.color: dialog.accentColor
    }
    Overlay.modal: Rectangle {
        color: Qt.alpha(dialog.backgroundColor, 0.72)
    }
}
