"""Template-aware card documents and transactional Anki writes.

Processing code produces a :class:`CardDocument`.  The document is the single
boundary between dictionary/OCR/LLM enrichment and Anki's concrete note type.
This keeps modernization and injection on the same field and media semantics.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import html as html_lib
import logging
import re
from typing import Any, Literal

from .card_templates import JAPANESE_VOCAB_MODEL_NAME, japanese_vocab_template
from .markdown_text import render_markdown


CardMode = Literal["modernize", "inject"]


@dataclass(frozen=True)
class CardModePolicy:
    """The small set of intentional differences between write workflows."""

    create_note: bool
    include_context: bool
    remove_replaced_media: bool
    tags: tuple[str, ...] = ()


MODE_POLICIES: dict[CardMode, CardModePolicy] = {
    "modernize": CardModePolicy(False, False, True),
    "inject": CardModePolicy(True, True, False, ("linguist-injected",)),
}


@dataclass(frozen=True)
class MediaAsset:
    filename: str
    data_base64: str


@dataclass
class CardDocument:
    expression: str
    values: dict[str, str | None]
    media: list[MediaAsset] = field(default_factory=list)
    obsolete_media: list[str] = field(default_factory=list)
    issues: list[str] = field(default_factory=list)
    tags: list[str] = field(default_factory=list)

    @property
    def ready(self) -> bool:
        return bool(self.expression.strip()) and not self.issues


def _join_field_values(current: str | None, incoming: str | None) -> str | None:
    """Combine logical values mapped onto the same physical Anki field."""
    if incoming is None:
        return current
    if current is None or current == "":
        return incoming
    if incoming == "":
        return current
    return f"{current}<br/>{incoming}"


def map_document_fields(document: CardDocument, deck_cfg: dict) -> dict[str, str]:
    """Map purpose-based document values to a configured Anki note schema."""
    fields_map = dict(deck_cfg.get("fields", {}))
    if not fields_map.get("expression") or not fields_map.get("meaning_text"):
        raise ValueError("The card template requires expression and meaning field mappings")

    mapped: dict[str, str | None] = {}
    logical_values = {"expression": document.expression, **document.values}
    for purpose in (
        "expression", "meaning_image", "meaning_text", "kanji_construction", "audio"
    ):
        field_name = fields_map.get(purpose)
        value = logical_values.get(purpose)
        if not field_name or value is None:
            continue
        mapped[field_name] = _join_field_values(mapped.get(field_name), value)
    return {name: value or "" for name, value in mapped.items()}


def sanitize_generated_text(value: object) -> str:
    """Normalize an LLM text fragment without damaging normal punctuation."""
    text = html_lib.unescape(str(value or ""))
    text = " ".join(text.replace("\u3000", " ").split())
    # Models occasionally prefix values with JSON residue, bullets, or labels.
    text = re.sub(r"^\s*\d{1,3}\s*[.)、:：-]\s*", "", text)
    text = re.sub(
        r"^(?:(?:sentence|translation|example)\s*)?[\s:：;；|/\\>*#•·\-–—]+",
        "",
        text,
        flags=re.IGNORECASE,
    )
    text = re.sub(r"^[\[(（【{｛［〔〈《「『<]+\s*", "", text)
    text = re.sub(r"\s*[\])）】}｝］〕〉》」』>]+$", "", text)
    return text.strip(" \t\r\n:：;；|/\\>*#•·-–—\"'`")


def format_llm_annotations_html(nuances: str, examples: list) -> str:
    output = ""
    if nuances:
        output += (
            "<div data-source='llm' style='margin-top:6px;font-style:italic;color:#888'>"
            f"<b>Nuance:</b> {html_lib.escape(str(nuances))}</div>"
        )
    valid_examples = []
    for item in examples:
        if not isinstance(item, dict):
            continue
        sentence = sanitize_generated_text(item.get("sentence"))
        translation = sanitize_generated_text(item.get("translation"))
        if sentence or translation:
            valid_examples.append((sentence, translation))
    if valid_examples:
        output += "<div data-source='llm' style='margin-top:10px'><b>Examples:</b><div style='margin-top:5px'>"
        for number, (sentence, translation) in enumerate(valid_examples, 1):
            sentence_html = html_lib.escape(sentence)
            translation_html = html_lib.escape(translation)
            separator = " — " if sentence_html and translation_html else ""
            output += (
                "<div style='margin-bottom:4px'>"
                f"<b>{number}. {sentence_html}</b>{separator}"
                f"<span style='color:#666;font-size:.9em'>{translation_html}</span></div>"
            )
        output += "</div></div>"
    return output


def format_dictionary_meaning_html(scraped: dict, target_word: str, nuances: str, examples: list) -> str:
    """Render authoritative dictionary entries and annotate only the exact one."""
    annotation = format_llm_annotations_html(nuances, examples)
    had_entries = bool(scraped and "entries" in scraped)
    entries = scraped.get("entries") if scraped else None
    if entries:
        exact_index = 0
        for index, entry in enumerate(entries):
            values = {value for form in entry.get("forms", []) if isinstance(form, dict)
                      for value in (form.get("word"), form.get("reading")) if value}
            values.update(value for value in (entry.get("word"), entry.get("reading")) if value)
            if target_word in values:
                exact_index = index
                break
        rendered = []
        ordered_entries = [(exact_index, entries[exact_index])]
        ordered_entries.extend((index, entry) for index, entry in enumerate(entries) if index != exact_index)
        for display_index, (source_index, entry) in enumerate(ordered_entries):
            word = html_lib.escape(str(entry.get("word", "")))
            reading = html_lib.escape(str(entry.get("reading", "")))
            heading = " / ".join(value for value in (reading, word) if value)
            body = [f"<div><b>{heading}</b></div>"]
            forms = []
            for form in entry.get("forms", []):
                if isinstance(form, dict):
                    text = " / ".join(html_lib.escape(str(value)) for value in
                                      (form.get("reading"), form.get("word")) if value)
                    if text and text != heading:
                        forms.append(text)
            if forms:
                body.append(f"<div><b>Forms:</b> {'; '.join(forms)}</div>")
            metadata = (["Common word"] if entry.get("is_common") else [])
            metadata += [html_lib.escape(str(value)) for value in entry.get("jlpt", [])]
            metadata += [html_lib.escape(str(value)) for value in entry.get("tags", [])]
            if metadata:
                body.append(f"<div>{' · '.join(metadata)}</div>")
            for sense in entry.get("senses", []):
                labels = [html_lib.escape(str(value)) for value in
                          sense.get("parts_of_speech", []) + sense.get("tags", [])
                          if "wikipedia definition" not in str(value).lower()]
                number = html_lib.escape(str(sense.get("number", "")))
                definitions = "; ".join(html_lib.escape(str(value)) for value in sense.get("definitions", []))
                label_html = f"<div><b>{' · '.join(labels)}</b></div>" if labels else ""
                body.append(f"<div style='margin-top:2px'>{label_html}<div><b>{number}.</b> {definitions}</div>")
                for label, key in (("See also", "see_also"), ("Antonyms", "antonyms"),
                                   ("Information", "info"), ("Restrictions", "restrictions")):
                    values = [html_lib.escape(str(value)) for value in sense.get(key, [])]
                    if values:
                        body.append(f"<div><b>{label}:</b> {'; '.join(values)}</div>")
                body.append("</div>")
            if source_index == exact_index and annotation:
                body.append(annotation)
            if display_index == 0:
                section_style = "padding-bottom:6px"
            else:
                section_style = "padding-top:8px"
            rendered.append(f"<section style='{section_style}'>" + "".join(body) + "</section>")
        separator = "<hr style='border:0;border-top:2px solid #888;margin:10px 0'/>"
        return separator.join(rendered)
    if had_entries:
        return "<div>No concise dictionary senses found.</div>"
    if scraped and scraped.get("found"):
        word = html_lib.escape(str(scraped.get("word") or target_word))
        reading = html_lib.escape(str(scraped.get("reading", "")))
        definition = html_lib.escape(str(scraped.get("definition", ""))).replace("\n", "<br/>")
        heading = " / ".join(value for value in (reading, word) if value)
        return f"<section><div><b>{heading}</b></div><div style='margin-top:7px'>{definition}</div>{annotation}</section>"
    return "<div>Not found in standard dictionary.</div>"


def format_anki_grammar_html(grammar_point: str, meaning: str, rules: str, examples: list) -> str:
    output = f"<div><b>Grammar Point:</b> <span style='font-size:1.2em;color:#e68e0d'>{html_lib.escape(str(grammar_point))}</span></div>"
    output += f"<div style='margin-top:5px'><b>Meaning:</b> {html_lib.escape(str(meaning))}</div>"
    if rules:
        output += f"<div style='margin-top:5px'><b>Structure/Rules:</b> <pre>{html_lib.escape(str(rules))}</pre></div>"
    if examples:
        output += format_llm_annotations_html("", examples)
    return output


def format_injection_context_html(type_tag: str = "", note: str = "") -> str:
    rows = []
    if type_tag:
        rows.append(f"<div><b>Learning focus:</b> {html_lib.escape(str(type_tag))}</div>")
    if note:
        rows.append(f"<div><b>Personal context:</b> {html_lib.escape(str(note))}</div>")
    return ("<aside data-source='user' style='margin-top:12px;border-top:1px dashed #888;padding-top:8px'>" + "".join(rows) + "</aside>") if rows else ""


def build_card_document(processed_data: dict, lang_key: str, mode: CardMode) -> CardDocument:
    """Build the canonical, template-ready document for either workflow."""
    try:
        policy = MODE_POLICIES[mode]
    except KeyError as exc:
        raise ValueError(f"Unknown card mode: {mode}") from exc
    original_word = str(processed_data.get("word", "") or "").strip()
    expression = str(processed_data.get("suggestion") or original_word).strip() if policy.create_note else original_word
    llm = processed_data.get("llm_response") or {}
    if lang_key.endswith("grammar"):
        meaning = format_anki_grammar_html(llm.get("grammar_point", expression), llm.get("meaning", ""), llm.get("rules", ""), llm.get("examples", []))
    else:
        meaning = format_dictionary_meaning_html(processed_data.get("scraped", {}), expression, llm.get("nuances", ""), llm.get("examples", []))
    if processed_data.get("meaning_override_markdown") is not None:
        meaning = render_markdown(
            str(processed_data.get("meaning_override_markdown", "")),
            processed_data.get("meaning_override_media"),
        )
    elif processed_data.get("meaning_override") is not None:
        meaning = str(processed_data.get("meaning_override", ""))
    if policy.include_context:
        meaning += format_injection_context_html(processed_data.get("type_tag", ""), processed_data.get("source_note", ""))
    media, obsolete, images = [], [], []
    new_name, new_data = processed_data.get("new_image_filename", ""), processed_data.get("new_image_b64")
    if new_name and new_data:
        media.append(MediaAsset(new_name, new_data)); images.append(f"<img src='{html_lib.escape(new_name, quote=True)}'/>")
        if policy.remove_replaced_media: obsolete += processed_data.get("orig_filenames", [])
    elif policy.remove_replaced_media:
        renamed_images = processed_data.get("renamed_images", [])
        has_retained_image = any(
            image.get("classification", "uncertain") != "dictionary"
            for image in renamed_images
        )
        for image in renamed_images:
            name, data = image.get("new_name", ""), image.get("b64")
            original = image.get("original_name")
            if image.get("classification") == "dictionary" and has_retained_image:
                if original:
                    obsolete.append(original)
                continue
            if name and data:
                media.append(MediaAsset(name, data)); images.append(f"<img src='{html_lib.escape(name, quote=True)}'/>")
                if original and original != name: obsolete.append(original)
        if not images:
            existing = processed_data.get("filename", "")
            if existing and processed_data.get("classification") != "dictionary": images.append(f"<img src='{html_lib.escape(existing, quote=True)}'/>")
            elif processed_data.get("classification") == "dictionary":
                # A dictionary decision must never remove the only image. If
                # replacement retrieval failed, preserve the original safely.
                if processed_data.get("new_image_b64"):
                    obsolete += processed_data.get("orig_filenames", [])
                elif existing:
                    images.append(f"<img src='{html_lib.escape(existing, quote=True)}'/>")
    audio = "" if policy.create_note else None
    audio_assets = list(processed_data.get("audio_assets") or [])
    if not audio_assets and processed_data.get("audio_filename"):
        audio_assets = [{
            "filename": processed_data.get("audio_filename", ""),
            "b64": processed_data.get("audio_b64"),
            "reading": processed_data.get("scraped", {}).get("reading", ""),
        }]
    audio_rows: list[str] = []
    for asset in audio_assets:
        audio_name, audio_data = str(asset.get("filename") or ""), asset.get("b64")
        if not audio_name:
            continue
        if audio_data:
            media.append(MediaAsset(audio_name, audio_data))
        reading = str(asset.get("reading") or "").strip()
        sound = f"[sound:{audio_name}]"
        audio_rows.append(
            f"{html_lib.escape(reading)} {sound}"
            if lang_key.startswith("japanese") and reading else sound
        )
    if audio_rows:
        audio = "<br/>".join(audio_rows)
    issues = list(processed_data.get("issues", []))
    if not expression: issues.append("Expression is empty")
    tag = re.sub(r"[^\w\-]+", "_", str(processed_data.get("type_tag", "")), flags=re.UNICODE).strip("_")[:80]
    tags = list(policy.tags)
    if tag:
        tags.append(f"linguist::{tag}")
    kanji = processed_data.get("kanji_construction", "")
    if processed_data.get("kanji_override_markdown") is not None:
        kanji = render_markdown(
            str(processed_data.get("kanji_override_markdown", "")),
            processed_data.get("kanji_override_media"),
        )
    return CardDocument(expression, {"meaning_image": "".join(images), "meaning_text": meaning,
        "kanji_construction": kanji, "audio": audio},
        media, obsolete, list(dict.fromkeys(issues)), tags)


def _validate_document(document: CardDocument, deck_cfg: dict, mode: CardMode) -> None:
    if not document.expression.strip():
        raise ValueError(f"Refusing to {mode} a card with an empty expression")
    if document.issues:
        raise ValueError("Card is not ready: " + "; ".join(document.issues))
    if not deck_cfg:
        raise ValueError("The selected deck is not configured")
    if mode == "inject" and (not deck_cfg.get("deck_name") or not deck_cfg.get("note_type")):
        raise ValueError("The injection target requires both an Anki deck and note type")


def resolve_write_config(deck_cfg: dict, mode: CardMode, note_info: dict | None = None) -> dict:
    """Resolve the concrete Anki schema at the template boundary.

    New Japanese vocabulary cards always use the managed model. Existing
    legacy cards are migrated in place at commit time when AnkiConnect exposes
    ``updateNoteModel``. Field values are always produced from the canonical
    document, so the migration never guesses a legacy-to-managed mapping.
    """
    resolved = dict(deck_cfg)
    if mode == "inject" and resolved.get("note_type") == JAPANESE_VOCAB_MODEL_NAME:
        spec = japanese_vocab_template()
        resolved["note_type"] = spec.model_name
        resolved["fields"] = spec.field_mapping()
        return resolved
    return resolved


def _validate_target_fields(
    fields: dict[str, str], *, note_info: dict | None = None, model_fields: Any = None
) -> None:
    available: set[str] | None = None
    note_fields = (note_info or {}).get("fields")
    if isinstance(note_fields, dict) and note_fields:
        available = set(note_fields)
    elif isinstance(model_fields, (list, tuple, set)):
        available = {str(value) for value in model_fields}
    if available is None:
        return
    missing = sorted(set(fields) - available)
    if missing:
        raise ValueError(
            "Configured card fields are absent from the Anki note type: "
            + ", ".join(missing)
            + ". Install/select the managed template or use Anki's Change Note Type "
              "command to map this legacy note before writing."
        )


def _rollback_media(anki_client, staged: list[tuple[str, str | None]]) -> None:
    for filename, previous in reversed(staged):
        try:
            if previous:
                anki_client.store_media_file(filename, previous)
            else:
                anki_client.delete_media_file(filename)
        except Exception as exc:  # best effort after the primary failure
            logging.warning("Could not roll back staged media '%s': %s", filename, exc)


def commit_card_document(
    anki_client,
    document: CardDocument,
    deck_cfg: dict,
    mode: CardMode,
    note_info: dict | None = None,
    media_before: dict[str, str | None] | None = None,
) -> int:
    """Commit one document and its media as a recoverable transaction.

    AnkiConnect does not expose a cross-action transaction, so media that would
    be overwritten is backed up and restored if the note operation fails.  Old
    media is removed only after the note points at its replacement.
    """
    deck_cfg = resolve_write_config(deck_cfg, mode, note_info)
    _validate_document(document, deck_cfg, mode)
    fields = map_document_fields(document, deck_cfg)

    actual_model = str((note_info or {}).get("modelName") or "")
    target_model = str(deck_cfg.get("note_type") or "")
    migration_required = bool(
        mode == "modernize" and actual_model and target_model
        and actual_model != target_model
    )

    model_fields = None
    if mode == "inject" or migration_required:
        try:
            model_fields = anki_client.get_model_fields(deck_cfg["note_type"])
        except (AttributeError, NotImplementedError):
            pass
    _validate_target_fields(
        fields,
        note_info=None if migration_required else note_info,
        model_fields=model_fields,
    )
    if target_model == JAPANESE_VOCAB_MODEL_NAME:
        spec = japanese_vocab_template()
        try:
            installed_templates = anki_client.get_model_templates(target_model)
        except (AttributeError, NotImplementedError):
            installed_templates = None
        if isinstance(installed_templates, dict):
            expected_templates = list(spec.templates)
            if list(installed_templates) != expected_templates:
                try:
                    logging.info(
                        "Upgrading managed note type '%s' from card templates %s to %s...",
                        target_model, list(installed_templates), expected_templates,
                    )
                    anki_client.install_model(
                        spec.model_name, list(spec.fields), spec.css, spec.templates,
                    )
                    installed_templates = anki_client.get_model_templates(target_model)
                except Exception as exc:
                    raise RuntimeError(
                        f"Could not automatically upgrade managed note type "
                        f"'{target_model}' from {list(installed_templates)} to "
                        f"{expected_templates}: {exc}. Run `python -m "
                        "linguist_anki_bridge.main --install-japanese-template` "
                        "with Anki open for a detailed installation error."
                    ) from exc
                if not isinstance(installed_templates, dict) or list(installed_templates) != expected_templates:
                    raise RuntimeError(
                        f"Managed note type upgrade did not produce the expected card "
                        f"templates {expected_templates}; Anki still reports "
                        f"{list(installed_templates or {})}."
                    )
                logging.info(
                    "Managed note type '%s' upgraded successfully.", target_model,
                )
            elif installed_templates != spec.templates:
                logging.info(
                    "Refreshing managed card HTML for note type '%s'...", target_model,
                )
                anki_client.update_model_templates(target_model, spec.templates)
        try:
            installed_styling = anki_client.get_model_styling(target_model)
        except (AttributeError, NotImplementedError):
            installed_styling = None
        if isinstance(installed_styling, dict) and installed_styling.get("css") != spec.css:
            logging.info(
                "Refreshing managed card styling for note type '%s'...", target_model,
            )
            anki_client.update_model_styling(target_model, spec.css)
    if migration_required:
        try:
            supported = anki_client.supports_action("updateNoteModel")
        except (AttributeError, NotImplementedError):
            supported = False
        if not supported:
            raise RuntimeError(
                f"Automatic migration from '{actual_model}' to '{target_model}' requires "
                "a current AnkiConnect release with updateNoteModel support. Update the "
                "AnkiConnect add-on and restart Anki, or use Anki's Change Note Type command."
            )

    unique_assets: dict[str, MediaAsset] = {}
    for asset in document.media:
        if not asset.filename or not asset.data_base64:
            continue
        if asset.filename in unique_assets and unique_assets[asset.filename] != asset:
            raise ValueError(f"Conflicting media payloads for '{asset.filename}'")
        unique_assets[asset.filename] = asset

    staged: list[tuple[str, str | None]] = []
    try:
        for asset in unique_assets.values():
            previous = None
            if media_before is not None and asset.filename in media_before:
                previous = media_before[asset.filename]
            else:
                try:
                    retrieved = anki_client.retrieve_media_file(asset.filename)
                    if isinstance(retrieved, str) and retrieved:
                        previous = retrieved
                except Exception:
                    # Missing media and older/fake clients are both safe to stage.
                    pass
            anki_client.store_media_file(asset.filename, asset.data_base64)
            staged.append((asset.filename, previous))

        if mode == "modernize":
            if not note_info or not note_info.get("noteId"):
                raise ValueError("Modernization requires an existing Anki note id")
            note_id = note_info["noteId"]
            if migration_required:
                anki_client.update_note_model(
                    note_id, target_model, fields, note_info.get("tags") or [],
                )
            else:
                anki_client.update_note_fields(note_id, fields)
        else:
            note_id = anki_client.add_note(
                deck_cfg["deck_name"], deck_cfg["note_type"], fields,
                tags=document.tags or None,
            )
            if not note_id:
                raise RuntimeError("AnkiConnect did not return a note id")
    except Exception:
        _rollback_media(anki_client, staged)
        raise

    referenced = set(unique_assets)
    for filename in dict.fromkeys(document.obsolete_media):
        if not filename or filename in referenced:
            continue
        try:
            anki_client.delete_media_file(filename)
        except Exception as exc:
            logging.warning("Card committed but old media '%s' could not be deleted: %s", filename, exc)
    return int(note_id)
