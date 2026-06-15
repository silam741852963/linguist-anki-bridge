import sys
import argparse
from linguist_anki_bridge.utils import setup_logging
from linguist_anki_bridge.tui.app import AnkiBridgeApp

def run():
    parser = argparse.ArgumentParser(description="Linguist Anki Bridge - Automate Anki modernization and ingestion")
    parser.add_argument("--debug", action="store_true", help="Enable debug/development logging and alerts")
    args = parser.parse_args()

    # Initialize log files
    setup_logging(debug=args.debug)

    # Load theme and compile CSS
    from linguist_anki_bridge.config import load_omarchy_theme
    from linguist_anki_bridge.tui.app import generate_runtime_css
    
    theme = load_omarchy_theme(debug=args.debug)
    generate_runtime_css(theme)

    # Launch app
    app = AnkiBridgeApp(theme=theme, debug=args.debug)
    app.run()

if __name__ == "__main__":
    run()
