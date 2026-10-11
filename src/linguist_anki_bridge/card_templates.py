from dataclasses import dataclass, field
from importlib.resources import files
from typing import Mapping


@dataclass(frozen=True)
class CardTemplateSpec:
    """A complete Anki note type definition managed by the bridge."""

    model_name: str
    fields: tuple[str, ...]
    card_name: str
    front: str
    back: str
    css: str
    additional_templates: Mapping[str, Mapping[str, str]] = field(default_factory=dict)

    @property
    def templates(self) -> dict[str, dict[str, str]]:
        """Return the AnkiConnect template payload for this note type."""
        result = {self.card_name: {"Front": self.front, "Back": self.back}}
        result.update({
            name: {"Front": value["Front"], "Back": value["Back"]}
            for name, value in self.additional_templates.items()
        })
        return result

    def field_mapping(self) -> dict[str, str]:
        """Return canonical logical-to-physical field mapping for the spec.

        Keeping this beside the template prevents modernize/inject code from
        inventing a second schema.  Language-specific specs can override it.
        """
        if self.model_name == JAPANESE_VOCAB_MODEL_NAME:
            return dict(JAPANESE_VOCAB_FIELD_MAPPING)
        return {name.lower(): name for name in self.fields}

    def validate_values(self, values: Mapping[str, object]) -> list[str]:
        """Report missing required fields before attempting an Anki write."""
        mapping = self.field_mapping()
        return [physical for logical, physical in mapping.items()
                if logical in {"expression", "meaning_text"}
                and not str(values.get(logical, "") or "").strip()]


JAPANESE_VOCAB_MODEL_NAME = "Linguist Japanese Vocabulary"
JAPANESE_VOCAB_FIELDS = ("Expression", "Picture", "Meaning", "Kanji", "Audio")
JAPANESE_VOCAB_FIELD_MAPPING = {
    "expression": "Expression",
    "meaning_image": "Picture",
    "meaning_text": "Meaning",
    "kanji_construction": "Kanji",
    "audio": "Audio",
}


def japanese_vocab_template() -> CardTemplateSpec:
    root = files("linguist_anki_bridge").joinpath("templates", "japanese_vocab")
    return CardTemplateSpec(
        model_name=JAPANESE_VOCAB_MODEL_NAME,
        fields=JAPANESE_VOCAB_FIELDS,
        # Keep comprehension at ordinal zero.  This preserves the existing
        # card when upgrading the former one-template managed note type and
        # maps naturally to the first legacy card during note migration.
        card_name="Comprehension",
        front=root.joinpath("front.html").read_text(encoding="utf-8").strip(),
        back=root.joinpath("back.html").read_text(encoding="utf-8").strip(),
        css=root.joinpath("style.css").read_text(encoding="utf-8").strip(),
        additional_templates={
            "Spelling": {
                "Front": root.joinpath("spelling_front.html").read_text(encoding="utf-8").strip(),
                "Back": root.joinpath("spelling_back.html").read_text(encoding="utf-8").strip(),
            },
            "Production": {
                "Front": root.joinpath("production_front.html").read_text(encoding="utf-8").strip(),
                "Back": root.joinpath("back.html").read_text(encoding="utf-8").strip(),
            },
        },
    )


def install_japanese_vocab_template(anki_client) -> str:
    """Create or safely refresh the managed Japanese vocabulary note type."""
    spec = japanese_vocab_template()
    return anki_client.install_model(
        model_name=spec.model_name,
        fields=list(spec.fields),
        css=spec.css,
        templates=spec.templates,
    )
