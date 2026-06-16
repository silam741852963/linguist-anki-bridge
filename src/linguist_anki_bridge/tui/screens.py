import json
import logging
import asyncio
import os
import csv
from bs4 import BeautifulSoup
from textual.app import ComposeResult
from textual.containers import Vertical, Horizontal, ScrollableContainer
from textual.screen import ModalScreen
from textual.widgets import Label, Button, Input, Static, TextArea, ListView, ListItem
from rich.table import Table
from rich.panel import Panel
from rich.console import Group
from rich.text import Text

from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.scraper import Crawl4AiScraper
from linguist_anki_bridge.tts import generate_tts_base64
from linguist_anki_bridge.utils import create_deck_backup

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

# --- DETAILS PANE ---
class DetailsPane(ScrollableContainer):
    can_focus = True

    def compose(self) -> ComposeResult:
        yield Static("", id="details-comparison")
        yield Static("", id="details-logs")

    def update_content(self, markup: str) -> None:
        self.update_comparison(markup)

    def update_comparison(self, markup) -> None:
        try:
            self.query_one("#details-comparison", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update DetailsPane comparison: {e}")
            
    def update_logs(self, markup: str) -> None:
        try:
            self.query_one("#details-logs", Static).update(markup)
        except Exception as e:
            logging.error(f"Failed to update DetailsPane logs: {e}")

# --- HTML FORMATTERS ---
def format_anki_meaning_html(definition: str, nuances: str, examples: list) -> str:
    html = f"<div><b>Definition:</b> {definition}</div>"
    if nuances:
        html += f"<div style='margin-top: 5px; font-style: italic; color: #888;'><b>Nuance:</b> {nuances}</div>"
    if examples:
        html += "<div style='margin-top: 10px;'><b>Examples:</b><ol style='margin: 5px 0; padding-left: 20px;'>"
        for ex in examples:
            sentence = ex.get("sentence", "")
            translation = ex.get("translation", "")
            html += f"<li style='margin-bottom: 3px;'><b>{sentence}</b><br/><span style='color: #666; font-size: 0.9em;'>{translation}</span></li>"
        html += "</ol></div>"
    return html

def format_anki_grammar_html(grammar_point: str, meaning: str, rules: str, examples: list) -> str:
    html = f"<div><b>Grammar Point:</b> <span style='font-size: 1.2em; color: #e68e0d;'>{grammar_point}</span></div>"
    html += f"<div style='margin-top: 5px;'><b>Meaning:</b> {meaning}</div>"
    if rules:
        html += f"<div style='margin-top: 5px;'><b>Structure/Rules:</b> <pre style='background: #f4f4f4; padding: 5px; border-radius: 3px; font-family: monospace;'>{rules}</pre></div>"
    if examples:
        html += "<div style='margin-top: 10px;'><b>Examples:</b><ol style='margin: 5px 0; padding-left: 20px;'>"
        for ex in examples:
            sentence = ex.get("sentence", "")
            translation = ex.get("translation", "")
            html += f"<li style='margin-bottom: 3px;'><b>{sentence}</b><br/><span style='color: #666; font-size: 0.9em;'>{translation}</span></li>"
        html += "</ol></div>"
    return html

# --- DATA PROCESSING LOGIC ---
async def process_legacy_card(anki_client, ocr_engine, scraper, app_config, note_info, is_grammar=False, log_cb=None) -> dict:
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    word = note_info.get("fields", {}).get("Word", {}).get("value", "")
    picture_html = note_info.get("fields", {}).get("Picture", {}).get("value", "")
    
    filename = ocr_engine.extract_image_filename(picture_html)
    if not filename:
        log(f"No screenshot image filename found in fields for word '{word}'.")
        return {
            "word": word,
            "filename": None,
            "ocr_text": "",
            "classification": "No Image",
            "scraped": {"found": False},
            "llm_response": None
        }
        
    log(f"Extracted image filename: '{filename}' for word '{word}'")
    
    # Retrieve media base64
    log(f"Retrieving media file '{filename}' from Anki...")
    loop = asyncio.get_running_loop()
    base64_data = await loop.run_in_executor(None, anki_client.retrieve_media_file, filename)
    
    # OCR
    lang_cfg = "jpn+eng+vie"
    if is_grammar:
        lang_cfg = app_config["decks"]["japanese"]["ocr_langs"]
    log(f"Performing OCR on '{filename}' using language config '{lang_cfg}'...")
    ocr_text = await loop.run_in_executor(None, ocr_engine.perform_ocr, base64_data, lang_cfg, app_config.get("ocr", {}))
    log(f"OCR complete. Extracted {len(ocr_text)} characters.")
    
    # Classify image
    is_dict = ocr_engine.is_dictionary_screenshot(ocr_text)
    classification = "dictionary" if is_dict else "visual_recall"
    log(f"Image classification: '{classification}' (Dictionary screen: {is_dict})")
    
    # Dictionary lookup for modernization transparency
    scraped = {"found": False}
    lang_key = None
    note_deck = note_info.get("deckName", "")
    for k, cfg in app_config.get("decks", {}).items():
        if cfg.get("deck_name") == note_deck:
            lang_key = k
            break
    if not lang_key:
        lang_key = "japanese"
        
    if not is_grammar:
        log(f"Querying dictionary scraper for '{word}' ({lang_key})...")
        try:
            if lang_key == "japanese":
                scraped = await scraper.scrape_jisho(word)
            elif lang_key == "english":
                scraped = await scraper.scrape_cambridge(word)
            elif lang_key == "taiwanese":
                scraped = await scraper.scrape_moedict(word)
            elif lang_key == "german":
                scraped = await scraper.scrape_dict_cc(word)
            
            if scraped.get("found"):
                log(f"Scraper lookup successful. Reading: '{scraped.get('reading', '')}'.")
            else:
                log("Scraper lookup found no matches.")
        except Exception as e:
            log(f"Dictionary scrape failed: {e}")
            
    return {
        "word": word,
        "filename": filename,
        "ocr_text": ocr_text,
        "classification": classification,
        "scraped": scraped,
        "llm_response": None
    }

async def process_ingest_item(scraper, llm_client, word_info: dict, lang_key: str, app_config: dict, log_cb=None) -> dict:
    def log(msg):
        logging.info(msg)
        if log_cb:
            log_cb(msg)

    word = word_info["word"]
    note_ctx = word_info.get("note", "")
    word_type = word_info.get("type", "")
    
    # Scrape
    log(f"Scraping standard dictionaries for '{word}' ({lang_key})...")
    scraped = {"found": False}
    if lang_key == "japanese":
        scraped = await scraper.scrape_jisho(word)
    elif lang_key == "english":
        scraped = await scraper.scrape_cambridge(word)
    elif lang_key == "taiwanese":
        scraped = await scraper.scrape_moedict(word)
    elif lang_key == "german":
        scraped = await scraper.scrape_dict_cc(word)
        
    found = scraped.get("found", False)
    definition = scraped.get("definition", "")
    reading = scraped.get("reading", "")
    is_conjugated = scraped.get("is_conjugated", False)
    suggestion = scraped.get("suggestion", "")
    
    if found:
        log(f"Dictionary match found. Reading: '{reading}'.")
    else:
        log("No dictionary match found.")
    
    loop = asyncio.get_running_loop()
    # Spelling correction suggestion using LLM if dictionary missed it or for other languages
    if not found and llm_client.model:
        log(f"Querying Ollama to check if '{word}' is conjugated or misspelled...")
        lemma_data = await loop.run_in_executor(None, llm_client.lemmatize_word, word, lang_key)
        if lemma_data.get("suggestion"):
            suggestion = lemma_data["suggestion"]
            is_conjugated = True
            log(f"Ollama suggested base form: '{suggestion}' (Conjugated: True)")
            
    # Generate definition/examples via LLM using context notes if available
    llm_response = {}
    if found or suggestion:
        target_word = suggestion if suggestion else word
        prompt_system = app_config["llm"]["system_prompt_vocab"]
        context_str = f"Context note: {note_ctx}. Type: {word_type}." if note_ctx else f"Type: {word_type}."
        log(f"Querying local Ollama model '{llm_client.model}' for vocabulary definition and examples...")
        try:
            llm_response = await loop.run_in_executor(
                None, llm_client.generate_card_content, target_word, context_str, lang_key.capitalize(), prompt_system
            )
            log("Ollama response received successfully.")
        except Exception as e:
            log(f"Ollama query failed: {e}. Falling back to scraped definition.")
            # Fallback to scraped dictionary definition
            llm_response = {
                "definition": definition,
                "nuances": f"Pronounced as: {reading}" if reading else "",
                "examples": []
            }
    else:
        log("No dictionary match and no LLM suggestion. Using default not-found card values.")
        llm_response = {
            "definition": "Not found in standard dictionary.",
            "nuances": "Please check spelling.",
            "examples": []
        }
        
    # gTTS audio generation
    audio_b64 = None
    try:
        log(f"Generating TTS audio pronunciation for '{suggestion if suggestion else word}'...")
        audio_b64 = await loop.run_in_executor(None, generate_tts_base64, suggestion if suggestion else word, lang_key)
        log("TTS audio generation completed.")
    except Exception as e:
        log(f"TTS generation failed: {e}")
        
    return {
        "word": word,
        "scraped": scraped,
        "suggestion": suggestion,
        "is_conjugated": is_conjugated,
        "llm_response": llm_response,
        "audio_b64": audio_b64
    }

def commit_card_modernization(anki_client, note_info, processed_data, lang_key, app_config) -> bool:
    deck_cfg = app_config["decks"].get(lang_key)
    if not deck_cfg:
        return False
        
    target_field = deck_cfg["fields"]["meaning_text"]
    img_field = deck_cfg["fields"]["meaning_image"]
    
    # Generate HTML content
    llm_res = processed_data["llm_response"]
    if lang_key == "grammar":
        html = format_anki_grammar_html(
            llm_res.get("grammar_point", processed_data["word"]),
            llm_res.get("meaning", ""),
            llm_res.get("rules", ""),
            llm_res.get("examples", [])
        )
    else:
        html = format_anki_meaning_html(
            llm_res.get("definition", ""),
            llm_res.get("nuances", ""),
            llm_res.get("examples", [])
        )
        
    # Update note
    fields = {target_field: html}
    if processed_data["classification"] == "dictionary":
        # Clear the image!
        fields[img_field] = ""
        
    anki_client.update_note_fields(note_info["noteId"], fields)
    return True

def commit_card_ingestion(anki_client, processed_data, lang_key, app_config) -> bool:
    deck_cfg = app_config["decks"].get(lang_key)
    if not deck_cfg:
        return False
        
    deck_name = deck_cfg["deck_name"]
    model_name = deck_cfg["note_type"]
    fields_map = deck_cfg["fields"]
    
    word = processed_data["suggestion"] if processed_data["suggestion"] else processed_data["word"]
    llm_res = processed_data["llm_response"]
    
    # Store audio first if generated
    audio_filename = f"tts_{lang_key}_{word.replace(' ', '_')}.mp3"
    if processed_data["audio_b64"]:
        anki_client.store_media_file(audio_filename, processed_data["audio_b64"])
        
    # Format meaning HTML
    html = format_anki_meaning_html(
        llm_res.get("definition", ""),
        llm_res.get("nuances", ""),
        llm_res.get("examples", [])
    )
    
    fields = {
        fields_map["expression"]: word,
        fields_map["meaning_text"]: html,
    }
    if processed_data["audio_b64"]:
        fields[fields_map["audio"]] = f"[sound:{audio_filename}]"
        
    anki_client.add_note(deck_name, model_name, fields)
    return True

# --- SELECTION LIST MODAL ---
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

# --- COMPARISON RENDERERS ---
from pathlib import Path

def find_anki_media_path(filename: str) -> str:
    if not filename:
        return ""
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

def render_image_to_ansi(image_path: str, max_width: int = 100) -> str:
    try:
        from PIL import Image
        img = Image.open(image_path)
        img = img.convert("RGB")
        
        # Calculate size
        w, h = img.size
        # Character aspect ratio is roughly 2:1 (vertical:horizontal)
        # But we render 2 pixels vertically per character block, so pixel aspect ratio in the grid is 1:1
        aspect = h / w
        target_width = min(max_width, w)
        target_height = int(target_width * aspect)
        # Ensure height is even
        if target_height % 2 != 0:
            target_height += 1
        if target_height <= 0:
            target_height = 2
            
        img = img.resize((target_width, target_height), Image.Resampling.LANCZOS)
        pixels = img.load()
        
        ansi_lines = []
        for y in range(0, target_height, 2):
            line_parts = []
            for x in range(target_width):
                top_color = pixels[x, y]
                bot_color = pixels[x, y+1]
                
                tr, tg, tb = top_color
                br, bg, bb = bot_color
                
                # ANSI sequence for top pixel as background (48), bottom pixel as foreground (38)
                part = f"\x1b[48;2;{tr};{tg};{tb}m\x1b[38;2;{br};{bg};{bb}m▄"
                line_parts.append(part)
            # Reset at the end of the line
            line_parts.append("\x1b[0m")
            ansi_lines.append("".join(line_parts))
            
        return "\n".join(ansi_lines)
    except Exception as e:
        return f"[red]Failed to render image: {e}[/]"

def make_side_by_side_comparison(word: str, note_id: str, original_fields: dict, processed_data: dict, is_grammar: bool, committed: bool = False) -> Table:
    table = Table.grid(expand=True)
    table.add_column(ratio=1)
    table.add_column(ratio=1)
    
    before_text = f"[bold yellow]Word/Phrase:[/] {word}\n"
    before_text += f"[bold yellow]Note ID:[/] {note_id}\n\n"
    before_text += "[bold underline]Original Fields & Content:[/]\n"
    filename = ""
    for field_name, val in original_fields.items():
        val_clean = val.get("value", "")
        if field_name == "Picture" or "<img" in val_clean:
            import re
            match = re.search(r'<img\s+[^>]*src=["\']([^"\']+)["\']', val_clean)
            if match:
                filename = match.group(1)
        val_clean = val_clean.replace("<div>", "").replace("</div>", "\n").replace("<br>", "\n").replace("<br/>", "\n")
        val_clean = BeautifulSoup(val_clean, "html.parser").get_text()
        if len(val_clean) > 250:
            val_clean = val_clean[:250] + "..."
        before_text += f"- [bold]{field_name}[/]: {val_clean.strip()}\n"
        
    before_renderables = [Text.from_markup(before_text)]
    
    if filename:
        media_path = find_anki_media_path(filename)
        if media_path:
            before_renderables.append(Text.from_markup(f"\n[bold green]Original Image:[/] [link=file://{media_path}]{filename}[/link]"))
            ansi_art = render_image_to_ansi(media_path, max_width=100)
            if ansi_art and not ansi_art.startswith("[red]"):
                before_renderables.append(Text.from_markup("\n[bold green]Legacy Screenshot Card Image:[/]"))
                before_renderables.append(Text.from_ansi(ansi_art))
        else:
            before_renderables.append(Text.from_markup(f"\n[bold green]Original Image Found:[/] {filename}"))
            
    before_panel = Panel(Group(*before_renderables), title="BEFORE (Current Card)", border_style="yellow")
    
    if not committed:
        if processed_data:
            classification = processed_data.get("classification")
            class_color = "yellow" if classification == "dictionary" else "green"
            class_str = "Dictionary Screenshot (Will REPLACE with text)" if classification == "dictionary" else "Visual Recall Image (Will KEEP image)"
            
            after_text = f"[yellow]● Preview of Extracted Data (Pending Commit)[/]\n\n"
            after_text += f"[bold green]Image Action:[/] [{class_color}]{class_str}[/]\n\n"
            
            ocr_txt = processed_data.get("ocr_text", "").strip()
            if ocr_txt:
                after_text += "[bold underline]OCR Extracted Text:[/]\n"
                if len(ocr_txt) > 250:
                    ocr_txt = ocr_txt[:250] + "..."
                after_text += f"```\n{ocr_txt}```\n\n"
                
            scraped = processed_data.get("scraped")
            if scraped and scraped.get("found"):
                after_text += "[bold underline]Scraped Dictionary Metadata:[/]\n"
                after_text += f"- [bold]Dictionary Entry[/]: {scraped.get('word')}\n"
                after_text += f"- [bold]Reading/Pronunciation[/]: {scraped.get('reading', '')}\n"
                after_text += f"- [bold]Raw Definition[/]: {scraped.get('definition', '')}\n"
                
            after_text += "\n[italic yellow]Press 'c' to run local Ollama and commit this card.[/]"
            after_panel = Panel(after_text, title="AFTER (Modernized Preview) - PREVIEW", border_style="yellow")
        else:
            after_text = (
                "\n"
                "[yellow]● Pending Commit[/]\n\n"
                "Modernized card preview (Ollama suggestion, OCR extraction, and field updates) will be generated and displayed here after committing.\n\n"
                "Press [bold]c[/] to commit this card."
            )
            after_panel = Panel(after_text, title="AFTER (Modernized Preview) - PENDING", border_style="yellow")
    elif not processed_data or not processed_data.get("llm_response"):
        after_text = (
            "\n"
            "[yellow]● Running OCR & Local Ollama Analysis...[/]\n"
            "Processing screenshot to extract text, determine action (Keep/Replace), and structure nuances/examples.\n"
        )
        after_panel = Panel(after_text, title="AFTER (Modernized Preview) - LOADING", border_style="yellow")
    else:
        llm_res = processed_data["llm_response"]
        classification = processed_data["classification"]
        filename = processed_data.get("filename") or filename
        
        class_color = "yellow" if classification == "dictionary" else "green"
        class_str = "Dictionary Screenshot (Will REPLACE with text)" if classification == "dictionary" else "Visual Recall Image (Will KEEP image)"
        
        after_text = ""
        media_path = find_anki_media_path(filename) if filename else ""
        if media_path:
            after_text += f"[bold green]Image Action:[/] [{class_color}]{class_str}[/]\n"
            after_text += f"[bold green]Image File:[/] [link=file://{media_path}]{filename}[/link]\n\n"
        else:
            after_text += f"[bold green]Image Action:[/] [{class_color}]{class_str}[/] ({filename})\n\n"
            
        after_text += "[bold underline]OCR Extracted Text:[/]\n"
        ocr_txt = processed_data.get("ocr_text", "").strip()
        if not ocr_txt:
            ocr_txt = "[No text found]"
        if len(ocr_txt) > 250:
            ocr_txt = ocr_txt[:250] + "..."
        after_text += f"```\n{ocr_txt}```\n\n"
        
        after_text += "[bold underline]Ollama Suggestion Preview:[/]\n"
        if is_grammar:
            after_text += f"- **Grammar Point**: {llm_res.get('grammar_point', '')}\n"
            after_text += f"- **Meaning**: {llm_res.get('meaning', '')}\n"
            after_text += f"- **Rules**: {llm_res.get('rules', '')}\n"
        else:
            after_text += f"- **Definition**: {llm_res.get('definition', '')}\n"
            after_text += f"- **Nuance**: {llm_res.get('nuances', '')}\n"
            
        examples = llm_res.get("examples", [])
        if examples:
            after_text += "- **Examples**:\n"
            for idx, ex in enumerate(examples[:2], 1):
                after_text += f"  {idx}. {ex.get('sentence')} -> {ex.get('translation')}\n"
                
        after_panel = Panel(after_text, title="AFTER (Modernized Preview)", border_style="green")
        
    table.add_row(before_panel, after_panel)
    return table

def make_ingest_side_by_side(word_info: dict, processed_data: dict) -> Table:
    table = Table.grid(expand=True)
    table.add_column(ratio=1)
    table.add_column(ratio=1)
    
    input_text = f"[bold yellow]Word/Phrase:[/] {word_info['word']}\n"
    input_text += f"[bold yellow]Language:[/] {word_info.get('language', '').upper()}\n"
    if word_info.get("type_tag"):
        input_text += f"[bold yellow]Type tag:[/] {word_info['type_tag']}\n"
    if word_info.get("note"):
        input_text += f"[bold yellow]Context Note:[/] {word_info['note']}\n"
        
    if "raw_text" in word_info:
        input_text += f"[bold yellow]Crawl Source length:[/] {len(word_info['raw_text'])} chars\n"
        # Extract plain text content snippet for preview
        raw = word_info["raw_text"]
        if "<html>" in raw or "<div" in raw or "<p" in raw:
            try:
                from bs4 import BeautifulSoup
                cleaned = BeautifulSoup(raw, "html.parser").get_text()
            except Exception:
                cleaned = raw
        else:
            cleaned = raw
        # Remove empty lines & strip
        cleaned = "\n".join([line.strip() for line in cleaned.splitlines() if line.strip()])
        snippet = cleaned[:500] + "..." if len(cleaned) > 500 else cleaned
        input_text += f"\n[bold underline]Cleaned Crawl Content (Snippet):[/]\n```\n{snippet}\n```\n"
        
    if processed_data and "scraped" in processed_data:
        scraped = processed_data["scraped"]
        if scraped.get("found"):
            input_text += "\n[bold underline]Scraped Dictionary Metadata:[/]\n"
            input_text += f"- [bold]Dictionary Entry[/]: {scraped.get('word')}\n"
            input_text += f"- [bold]Reading/Pronunciation[/]: {scraped.get('reading', '')}\n"
            input_text += f"- [bold]Raw Definition[/]: {scraped.get('definition', '')}\n"
            if scraped.get("is_common"):
                input_text += f"- [bold]Common Word[/]: Yes\n"
            if scraped.get("audio_url"):
                input_text += f"- [bold]Audio Link[/]: {scraped.get('audio_url')}\n"
                
    input_panel = Panel(input_text, title="INPUT DATA", border_style="yellow")
    
    if not processed_data:
        output_text = (
            "\n"
            "[yellow]● Scraping Dictionary & Querying Local Ollama...[/]\n"
            "Running anti-bot scrapers via Crawl4AI and requesting custom nuances/examples from local LLM.\n"
        )
        output_panel = Panel(output_text, title="GENERATED CARD PREVIEW - LOADING", border_style="yellow")
    else:
        scraped = processed_data["scraped"]
        is_conj = processed_data["is_conjugated"]
        llm_res = processed_data["llm_response"]
        
        spell_status = "[bold green]Dictionary Base Form[/]"
        if is_conj:
            spell_status = f"[bold yellow]Conjugated! Suggestion base: '{processed_data['suggestion']}'[/]"
        elif not scraped.get("found"):
            spell_status = "[bold red]Not found in standard dictionary![/]"
            
        output_text = f"[bold green]Spelling/Validation:[/] {spell_status}\n\n"
        output_text += f"**Scraped Definition**: {scraped.get('definition', '[None]')}\n\n"
        output_text += "[bold underline]Ollama Card Preview:[/]\n"
        output_text += f"- **Definition**: {llm_res.get('definition', '')}\n"
        output_text += f"- **Nuance/Notes**: {llm_res.get('nuances', '')}\n"
        
        examples = llm_res.get("examples", [])
        if examples:
            output_text += "- **Examples**:\n"
            for idx, ex in enumerate(examples[:2], 1):
                output_text += f"  {idx}. {ex.get('sentence')} -> {ex.get('translation')}\n"
                
        output_text += f"- **TTS Audio status**: {'[green]Generated[/]' if processed_data['audio_b64'] else '[red]None[/]'}\n"
        output_panel = Panel(output_text, title="GENERATED CARD PREVIEW", border_style="green")
        
    table.add_row(input_panel, output_panel)
    return table


