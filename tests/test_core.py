import base64
import copy
import json
import os
import tempfile
import unittest
import asyncio
import types
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch
from textual.app import App

os.environ.setdefault("CRAWL4_AI_BASE_DIRECTORY", tempfile.gettempdir())

from linguist_anki_bridge import config as config_module
from linguist_anki_bridge.anki import AnkiConnectClient
from linguist_anki_bridge.card_model import (
    CardDocument,
    MediaAsset,
    commit_card_document,
    format_llm_annotations_html,
    map_document_fields,
    sanitize_generated_text,
)
from linguist_anki_bridge.card_templates import (
    JAPANESE_VOCAB_FIELDS,
    JAPANESE_VOCAB_MODEL_NAME,
    japanese_vocab_template,
)
from linguist_anki_bridge.config import ConfigManager, DEFAULT_CONFIG
from linguist_anki_bridge.llm import OllamaClient
from linguist_anki_bridge.markdown_text import html_to_markdown, render_markdown
from linguist_anki_bridge.ocr import OcrEngine
from linguist_anki_bridge.scraper import Crawl4AiScraper
from linguist_anki_bridge.tui.app import AnkiBridgeApp, TuiLogHandler, TuiLogMessage
from linguist_anki_bridge.tui.setup import InputManagementScreen
from linguist_anki_bridge.snapshots import SnapshotManager
from linguist_anki_bridge.tui.screens import (
    build_card_document,
    commit_card_ingestion,
    commit_card_modernization,
    dictionary_llm_context,
    fetch_web_image,
    fetch_kanji_construction_if_needed,
    format_dictionary_meaning_html,
    indexed_media_filename,
    kanji_summary_for_tui,
    process_inject_item,
    safe_media_stem,
)


class MarkdownTextTests(unittest.TestCase):
    def test_markdown_renders_for_anki_and_escapes_raw_html(self):
        rendered = render_markdown("## Meaning\n\n- **actor**\n- performer\n\n<script>bad()</script>")
        self.assertIn("<h2>Meaning</h2>", rendered)
        self.assertIn("<strong>actor</strong>", rendered)
        self.assertNotIn("<script>", rendered)

    def test_html_round_trip_preserves_embedded_image_without_exposing_payload(self):
        markdown, media = html_to_markdown(
            "<section><b>Kanji 俳</b><hr><img src='data:image/gif;base64,R0lGODlh'/></section>"
        )
        self.assertIn("**Kanji 俳**", markdown)
        self.assertIn("---", markdown)
        self.assertIn("{{LINGUIST_MEDIA_0}}", markdown)
        self.assertNotIn("R0lGOD", markdown)
        rendered = render_markdown(markdown, media)
        self.assertIn("data:image/gif;base64,R0lGODlh", rendered)


class ConfigTests(unittest.TestCase):
    def test_migrates_legacy_deck_keys(self):
        migrated = ConfigManager._migrate({
            "anki": {"auto_backup_before_write": True},
            "decks": {"japanese": {"deck_name": "Old"}},
        })
        self.assertEqual(migrated["decks"]["japanese_vocab"]["deck_name"], "Old")
        self.assertNotIn("japanese", migrated["decks"])
        self.assertNotIn("auto_backup_before_write", migrated["anki"])

    def test_reload_resets_deleted_in_memory_values(self):
        with tempfile.TemporaryDirectory() as td:
            config_file = Path(td) / "config.yaml"
            config_file.write_text("dry_run: false\n", encoding="utf-8")
            with patch.object(config_module, "CONFIG_FILE", config_file):
                manager = ConfigManager()
                self.assertFalse(manager.config["dry_run"])
                manager.config["filters"]["remove_parentheses"] = False
                manager.load()
                self.assertTrue(manager.config["filters"]["remove_parentheses"])

    def test_default_nested_config_is_not_shared(self):
        original = copy.deepcopy(DEFAULT_CONFIG)
        with patch.object(config_module, "CONFIG_FILE", Path("/nonexistent/config.yaml")):
            manager = ConfigManager()
        manager.config["decks"]["japanese_vocab"]["fields"]["expression"] = "Changed"
        self.assertEqual(DEFAULT_CONFIG, original)


class CardTemplateTests(unittest.TestCase):
    def test_japanese_template_matches_bridge_fields_and_hides_answers_on_front(self):
        spec = japanese_vocab_template()
        self.assertEqual(spec.model_name, JAPANESE_VOCAB_MODEL_NAME)
        self.assertEqual(spec.fields, JAPANESE_VOCAB_FIELDS)
        self.assertIn("{{Expression}}", spec.front)
        self.assertNotIn("{{Meaning}}", spec.front)
        self.assertNotIn("{{Audio}}", spec.front)
        self.assertEqual(list(spec.templates), ["Comprehension", "Spelling", "Production"])
        self.assertIn("{{type:Expression}}", spec.templates["Spelling"]["Front"])
        self.assertIn("{{Audio}}", spec.templates["Spelling"]["Front"])
        self.assertNotIn("{{Expression}}", spec.templates["Production"]["Front"])
        self.assertNotIn("{{Meaning}}", spec.templates["Production"]["Front"])
        self.assertIn("{{Picture}}", spec.templates["Production"]["Front"])
        for field in JAPANESE_VOCAB_FIELDS:
            self.assertIn(f"{{{{{field}}}}}", spec.front + spec.back)
        self.assertIn(".nightMode", spec.css)
        self.assertIn('[data-source="llm"]', spec.css)

    def test_model_installer_creates_missing_model(self):
        client = AnkiConnectClient()
        client.get_models = Mock(return_value=[])
        client.create_model = Mock(return_value={})
        result = client.install_model("Managed", ["Front", "Back"], ".card {}", {
            "Card": {"Front": "{{Front}}", "Back": "{{Back}}"},
        })
        self.assertEqual(result, "created")
        client.create_model.assert_called_once()

    def test_model_installer_updates_only_an_exact_managed_schema(self):
        client = AnkiConnectClient()
        client.get_models = Mock(return_value=["Managed"])
        client.get_model_fields = Mock(return_value=["Expression", "Meaning"])
        client.get_model_templates = Mock(return_value={
            "Card": {"Front": "{{Expression}}", "Back": "{{Meaning}}"},
        })
        client.update_model_templates = Mock()
        client.update_model_styling = Mock()
        templates = {"Card": {"Front": "{{Expression}}", "Back": "{{Meaning}}"}}
        self.assertEqual(
            client.install_model("Managed", ["Expression", "Meaning"], ".card {}", templates),
            "updated",
        )
        client.update_model_templates.assert_called_once_with("Managed", templates)
        client.update_model_styling.assert_called_once_with("Managed", ".card {}")

        client.get_model_fields.return_value = ["Unexpected"]
        with self.assertRaisesRegex(ValueError, "Refusing to overwrite"):
            client.install_model("Managed", ["Expression", "Meaning"], ".card {}", templates)

    def test_model_installer_upgrades_single_japanese_card_without_deleting_it(self):
        client = AnkiConnectClient()
        client.get_models = Mock(return_value=[JAPANESE_VOCAB_MODEL_NAME])
        client.get_model_fields = Mock(return_value=list(JAPANESE_VOCAB_FIELDS))
        client.get_model_templates = Mock(return_value={
            "Japanese Recognition": {"Front": "old", "Back": "old"},
        })
        client.rename_model_template = Mock()
        client.add_model_template = Mock()
        client.reposition_model_template = Mock()
        client.update_model_templates = Mock()
        client.update_model_styling = Mock()
        spec = japanese_vocab_template()

        result = client.install_model(
            spec.model_name, list(spec.fields), spec.css, spec.templates,
        )

        self.assertEqual(result, "updated")
        client.rename_model_template.assert_called_once_with(
            spec.model_name, "Japanese Recognition", "Comprehension",
        )
        self.assertEqual(
            [call.args[1] for call in client.add_model_template.call_args_list],
            ["Spelling", "Production"],
        )
        client.reposition_model_template.assert_not_called()

    def test_commit_rejects_stale_managed_card_template_shape(self):
        client = Mock()
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)
        client.get_model_templates.return_value = {
            "Japanese Recognition": {"Front": "old", "Back": "old"},
        }
        document = CardDocument(expression="攻撃", values={"meaning_text": "attack"})
        deck = {
            "deck_name": "Japanese",
            "note_type": JAPANESE_VOCAB_MODEL_NAME,
            "fields": japanese_vocab_template().field_mapping(),
        }

        with self.assertRaisesRegex(RuntimeError, "install-japanese-template"):
            commit_card_document(client, document, deck, "inject")

        client.add_note.assert_not_called()

    def test_exact_expression_lookup_filters_html_and_substring_candidates(self):
        client = AnkiConnectClient()
        client.find_notes = Mock(return_value=[1, 2, 3])
        client.get_notes_info = Mock(return_value=[
            {"noteId": 1, "fields": {"Expression": {"value": "<b>俳優</b>"}}},
            {"noteId": 2, "fields": {"Expression": {"value": "俳優組合"}}},
            {"noteId": 3, "fields": {
                "Expression": {"value": "役者"},
                "Meaning": {"value": "俳優"},
            }},
        ])

        result = client.find_exact_expression('Japanese "Vocab"', "俳優", ["Expression"])

        self.assertEqual([note["noteId"] for note in result], [1])
        client.find_notes.assert_called_once_with('deck:"Japanese \\"Vocab\\"" "俳優"')

    def test_media_snapshot_retrieval_uses_one_multi_request(self):
        client = AnkiConnectClient()
        client._request = Mock(return_value=[
            {"result": "image-data", "error": None},
            {"result": False, "error": None},
        ])

        result = client.retrieve_media_files(["image.jpg", "audio.mp3", "image.jpg"])

        self.assertEqual(result, {"image.jpg": "image-data", "audio.mp3": None})
        client._request.assert_called_once_with(
            "multi",
            actions=[
                {
                    "action": "retrieveMediaFile", "version": 6,
                    "params": {"filename": "image.jpg"},
                },
                {
                    "action": "retrieveMediaFile", "version": 6,
                    "params": {"filename": "audio.mp3"},
                },
            ],
        )

    def test_note_model_migration_uses_in_place_ankiconnect_action(self):
        client = AnkiConnectClient()
        client._request = Mock(return_value=None)

        client.update_note_model(7, "Managed", {"Expression": "俳優"}, ["tag"])

        client._request.assert_called_once_with(
            "updateNoteModel",
            note={
                "id": 7,
                "modelName": "Managed",
                "fields": {"Expression": "俳優"},
                "tags": ["tag"],
            },
        )


class OllamaPromptTests(unittest.TestCase):
    def test_vocabulary_prompt_uses_translation_language(self):
        client = OllamaClient(model="test")
        client._post = Mock(return_value={
            "response": '{"definition":"must be discarded","nuances":"formal usage","examples":[]}'
        })
        result = client.generate_card_content(
            "食べる", "", "Japanese", "Vietnamese", "Explain with {lang} translations"
        )
        payload = client._post.call_args.args[1]
        self.assertEqual(result["nuances"], "formal usage")
        self.assertEqual(set(result), {"nuances", "examples"})
        self.assertTrue(payload["system"].startswith("Explain with Vietnamese translations"))
        self.assertIn("Generate only a usage nuance", payload["system"])
        self.assertEqual(payload["format"]["properties"]["examples"]["minItems"], 3)
        self.assertIn("Source language: \"Japanese\"", payload["prompt"])
        self.assertIn("Vietnamese", payload["prompt"])

    def test_vocabulary_prompt_keeps_structured_dictionary_context(self):
        client = OllamaClient(model="test")
        client._post = Mock(return_value={"response": '{"nuances":"ok","examples":[]}'})
        context = '[{"word":"俳優"},{"word":"俳優組合"}]'
        client.generate_card_content("俳優", context, "Japanese", "English", "Prompt")
        prompt = client._post.call_args.args[1]["prompt"]
        self.assertIn("俳優組合", prompt)
        self.assertIn("Do not output dictionary definitions", prompt)
        self.assertNotIn('"definition"', prompt)

    def test_vocabulary_generation_normalizes_model_aliases(self):
        client = OllamaClient(model="test")
        client._post = Mock(return_value={
            "response": json.dumps({
                "usage_nuance": "Professional and gender-neutral.",
                "example_sentences": [
                    {"japanese": "彼は俳優です。", "english": "He is an actor."}
                ],
            }, ensure_ascii=False)
        })
        result = client.generate_card_content("俳優", "parsed data", "Japanese", "English", "Prompt")
        self.assertEqual(result["nuances"], "Professional and gender-neutral.")
        self.assertEqual(result["examples"][0]["sentence"], "彼は俳優です。")
        self.assertEqual(result["examples"][0]["translation"], "He is an actor.")

    def test_vocabulary_generation_retries_wrong_response_shape(self):
        client = OllamaClient(model="test")
        client._post = Mock(side_effect=[
            {"response": '{"definition":"actor"}'},
            {"response": '{"nuance":"Professional term.","sentences":["彼は俳優です。"]}'},
        ])
        result = client.generate_card_content("俳優", "parsed data", "Japanese", "English", "Prompt")
        self.assertEqual(client._post.call_count, 2)
        self.assertEqual(result["nuances"], "Professional term.")
        self.assertEqual(result["examples"][0]["sentence"], "彼は俳優です。")

    def test_grammar_prompt_uses_translation_language(self):
        client = OllamaClient(model="test")
        client._post = Mock(return_value={"response": '{"grammar_point":"x"}'})
        client.generate_grammar_content("raw", "English", "Answer in {lang}")
        self.assertEqual(client._post.call_args.args[1]["system"], "Answer in English")

    def test_rejects_invalid_response_shape(self):
        client = OllamaClient(model="test")
        client._post = Mock(return_value={"response": "[]"})
        with self.assertRaises(ValueError):
            client.generate_card_content("word", "", "English", "English", "Prompt")


class CommitTests(unittest.TestCase):
    def setUp(self):
        self.config = copy.deepcopy(DEFAULT_CONFIG)
        deck = self.config["decks"]["japanese_vocab"]
        deck["deck_name"] = "Japanese"
        deck["note_type"] = "Basic"

    def processed(self):
        return {
            "word": "攻撃",
            "suggestion": "",
            "llm_response": {
                "definition": "hallucinated definition must be ignored",
                "nuances": "Often used for a deliberate attack.",
                "examples": [],
            },
            "classification": "dictionary",
            "orig_filenames": ["old.png"],
            "renamed_images": [],
            "new_image_b64": base64.b64encode(b"image").decode(),
            "new_image_filename": "new.png",
            "audio_b64": None,
            "audio_filename": "",
            "kanji_construction": "",
            "scraped": {
                "found": True,
                "word": "攻撃",
                "reading": "こうげき",
                "definition": "attack (dictionary parsing)",
            },
        }

    def test_modernization_updates_before_deleting_old_media(self):
        client = Mock()
        events = []
        client.store_media_file.side_effect = lambda name, data: events.append(("store", name))
        client.update_note_fields.side_effect = lambda note, fields: events.append(("update", note))
        client.delete_media_file.side_effect = lambda name: events.append(("delete", name))
        commit_card_modernization(
            client, {"noteId": 7}, self.processed(), "japanese_vocab", self.config
        )
        self.assertLess(events.index(("update", 7)), events.index(("delete", "old.png")))

    def test_modernization_migrates_legacy_note_in_place(self):
        client = Mock()
        client.supports_action.return_value = True
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)

        note_id = commit_card_modernization(
            client,
            {
                "noteId": 7,
                "modelName": "2. Picture Words",
                "tags": ["existing"],
                "fields": {"Word": {"value": "攻撃"}},
            },
            self.processed(), "japanese_vocab", self.config,
        )

        self.assertEqual(note_id, 7)
        client.update_note_model.assert_called_once()
        args = client.update_note_model.call_args.args
        self.assertEqual(args[0:2], (7, JAPANESE_VOCAB_MODEL_NAME))
        self.assertEqual(args[2]["Expression"], "攻撃")
        self.assertEqual(args[3], ["existing"])
        client.update_note_fields.assert_not_called()

    def test_modernization_requires_current_ankiconnect_for_migration(self):
        client = Mock()
        client.supports_action.return_value = False
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)

        with self.assertRaisesRegex(RuntimeError, "updateNoteModel"):
            commit_card_modernization(
                client,
                {
                    "noteId": 7,
                    "modelName": "2. Picture Words",
                    "fields": {"Word": {"value": "攻撃"}},
                },
                self.processed(), "japanese_vocab", self.config,
            )

        client.store_media_file.assert_not_called()
        client.update_note_model.assert_not_called()

    def test_empty_word_never_deletes_note(self):
        client = Mock()
        processed = self.processed()
        processed["word"] = ""
        with self.assertRaises(ValueError):
            commit_card_modernization(
                client, {"noteId": 7}, processed, "japanese_vocab", self.config
            )
        client.delete_notes.assert_not_called()

    def test_modernization_uses_dictionary_definition_not_llm_definition(self):
        client = Mock()
        commit_card_modernization(
            client, {"noteId": 7}, self.processed(), "japanese_vocab", self.config
        )
        fields = client.update_note_fields.call_args.args[1]
        meaning = fields[japanese_vocab_template().field_mapping()["meaning_text"]]
        self.assertIn("attack (dictionary parsing)", meaning)
        self.assertIn("Often used for a deliberate attack.", meaning)
        self.assertNotIn("hallucinated definition", meaning)

    def test_ingestion_cleans_staged_media_if_add_fails(self):
        client = Mock()
        client.add_note.side_effect = RuntimeError("fail")
        processed = self.processed()
        with self.assertRaises(RuntimeError):
            commit_card_ingestion(client, processed, "japanese_vocab", self.config)
        client.delete_media_file.assert_called_with("new.png")

    def test_injection_without_dictionary_uses_context_and_template_fields(self):
        client = Mock()
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)
        client.add_note.return_value = 42
        config = copy.deepcopy(self.config)
        config["decks"]["japanese_vocab"]["note_type"] = JAPANESE_VOCAB_MODEL_NAME
        config["decks"]["japanese_vocab"]["fields"] = japanese_vocab_template().field_mapping()
        processed = {
            "word": "推し活",
            "suggestion": "",
            "scraped": {"found": False},
            "llm_response": {},
            "type_tag": "internet slang",
            "source_note": "Seen in a concert announcement.",
        }

        note_id = commit_card_ingestion(
            client, processed, "japanese_vocab", config
        )

        self.assertEqual(note_id, 42)
        deck, model, fields = client.add_note.call_args.args
        self.assertEqual((deck, model), ("Japanese", JAPANESE_VOCAB_MODEL_NAME))
        self.assertEqual(fields["Expression"], "推し活")
        self.assertIn("Not found in standard dictionary", fields["Meaning"])
        self.assertIn("internet slang", fields["Meaning"])
        self.assertIn("Seen in a concert announcement", fields["Meaning"])
        self.assertEqual(
            client.add_note.call_args.kwargs["tags"],
            ["linguist-injected", "linguist::internet_slang"],
        )

    def test_modernization_rejects_wrong_note_schema_before_staging_media(self):
        client = Mock()
        note = {"noteId": 7, "fields": {"Front": {"value": "攻撃"}}}

        with self.assertRaisesRegex(ValueError, "absent from the Anki note type"):
            commit_card_modernization(
                client, note, self.processed(), "japanese_vocab", self.config
            )

        client.store_media_file.assert_not_called()
        client.update_note_fields.assert_not_called()


class CardDocumentTransactionTests(unittest.TestCase):
    def setUp(self):
        self.deck = {
            "deck_name": "Japanese",
            "note_type": JAPANESE_VOCAB_MODEL_NAME,
            "fields": japanese_vocab_template().field_mapping(),
        }

    def test_mapping_combines_logical_values_that_share_a_physical_field(self):
        document = CardDocument(
            expression="俳優",
            values={
                "meaning_image": "<img src='actor.jpg'/>",
                "meaning_text": "actor",
                "kanji_construction": None,
                "audio": None,
            },
        )
        deck = copy.deepcopy(self.deck)
        deck["fields"]["meaning_image"] = "Meaning"

        fields = map_document_fields(document, deck)

        self.assertEqual(fields["Expression"], "俳優")
        self.assertEqual(fields["Meaning"], "<img src='actor.jpg'/><br/>actor")

    def test_failed_note_write_restores_overwritten_media(self):
        client = Mock()
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)
        client.retrieve_media_file.return_value = "old-base64"
        client.add_note.side_effect = RuntimeError("note rejected")
        document = CardDocument(
            expression="俳優",
            values={"meaning_text": "actor"},
            media=[MediaAsset("img_japanese_vocab_俳優_0.jpg", "new-base64")],
        )

        with self.assertRaisesRegex(RuntimeError, "note rejected"):
            commit_card_document(client, document, self.deck, "inject")

        self.assertEqual(
            client.store_media_file.call_args_list,
            [
                unittest.mock.call("img_japanese_vocab_俳優_0.jpg", "new-base64"),
                unittest.mock.call("img_japanese_vocab_俳優_0.jpg", "old-base64"),
            ],
        )
        client.delete_media_file.assert_not_called()

    def test_preloaded_snapshot_media_is_reused_during_commit(self):
        client = Mock()
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)
        client.add_note.return_value = 42
        document = CardDocument(
            expression="俳優",
            values={"meaning_text": "actor"},
            media=[MediaAsset("image.jpg", "new-base64")],
        )

        note_id = commit_card_document(
            client, document, self.deck, "inject",
            media_before={"image.jpg": "old-base64"},
        )

        self.assertEqual(note_id, 42)
        client.retrieve_media_file.assert_not_called()
        client.store_media_file.assert_called_once_with("image.jpg", "new-base64")

    def test_duplicate_media_filename_with_different_content_is_rejected(self):
        client = Mock()
        client.get_model_fields.return_value = list(JAPANESE_VOCAB_FIELDS)
        document = CardDocument(
            expression="俳優",
            values={"meaning_text": "actor"},
            media=[MediaAsset("same.jpg", "one"), MediaAsset("same.jpg", "two")],
        )

        with self.assertRaisesRegex(ValueError, "Conflicting media payloads"):
            commit_card_document(client, document, self.deck, "inject")

        client.store_media_file.assert_not_called()
        client.add_note.assert_not_called()

    def test_modernize_document_preserves_existing_audio_when_no_replacement_exists(self):
        document = build_card_document(
            {
                "word": "俳優",
                "scraped": {"found": True, "word": "俳優", "definition": "actor"},
                "llm_response": {},
                "classification": "visual_recall",
                "filename": "existing.jpg",
                "audio_b64": None,
                "audio_filename": "",
            },
            "japanese_vocab",
            "modernize",
        )

        fields = map_document_fields(document, self.deck)

        self.assertNotIn("Audio", fields)
        self.assertEqual(fields["Picture"], "<img src='existing.jpg'/>")

    def test_dictionary_screenshot_is_replaced_only_after_fetch_succeeds(self):
        base = {
            "word": "進む",
            "scraped": {"found": False},
            "llm_response": {},
            "classification": "dictionary",
            "filename": "dictionary.png",
            "orig_filenames": ["dictionary.png"],
            "renamed_images": [],
            "audio_b64": None,
            "audio_filename": "",
        }
        failed = build_card_document(base, "japanese_vocab", "modernize")
        self.assertEqual(map_document_fields(failed, self.deck)["Picture"], "<img src='dictionary.png'/>")
        self.assertNotIn("dictionary.png", failed.obsolete_media)

        fetched = build_card_document({
            **base,
            "new_image_filename": "commons_進む_0.jpg",
            "new_image_b64": base64.b64encode(b"replacement").decode(),
        }, "japanese_vocab", "modernize")
        self.assertEqual(map_document_fields(fetched, self.deck)["Picture"], "<img src='commons_進む_0.jpg'/>")
        self.assertIn("dictionary.png", fetched.obsolete_media)


class InjectProcessingTests(unittest.TestCase):
    def test_english_kanji_failure_falls_back_to_kanjiapi(self):
        scraper = Mock()

        async def scrape(char, url, schema):
            if "jisho.org" in url:
                raise RuntimeError("HTTP Error 502")
            if "kanjiapi.dev" in url:
                return f"<section data-kanji='{char}'>KanjiAPI {char}</section>"
            return ""

        scraper.scrape_kanji_details = AsyncMock(side_effect=scrape)
        config = copy.deepcopy(DEFAULT_CONFIG)
        result = asyncio.run(fetch_kanji_construction_if_needed("進む", scraper, config))
        self.assertIn("KanjiAPI 進", result)
        self.assertEqual(scraper.scrape_kanji_details.await_count, 2)

    def test_vietnamese_kanji_failure_falls_back_per_character(self):
        scraper = Mock()

        async def scrape(char, url, schema):
            if "hvdic.thivien.net" in url:
                raise RuntimeError("certificate has expired")
            return f"<section data-kanji='{char}'>Jisho {char}</section>"

        scraper.scrape_kanji_details = AsyncMock(side_effect=scrape)
        config = copy.deepcopy(DEFAULT_CONFIG)
        config["kanji"].update({
            "source_lang": "vietnamese",
            "url_template": "https://hvdic.thivien.net/whv/{char}",
        })

        result = asyncio.run(fetch_kanji_construction_if_needed(
            "俳優", scraper, config,
        ))

        self.assertIn("Jisho 俳", result)
        self.assertIn("Jisho 優", result)
        self.assertEqual(scraper.scrape_kanji_details.await_count, 4)

    def test_dictionary_free_processing_preserves_author_context(self):
        scraper = Mock()
        scraper.scrape_jisho = AsyncMock(return_value={"found": False})
        llm = Mock(model=None)
        config = copy.deepcopy(DEFAULT_CONFIG)
        config["dictionary"]["preset"] = "jisho"

        with (
            patch(
                "linguist_anki_bridge.tui.screens.fetch_kanji_construction_if_needed",
                new=AsyncMock(return_value=""),
            ),
            patch(
                "linguist_anki_bridge.tui.screens.fetch_web_image",
                new=AsyncMock(return_value=(None, "")),
            ),
            patch(
                "linguist_anki_bridge.tui.screens.fetch_audio_base64_if_any",
                new=AsyncMock(return_value=(None, "")),
            ),
        ):
            result = asyncio.run(process_inject_item(
                scraper,
                llm,
                {
                    "word": "推し活",
                    "type_tag": "internet slang",
                    "note": "Seen in a concert announcement.",
                },
                "japanese_vocab",
                config,
            ))

        self.assertFalse(result["scraped"]["found"])
        self.assertEqual(result["word"], "推し活")
        self.assertEqual(result["type_tag"], "internet slang")
        self.assertEqual(result["source_note"], "Seen in a concert announcement.")


class RuntimeHelperTests(unittest.TestCase):
    def test_image_preview_has_rule_before_dictionary_details(self):
        source = Path("src/linguist_anki_bridge/tui/app.py").read_text(encoding="utf-8")
        marker = '"─────────────────────────\\n\\n"'
        self.assertIn(marker, source)
        self.assertLess(source.index(marker), source.index("preview.update_dict_scrape(classification_markup"))

    def test_universal_search_uses_pane_focused_before_modals(self):
        table = Mock(id="table-queue", row_count=1)
        table.get_row_at.return_value = ("俳優", "actor.jpg")
        app = Mock()
        app._search_origin_widget = table
        app._resolve_search_target = types.MethodType(
            AnkiBridgeApp._resolve_search_target, app,
        )
        app.jump_to_search_match = Mock()
        app.push_screen = Mock()

        AnkiBridgeApp.on_search_query_submitted(app, "俳優")

        self.assertEqual(app.search_matches, [0])
        self.assertIs(app.search_target_widget, table)
        app.jump_to_search_match.assert_called_once()
        app.push_screen.assert_called_once()

    def test_injection_form_uses_normal_insert_and_write_command(self):
        async def exercise():
            manager = Mock()
            manager.config = {"decks": {"japanese_vocab": {"deck_name": "Japanese"}}}
            app = App()
            result = []
            async with app.run_test() as pilot:
                screen = InputManagementScreen(manager, "japanese_vocab")
                app.push_screen(screen, result.append)
                await pilot.pause()
                await pilot.press("i", "t", "e", "s", "t", "escape", ":", "w")
                await pilot.pause()
            return result

        result = asyncio.run(exercise())
        self.assertEqual(result[0]["word"], "test")
        self.assertEqual(result[0]["action"], "manual")
        self.assertEqual(result[0]["words"], ["test"])

    def test_generated_examples_are_sanitized_and_kept_on_one_line(self):
        self.assertEqual(sanitize_generated_text("  : • Sentence text  "), "Sentence text")
        self.assertEqual(sanitize_generated_text(" ｛《［日本語です。］》｝ "), "日本語です。")
        self.assertEqual(sanitize_generated_text(" <{Translation text}> "), "Translation text")
        rendered = format_llm_annotations_html("", [{
            "sentence": " :: 日本語です。 ",
            "translation": " - It is Japanese. ",
        }])
        self.assertIn("1. 日本語です。</b> — <span", rendered)
        self.assertIn("It is Japanese.", rendered)
        self.assertNotIn("<br", rendered)


class SnapshotManagerTests(unittest.TestCase):
    def test_modernization_snapshot_restores_fields_and_media(self):
        with tempfile.TemporaryDirectory() as directory:
            manager = SnapshotManager(Path(directory) / "snapshots.json")
            snapshot_id = manager.create(
                word="俳優", mode="modernize", deck_key="japanese_vocab",
                note={"noteId": 42, "modelName": "Legacy", "fields": {
                    "Word": {"value": "俳優"}, "Meaning": {"value": "old"},
                }},
                processed={"word": "俳優", "audio_b64": "large-payload"},
                dry_run=False, media_before={"image.jpg": "old-base64", "new.mp3": None},
            )
            manager.finalize(snapshot_id, result_note_id=42)
            anki = Mock()
            anki.get_notes_info.return_value = [{"modelName": "Legacy"}]
            message = manager.revert(snapshot_id, anki)
            anki.store_media_file.assert_called_once_with("image.jpg", "old-base64")
            anki.delete_media_file.assert_called_once_with("new.mp3")
            anki.update_note_fields.assert_called_once_with(42, {"Word": "俳優", "Meaning": "old"})
            self.assertIn("Restored note 42", message)

    def test_snapshot_revert_restores_original_note_type(self):
        with tempfile.TemporaryDirectory() as directory:
            manager = SnapshotManager(Path(directory) / "snapshots.json")
            snapshot_id = manager.create(
                word="俳優", mode="modernize", deck_key="japanese_vocab",
                note={
                    "noteId": 42, "modelName": "2. Picture Words",
                    "tags": ["old-tag"],
                    "fields": {"Word": {"value": "俳優"}},
                },
                processed={"word": "俳優"}, dry_run=False, media_before={},
            )
            manager.finalize(snapshot_id, result_note_id=42)
            anki = Mock()
            anki.get_notes_info.return_value = [
                {"modelName": JAPANESE_VOCAB_MODEL_NAME},
            ]
            anki.supports_action.return_value = True

            manager.revert(snapshot_id, anki)

            anki.update_note_model.assert_called_once_with(
                42, "2. Picture Words", {"Word": "俳優"}, ["old-tag"],
            )
            anki.update_note_fields.assert_not_called()

    def test_injection_snapshot_revert_deletes_created_note(self):
        with tempfile.TemporaryDirectory() as directory:
            manager = SnapshotManager(Path(directory) / "snapshots.json")
            snapshot_id = manager.create(
                word="推し活", mode="inject", deck_key="japanese_vocab",
                note=None, processed={"word": "推し活"}, dry_run=False,
                media_before={"audio.mp3": None},
            )
            manager.finalize(snapshot_id, result_note_id=88)
            anki = Mock()
            manager.revert(snapshot_id, anki)
            anki.delete_notes.assert_called_once_with([88])
            anki.delete_media_file.assert_called_once_with("audio.mp3")

    def test_media_stem_is_safe_and_stable(self):
        first = safe_media_stem("../../食べる / test")
        self.assertEqual(first, safe_media_stem("../../食べる / test"))
        self.assertEqual(first, "食べる_test")
        self.assertNotIn("/", first)
        self.assertNotIn("..", first)

    def test_media_names_use_zero_based_indexes_without_hashes(self):
        self.assertEqual(
            indexed_media_filename("img", "俳優", "jpg", 0, "japanese_vocab"),
            "img_japanese_vocab_俳優_0.jpg",
        )
        self.assertEqual(
            indexed_media_filename("audio", "俳優", "mp3", scope="japanese_vocab"),
            "audio_japanese_vocab_俳優_0.mp3",
        )

    def test_log_handler_posts_thread_safe_message(self):
        app = Mock()
        handler = TuiLogHandler(app)
        handler.emit(__import__("logging").LogRecord("x", 20, __file__, 1, "hello", (), None))
        message = app.post_message.call_args.args[0]
        self.assertIsInstance(message, TuiLogMessage)
        self.assertEqual(message.text, "hello")

    def test_deck_alias_normalization(self):
        fake = Mock()
        fake.active_deck_key = "english_vocab"
        fake.config_manager.config = copy.deepcopy(DEFAULT_CONFIG)
        self.assertEqual(AnkiBridgeApp.normalize_deck_key(fake, "japanese"), "japanese_vocab")
        self.assertIsNone(AnkiBridgeApp.normalize_deck_key(fake, "unknown"))

    def test_space_menu_defers_settings_until_selection_key_finishes(self):
        fake = Mock()
        AnkiBridgeApp.on_space_menu_result(fake, "settings")
        fake.call_after_refresh.assert_called_once_with(fake.action_configure_setup)

    def test_custom_extraction_schema_validation(self):
        with self.assertRaises(ValueError):
            Crawl4AiScraper._validate_schema({"fields": []})
        Crawl4AiScraper._validate_schema({
            "baseSelector": ".entry",
            "fields": [{"name": "definition", "selector": ".definition", "type": "text"}],
        })


class DictionaryParsingTests(unittest.TestCase):
    @staticmethod
    def parsed_entries():
        return {
            "found": True,
            "word": "俳優",
            "reading": "はいゆう",
            "entries": [
                {
                    "word": "俳優", "reading": "はいゆう",
                    "forms": [{"word": "俳優", "reading": "はいゆう"}],
                    "is_common": True, "jlpt": ["JLPT N3"], "tags": ["Wanikani level 23"],
                    "senses": [{
                        "number": 1, "definitions": ["actor", "actress"],
                        "parts_of_speech": ["Noun"], "tags": [], "see_also": ["役者"],
                        "antonyms": [], "info": [], "restrictions": [], "links": [],
                    }],
                },
                {
                    "word": "俳優組合", "reading": "はいゆうくみあい",
                    "forms": [{"word": "俳優組合", "reading": "はいゆうくみあい"}],
                    "is_common": False, "jlpt": [], "tags": [],
                    "senses": [{
                        "number": 1, "definitions": ["British Actors' Equity Association"],
                        "parts_of_speech": ["Noun"], "tags": ["Organization name"],
                        "see_also": [], "antonyms": [], "info": [], "restrictions": [], "links": [],
                    }],
                },
            ],
        }

    def test_jisho_parser_preserves_all_entries_and_senses(self):
        payload = {
            "data": [
                {
                    "is_common": True,
                    "tags": ["wanikani23"],
                    "jlpt": ["jlpt-n3"],
                    "japanese": [{"word": "俳優", "reading": "はいゆう"}],
                    "senses": [
                        {
                            "english_definitions": ["actor", "actress"],
                            "parts_of_speech": ["Noun"],
                            "see_also": ["役者"],
                            "links": [],
                        },
                        {
                            "english_definitions": ["Actor"],
                            "parts_of_speech": ["Wikipedia definition"],
                            "links": [{"text": "Wikipedia", "url": "https://example.test"}],
                        },
                    ],
                },
                {
                    "japanese": [{"word": "俳優組合", "reading": "はいゆうくみあい"}],
                    "senses": [{"english_definitions": ["British Actors' Equity Association"]}],
                },
                {
                    "japanese": [{"word": "俳優一覧"}],
                    "senses": [{
                        "english_definitions": ["Lists of actors"],
                        "parts_of_speech": ["Wikipedia definition"],
                        "links": [{"text": "Wikipedia", "url": "https://example.test"}],
                    }],
                },
            ]
        }
        result = Crawl4AiScraper.parse_jisho_response("俳優", payload)
        self.assertEqual(len(result["entries"]), 3)
        self.assertEqual(len(result["entries"][0]["senses"]), 2)
        self.assertIn("Wanikani level 23", result["definition"])
        self.assertIn("俳優組合", result["llm_context"])
        self.assertIn("Actor", result["definition"])
        self.assertIn("俳優一覧", result["definition"])
        self.assertIn("Lists of actors", result["llm_context"])
        self.assertIn("役者", result["llm_context"])
        self.assertNotIn("Wikipedia definition", result["definition"])
        self.assertNotIn("Wikipedia", result["llm_context"])

    def test_jisho_rendered_html_fallback_parses_entries(self):
        page = """
        <div class="concept_light clearfix">
          <div class="concept_light-representation">
            <span class="furigana">すすむ</span><span class="text">進む</span>
          </div>
          <span class="concept_light-tag concept_light-common">Common word</span>
          <span class="concept_light-tag">JLPT N5</span>
          <div class="meaning-wrapper">
            <span class="meaning-tags">Godan verb</span>
            <span class="meaning-meaning">to advance; to move forward</span>
          </div>
        </div>
        """
        result = Crawl4AiScraper.parse_jisho_html("進む", page)
        self.assertTrue(result["found"])
        self.assertEqual(result["word"], "進む")
        self.assertEqual(result["reading"], "すすむ")
        self.assertIn("to advance", result["definition"])
        self.assertIn("Common word · JLPT N5", result["definition"])

    def test_jisho_direct_request_retries_before_success(self):
        failed = OSError("temporary 502")
        payload = json.dumps({
            "data": [{
                "japanese": [{"word": "進む", "reading": "すすむ"}],
                "senses": [{"english_definitions": ["to advance"]}],
            }]
        }).encode()
        loop = Mock()
        loop.run_in_executor = AsyncMock(side_effect=[failed, payload])
        scraper = Crawl4AiScraper()
        with patch("linguist_anki_bridge.scraper.asyncio.get_running_loop", return_value=loop):
            result = asyncio.run(scraper.scrape_jisho(
                "進む", retry_count=3, backoff=0, browser_fallback=False,
            ))
        self.assertTrue(result["found"])
        self.assertEqual(loop.run_in_executor.await_count, 2)

    def test_web_image_is_normalized_to_baseline_rgb_jpeg(self):
        from io import BytesIO
        from PIL import Image

        image_buffer = BytesIO()
        source = Image.new("CMYK", (24, 24), (0, 0, 0, 0))
        for x in range(12, 24):
            for y in range(24):
                source.putpixel((x, y), (0, 100, 100, 0))
        source.save(image_buffer, "JPEG")
        api_payload = json.dumps({
            "query": {"pages": {"1": {
                "title": "進む", "thumbnail": {"source": "https://example.test/image.jpg"}
            }}}
        }).encode()
        loop = Mock()
        loop.run_in_executor = AsyncMock(side_effect=[api_payload, image_buffer.getvalue()])
        with patch("linguist_anki_bridge.tui.screens.asyncio.get_running_loop", return_value=loop):
            encoded, filename = asyncio.run(fetch_web_image("進む"))
        with Image.open(BytesIO(base64.b64decode(encoded))) as normalized:
            self.assertEqual(normalized.format, "JPEG")
            self.assertEqual(normalized.mode, "RGB")
            self.assertFalse(normalized.info.get("progression"))
        self.assertTrue(filename.endswith(".jpg"))

    def test_image_classification_is_deterministic_and_model_independent(self):
        engine = OcrEngine()
        dictionary_text = (
            "俳優\nCommon word JLPT N3 Wanikani level 23\nPlay audio\nLinks\n"
            "Noun\n1. actor; actress; player; performer\nSee also 役者"
        )
        self.assertEqual(engine.classify_image_ocr(dictionary_text), "dictionary")
        self.assertEqual(engine.classify_image_ocr("駅前で猫が眠っている"), "visual_recall")
        self.assertEqual(engine.classify_image_ocr(""), "visual_recall")

    def test_modern_image_classifier_uses_uncertain_for_empty_ocr(self):
        engine = OcrEngine()
        features = {
            "text_coverage": 0.0, "token_count": 0, "row_count": 0,
            "row_density": 0.0, "alignment": 0.0, "edge_density": 0.3,
            "entropy": 0.7, "colour_std": 0.5, "dominant_colour": 0.1,
        }
        with patch.object(engine, "extract_visual_features", return_value=features), \
             patch.object(engine, "_learned_probability", return_value=None):
            result = engine.classify_image("aGVsbG8=", "", config={})
        self.assertEqual(result["classification"], "uncertain")
        self.assertIn("probability", result)

    def test_image_feedback_is_stored_as_user_training_label(self):
        engine = OcrEngine()
        with tempfile.TemporaryDirectory() as directory:
            engine.feedback_path = Path(directory) / "feedback.json"
            engine.record_classification_feedback(
                base64.b64encode(b"image").decode(),
                {"probability": 0.5, "features": {"ocr_score": 0}},
                "dictionary",
            )
            row = json.loads(engine.feedback_path.read_text())[0]
        self.assertEqual(row["source"], "user")
        self.assertEqual(row["label"], "dictionary")

    def test_llm_input_contains_only_dictionary_parsing_and_ocr(self):
        parsed = self.parsed_entries()
        parsed["llm_context"] = '[{"word":"俳優"}]'
        context = dictionary_llm_context(parsed, "OCR DICTIONARY TEXT")
        self.assertIn("EXACT DICTIONARY PARSING", context)
        self.assertIn("俳優", context)
        self.assertIn("RAW OCR RESULT", context)
        self.assertIn("OCR DICTIONARY TEXT", context)

    def test_dictionary_html_is_authoritative_and_llm_is_under_exact_entry(self):
        result = format_dictionary_meaning_html(
            self.parsed_entries(),
            "俳優",
            "Gender-neutral professional term.",
            [{"sentence": "彼は俳優です。", "translation": "He is an actor."}],
        )
        exact_pos = result.index("<b>はいゆう / 俳優</b>")
        related_pos = result.index("<b>はいゆうくみあい / 俳優組合</b>")
        nuance_pos = result.index("Gender-neutral professional term.")
        self.assertLess(exact_pos, nuance_pos)
        self.assertLess(nuance_pos, related_pos)
        self.assertIn("actor; actress", result)
        self.assertIn("British Actors&#x27; Equity Association", result)
        self.assertEqual(result.count("data-source='llm'"), 2)
        self.assertNotIn("<b>Definition:</b>", result)

    def test_dictionary_html_keeps_cached_wikipedia_definitions_without_source_labels_or_links(self):
        parsed = self.parsed_entries()
        parsed["entries"][0]["senses"].append({
            "number": 2,
            "definitions": ["Actor"],
            "parts_of_speech": ["Wikipedia definition"],
            "tags": [], "see_also": [], "antonyms": [], "info": [], "restrictions": [],
            "links": [{"text": "Read Wikipedia", "url": "https://example.test"}],
        })
        parsed["entries"][0]["senses"][0]["links"] = [
            {"text": "Unwanted link", "url": "https://example.test"}
        ]
        result = format_dictionary_meaning_html(parsed, "俳優", "", [])
        self.assertIn("<b>2.</b> Actor", result)
        self.assertNotIn("Wikipedia definition", result)
        self.assertNotIn("<b>Links:</b>", result)
        self.assertNotIn("Unwanted link", result)

    def test_dictionary_html_compacts_senses_and_separates_related_entries(self):
        result = format_dictionary_meaning_html(self.parsed_entries(), "俳優", "", [])
        self.assertIn("margin-top:2px", result)
        self.assertNotIn("RELATED DICTIONARY RESULT", result)
        self.assertIn("<hr style='border:0;border-top:2px solid #888", result)

    def test_kanji_preview_abbreviates_embedded_stroke_image(self):
        result = kanji_summary_for_tui(
            "<div><b>Kanji 俳</b><br><img src='data:image/gif;base64,R0lGODlhAQABAIAAAAAA'/><br>Meaning: actor</div>"
        )
        self.assertIn("Kanji 俳", result)
        self.assertIn("[Stroke-order image]", result)
        self.assertIn("Meaning: actor", result)
        self.assertNotIn("R0lGOD", result)

    def test_hvdic_parser_preserves_lines_and_embeds_stroke_gif(self):
        page = b"""
        <div class='hvres han-word'><div class='hvres-details'><div class='hvres-meaning'>
          <div class='hvres-animation'>widget</div>
          Am Han Viet: bai, boi<br/>Tong net: 10<br/>Bo: nhan<br/>Hinh thai: left-right
        </div></div></div>
        <div class='hvres' data-hvres-idx='1'>
          <span class='hvres-spell'>bai</span><span class='hvres-info'>reading</span>
          <div class='hvres-details'><p class='hvres-source'>Tu dien trich dan</p>
          <div class='hvres-meaning'>1. actor<br/>2. performer</div></div>
        </div>
        """
        gif = b"GIF89a" + b"stroke-data"
        scraper = Crawl4AiScraper()
        scraper._fetch_bytes = AsyncMock(side_effect=[page, gif])
        result = asyncio.run(scraper._scrape_hvdic_kanji("俳", "https://hvdic.thivien.net/whv/x"))
        self.assertIn("data:image/gif;base64,", result)
        self.assertIn("Tong net: 10", result)
        self.assertIn("1. actor", result)
        self.assertIn("2. performer", result)
        self.assertNotIn("widget", result)
        self.assertIn("<section data-kanji='俳'", result)
        self.assertIn("border-bottom:2px solid #888", result)

if __name__ == "__main__":
    unittest.main()
