#[cfg(test)]
mod accessibility_contract;
mod backend;
#[allow(dead_code)] // N21 backend/QML binding follows this bounded model.
mod batch_model;
mod commit_model;
mod controller;
mod draft;
mod preview_model;
mod review_model;
mod theme;

use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQmlEngine, QUrl};

fn main() {
    // This utility does not use frame generation. A stale LSFG implicit layer
    // otherwise makes Vulkan print a loader error before Qt creates a window.
    if std::env::var_os("DISABLE_LSFGVK").is_none() {
        // SAFETY: process environment is adjusted before Qt or worker threads start.
        unsafe { std::env::set_var("DISABLE_LSFGVK", "1") };
    }
    let logging = std::env::var("QT_LOGGING_RULES").unwrap_or_default();
    if !logging.contains("qt.multimedia.ffmpeg.info") {
        let separator = if logging.is_empty() { "" } else { ";" };
        // SAFETY: process environment is adjusted before Qt or worker threads start.
        unsafe {
            std::env::set_var(
                "QT_LOGGING_RULES",
                format!("{logging}{separator}qt.multimedia.ffmpeg.info=false"),
            )
        };
    }
    let mut application = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(mut engine) = engine.as_mut() {
        {
            let base: std::pin::Pin<&mut QQmlEngine> = engine.as_mut().upcast_pin();
            base.set_output_warnings_to_standard_error(true);
        }
        engine
            .as_mut()
            .on_object_creation_failed(|_, url| {
                eprintln!("Failed to create native GUI from {url:?}");
            })
            .release();
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
