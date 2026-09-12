import re
import base64
import json
import html as html_lib
import logging
import asyncio
import os
import csv
import urllib.request
import urllib.parse
import io
import copy
from bs4 import BeautifulSoup
from rich.markup import escape

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Vertical, Horizontal, ScrollableContainer
from textual.screen import ModalScreen, Screen
from textual.widgets import Label, Button, Input, Static, TextArea, ListView, ListItem, RichLog, Rule, Footer
from rich.table import Table
from rich.panel import Panel
from rich.console import Group
from rich.text import Text

from linguist_anki_bridge.tts import generate_tts_base64
from linguist_anki_bridge.utils import create_deck_backup, get_media_cache_dir
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.scraper import Crawl4AiScraper
from linguist_anki_bridge.card_model import (
    CardDocument,
    MediaAsset,
    build_card_document as _build_card_document,
    commit_card_document,
    format_anki_grammar_html as _format_anki_grammar_html,
    format_dictionary_meaning_html as _format_dictionary_meaning_html,
    format_injection_context_html as _format_injection_context_html,
    format_llm_annotations_html as _format_llm_annotations_html,
    map_document_fields,
)
from linguist_anki_bridge.card_templates import japanese_vocab_template
from linguist_anki_bridge.snapshots import SnapshotManager

# --- INPUT DIALOG ---
class InputDialog(ModalScreen[str]):
    def __init__(self, title: str, placeholder: str = ""):
        super().__init__()
        self.dialog_title = title
        self.placeholder = placeholder

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-dialog-small"):
            yield Label(f"[bold accent]{self.dialog_title}[/]", id="modal-title")
            yield Input(placeholder=self.placeholder, id="dialog-input")
            with Horizontal(classes="mt-1"):
                yield Button("OK", variant="success", id="btn-ok")
                yield Button("Cancel", variant="error", id="btn-cancel")

    def on_mount(self) -> None:
        self.query_one("#dialog-input", Input).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-ok":
            val = self.query_one("#dialog-input", Input).value.strip()
            self.dismiss(val)
        else:
            self.dismiss("")

    def on_input_submitted(self, event: Input.Submitted) -> None:
        self.dismiss(event.value.strip())


class SnapshotManagementScreen(Screen):
    """Inspect and revert every recorded write for one expression."""

    BINDINGS = [
        Binding("escape", "close", "Back", priority=True),
        Binding("r", "revert", "Revert Snapshot"),
        Binding("enter", "revert", "Revert Snapshot"),
    ]

    def __init__(self, manager: SnapshotManager, anki_client, word: str):
        super().__init__()
        self.manager = manager
        self.anki_client = anki_client
        self.word = word
        self.snapshots: list[dict] = []
        self.selected_index = 0

    def compose(self) -> ComposeResult:
        yield Label(f"[bold accent]● SNAPSHOTS — {escape(self.word)}[/]", classes="pane-title")
        with Horizontal():
            with Vertical(classes="pane"):
                yield ListView(id="snapshot-list")
            with ScrollableContainer(classes="pane"):
                yield Static(Text(""), id="snapshot-details")
        yield Footer()

    def on_mount(self) -> None:
        self.snapshots = self.manager.list_for_word(self.word)
        listing = self.query_one("#snapshot-list", ListView)
        if not self.snapshots:
            listing.append(ListItem(Label("No snapshots for this word."), id="snapshot-empty"))
            self.query_one("#snapshot-details", Static).update(Text("Commit or dry-run this word to create a snapshot."))
            return
        for index, item in enumerate(self.snapshots):
            label = (
                f"{item.get('created_at', '')}  "
                f"{str(item.get('mode', '')).title()}  [{item.get('status', '')}]"
            )
            listing.append(ListItem(Label(label), id=f"snapshot-{index}"))
        listing.index = 0
        listing.focus()
        self._show(0)

    def _show(self, index: int) -> None:
        if not (0 <= index < len(self.snapshots)):
            return
        self.selected_index = index
        record = self.snapshots[index]
        original = record.get("original_note") or {}
        lines = [
            f"Snapshot: {record.get('id', '')}",
            f"Created: {record.get('created_at', '')}",
            f"Word: {record.get('word', '')}",
            f"Mode: {record.get('mode', '')}",
            f"Deck key: {record.get('deck_key', '')}",
            f"Status: {record.get('status', '')}",
            f"Dry run: {'Yes' if record.get('dry_run') else 'No'}",
            f"Original note: {original.get('note_id') or 'None (new injection)'}",
            f"Result note: {record.get('result_note_id') or 'None'}",
            f"Split sibling notes: {', '.join(map(str, record.get('created_note_ids') or [])) or 'None'}",
            f"Note type: {original.get('model_name') or '—'}",
            f"Tags: {', '.join(original.get('tags') or []) or '—'}",
            "",
            "ORIGINAL FIELDS",
            "───────────────",
        ]
        fields = original.get("fields") or {}
        lines.extend(f"{name}:\n{value}" for name, value in fields.items())
        lines += [
            "",
            "TRACKED MEDIA",
            "─────────────",
            *(
                f"{name}: {'stored previous content' if data else 'did not exist before write'}"
                for name, data in (record.get("media_before") or {}).items()
            ),
            "",
            "PROCESSED CARD DATA",
            "───────────────────",
            json.dumps(record.get("processed") or {}, ensure_ascii=False, indent=2),
        ]
        if record.get("error"):
            lines += ["", f"ERROR: {record['error']}"]
        self.query_one("#snapshot-details", Static).update(Text("\n".join(lines)))

    def on_list_view_highlighted(self, event: ListView.Highlighted) -> None:
        if event.list_view.id != "snapshot-list" or not event.item or not event.item.id:
            return
        if event.item.id.startswith("snapshot-") and event.item.id != "snapshot-empty":
            self._show(int(event.item.id.rsplit("-", 1)[1]))

    def action_close(self) -> None:
        self.app.pop_screen()

    def action_revert(self) -> None:
        if not self.snapshots:
            self.notify("No snapshot selected.", severity="warning")
            return
        self.run_worker(self._revert_selected(), exclusive=True)

    async def _revert_selected(self) -> None:
        record = self.snapshots[self.selected_index]
        try:
            message = await asyncio.get_running_loop().run_in_executor(
                None, self.manager.revert, record["id"], self.anki_client
            )
            self.snapshots[self.selected_index] = self.manager.get(record["id"]) or record
            self._show(self.selected_index)
            self.notify(message, severity="information")
        except Exception as exc:
            self.notify(f"Snapshot revert failed: {exc}", severity="error")

# --- PREVIEW PANE ---
class PreviewPane(ScrollableContainer):
    can_focus = True
    BINDINGS = [
        Binding("tab", "next_pane", "Next Pane", priority=True),
        Binding("shift+tab", "prev_pane", "Previous Pane", priority=True),
        ("w", "edit_word", "Edit Word"),
        ("m", "edit_meaning", "Edit Meaning"),
        ("k", "edit_kanji", "Edit Kanji"),
        ("v", "select_audio", "Select Audio"),
        ("i", "select_image", "Select Image"),
        ("x", "classify_image", "Confirm Image Class"),
        ("r", "manage_snapshots", "Snapshots"),
        Binding("n", "next_split", "Next Split", priority=True),
        Binding("p", "previous_split", "Previous Split", priority=True),
        Binding("right", "next_split", "", show=False, priority=True),
        Binding("left", "previous_split", "", show=False, priority=True),
    ]
    def action_edit_word(self) -> None:
        self.app.action_edit_word()
    def action_edit_meaning(self) -> None:
        self.app.action_edit_meaning()
    def action_edit_kanji(self) -> None:
        self.app.action_edit_kanji()
    def action_select_audio(self) -> None:
        self.app.action_select_audio()
    def action_select_image(self) -> None:
        self.app.action_select_image()
    def action_classify_image(self) -> None:
        self.app.action_confirm_image_classification()
    def action_manage_snapshots(self) -> None:
        self.app.action_manage_snapshots()
    def action_next_split(self) -> None:
        self.app.action_next_preview_split()
    def action_previous_split(self) -> None:
        self.app.action_previous_preview_split()
    def action_next_pane(self) -> None:
        self.app.action_next_pane()
    def action_prev_pane(self) -> None:
        self.app.action_prev_pane()

    def compose(self) -> ComposeResult:
        yield Label("[bold accent]● PREVIEW [4][/]", id="preview-title", classes="pane-title")
        yield Static("", id="details-comparison")
        yield Static("", id="details-image-classification")
        yield Static("", id="details-dict-scrape")
        yield Static("", id="details-llm")
        yield Static("", id="details-kanji-scrape")

    def update_content(self, markup: str) -> None:
        self.update_comparison(markup)

    def update_title(self, current: int = 0, total: int = 0) -> None:
        split = f" [bold yellow]SPLIT {current}/{total}[/]" if total > 1 else ""
        self.query_one("#preview-title", Label).update(f"[bold accent]● PREVIEW [4][/]{split}")

    def update_comparison(self, markup) -> None:
        try:
            self.query_one("#details-comparison", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update PreviewPane comparison: {e}")

    def update_dict_scrape(self, markup) -> None:
        try:
            self.query_one("#details-dict-scrape", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update PreviewPane dict scrape: {e}")

    def update_image_classification(self, markup) -> None:
        try:
            self.query_one("#details-image-classification", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update PreviewPane image classification: {e}")

    def update_kanji_scrape(self, markup) -> None:
        try:
            self.query_one("#details-kanji-scrape", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update PreviewPane kanji scrape: {e}")

    def update_llm(self, markup) -> None:
        try:
            self.query_one("#details-llm", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update PreviewPane LLM details: {e}")

# --- LOG PANE ---
class PaneRichLog(RichLog):
    BINDINGS = list(RichLog.BINDINGS) + [
        Binding("tab", "next_pane", "Next Pane", priority=True),
        Binding("shift+tab", "prev_pane", "Previous Pane", priority=True),
    ]

    def action_next_pane(self) -> None:
        self.app.action_next_pane()

    def action_prev_pane(self) -> None:
        self.app.action_prev_pane()


class LogPane(Vertical):
    can_focus = True
    BINDINGS = [
        ("o", "date_select", "Date Log Select"),
        ("c", "copy_log_clipboard", "Copy Log")
    ]
    def action_date_select(self) -> None:
        self.app.action_date_select()
    def action_copy_log_clipboard(self) -> None:
        self.app.action_copy_log_clipboard()

    def compose(self) -> ComposeResult:
        yield Label("[bold accent]● LOG [5][/]", classes="pane-title")
        yield PaneRichLog(id="details-logs", markup=True, highlight=True, max_lines=1000)

# --- HTML FORMATTERS ---
def format_llm_annotations_html(nuances: str, examples: list) -> str:
    """Render only the two fields that vocabulary LLM generation is allowed to own."""
    return _format_llm_annotations_html(nuances, examples)
    output = ""
    if nuances:
        output += (
            "<div data-source='llm' style='margin-top:6px;font-style:italic;color:#888'>"
            f"<b>Nuance:</b> {html_lib.escape(str(nuances))}</div>"
        )
    if examples:
        output += (
            "<div data-source='llm' style='margin-top:10px'><b>Examples:</b>"
            "<ol style='margin:5px 0;padding-left:20px'>"
        )
        for ex in examples:
            if not isinstance(ex, dict):
                continue
            sentence = html_lib.escape(str(ex.get("sentence", "")))
            translation = html_lib.escape(str(ex.get("translation", "")))
            output += (
                f"<li style='margin-bottom:3px'><b>{sentence}</b><br/>"
                f"<span style='color:#666;font-size:.9em'>{translation}</span></li>"
            )
        output += "</ol></div>"
    return output

def format_dictionary_meaning_html(scraped: dict, target_word: str,
                                   nuances: str, examples: list) -> str:
    """Render parsed dictionary data verbatim, annotating only the exact entry with LLM output."""
    return _format_dictionary_meaning_html(scraped, target_word, nuances, examples)

def format_anki_grammar_html(grammar_point: str, meaning: str, rules: str, examples: list) -> str:
    return _format_anki_grammar_html(grammar_point, meaning, rules, examples)


def format_injection_context_html(type_tag: str = "", note: str = "") -> str:
    """Render user-authored injection context without giving it to the dictionary parser."""
    return _format_injection_context_html(type_tag, note)
    rows = []
    if type_tag:
        rows.append(f"<div><b>Learning focus:</b> {html_lib.escape(str(type_tag))}</div>")
    if note:
        rows.append(f"<div><b>Personal context:</b> {html_lib.escape(str(note))}</div>")
    if not rows:
        return ""
    return (
        "<aside data-source='user' style='margin-top:12px;border-top:1px dashed #888;"
        "padding-top:8px'>" + "".join(rows) + "</aside>"
    )


def build_card_document(processed_data: dict, lang_key: str, mode: str) -> CardDocument:
    """Build the canonical card represented by a processing result."""
    return _build_card_document(processed_data, lang_key, mode)
    if mode not in {"modernize", "inject"}:
        raise ValueError(f"Unknown card mode: {mode}")

    original_word = str(processed_data.get("word", "") or "").strip()
    expression = (
        str(processed_data.get("suggestion") or original_word).strip()
        if mode == "inject" else original_word
    )
    llm_result = processed_data.get("llm_response") or {}
    is_grammar = lang_key.endswith("grammar")
    if is_grammar:
        meaning_html = format_anki_grammar_html(
            llm_result.get("grammar_point", expression),
            llm_result.get("meaning", ""),
            llm_result.get("rules", ""),
            llm_result.get("examples", []),
        )
    else:
        meaning_html = format_dictionary_meaning_html(
            processed_data.get("scraped", {}),
            expression,
            llm_result.get("nuances", ""),
            llm_result.get("examples", []),
        )
    if mode == "inject":
        meaning_html += format_injection_context_html(
            processed_data.get("type_tag", ""), processed_data.get("source_note", "")
        )

    media: list[MediaAsset] = []
    obsolete_media: list[str] = []
    image_html_parts: list[str] = []
    new_image_name = processed_data.get("new_image_filename", "")
    new_image_data = processed_data.get("new_image_b64")
    if new_image_name and new_image_data:
        media.append(MediaAsset(new_image_name, new_image_data))
        image_html_parts.append(f"<img src='{html_lib.escape(new_image_name, quote=True)}'/>")
        if mode == "modernize":
            obsolete_media.extend(processed_data.get("orig_filenames", []))
    elif mode == "modernize":
        renamed_images = processed_data.get("renamed_images", [])
        for image in renamed_images:
            filename = image.get("new_name", "")
            data = image.get("b64")
            if filename and data:
                media.append(MediaAsset(filename, data))
                image_html_parts.append(f"<img src='{html_lib.escape(filename, quote=True)}'/>")
                original_name = image.get("original_name")
                if original_name and original_name != filename:
                    obsolete_media.append(original_name)
        if not image_html_parts:
            existing = processed_data.get("filename", "")
            if existing and processed_data.get("classification") != "dictionary":
                image_html_parts.append(f"<img src='{html_lib.escape(existing, quote=True)}'/>")
            elif processed_data.get("classification") == "dictionary":
                obsolete_media.extend(processed_data.get("orig_filenames", []))

    audio_name = processed_data.get("audio_filename", "")
    audio_data = processed_data.get("audio_b64")
    audio_html: str | None = "" if mode == "inject" else None
    if audio_name and audio_data:
        media.append(MediaAsset(audio_name, audio_data))
        reading = processed_data.get("scraped", {}).get("reading", "")
        has_kanji = any("\u4e00" <= char <= "\u9fff" for char in expression)
        sound = f"[sound:{audio_name}]"
        audio_html = (
            f"{html_lib.escape(str(reading))} {sound}"
            if lang_key.startswith("japanese") and has_kanji and reading else sound
        )

    issues = list(processed_data.get("issues", []))
    if not expression:
        issues.append("Expression is empty")
    # Injection is an authoring workflow: dictionary data is preferred, but a
    # user may intentionally inject a new word with OCR/context only.  The
    # template boundary still validates the required expression/meaning fields
    # at commit time, so absence of a scrape must not make every inject card
    # uncommittable.

    type_tag = safe_media_stem(str(processed_data.get("type_tag", "")), "")
    tags = ["linguist-injected"]
    if type_tag:
        tags.append(f"linguist::{type_tag}")
    return CardDocument(
        expression=expression,
        values={
            "meaning_image": "".join(image_html_parts),
            "meaning_text": meaning_html,
            "kanji_construction": processed_data.get("kanji_construction", ""),
            "audio": audio_html,
        },
        media=media,
        obsolete_media=obsolete_media,
        issues=list(dict.fromkeys(issues)),
        tags=tags if mode == "inject" else [],
    )

# --- HELPER FUNCTIONS ---
def clean_html_for_tui(html: str) -> str:
    if not html:
        return ""
    # Format simple blocks nicely
    h = html.replace("<div>", "").replace("</div>", "\n").replace("<br>", "\n").replace("<br/>", "\n")
    h = h.replace("<p>", "").replace("</p>", "\n").replace("<li>", "\n- ").replace("</li>", "")
    h = BeautifulSoup(h, "html.parser").get_text().strip()
    return escape(h)

def meaning_summary_for_tui(value: str) -> str:
    """Preserve dictionary entry boundaries while converting card HTML to text."""
    if not value:
        return ""
    soup = BeautifulSoup(str(value), "html.parser")
    sections = soup.find_all("section")
    if not sections:
        return clean_html_for_tui(value)
    rendered = []
    for section in sections:
        for br in section.find_all("br"):
            br.replace_with("\n")
        text = re.sub(r"\n{3,}", "\n\n", section.get_text("\n").strip())
        rendered.append(text)
    return escape("\n─────────────────────────\n".join(rendered))

def kanji_summary_for_tui(value: str) -> str:
    """Readable Kanji text with embedded stroke-order payloads abbreviated."""
    if not value:
        return ""
    abbreviated = re.sub(
        r"data:image/(?:gif|png|webp);base64,[A-Za-z0-9+/=,\s]+",
        "[Stroke-order image]",
        str(value),
        flags=re.IGNORECASE,
    )
    soup = BeautifulSoup(abbreviated, "html.parser")
    for image in soup.find_all("img"):
        image.replace_with("[Stroke-order image]")
    sections = soup.select("section[data-kanji]")
    if sections:
        blocks = [re.sub(r"\n{3,}", "\n\n", section.get_text("\n").strip()) for section in sections]
        return escape("\n═════════════════════════\n".join(blocks))
    text = re.sub(r"\n{3,}", "\n\n", soup.get_text("\n").strip())
    # Compatibility with cached pre-section Kanji HTML.
    text = re.sub(r"\n(?=Kanji\s+[\u4e00-\u9fff])", "\n═════════════════════════\n", text)
    return escape(text)

def get_file_extension(filename: str) -> str:
    if "." in filename:
        return filename.split(".")[-1]
    return "jpg"

def safe_media_stem(value: str, fallback: str = "media") -> str:
    cleaned = re.sub(r"[^\w\-]+", "_", value or "", flags=re.UNICODE).strip("_")
    return cleaned[:80] or fallback

def indexed_media_filename(prefix: str, word: str, extension: str, index: int = 0, scope: str = "") -> str:
    parts = [prefix]
    if scope:
        parts.append(safe_media_stem(scope, "media"))
    parts.extend((safe_media_stem(word, "media"), str(index)))
    return f"{'_'.join(parts)}.{extension.lstrip('.')}"

def audio_extension(source: str) -> str:
    if not source or source == "tts":
        return "mp3"
    suffix = Path(urllib.parse.urlparse(source).path).suffix.lower().lstrip(".")
    return suffix if suffix in {"mp3", "ogg", "wav", "m4a"} else "mp3"

def extract_image_filenames(html: str) -> list[str]:
    if not html:
        return []
    soup = BeautifulSoup(html, "html.parser")
    filenames = []
    for img in soup.find_all("img"):
        src = img.get("src", "")
        if src and not src.startswith("http"):
            filenames.append(src)
    return filenames

def clean_word_field(word: str, app_config: dict) -> str:
    if not word:
        return ""
    # Clean HTML tags first
    from bs4 import BeautifulSoup
    word = BeautifulSoup(word, "html.parser").get_text().strip()

    filters = app_config.get("filters", {})
    if filters.get("remove_parentheses", True):
        import re
        word = re.sub(r"\([^\)]*\)", "", word)
        word = re.sub(r"（[^）]*）", "", word)
        word = word.strip()

    if filters.get("clean_word_only", False):
        import re
        word = "".join(re.findall(r"[\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff\u3005a-zA-Z0-9\s]+", word)).strip()

    return word


def extract_legacy_expressions(value: str, app_config: dict) -> list[str]:
    """Extract deliberate legacy field lines without splitting inline spans.

    A line break (``br``, block closing tag, or a literal newline) represents
    a separate expression. Multiple sound tags never influence this decision,
    which keeps words with alternate readings on one note.
    """
    if not value:
        return []
    separated = re.sub(
        r"(?i)<br\s*/?>|</?(?:div|p|li|tr)(?:\s[^>]*)?>",
        "\n", str(value),
    )
    text = BeautifulSoup(separated, "html.parser").get_text()
    expressions: list[str] = []
    for line in text.splitlines():
        expression = clean_word_field(line, app_config)
        if expression and expression not in expressions:
            expressions.append(expression)
    return expressions


def extract_legacy_audio_entries(note_info: dict) -> list[dict[str, str]]:
    """Return ordered sound/readings from any legacy field containing audio."""
    entries: list[dict[str, str]] = []
    for field in (note_info.get("fields") or {}).values():
        value = str(field.get("value", "") if isinstance(field, dict) else field)
        matches = list(re.finditer(r"\[sound:([^\]]+)\]", value, flags=re.IGNORECASE))
        for index, match in enumerate(matches):
            end = matches[index + 1].start() if index + 1 < len(matches) else len(value)
            reading_html = value[match.end():end]
            reading = BeautifulSoup(reading_html, "html.parser").get_text(" ", strip=True)
            entries.append({"filename": match.group(1).strip(), "reading": reading})
    return entries

def find_anki_media_path(filename: str) -> str:
    if not filename:
        return ""
    from pathlib import Path

    # Check the application cache first.
    temp_path = get_media_cache_dir() / filename
    if temp_path.exists():
        return str(temp_path)

    paths = [
        Path.home() / ".local/share/Anki2/User 1/collection.media",
        Path.home() / ".var/app/net.ankiweb.Anki/data/Anki2/User 1/collection.media",
        Path.home() / "Anki/User 1/collection.media",
    ]
    for p in paths:
        full_path = p / filename
        if full_path.exists():
            return str(full_path)
    return ""

async def fetch_web_image(word: str, log_cb=None, suffix: str = "") -> tuple[str, str]:
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    search_word = re.sub(r"[^\w\s\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff]", "", word).strip()
    if not search_word:
        return None, ""

    query_str = search_word
    if suffix:
        query_str += " " + suffix.strip()

    loop = asyncio.get_running_loop()
    user_agent = "LinguistAnkiBridge/1.0 (language-learning card image replacement)"

    async def get_json(url: str) -> dict:
        request = urllib.request.Request(url, headers={"User-Agent": user_agent})
        payload = await loop.run_in_executor(None, lambda: urllib.request.urlopen(request, timeout=6.0).read())
        return json.loads(payload.decode("utf-8"))

    async def download_candidate(url: str, provider: str) -> tuple[str, str]:
        if not url or url.lower().endswith(".svg"):
            return None, ""
        request = urllib.request.Request(url, headers={"User-Agent": user_agent})
        payload = await loop.run_in_executor(None, lambda: urllib.request.urlopen(request, timeout=10.0).read())
        # Reject API error pages and broken thumbnails before they reach Anki.
        from PIL import Image, ImageStat
        with Image.open(io.BytesIO(payload)) as image:
            image.verify()
        with Image.open(io.BytesIO(payload)) as image:
            sample = image.convert("RGBA")
            # Composite alpha onto white, then encode a baseline RGB JPEG.
            # This avoids CMYK/ICC/alpha decoder differences in lightweight
            # Linux image viewers while matching what Anki displays.
            background = Image.new("RGBA", sample.size, "white")
            background.alpha_composite(sample)
            sample = background.convert("RGB")
            sample.thumbnail((256, 256))
            luminance = sample.convert("L")
            pixels = list(luminance.get_flattened_data())
            mean_light = sum(pixels) / max(1, len(pixels))
            dark_ratio = sum(value < 18 for value in pixels) / max(1, len(pixels))
            contrast = ImageStat.Stat(luminance).stddev[0]
            if mean_light < 28 or dark_ratio > 0.82 or contrast < 4:
                raise ValueError(
                    f"low-quality image (brightness={mean_light:.1f}, dark={dark_ratio:.0%}, contrast={contrast:.1f})"
                )
        with Image.open(io.BytesIO(payload)) as image:
            rgba = image.convert("RGBA")
            background = Image.new("RGBA", rgba.size, "white")
            background.alpha_composite(rgba)
            normalized = io.BytesIO()
            background.convert("RGB").save(
                normalized, format="JPEG", quality=90, optimize=False,
                progressive=False, subsampling=0,
            )
        normalized_payload = normalized.getvalue()
        filename = indexed_media_filename(provider, search_word, "jpg")
        return base64.b64encode(normalized_payload).decode("utf-8"), filename

    # Search multiple results and languages. The former single-result English
    # query frequently selected an unrelated page and stopped immediately.
    for language in ("ja", "en"):
        language_query = search_word if language == "ja" else (suffix.strip() or search_word)
        url = (
            f"https://{language}.wikipedia.org/w/api.php?action=query&format=json&generator=search&"
            f"gsrsearch={urllib.parse.quote(language_query)}&gsrlimit=6&prop=pageimages&"
            "piprop=thumbnail|original&pithumbsize=640"
        )
        log(f"Searching {language}.wikipedia.org for illustrative images of '{language_query}'...")
        try:
            pages = (await get_json(url)).get("query", {}).get("pages", {})
            for page in sorted(pages.values(), key=lambda value: value.get("index", 999)):
                image_url = page.get("thumbnail", {}).get("source") or page.get("original", {}).get("source")
                if not image_url:
                    log(f"No pageimage found for Wikipedia page '{page.get('title')}'. Trying the next result.")
                    continue
                try:
                    result = await download_candidate(image_url, "wiki")
                    if result[0]:
                        log(f"Downloaded replacement image from Wikipedia page '{page.get('title')}' as '{result[1]}'.")
                        return result
                except Exception as candidate_error:
                    log(f"Rejected Wikipedia image candidate '{page.get('title')}': {candidate_error}")
        except Exception as error:
            log(f"{language}.wikipedia.org image search failed: {error}")

    # Wikimedia Commons searches actual media files rather than encyclopedia
    # pages and is a useful fallback for verbs or concepts without a pageimage.
    commons_query = suffix.strip() or query_str
    commons_url = (
        "https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&"
        f"gsrsearch={urllib.parse.quote(commons_query)}&gsrnamespace=6&gsrlimit=10&"
        "prop=imageinfo&iiprop=url|mime&iiurlwidth=640"
    )
    log(f"Searching Wikimedia Commons media for '{commons_query}'...")
    try:
        pages = (await get_json(commons_url)).get("query", {}).get("pages", {})
        for page in sorted(pages.values(), key=lambda value: value.get("index", 999)):
            info = next(iter(page.get("imageinfo", [])), {})
            if str(info.get("mime", "")).lower() not in {"image/jpeg", "image/png", "image/webp", "image/gif"}:
                continue
            image_url = info.get("thumburl") or info.get("url")
            try:
                result = await download_candidate(image_url, "commons")
                if result[0]:
                    log(f"Downloaded replacement image from Wikimedia Commons as '{result[1]}'.")
                    return result
            except Exception as candidate_error:
                log(f"Rejected Wikimedia Commons candidate '{page.get('title')}': {candidate_error}")
    except Exception as error:
        log(f"Wikimedia Commons image search failed: {error}")

    log(f"No usable illustrative image found for '{query_str}'. The original image will be preserved.")
    return None, ""

async def fetch_audio_base64_if_any(word: str, base_lang: str, scraped: dict, log_cb=None) -> tuple[str, str]:
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    audio_url = scraped.get("audio_url")
    audio_b64 = None
    if audio_url and audio_url.startswith("//"):
        audio_url = "https:" + audio_url

    if audio_url:
        log(f"Attempting to download audio from {audio_url}...")
        try:
            req = urllib.request.Request(audio_url, headers={"User-Agent": "Mozilla/5.0"})
            loop = asyncio.get_running_loop()
            audio_data = await loop.run_in_executor(None, lambda: urllib.request.urlopen(req, timeout=5.0).read())
            audio_b64 = base64.b64encode(audio_data).decode("utf-8")
            log("Audio downloaded successfully from source.")
            return audio_b64, audio_url
        except Exception as e:
            log(f"Failed to download audio from source: {e}. Falling back to TTS...")

    try:
        log(f"Generating TTS audio pronunciation for '{word}'...")
        loop = asyncio.get_running_loop()
        audio_b64 = await loop.run_in_executor(None, generate_tts_base64, word, base_lang)
        log("TTS audio generation completed.")
        return audio_b64, "tts"
    except Exception as e:
        log(f"TTS generation failed: {e}")
        return None, ""

async def fetch_kanji_construction_if_needed(word: str, scraper, app_config: dict, log_cb=None, llm_client=None) -> str:
    word = clean_word_field(word, app_config)
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    kanji_cfg = app_config.get("kanji", {})
    if not kanji_cfg.get("enabled", True):
        return ""

    kanji_chars = [c for c in word if "\u4e00" <= c <= "\u9fff"]
    if not kanji_chars:
        return ""

    log(f"Detected Kanji characters: {kanji_chars}. Fetching construction...")

    source_lang = str(kanji_cfg.get("source_lang", "english") or "english").strip().lower()
    url_template = kanji_cfg.get("url_template")
    schema = kanji_cfg.get("schema")

    # Self-healing logic for mixed config
    if source_lang == "english":
        if not url_template or "hvdic.thivien.net" in url_template:
            url_template = "https://jisho.org/search/{char}%23kanji"
            schema = None
        if not schema or schema.get("name") != "jisho_kanji":
            schema = {
                "name": "jisho_kanji",
                "baseSelector": ".kanji",
                "fields": [
                    {"name": "meanings", "selector": ".kanji-details__main-meanings", "type": "text"},
                    {"name": "strokes", "selector": ".kanji-details__stroke_count strong", "type": "text"},
                    {"name": "radical", "selector": ".radicals span", "type": "text"},
                    {"name": "parts", "selector": ".parts a", "type": "text"}
                ]
            }
    else: # vietnamese
        if not url_template or "jisho.org" in url_template:
            url_template = "https://hvdic.thivien.net/whv/{char}"
            schema = None
        if not schema or schema.get("name") != "hvdic_kanji":
            schema = {
                "name": "hvdic_kanji",
                "baseSelector": ".hvres",
                "fields": [
                    {"name": "spell", "selector": ".hvres-spell", "type": "text"},
                    {"name": "chi_tiet", "selector": ".hvres-details", "type": "text"},
                    {"name": "nghia", "selector": ".hvres-meaning", "type": "text"}
                ]
            }

    results = []
    for char in kanji_chars:
        try:
            char_info = await scraper.scrape_kanji_details(char, url_template, schema)
            if char_info:
                results.append(char_info)
                continue
            log(f"Kanji source returned no details for '{char}'.")
        except Exception as e:
            log(f"Kanji source failed for '{char}': {e}")

        # Try a source independent from the failed primary. English Jisho
        # failures use the KANJIDIC-backed JSON API; Vietnamese HVDic failures
        # first use Jisho so configured output remains as rich as possible.
        fallback_sources = (
            [("KanjiAPI", "https://kanjiapi.dev/v1/kanji/{char}", {"name": "kanjiapi"}),
             ("HVDic", "https://hvdic.thivien.net/whv/{char}", {"name": "hvdic_kanji"})]
            if source_lang == "english" else
            [("Jisho", "https://jisho.org/search/{char}%23kanji", {"name": "jisho_kanji"}),
             ("KanjiAPI", "https://kanjiapi.dev/v1/kanji/{char}", {"name": "kanjiapi"})]
        )
        for fallback_name, fallback_url, fallback_schema in fallback_sources:
            try:
                log(f"Falling back to {fallback_name} Kanji details for '{char}'...")
                fallback = await scraper.scrape_kanji_details(
                    char, fallback_url, fallback_schema,
                )
                if fallback:
                    results.append(fallback)
                    break
                log(f"{fallback_name} also returned no Kanji details for '{char}'.")
            except Exception as fallback_error:
                log(f"{fallback_name} Kanji fallback failed for '{char}': {fallback_error}")

    return "<br/>".join(results)

def dictionary_llm_context(scraped: dict, ocr_text: str = "") -> str:
    """Provide the LLM only parsed dictionary evidence and the raw OCR result."""
    if scraped and scraped.get("llm_context"):
        parsing = scraped["llm_context"]
    else:
        parsed_fields = {
            key: value
            for key, value in (scraped or {}).items()
            if key not in {"llm_context", "audio_url"}
        }
        parsing = json.dumps(parsed_fields, ensure_ascii=False, indent=2)
    return (
        "EXACT DICTIONARY PARSING (authoritative; do not rewrite it):\n"
        f"{parsing}\n\n"
        "RAW OCR RESULT (supporting context only):\n"
        f"{ocr_text or '(no OCR text available)'}"
    )

# --- DATA PROCESSING LOGIC ---
async def _process_legacy_card_single(anki_client, ocr_engine, scraper, app_config, note_info, is_grammar=False, log_cb=None, llm_client=None, deck_key=None, rate_limiter=None) -> dict:
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    note_deck = note_info.get("deckName", "")
    lang_key = deck_key
    if not lang_key:
        for k, cfg in app_config.get("decks", {}).items():
            if cfg.get("deck_name") == note_deck:
                lang_key = k
                break
    if not lang_key:
        lang_key = "japanese_vocab"

    base_lang = lang_key.split("_")[0]
    translation_lang = app_config.get("llm", {}).get("translation_language", "English")
    word_field = app_config["decks"].get(lang_key, {}).get("fields", {}).get("expression", "Word")
    note_fields = note_info.get("fields", {})
    word = note_info.get("_linguist_expression", "")
    if not word:
        word = note_fields.get(word_field, {}).get("value", "")
    if not word:
        for candidate in ("Expression", "Word", "Front", "Vocabulary"):
            if note_fields.get(candidate, {}).get("value"):
                word = note_fields[candidate]["value"]
                break
    word = clean_word_field(word, app_config)

    picture_field = app_config["decks"].get(lang_key, {}).get("fields", {}).get("meaning_image", "Picture")
    picture_html = note_fields.get(picture_field, {}).get("value", "")
    if not picture_html:
        for candidate in ("Picture", "Image", "Meaning Image", "MeaningImage"):
            value = note_fields.get(candidate, {}).get("value", "")
            if "<img" in value.lower():
                picture_html = value
                break
    if not picture_html:
        picture_html = next(
            (
                field.get("value", "") for field in note_fields.values()
                if "<img" in str(field.get("value", "")).lower()
            ),
            "",
        )

    # Extract original images
    orig_filenames = extract_image_filenames(picture_html)
    fallback_fn = ocr_engine.extract_image_filename(picture_html)
    if fallback_fn and fallback_fn not in orig_filenames:
        orig_filenames.append(fallback_fn)

    filename = orig_filenames[0] if orig_filenames else ""
    classification = "visual_recall"
    classification_result = {}
    classification_results = []
    ocr_text = str(note_info.get("_linguist_ocr_text") or "")
    retrieved_media = {}

    if orig_filenames:
        log(f"Detected {len(orig_filenames)} image(s) for word '{word}': {orig_filenames}")
        loop = asyncio.get_running_loop()
        lang_cfg = app_config.get("decks", {}).get(lang_key, {}).get("ocr_langs", "jpn+eng+vie")
        classifier_config = {
            **app_config.get("image_classification", {}),
            "ocr": app_config.get("ocr", {}),
        }
        ocr_sections = []
        for image_index, image_filename in enumerate(orig_filenames):
            log(f"Retrieving media file '{image_filename}' from Anki...")
            base64_data = await loop.run_in_executor(
                None, anki_client.retrieve_media_file, image_filename
            )
            retrieved_media[image_filename] = base64_data
            if not base64_data:
                result = {
                    "filename": image_filename,
                    "index": image_index,
                    "classification": "uncertain",
                    "probability": 0.5,
                    "source": "media-unavailable",
                    "reason": "Anki media could not be retrieved",
                    "ocr_text": "",
                }
                classification_results.append(result)
                continue
            log(f"Performing OCR on '{image_filename}' using language config '{lang_cfg}'...")
            image_ocr = await loop.run_in_executor(
                None, ocr_engine.perform_ocr, base64_data, lang_cfg,
                app_config.get("ocr", {}), log,
            )
            log(f"OCR complete for '{image_filename}'. Extracted {len(image_ocr)} characters.")
            log(f"Classifying '{image_filename}' using OCR, layout, and visual structure...")
            result = await loop.run_in_executor(
                None, ocr_engine.classify_image, base64_data, image_ocr,
                lang_cfg, classifier_config, llm_client,
            )
            result = {
                **result,
                "filename": image_filename,
                "index": image_index,
                "ocr_text": image_ocr,
            }
            classification_results.append(result)
            if image_ocr:
                ocr_sections.append(
                    f"[Image: {image_filename}; classification: "
                    f"{result.get('classification', 'uncertain')}]\n{image_ocr}"
                )
            log(
                f"Image '{image_filename}' classification: '{result['classification']}' "
                f"(dictionary probability={result['probability']:.2f}, "
                f"source={result['source']})"
            )
        states = [result.get("classification", "uncertain") for result in classification_results]
        if "uncertain" in states:
            classification = "uncertain"
        elif states and all(state == "dictionary" for state in states):
            classification = "dictionary"
        elif "dictionary" in states:
            classification = "mixed"
        else:
            classification = "visual_recall"
        classification_result = classification_results[0] if classification_results else {}
        ocr_text = "\n\n".join(ocr_sections)
        if classification == "uncertain":
            log("At least one image classification is uncertain; preserving it until user confirmation in Preview.")
    else:
        log(f"No screenshot image filename found in fields for word '{word}'.")
        classification = "No Image"

    # Rename original images and copy them to the application media cache.
    renamed_images = []
    temp_dir = get_media_cache_dir()

    import re
    import base64
    clean_word_for_fn = safe_media_stem(word, "word")

    for idx, orig_fn in enumerate(orig_filenames):
        try:
            b64_data = retrieved_media.get(orig_fn)
            if b64_data is None:
                log(f"Retrieving media file '{orig_fn}' from Anki for renaming...")
                loop = asyncio.get_running_loop()
                b64_data = await loop.run_in_executor(None, anki_client.retrieve_media_file, orig_fn)
                retrieved_media[orig_fn] = b64_data
            else:
                log(f"Reusing cached media file '{orig_fn}' for renaming...")
            if b64_data:
                ext = get_file_extension(orig_fn)
                new_fn = indexed_media_filename("img", clean_word_for_fn, ext, idx, lang_key)

                # Save a local preview copy.
                with open(temp_dir / new_fn, "wb") as f:
                    f.write(base64.b64decode(b64_data))

                renamed_images.append({
                    "original_name": orig_fn,
                    "new_name": new_fn,
                    "b64": b64_data,
                    "classification": next(
                        (result.get("classification") for result in classification_results
                         if result.get("filename") == orig_fn),
                        "uncertain",
                    ),
                })
                log(f"Cached '{orig_fn}' as '{new_fn}'.")
        except Exception as e:
            log(f"Failed to retrieve/rename '{orig_fn}': {e}")

    scraped = {"found": False}
    if not is_grammar:
        dict_cfg = app_config.get("dictionary", {})
        preset = dict_cfg.get("preset", base_lang)

        if preset == "custom":
            url_template = dict_cfg.get("url_template", "")
            schema = dict_cfg.get("schema", {})
            log(f"Querying custom dictionary scraper for '{word}'...")
            try:
                if rate_limiter:
                    await rate_limiter.wait("dictionary")
                scraped = await scraper.scrape_custom_dict(word, url_template, schema)
            except Exception as e:
                log(f"Custom dictionary scrape failed: {e}")
        else:
            active_dict = preset if preset in ["jisho", "cambridge", "moedict", "dict_cc"] else base_lang
            log(f"Querying standard dictionary '{active_dict}' for '{word}'...")
            try:
                if rate_limiter:
                    await rate_limiter.wait("dictionary")
                if active_dict == "japanese" or active_dict == "jisho":
                    scraped = await scraper.scrape_jisho(
                        word,
                        retry_count=dict_cfg.get("retry_count", 3),
                        backoff=dict_cfg.get("retry_backoff_seconds", 0.6),
                        browser_fallback=dict_cfg.get("browser_fallback", True),
                    )
                elif active_dict == "english" or active_dict == "cambridge":
                    scraped = await scraper.scrape_cambridge(word)
                elif active_dict == "taiwanese" or active_dict == "moedict":
                    scraped = await scraper.scrape_moedict(word)
                elif active_dict == "german" or active_dict == "dict_cc":
                    scraped = await scraper.scrape_dict_cc(word)

                if scraped.get("found"):
                    log(f"Scraper lookup successful. Reading: '{scraped.get('reading', '')}'.")
                else:
                    log("Scraper lookup found no matches.")
            except Exception as e:
                log(f"Dictionary scrape failed: {e}")

    llm_response = {}
    if not is_grammar:
        target_word = scraped.get("word") if scraped.get("word") else word
        prompt_system = app_config["llm"]["system_prompt_vocab"]
        log(f"Querying local Ollama model '{getattr(llm_client, 'model', None)}' for nuance and examples...")
        try:
            if rate_limiter:
                await rate_limiter.wait("ollama")
            loop = asyncio.get_running_loop()
            llm_response = await loop.run_in_executor(
                None, llm_client.generate_card_content, target_word, dictionary_llm_context(scraped, ocr_text), base_lang.capitalize(), translation_lang, prompt_system
            )
            log("Ollama response received successfully.")
        except Exception as e:
            log(f"Ollama nuance/example generation failed: {e}. Keeping dictionary parsing only.")
            llm_response = {
                "nuances": "",
                "examples": []
            }
    else:
        target_word = word
        prompt_system = app_config["llm"]["system_prompt_grammar"]
        log(f"Querying local Ollama model '{getattr(llm_client, 'model', None)}' for grammar explanation...")
        try:
            if rate_limiter:
                await rate_limiter.wait("ollama")
            loop = asyncio.get_running_loop()
            llm_response = await loop.run_in_executor(
                None, llm_client.generate_grammar_content, word, translation_lang, prompt_system
            )
            log("Ollama response received successfully.")
        except Exception as e:
            log(f"Ollama query failed: {e}.")
            llm_response = {
                "grammar_point": word,
                "meaning": "Grammar explanation not found.",
                "rules": "",
                "examples": []
            }

    if rate_limiter:
        await rate_limiter.wait("kanji")
    kanji_construction = await fetch_kanji_construction_if_needed(
        scraped.get("word") if scraped.get("word") else word,
        scraper, app_config, log_cb, llm_client
    )

    new_image_b64 = None
    new_image_filename = ""
    img_search_cfg = app_config.get("image_search", {})
    enabled_for_empty = img_search_cfg.get("enabled_for_empty", True)
    suffix = img_search_cfg.get("suffix", "")
    if not suffix and scraped.get("entries"):
        exact = scraped["entries"][0]
        visual_terms = []
        for sense in exact.get("senses", []):
            visual_terms.extend(sense.get("definitions", []))
            if len(visual_terms) >= 3:
                break
        suffix = " ".join(str(term) for term in visual_terms[:3])

    if (classification == "dictionary" or (classification == "No Image" and enabled_for_empty)) and not note_info.get("_linguist_skip_web_image"):
        log(f"Card has dictionary screenshot or no image. Fetching illustrative image from internet for '{word}'...")
        if rate_limiter:
            await rate_limiter.wait("image")
        new_image_b64, new_image_filename = await fetch_web_image(word, log_cb, suffix)
        if new_image_b64 and new_image_filename:
            try:
                with open(temp_dir / new_image_filename, "wb") as f:
                    f.write(base64.b64decode(new_image_b64))
                log(f"Cached illustrative web image as '{new_image_filename}'.")
            except Exception as e:
                log(f"Failed to cache web image: {e}")

    legacy_audio_assets = list(note_info.get("_linguist_audio_assets") or [])
    audio_b64 = legacy_audio_assets[0].get("b64") if legacy_audio_assets else None
    audio_filename = legacy_audio_assets[0].get("filename", "") if legacy_audio_assets else ""
    audio_assets = legacy_audio_assets
    if not is_grammar and not legacy_audio_assets:
        if rate_limiter:
            await rate_limiter.wait("tts")
        audio_b64, audio_src = await fetch_audio_base64_if_any(target_word, base_lang, scraped, log_cb)
        if audio_b64:
            audio_filename = indexed_media_filename(
                "audio", target_word, audio_extension(audio_src), scope=lang_key
            )
            try:
                with open(temp_dir / audio_filename, "wb") as f:
                    f.write(base64.b64decode(audio_b64))
                log(f"Cached audio as '{audio_filename}'.")
            except Exception as e:
                log(f"Failed to cache audio: {e}")
            audio_assets = [{
                "filename": audio_filename,
                "b64": audio_b64,
                "reading": str(scraped.get("reading") or ""),
            }]

    return {
        "word": word,
        "filename": filename,
        "orig_filenames": orig_filenames,
        "renamed_images": renamed_images,
        "ocr_text": ocr_text,
        "classification": classification,
        "classification_result": classification_result,
        "classification_results": classification_results,
        "scraped": scraped,
        "llm_response": llm_response,
        "kanji_construction": kanji_construction,
        "new_image_b64": new_image_b64,
        "new_image_filename": new_image_filename,
        "audio_b64": audio_b64,
        "audio_filename": audio_filename,
        "audio_assets": audio_assets,
    }


async def process_legacy_card(anki_client, ocr_engine, scraper, app_config, note_info, is_grammar=False, log_cb=None, llm_client=None, deck_key=None, rate_limiter=None) -> dict:
    """Modernize one legacy note, splitting only true multi-expression fields.

    OCR is performed once. Each expression still receives independent
    dictionary, LLM, Kanji, image, and audio treatment. Existing audio tracks
    are assigned by filename (then by order), while a single expression keeps
    every pronunciation track on the same managed note.
    """
    note_fields = note_info.get("fields") or {}
    lang_key = deck_key
    if not lang_key:
        note_deck = note_info.get("deckName", "")
        lang_key = next(
            (key for key, cfg in app_config.get("decks", {}).items()
             if cfg.get("deck_name") == note_deck),
            "japanese_vocab",
        )
    word_field = app_config.get("decks", {}).get(lang_key, {}).get("fields", {}).get("expression", "Word")
    raw_word = str((note_fields.get(word_field) or {}).get("value", ""))
    if not raw_word:
        for candidate in ("Expression", "Word", "Front", "Vocabulary"):
            raw_word = str((note_fields.get(candidate) or {}).get("value", ""))
            if raw_word:
                break
    expressions = extract_legacy_expressions(raw_word, app_config)
    if is_grammar or note_info.get("_linguist_disable_split"):
        return await _process_legacy_card_single(
            anki_client, ocr_engine, scraper, app_config, note_info, is_grammar,
            log_cb, llm_client, deck_key, rate_limiter,
        )

    def log(message: str) -> None:
        logging.info(message)
        if log_cb:
            log_cb(message)

    audio_entries = extract_legacy_audio_entries(note_info)
    for entry in audio_entries:
        try:
            # AnkiConnect is local and these payload lookups are normally a few
            # milliseconds.  Keeping the tiny ordered read here also avoids a
            # separate executor wake-up for every pronunciation track.
            entry["b64"] = anki_client.retrieve_media_file(entry["filename"]) or ""
        except Exception as exc:
            entry["b64"] = ""
            log(f"Could not retrieve legacy audio '{entry['filename']}': {exc}")

    if len(expressions) <= 1:
        child = copy.deepcopy(note_info)
        child["_linguist_audio_assets"] = audio_entries
        child["_linguist_disable_split"] = True
        return await _process_legacy_card_single(
            anki_client, ocr_engine, scraper, app_config, child, is_grammar,
            log_cb, llm_client, deck_key, rate_limiter,
        )

    log(f"Detected {len(expressions)} expressions in one legacy note; preparing a split preview.")

    def audio_for(expression: str, index: int) -> list[dict[str, str]]:
        exact = [entry for entry in audio_entries if expression in entry.get("filename", "")]
        if exact:
            return exact
        if len(audio_entries) == len(expressions):
            return [audio_entries[index]]
        return []

    results: list[dict] = []
    first: dict | None = None
    for index, expression in enumerate(expressions):
        child = copy.deepcopy(note_info)
        child["_linguist_expression"] = expression
        child["_linguist_disable_split"] = True
        child["_linguist_audio_assets"] = audio_for(expression, index)
        if first is not None:
            child["_linguist_ocr_text"] = first.get("ocr_text", "")
            # Avoid repeating OCR and media retrieval. Visual mnemonic images
            # are shared; dictionary screenshots are replaced per expression.
            for field in child.get("fields", {}).values():
                if isinstance(field, dict) and "<img" in str(field.get("value", "")).lower():
                    field["value"] = ""
            if first.get("classification") not in {"dictionary", "No Image"}:
                child["_linguist_skip_web_image"] = True
        result = await _process_legacy_card_single(
            anki_client, ocr_engine, scraper, app_config, child, is_grammar,
            log_cb, llm_client, deck_key, rate_limiter,
        )
        if first is None:
            first = result
        elif first.get("classification") not in {"dictionary", "No Image"}:
            for key in ("filename", "orig_filenames", "renamed_images", "classification",
                        "classification_result", "classification_results"):
                result[key] = copy.deepcopy(first.get(key))
        result["split_index"] = index
        result["split_count"] = len(expressions)
        results.append(result)

    combined = dict(results[0])
    combined["split_results"] = results
    combined["split_count"] = len(results)
    return combined

async def process_inject_item(scraper, llm_client, word_info: dict, lang_key: str, app_config: dict, log_cb=None) -> dict:
    """Enrich one explicit injection request for the canonical card builder.

    This function deliberately returns source data rather than Anki field
    names.  ``build_card_document(..., mode="inject")`` is the sole boundary
    that turns this result into a template-aware card.
    """
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    word = word_info["word"]
    if word:
        word = BeautifulSoup(word, "html.parser").get_text().strip()

    ocr_text = word_info.get("ocr_text", "")
    author_context = {
        "type_tag": str(word_info.get("type_tag", "") or "").strip(),
        "source_note": str(
            word_info.get("source_note", word_info.get("note", "")) or ""
        ).strip(),
    }

    loop = asyncio.get_running_loop()
    is_grammar = lang_key.endswith("grammar")
    base_lang = lang_key.split("_")[0]
    translation_lang = app_config.get("llm", {}).get("translation_language", "English")

    if is_grammar:
        llm_response = {}
        raw_text = word_info.get("raw_text", "")
        prompt_system = app_config["llm"]["system_prompt_grammar"]
        log(f"Querying local Ollama model '{getattr(llm_client, 'model', None)}' for grammar explanation...")
        try:
            llm_response = await loop.run_in_executor(
                None, llm_client.generate_grammar_content, raw_text if raw_text else word, translation_lang, prompt_system
            )
            log("Ollama response received successfully.")
        except Exception as e:
            log(f"Ollama query failed: {e}.")
            llm_response = {
                "grammar_point": word,
                "meaning": "Failed to parse grammar explanation.",
                "rules": "",
                "examples": []
            }

        kanji_construction = await fetch_kanji_construction_if_needed(word, scraper, app_config, log_cb, llm_client)

        return {
            **author_context,
            "word": word,
            "scraped": {"found": True, "definition": "Grammar explanation"},
            "suggestion": "",
            "is_conjugated": False,
            "llm_response": llm_response,
            "audio_b64": None,
            "audio_filename": "",
            "kanji_construction": kanji_construction,
            "new_image_b64": None,
            "new_image_filename": ""
        }

    dict_cfg = app_config.get("dictionary", {})
    preset = dict_cfg.get("preset", base_lang)

    scraped = {"found": False}
    if preset == "custom":
        url_template = dict_cfg.get("url_template", "")
        schema = dict_cfg.get("schema", {})
        log(f"Scraping custom dictionary for '{word}'...")
        try:
            scraped = await scraper.scrape_custom_dict(word, url_template, schema)
        except Exception as e:
            log(f"Custom dictionary scrape failed: {e}")
    else:
        active_dict = preset if preset in ["jisho", "cambridge", "moedict", "dict_cc"] else base_lang
        log(f"Scraping standard dictionary '{active_dict}' for '{word}'...")
        try:
            if active_dict == "japanese" or active_dict == "jisho":
                scraped = await scraper.scrape_jisho(
                    word,
                    retry_count=dict_cfg.get("retry_count", 3),
                    backoff=dict_cfg.get("retry_backoff_seconds", 0.6),
                    browser_fallback=dict_cfg.get("browser_fallback", True),
                )
            elif active_dict == "english" or active_dict == "cambridge":
                scraped = await scraper.scrape_cambridge(word)
            elif active_dict == "taiwanese" or active_dict == "moedict":
                scraped = await scraper.scrape_moedict(word)
            elif active_dict == "german" or active_dict == "dict_cc":
                scraped = await scraper.scrape_dict_cc(word)
        except Exception as e:
            log(f"Dictionary scrape failed: {e}")

    found = scraped.get("found", False)
    reading = scraped.get("reading", "")
    is_conjugated = scraped.get("is_conjugated", False)
    suggestion = scraped.get("suggestion", "")

    if found:
        log(f"Dictionary match found. Reading: '{reading}'.")
    else:
        log("No dictionary match found.")

    if not found and getattr(llm_client, "model", None):
        log(f"Querying Ollama to check if '{word}' is conjugated or misspelled...")
        lemma_data = await loop.run_in_executor(None, llm_client.lemmatize_word, word, base_lang)
        if lemma_data.get("suggestion"):
            suggestion = lemma_data["suggestion"]
            is_conjugated = True
            log(f"Ollama suggested base form: '{suggestion}' (Conjugated: True)")

    target_word = suggestion if suggestion else word

    async def generate_llm_annotations():
        if not (found or suggestion):
            log("No dictionary match and no LLM suggestion. Using default not-found card values.")
            return {"nuances": "", "examples": []}
        prompt_system = app_config["llm"]["system_prompt_vocab"]
        context_str = dictionary_llm_context(scraped, ocr_text)
        log(f"Querying local Ollama model '{getattr(llm_client, 'model', None)}' for nuance and examples...")
        try:
            response = await loop.run_in_executor(
                None, llm_client.generate_card_content, target_word, context_str, base_lang.capitalize(), translation_lang, prompt_system
            )
            log("Ollama response received successfully.")
            return response
        except Exception as e:
            log(f"Ollama nuance/example generation failed: {e}. Keeping dictionary parsing only.")
            return {"nuances": "", "examples": []}

    async def fetch_injection_image():
        log(f"Fetching illustrative image from Wikipedia for injection word '{target_word}'...")
        suffix = app_config.get("image_search", {}).get("suffix", "")
        return await fetch_web_image(target_word, log_cb, suffix)

    # These jobs are independent after dictionary resolution. Running them in
    # parallel avoids serial browser, model, image, and TTS wait time.
    llm_response, kanji_construction, image_result, audio_result = await asyncio.gather(
        generate_llm_annotations(),
        fetch_kanji_construction_if_needed(target_word, scraper, app_config, log_cb, llm_client),
        fetch_injection_image(),
        fetch_audio_base64_if_any(target_word, base_lang, scraped, log_cb),
    )
    new_image_b64, new_image_filename = image_result
    audio_b64, audio_src = audio_result
    audio_filename = ""
    if audio_b64:
        audio_filename = indexed_media_filename(
            "audio", target_word, audio_extension(audio_src), scope=lang_key
        )

    return {
        **author_context,
        "word": word,
        "scraped": scraped,
        "suggestion": suggestion,
        "is_conjugated": is_conjugated,
        "llm_response": llm_response,
        "ocr_text": ocr_text,
        "audio_b64": audio_b64,
        "audio_filename": audio_filename,
        "kanji_construction": kanji_construction,
        "new_image_b64": new_image_b64,
        "new_image_filename": new_image_filename
    }


# Compatibility for integrations using the pre-template terminology.
process_ingest_item = process_inject_item

def commit_card_modernization(
    anki_client, note_info, processed_data, lang_key, app_config,
    media_before: dict[str, str | None] | None = None,
) -> int:
    """Update an existing card through the managed template contract."""
    deck_cfg = app_config.get("decks", {}).get(lang_key)
    if not deck_cfg:
        raise ValueError(f"Unknown deck key: {lang_key}")
    if lang_key == "japanese_vocab":
        spec = japanese_vocab_template()
        deck_cfg = {**deck_cfg, "note_type": spec.model_name, "fields": spec.field_mapping()}
    split_results = list(processed_data.get("split_results") or [])
    if len(split_results) <= 1:
        document = build_card_document(processed_data, lang_key, "modernize")
        return commit_card_document(
            anki_client, document, deck_cfg, "modernize", note_info,
            media_before=media_before,
        )

    # Preserve the original note id (and therefore its existing scheduling)
    # for the first expression. The remaining expressions become managed
    # notes with the same legacy tags and all three managed card templates.
    original_fields = {
        str(name): str(field.get("value", "") if isinstance(field, dict) else field)
        for name, field in (note_info.get("fields") or {}).items()
    }
    original_model = str(note_info.get("modelName") or "")
    original_tags = list(note_info.get("tags") or [])
    original_note_id = int(note_info.get("noteId") or 0)
    created_note_ids: list[int] = []
    try:
        first_document = build_card_document(split_results[0], lang_key, "modernize")
        result_note_id = commit_card_document(
            anki_client, first_document, deck_cfg, "modernize", note_info,
            media_before=media_before,
        )
        for split_result in split_results[1:]:
            document = build_card_document(split_result, lang_key, "modernize")
            document.tags = list(original_tags)
            created = commit_card_document(
                anki_client, document, deck_cfg, "inject",
                media_before=media_before,
            )
            created_note_ids.append(int(created))
        processed_data["created_note_ids"] = created_note_ids
        return int(result_note_id)
    except Exception:
        if created_note_ids:
            try:
                anki_client.delete_notes(created_note_ids)
            except Exception as cleanup_exc:
                logging.warning("Could not delete partially created split notes: %s", cleanup_exc)
        if original_note_id and original_model:
            try:
                anki_client.update_note_model(
                    original_note_id, original_model, original_fields, original_tags,
                )
            except Exception as restore_exc:
                logging.error("Could not restore original note after split failure: %s", restore_exc)
        for filename, previous in (media_before or {}).items():
            try:
                if previous:
                    anki_client.store_media_file(filename, previous)
                else:
                    anki_client.delete_media_file(filename)
            except Exception as media_exc:
                logging.warning("Could not restore split media '%s': %s", filename, media_exc)
        raise


def commit_card_injection(
    anki_client, processed_data, lang_key, app_config,
    media_before: dict[str, str | None] | None = None,
) -> int:
    """Create a new card using the managed Japanese template contract."""
    deck_cfg = app_config.get("decks", {}).get(lang_key)
    if not deck_cfg:
        raise ValueError(f"Unknown deck key: {lang_key}")
    if lang_key == "japanese_vocab":
        spec = japanese_vocab_template()
        deck_cfg = {
            **deck_cfg,
            "note_type": spec.model_name,
            "fields": spec.field_mapping(),
        }
    document = build_card_document(processed_data, lang_key, "inject")
    return commit_card_document(
        anki_client, document, deck_cfg, "inject", media_before=media_before,
    )


# Backward-compatible API for callers created before “inject” became canonical.
commit_card_ingestion = commit_card_injection

class SelectionListModal(ModalScreen[str]):
    def __init__(self, title: str, choices: list):
        super().__init__()
        self.modal_title = title
        self.choices = choices

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-dialog-list"):
            yield Label(f"[bold accent]{self.modal_title}[/]", id="modal-title")
            items = [ListItem(Label(c), id=f"choice-{idx}") for idx, c in enumerate(self.choices)]
            yield ListView(*items, id="choices-list")

    def on_mount(self) -> None:
        self.query_one("#choices-list", ListView).focus()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if event.item and event.item.id:
            idx = int(event.item.id.replace("choice-", ""))
            self.dismiss(self.choices[idx])

    def key_escape(self) -> None:
        self.dismiss("")

def make_parallel_comparison(original_fields: dict, processed_data: dict, deck_cfg: dict, is_grammar: bool) -> Table:
    """Compare the actual legacy schema with the managed target schema."""
    table = Table(expand=True, show_lines=True)
    table.add_column("Anki Field", style="bold cyan", width=20)
    table.add_column("Before (Current Value)", style="dim white", ratio=1)
    table.add_column("After (Proposed Value)", style="green", ratio=1)

    fields = deck_cfg.get("fields") or {}
    kanji_field = fields.get("kanji_construction")
    meaning_field = fields.get("meaning_text")

    def display(field_name: str, value: str) -> str:
        if field_name == kanji_field or "kanji" in field_name.lower():
            return kanji_summary_for_tui(value)
        if field_name == meaning_field:
            return meaning_summary_for_tui(value)
        images = extract_image_filenames(value)
        return escape(", ".join(images)) if images else clean_html_for_tui(value)

    target_fields = {}
    if processed_data:
        lang_key = "japanese_grammar" if is_grammar else "japanese_vocab"
        document = build_card_document(processed_data, lang_key, "modernize")
        target_fields = map_document_fields(document, deck_cfg)

    field_names = list(original_fields)
    field_names.extend(name for name in target_fields if name not in original_fields)
    for field_name in field_names:
        before_raw = original_fields.get(field_name, {}).get("value", "")
        before = display(field_name, before_raw) if field_name in original_fields else "[dim]—[/]"
        if not processed_data:
            after = "[dim]—[/]"
        elif field_name not in target_fields:
            after = "[dim](Not in managed template)[/]"
        else:
            proposed = display(field_name, target_fields[field_name])
            after = (
                "[dim white](Unchanged)[/]"
                if before_raw and display(field_name, before_raw) == proposed
                else proposed or "[dim red](Cleared)[/]"
            )
        table.add_row(field_name, before or "[dim](Empty)[/]", after)
    return table

def make_inject_comparison_table(processed_data: dict, deck_cfg: dict) -> Table:
    table = Table(expand=True, show_lines=True)
    table.add_column("Anki Field Name", style="bold cyan", width=18)
    table.add_column("Proposed Value", style="green", ratio=1)

    fields_map = deck_cfg.get("fields", {})
    word = processed_data.get("suggestion") if (processed_data and processed_data.get("suggestion")) else (processed_data.get("word", "") if processed_data else "")
    llm_res = processed_data.get("llm_response") if processed_data else None
    is_grammar = deck_cfg.get("deck_name", "").lower().find("grammar") != -1 or (processed_data and processed_data.get("scraped", {}).get("definition") == "Grammar explanation")
    base_lang = deck_cfg.get("deck_name", "").lower().split(" ")[0] if deck_cfg.get("deck_name") else "japanese"

    for purpose, field_name in fields_map.items():
        if not field_name:
            continue

        val = "(Press 'p' to process preview)"
        if processed_data and llm_res:
            if purpose == "expression":
                val = escape(word)
            elif purpose == "meaning_image":
                img_filename = processed_data.get("new_image_filename")
                if img_filename:
                    val = escape(f"[Image: {img_filename}]")
                else:
                    val = "(No Image)"
            elif purpose == "meaning_text":
                if is_grammar:
                    m_html = format_anki_grammar_html(
                        llm_res.get("grammar_point", word),
                        llm_res.get("meaning", ""),
                        llm_res.get("rules", ""),
                        llm_res.get("examples", [])
                    )
                else:
                    m_html = format_dictionary_meaning_html(
                        processed_data.get("scraped", {}),
                        word,
                        llm_res.get("nuances", ""),
                        llm_res.get("examples", [])
                    )
                val = meaning_summary_for_tui(m_html)
            elif purpose == "kanji_construction":
                kanji_html = processed_data.get("kanji_construction", "")
                val = kanji_summary_for_tui(kanji_html) if kanji_html else "(No Kanji Details)"
            elif purpose == "audio":
                audio_filename = processed_data.get("audio_filename")
                if audio_filename:
                    has_kanji = any("\u4e00" <= c <= "\u9fff" for c in word)
                    reading = processed_data.get("scraped", {}).get("reading", "")
                    if reading and has_kanji:
                        val = escape(f"{reading} [sound:{audio_filename}]")
                    else:
                        val = escape(f"[sound:{audio_filename}]")
                else:
                    val = "(No Audio)"
        elif processed_data:
            if purpose == "expression":
                val = escape(word)
            else:
                val = "(Awaiting Ollama analysis...)"
        table.add_row(field_name, val)

    return table


make_ingest_comparison_table = make_inject_comparison_table

# --- ADVANCED MODALS ---
from textual import events
from pathlib import Path

class BottomRightPaletteModal(ModalScreen[str]):
    def __init__(self, title: str, choices: list[tuple[str, str]]):
        super().__init__()
        self.modal_title = title
        self.choices = choices

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-bottom-right-palette"):
            yield Label(f"[bold accent]{self.modal_title}[/]", id="modal-title")
            items = []
            for idx, (label, val) in enumerate(self.choices):
                items.append(ListItem(Label(f"{idx + 1}. {label}"), id=f"choice-{idx}"))
            yield ListView(*items, id="choices-list")

    def on_mount(self) -> None:
        self.query_one("#choices-list", ListView).focus()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if event.item and event.item.id:
            idx = int(event.item.id.replace("choice-", ""))
            self.dismiss(self.choices[idx][1])

    def key_escape(self) -> None:
        self.dismiss("")

    def key_1(self) -> None: self.select_idx(0)
    def key_2(self) -> None: self.select_idx(1)
    def key_3(self) -> None: self.select_idx(2)
    def key_4(self) -> None: self.select_idx(3)
    def key_5(self) -> None: self.select_idx(4)
    def key_6(self) -> None: self.select_idx(5)
    def key_7(self) -> None: self.select_idx(6)
    def key_8(self) -> None: self.select_idx(7)
    def key_9(self) -> None: self.select_idx(8)

    def select_idx(self, idx: int) -> None:
        if idx < len(self.choices):
            self.dismiss(self.choices[idx][1])

class SpacebarMenuModal(ModalScreen[str]):
    BINDINGS = [
        Binding("i", "choose_inject", "Manual Input", priority=True),
        Binding("b", "choose_batch", "Batch Jobs", priority=True),
        Binding("s", "choose_settings", "Settings", priority=True),
        Binding("space", "choose_search", "Search", priority=True),
        Binding("escape", "cancel", "Cancel", priority=True),
    ]

    def __init__(self) -> None:
        super().__init__(id="screen-spacebar-menu")

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-bottom-right-palette"):
            yield Label("[bold accent]Bridge Palette Menu[/]", id="modal-title")
            yield Label("[bold]i[/] - Manual Input")
            yield Label("[bold]b[/] - Batch Modernization Jobs")
            yield Label("[bold]s[/] - Settings")
            yield Label("[bold]space[/] - Universal Pane Search")

    def on_mount(self) -> None:
        pass

    def action_choose_inject(self) -> None:
        self.dismiss("inject")

    def action_choose_settings(self) -> None:
        self.dismiss("settings")

    def action_choose_batch(self) -> None:
        self.dismiss("batch")

    def action_choose_search(self) -> None:
        self.dismiss("search")

    def action_cancel(self) -> None:
        self.dismiss("")

class FieldEditModal(ModalScreen[str]):
    def __init__(self, field_name: str, current_value: str):
        super().__init__()
        self.field_name = field_name
        self.current_value = current_value
        self.mode = "normal"
        self.search_mode = False
        self.command_mode = False
        self.search_pattern = ""
        self.search_matches = []
        self.current_match_idx = 0

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-edit-field"):
            yield Label(f"[bold accent]Edit Field: {self.field_name}[/]")
            with Horizontal(id="field-search-top"):
                yield Input(placeholder="Press / to search or : for a command", id="field-search-input")
                yield Label("[NORMAL]", id="field-search-status")
            yield Rule()
            yield TextArea(self.current_value, id="edit-textarea")
            yield Label("[dim]NORMAL: i insert · arrows move · / search · :w save · :q cancel[/]", classes="modal-footer-hint")

    def on_mount(self) -> None:
        ta = self.query_one("#edit-textarea", TextArea)
        ta.read_only = True
        ta.focus()
        lines = ta.text.splitlines()
        ta.cursor_location = (len(lines) - 1, len(lines[-1]) if lines else 0)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "field-search-input":
            value = event.value.strip()
            if self.command_mode:
                if value in {"w", "wq", "x"}:
                    self.dismiss(self.query_one("#edit-textarea", TextArea).text)
                elif value in {"q", "q!"}:
                    self.dismiss(None)
                else:
                    self.notify(f"Unknown command: {value}", severity="warning")
                    self.enter_normal_mode()
            else:
                self.perform_field_search(value)

    def update_editor_mode(self) -> None:
        status = self.query_one("#field-search-status", Label)
        if self.search_mode and self.search_matches:
            status.update(f"[SEARCH {self.current_match_idx + 1}/{len(self.search_matches)}]")
        else:
            status.update(f"[{self.mode.upper()}]")

    def enter_normal_mode(self) -> None:
        self.mode = "normal"
        self.search_mode = False
        self.command_mode = False
        self.search_pattern = ""
        self.search_matches = []
        search = self.query_one("#field-search-input", Input)
        search.value = ""
        search.placeholder = "Press / to search or : for a command"
        ta = self.query_one("#edit-textarea", TextArea)
        ta.read_only = True
        ta.focus()
        self.update_editor_mode()

    def enter_prompt_mode(self, command: bool) -> None:
        self.command_mode = command
        self.mode = "command" if command else "search"
        search = self.query_one("#field-search-input", Input)
        search.value = ""
        search.placeholder = ":w save · :q cancel" if command else "/pattern, then Enter"
        search.focus()
        self.update_editor_mode()

    def perform_field_search(self, pattern: str) -> None:
        self.search_pattern = pattern
        status_lbl = self.query_one("#field-search-status", Label)
        if not pattern:
            self.search_mode = False
            self.search_matches = []
            status_lbl.update("")
            return

        ta = self.query_one("#edit-textarea", TextArea)
        text = ta.text
        self.search_matches = []
        lines = text.splitlines()
        for r, line in enumerate(lines):
            start = 0
            while True:
                idx = line.lower().find(pattern.lower(), start)
                if idx == -1:
                    break
                self.search_matches.append((r, idx))
                start = idx + len(pattern)

        if self.search_matches:
            self.search_mode = True
            self.command_mode = False
            self.mode = "search"
            self.current_match_idx = 0
            ta.focus()
            self.focus_search_match()
        else:
            self.search_mode = False
            status_lbl.update("[SEARCH: no matches]")
            self.notify("No matches found in text.")

    def focus_search_match(self) -> None:
        if not self.search_matches:
            return
        coord = self.search_matches[self.current_match_idx]
        ta = self.query_one("#edit-textarea", TextArea)
        ta.cursor_location = coord
        from textual.widgets.text_area import Selection
        end_coord = (coord[0], coord[1] + len(self.search_pattern))
        ta.selection = Selection(coord, end_coord)

        status_lbl = self.query_one("#field-search-status", Label)
        self.update_editor_mode()

    def search_next(self) -> None:
        if not self.search_matches:
            return
        self.current_match_idx = (self.current_match_idx + 1) % len(self.search_matches)
        self.focus_search_match()

    def search_prev(self) -> None:
        if not self.search_matches:
            return
        self.current_match_idx = (self.current_match_idx - 1) % len(self.search_matches)
        self.focus_search_match()

    def exit_field_search_mode(self) -> None:
        ta = self.query_one("#edit-textarea", TextArea)
        from textual.widgets.text_area import Selection
        ta.selection = Selection(ta.selection.start, ta.selection.start)
        self.enter_normal_mode()

    def move_normal_cursor(self, key: str) -> None:
        ta = self.query_one("#edit-textarea", TextArea)
        row, column = ta.cursor_location
        lines = ta.text.splitlines() or [""]
        if key == "left":
            column = max(0, column - 1)
        elif key == "right":
            column = min(len(lines[row]), column + 1)
        elif key == "down":
            row = min(len(lines) - 1, row + 1)
            column = min(column, len(lines[row]))
        elif key == "up":
            row = max(0, row - 1)
            column = min(column, len(lines[row]))
        elif key == "0":
            column = 0
        ta.cursor_location = (row, column)

    def on_key(self, event: events.Key) -> None:
        if event.key == "escape":
            event.prevent_default()
            if self.search_mode:
                self.exit_field_search_mode()
            else:
                self.enter_normal_mode()
        elif self.mode == "normal" and event.key == "i":
            event.prevent_default()
            self.mode = "insert"
            self.query_one("#edit-textarea", TextArea).read_only = False
            self.update_editor_mode()
        elif self.mode == "normal" and event.key == "a":
            event.prevent_default()
            ta = self.query_one("#edit-textarea", TextArea)
            row, column = ta.cursor_location
            ta.cursor_location = (row, min(len((ta.text.splitlines() or [""])[row]), column + 1))
            ta.read_only = False
            self.mode = "insert"
            self.update_editor_mode()
        elif self.mode == "normal" and (event.character == "/" or event.key == "slash"):
            event.prevent_default()
            self.enter_prompt_mode(False)
        elif self.mode == "normal" and (event.character == ":" or event.key == "colon"):
            event.prevent_default()
            self.enter_prompt_mode(True)
        elif self.mode == "normal" and event.key in {"left", "down", "up", "right", "home"}:
            event.prevent_default()
            self.move_normal_cursor("0" if event.key == "home" else event.key)
        elif event.key == "n":
            if self.search_mode:
                if self.focused and self.focused.id == "field-search-input":
                    return
                event.prevent_default()
                self.search_next()
        elif event.key == "p":
            if self.search_mode:
                if self.focused and self.focused.id == "field-search-input":
                    return
                event.prevent_default()
                self.search_prev()

class LogViewerModal(ModalScreen[None]):
    def __init__(self, date_str: str, file_path: Path):
        super().__init__()
        self.date_str = date_str
        self.file_path = file_path
        self.matches = []
        self.current_match_idx = 0
        self.search_pattern = ""

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-log-viewer"):
            yield Label(f"[bold accent]Log History Viewer: {self.date_str}[/]")
            with Horizontal(id="log-viewer-top"):
                yield Input(placeholder="Search pattern...", id="log-search-input")
                yield Label("0 matches", id="log-search-status")
            yield Rule()
            yield TextArea(read_only=True, id="log-viewer-textarea")

    def on_mount(self) -> None:
        try:
            with open(self.file_path, "r", encoding="utf-8") as f:
                content = f.read()
            ta = self.query_one("#log-viewer-textarea", TextArea)
            ta.text = content
            lines = content.splitlines()
            ta.cursor_location = (len(lines) - 1, 0)
        except Exception as e:
            self.query_one("#log-viewer-textarea", TextArea).text = f"Failed to load log file: {e}"

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "log-search-input":
            self.search_pattern = event.value.strip()
            self.recalculate_matches()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "log-search-input":
            self.query_one("#log-viewer-textarea", TextArea).focus()

    def recalculate_matches(self) -> None:
        pattern = self.search_pattern
        status_lbl = self.query_one("#log-search-status", Label)
        if not pattern:
            self.matches = []
            status_lbl.update("0 matches")
            return

        ta = self.query_one("#log-viewer-textarea", TextArea)
        text = ta.text
        self.matches = []
        lines = text.splitlines()
        for r, line in enumerate(lines):
            start = 0
            while True:
                idx = line.lower().find(pattern.lower(), start)
                if idx == -1:
                    break
                self.matches.append((r, idx))
                start = idx + len(pattern)

        if self.matches:
            self.current_match_idx = 0
            self.focus_match()
        else:
            status_lbl.update("0 matches")

    def focus_match(self) -> None:
        if not self.matches:
            return
        coord = self.matches[self.current_match_idx]
        ta = self.query_one("#log-viewer-textarea", TextArea)
        ta.cursor_location = coord
        from textual.widgets.text_area import Selection
        end_coord = (coord[0], coord[1] + len(self.search_pattern))
        ta.selection = Selection(coord, end_coord)

        status_lbl = self.query_one("#log-search-status", Label)
        status_lbl.update(f"[{self.current_match_idx + 1}/{len(self.matches)}]")

    def action_next_match(self) -> None:
        if not self.matches:
            return
        self.current_match_idx = (self.current_match_idx + 1) % len(self.matches)
        self.focus_match()

    def action_prev_match(self) -> None:
        if not self.matches:
            return
        self.current_match_idx = (self.current_match_idx - 1) % len(self.matches)
        self.focus_match()

    def on_key(self, event: events.Key) -> None:
        if event.key == "n":
            if self.focused and self.focused.id == "log-search-input":
                return
            event.prevent_default()
            self.action_next_match()
        elif event.key == "p":
            if self.focused and self.focused.id == "log-search-input":
                return
            event.prevent_default()
            self.action_prev_match()
        elif event.key == "escape":
            event.prevent_default()
            self.dismiss()

class UniversalSearchModal(ModalScreen[str]):
    def compose(self) -> ComposeResult:
        with Vertical(id="modal-universal-search"):
            yield Label("[bold accent]Universal Search[/]")
            yield Label("[dim]Enter query to search in focused pane, Enter to submit[/]\n")
            yield Input(placeholder="Search text...", id="search-input")

    def on_mount(self) -> None:
        self.query_one("#search-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        event.stop()
        self.dismiss(event.value.strip())

    def key_escape(self) -> None:
        self.dismiss("")

class LoadingScreen(Screen):
    BINDINGS = [
        ("q", "quit", "Quit App")
    ]

    def __init__(self, app_inst) -> None:
        super().__init__()
        self.app_inst = app_inst
        self.step_statuses = {
            "anki": "Pending",
            "ollama": "Pending",
            "cards": "Pending"
        }
        self.instructions = ""
        self.is_checking = False

    def compose(self) -> ComposeResult:
        with Vertical(id="loading-container"):
            yield Label("[bold accent]Linguist Anki Bridge[/] - System Initialization\n", id="loading-title")
            yield Static("", id="loading-status-area")
            yield Label("", id="loading-instructions")
            yield Label("\\[q] Quit", id="loading-keys-hint")

    def on_mount(self) -> None:
        self.check_worker = self.run_worker(self.run_checks())

    def update_display(self) -> None:
        status_text = ""

        def format_status(status):
            if status == "Pending":
                return "[yellow]⏳ Pending[/]"
            elif status == "Checking":
                return "[cyan]🔄 Checking...[/]"
            elif status == "Success":
                return "[green]✔ Success[/]"
            else:
                return "[red]✘ Failed[/]"

        status_text += f"Connecting to local AnkiConnect... {format_status(self.step_statuses['anki'])}\n"
        status_text += f"Connecting to local Ollama... {format_status(self.step_statuses['ollama'])}\n"
        status_text += f"Fetching initial card data... {format_status(self.step_statuses['cards'])}\n"

        self.query_one("#loading-status-area", Static).update(status_text)

        if self.instructions:
            self.query_one("#loading-instructions", Label).update(f"[bold red]Instructions:[/]\n{self.instructions}")
        else:
            self.query_one("#loading-instructions", Label).update("")

    async def run_checks(self) -> None:
        if self.is_checking:
            return
        self.is_checking = True

        try:
            while True:
                self.step_statuses = {
                    "anki": "Checking",
                    "ollama": "Checking",
                    "cards": "Checking"
                }
                self.instructions = ""
                self.update_display()

                loop = asyncio.get_running_loop()

                async def check_anki():
                    try:
                        anki_online = await loop.run_in_executor(None, self.app_inst.anki.is_online)
                        if anki_online:
                            self.step_statuses["anki"] = "Success"
                            return True
                    except Exception:
                        pass
                    self.step_statuses["anki"] = "Failed"
                    return False

                async def check_ollama():
                    try:
                        ollama_online = await loop.run_in_executor(None, self.app_inst.ollama.is_online)
                        if ollama_online:
                            self.step_statuses["ollama"] = "Success"
                            return True
                    except Exception:
                        pass
                    self.step_statuses["ollama"] = "Failed"
                    return False

                async def check_cards():
                    try:
                        if not self.app_inst.config_manager.config["decks"].get(self.app_inst.active_deck_key, {}).get("deck_name"):
                            for candidate, candidate_cfg in self.app_inst.config_manager.config["decks"].items():
                                if candidate_cfg.get("deck_name"):
                                    self.app_inst.active_deck_key = candidate
                                    break
                        deck_cfg = self.app_inst.config_manager.config["decks"][self.app_inst.active_deck_key]
                        deck_name = deck_cfg["deck_name"]
                        field_img = deck_cfg["fields"]["meaning_image"]

                        note_ids = await loop.run_in_executor(None, self.app_inst.anki.find_notes, f"deck:\"{deck_name}\"")
                        notes = await loop.run_in_executor(None, self.app_inst.anki.get_notes_info, note_ids)

                        self.app_inst.queue_items = []
                        self.app_inst.processed_cache = {}

                        for note in notes:
                            field_val = note.get("fields", {}).get(field_img, {}).get("value", "")
                            img_file = self.app_inst.ocr.extract_image_filename(field_val)
                            if img_file:
                                self.app_inst.queue_items.append({
                                    "type": "modernize",
                                    "deck_key": self.app_inst.active_deck_key,
                                    "note_id": note["noteId"],
                                    "note": note,
                                    "word": note.get("fields", {}).get(deck_cfg["fields"].get("expression", "Word"), {}).get("value", ""),
                                    "filename": img_file,
                                    "status": "Ready",
                                    "selected": False
                                })

                        if not self.app_inst.queue_items:
                            deck_keys = [
                                "japanese_vocab", "japanese_grammar",
                                "english_vocab", "english_grammar",
                                "taiwanese_vocab", "taiwanese_grammar",
                                "german_vocab", "german_grammar"
                            ]
                            for key in deck_keys:
                                if key == self.app_inst.active_deck_key:
                                    continue
                                d_cfg = self.app_inst.config_manager.config["decks"].get(key)
                                if d_cfg and d_cfg.get("deck_name"):
                                    d_name = d_cfg["deck_name"]
                                    d_img = d_cfg["fields"]["meaning_image"]
                                    d_ids = await loop.run_in_executor(None, self.app_inst.anki.find_notes, f"deck:\"{d_name}\"")
                                    d_notes = await loop.run_in_executor(None, self.app_inst.anki.get_notes_info, d_ids)
                                    for d_note in d_notes:
                                        df_val = d_note.get("fields", {}).get(d_img, {}).get("value", "")
                                        d_img_file = self.app_inst.ocr.extract_image_filename(df_val)
                                        if d_img_file:
                                            self.app_inst.queue_items.append({
                                                "type": "modernize",
                                                "deck_key": key,
                                                "note_id": d_note["noteId"],
                                                "note": d_note,
                                                "word": d_note.get("fields", {}).get(d_cfg["fields"].get("expression", "Word"), {}).get("value", ""),
                                                "filename": d_img_file,
                                                "status": "Ready",
                                                "selected": False
                                            })
                                    if self.app_inst.queue_items:
                                        self.app_inst.active_deck_key = key
                                        break

                        self.app_inst.deck_queues[self.app_inst.active_deck_key] = self.app_inst.queue_items
                        self.app_inst.rebuild_queue_table()
                        self.app_inst.update_details()
                        self.app_inst.initial_scan_done = True

                        # A reachable deck with zero legacy cards is a valid,
                        # ready state rather than a startup failure.
                        self.step_statuses["cards"] = "Success"
                        return True
                    except Exception as e:
                        logging.error(f"Failed to fetch initial card data: {e}")
                    self.step_statuses["cards"] = "Failed"
                    return False

                await asyncio.gather(
                    check_anki(),
                    check_ollama(),
                    check_cards(),
                    return_exceptions=True
                )
                self.update_display()

                anki_ok = self.step_statuses["anki"] == "Success"
                ollama_ok = self.step_statuses["ollama"] == "Success"
                cards_ok = self.step_statuses["cards"] == "Success"

                if anki_ok and ollama_ok and cards_ok:
                    self.app_inst.health_status = {
                        "anki": True,
                        "ollama": True,
                        "jisho": True,
                        "cambridge": True,
                        "moedict": True,
                        "dict_cc": True
                    }
                    self.app_inst.update_status_table()

                    self.app_inst.run_worker(self.app_inst.health_loop())
                    self.app_inst.run_worker(self.app_inst.load_ollama_models())
                    self.dismiss()
                    break

                # Build specific instructions for failure
                base_instructions = ""
                if not anki_ok:
                    base_instructions = (
                        "AnkiConnect is offline. Please make sure:\n"
                        "- Anki is open and running.\n"
                        "- AnkiConnect add-on is installed.\n"
                        "- Configuration url matches http://localhost:8765"
                    )
                elif not ollama_ok:
                    base_instructions = (
                        "Ollama is offline. Please make sure:\n"
                        "- Ollama is running ('ollama serve').\n"
                        f"- Model '{self.app_inst.ollama.model}' is installed.\n"
                        "- Ollama service is accessible."
                    )
                else:
                    base_instructions = (
                        "Could not load initial card data from Anki.\n"
                        "Please verify that your deck configurations match actual decks in Anki."
                    )

                for sec in range(3, 0, -1):
                    self.instructions = base_instructions + f"\n\n[yellow]Retrying automatically in {sec} seconds...[/]"
                    self.update_display()
                    await asyncio.sleep(1.0)
        finally:
            self.is_checking = False

    def action_quit(self) -> None:
        self.app_inst.exit()

class SearchNavigationScreen(Screen):
    BINDINGS = [
        ("n", "search_next", "Next Match"),
        ("p", "search_prev", "Prev Match"),
        ("escape", "exit_search", "Exit Search")
    ]

    def compose(self) -> ComposeResult:
        yield Footer()

    def on_mount(self) -> None:
        try:
            self.app.query_one("Header").add_class("header-search-active")
        except Exception:
            pass
        self.update_header()

    def on_unmount(self) -> None:
        try:
            self.app.query_one("Header").remove_class("header-search-active")
        except Exception:
            pass
        self.app.title = "Linguist Anki Bridge"

    def update_header(self) -> None:
        if not self.app.search_matches:
            self.app.title = f"🔍 [SEARCH: {self.app.search_query}] - No Matches"
        else:
            self.app.title = f"🔍 [SEARCH: {self.app.search_query}] Match {self.app.search_current_idx + 1}/{len(self.app.search_matches)}"

    def action_search_next(self) -> None:
        self.app.search_next_match()
        self.update_header()

    def action_search_prev(self) -> None:
        self.app.search_prev_match()
        self.update_header()

    def action_exit_search(self) -> None:
        self.app.exit_search_mode()
        self.dismiss()
