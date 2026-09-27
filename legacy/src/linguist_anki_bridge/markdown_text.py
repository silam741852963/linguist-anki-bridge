"""Markdown authoring support for Anki's HTML-backed fields."""

from __future__ import annotations

import re
from bs4 import BeautifulSoup, NavigableString, Tag
from markdown_it import MarkdownIt


_MARKDOWN = MarkdownIt("commonmark", {"html": False, "breaks": True}).enable("table")


def render_markdown(source: str, embedded_html: dict[str, str] | None = None) -> str:
    """Render user Markdown safely, restoring trusted media placeholders afterward."""
    rendered = _MARKDOWN.render(source or "")
    for token, markup in (embedded_html or {}).items():
        rendered = rendered.replace(token, markup)
    return rendered.strip()


def html_to_markdown(markup: str) -> tuple[str, dict[str, str]]:
    """Convert card HTML to readable Markdown without exposing base64 images."""
    soup = BeautifulSoup(markup or "", "html.parser")
    embedded: dict[str, str] = {}

    def convert(node) -> str:
        if isinstance(node, NavigableString):
            return str(node)
        if not isinstance(node, Tag):
            return ""
        name = node.name.lower()
        if name == "img":
            token = f"{{{{LINGUIST_MEDIA_{len(embedded)}}}}}"
            embedded[token] = str(node)
            return token
        if name == "br":
            return "\n"
        if name == "hr":
            return "\n\n---\n\n"
        inner = "".join(convert(child) for child in node.children)
        if name in {"strong", "b"}:
            return f"**{inner.strip()}**"
        if name in {"em", "i"}:
            return f"*{inner.strip()}*"
        if name == "code":
            return f"`{inner.strip()}`"
        if name == "a":
            return f"[{inner.strip()}]({node.get('href', '')})"
        if name in {"h1", "h2", "h3", "h4", "h5", "h6"}:
            return f"\n{'#' * int(name[1])} {inner.strip()}\n"
        if name == "li":
            parent = node.parent.name.lower() if isinstance(node.parent, Tag) else "ul"
            return f"\n{'1.' if parent == 'ol' else '-'} {inner.strip()}"
        if name in {"ul", "ol", "section"}:
            return f"\n{inner.strip()}\n"
        if name in {"div", "p", "pre", "blockquote"}:
            return f"{inner.strip()}\n"
        return inner

    markdown = "".join(convert(child) for child in soup.children)
    markdown = re.sub(r"[ \t]+\n", "\n", markdown)
    markdown = re.sub(r"\n{3,}", "\n\n", markdown).strip()
    return markdown, embedded
