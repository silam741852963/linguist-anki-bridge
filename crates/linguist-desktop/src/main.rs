mod backend;
#[allow(dead_code)] // N19 transport/controller binding follows this state contract.
mod commit_model;
mod controller;
mod draft;
mod review_model;
mod theme;

use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQmlEngine, QUrl};

fn main() {
    let mut application = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(
            "qrc:/qt/qml/io/github/lam/linguist_anki_bridge/qml/Main.qml",
        ));
    }
    if let Some(engine) = engine.as_mut() {
        let engine: std::pin::Pin<&mut QQmlEngine> = engine.upcast_pin();
        engine.on_quit(|_| {}).release();
    }

    if let Some(application) = application.as_mut() {
        application.exec();
    }
}
