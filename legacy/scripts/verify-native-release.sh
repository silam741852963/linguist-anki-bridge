#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

echo "Checking package metadata"
bash -n PKGBUILD.native
makepkg --printsrcinfo -p PKGBUILD.native >/dev/null

echo "Checking source and functionality"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

if command -v qmllint >/dev/null; then
    qmllint \
        crates/linguist-desktop/qml/Main.qml \
        crates/linguist-desktop/qml/ReviewQueue.qml \
        crates/linguist-desktop/qml/ReviewWorkspace.qml \
        crates/linguist-desktop/qml/BatchWorkspace.qml
else
    echo "Skipping QML lint: qmllint is unavailable"
fi

if [[ -x .venv/bin/python ]]; then
    PYTHONPATH=src .venv/bin/python -m pytest -q
else
    echo "Skipping Python fallback tests: .venv/bin/python is unavailable"
fi

echo "Building release binary"
cargo build --locked --release -p linguist-desktop

stage_root=$(mktemp -d)
cleanup() {
    rm -rf -- "$stage_root"
}
trap cleanup EXIT

install -Dm755 \
    target/release/linguist-anki-bridge-native \
    "$stage_root/usr/bin/linguist-anki-bridge-native"
install -Dm644 \
    linguist-anki-bridge-native.desktop \
    "$stage_root/usr/share/applications/linguist-anki-bridge-native.desktop"
install -Dm644 \
    docs/native-preview-release.md \
    "$stage_root/usr/share/doc/linguist-anki-bridge-native-git/native-preview-release.md"
install -Dm644 \
    docs/native-accessibility.md \
    "$stage_root/usr/share/doc/linguist-anki-bridge-native-git/native-accessibility.md"
install -Dm644 \
    LICENSE \
    "$stage_root/usr/share/licenses/linguist-anki-bridge-native-git/LICENSE"

test -x "$stage_root/usr/bin/linguist-anki-bridge-native"
grep -Fxq 'Exec=linguist-anki-bridge-native' \
    "$stage_root/usr/share/applications/linguist-anki-bridge-native.desktop"
grep -Fq 'qml/Main.qml' crates/linguist-desktop/build.rs
grep -Fq 'qrc:/qt/qml/io/github/lam/linguist_anki_bridge/qml/Main.qml' \
    crates/linguist-desktop/src/main.rs

echo "Native release gate passed"
