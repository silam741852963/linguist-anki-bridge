import sys
import argparse
from linguist_anki_bridge.utils import setup_logging

def run():
    parser = argparse.ArgumentParser(description="Linguist Anki Bridge - Automate Anki modernization and ingestion")
    parser.add_argument("--debug", action="store_true", help="Enable debug/development logging and alerts")
    parser.add_argument(
        "--install-japanese-template",
        action="store_true",
        help="Create or update the managed Japanese vocabulary note type in Anki",
    )
    parser.add_argument(
        "--anki-url",
        help="Override the configured AnkiConnect URL for template installation",
    )
    args = parser.parse_args()

    # Initialize log files
    setup_logging(debug=args.debug)

    if args.install_japanese_template:
        from linguist_anki_bridge.anki import AnkiConnectClient
        from linguist_anki_bridge.card_templates import (
            JAPANESE_VOCAB_FIELD_MAPPING,
            install_japanese_vocab_template,
            japanese_vocab_template,
        )
        from linguist_anki_bridge.config import ConfigManager

        config_manager = ConfigManager()
        anki_url = args.anki_url or config_manager.config["anki"]["url"]
        result = install_japanese_vocab_template(AnkiConnectClient(url=anki_url))
        spec = japanese_vocab_template()
        print(f"{spec.model_name}: {result}.")
        print(f"Cards: {', '.join(spec.templates)}")
        print("Configure Japanese Vocabulary with:")
        print(f"  Note type: {spec.model_name}")
        for purpose, field in JAPANESE_VOCAB_FIELD_MAPPING.items():
            print(f"  {purpose}: {field}")
        return

    # Load theme and compile CSS
    from linguist_anki_bridge.config import load_omarchy_theme
    from linguist_anki_bridge.tui.app import AnkiBridgeApp, generate_runtime_css
    
    theme = load_omarchy_theme(debug=args.debug)
    generate_runtime_css(theme)

    # Launch app
    app = AnkiBridgeApp(theme=theme, debug=args.debug)
    app.run()

if __name__ == "__main__":
    run()
