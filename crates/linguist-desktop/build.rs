use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.lam.linguist_anki_bridge")
            .qml_file("qml/Main.qml")
            .qml_file("qml/ReviewQueue.qml")
            .qml_file("qml/ReviewWorkspace.qml")
            .qml_file("qml/BatchWorkspace.qml"),
    )
    .qt_module("Network")
    .qt_module("QuickControls2")
    .qrc_resources([
        "qml/icons/add.svg",
        "qml/icons/card.svg",
        "qml/icons/list.svg",
        "qml/icons/pause.svg",
        "qml/icons/play.svg",
        "qml/icons/preview.svg",
        "qml/icons/redo.svg",
        "qml/icons/refresh.svg",
        "qml/icons/settings.svg",
        "qml/icons/undo.svg",
    ])
    .files(["src/backend.rs"])
    .build();
}
