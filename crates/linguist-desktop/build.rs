use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.lam.linguist_anki_bridge")
            .qml_file("qml/Main.qml")
            .qml_file("qml/NavigationRail.qml")
            .qml_file("qml/ReviewQueue.qml")
            .qml_file("qml/ReviewWorkspace.qml")
            .qml_file("qml/BatchWorkspace.qml"),
    )
    .qt_module("Network")
    .qt_module("QuickControls2")
    .files(["src/backend.rs"])
    .build();
}
