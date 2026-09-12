"""Keyboard-first Textual screens for durable modernization jobs."""

from __future__ import annotations

import asyncio
import calendar
import datetime as dt
import re
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, ScrollableContainer
from textual.screen import ModalScreen, Screen
from textual.widgets import DataTable, Footer, Input, Label, OptionList, Select, Static
from textual.widgets.option_list import Option


class BatchCompletionInput(Input):
    """Input backed by the selector's IDE-like completion menu."""

    def on_focus(self) -> None:
        screen = self.screen
        if isinstance(screen, BatchJobSelectorScreen):
            screen.activate_completions(self)

    def on_key(self, event) -> None:
        screen = self.screen
        if isinstance(screen, BatchJobSelectorScreen) and screen.handle_completion_key(self, event):
            event.prevent_default()
            event.stop()


class BatchConfirmation(ModalScreen[bool]):
    """A deliberately keyboard-only destructive-action confirmation."""

    DEFAULT_CSS = """
    BatchConfirmation { align: center middle; }
    BatchConfirmation #batch-confirm-dialog {
        width: 76; height: auto; min-height: 11; max-height: 22;
        padding: 1 3; background: $background; border: double $error;
    }
    BatchConfirmation #batch-confirm-title {
        width: 100%; text-align: center; color: #ffcc00;
        text-style: bold; margin-bottom: 1;
    }
    BatchConfirmation #batch-confirm-message { margin: 1 0 2 0; }
    BatchConfirmation #batch-confirm-keys { width: 100%; text-align: center; }
    """

    BINDINGS = [
        Binding("y", "confirm", "Yes", priority=True),
        Binding("n", "cancel", "No", priority=True),
        Binding("escape", "cancel", "No", priority=True),
    ]

    def __init__(self, message: str, *, title: str = "Confirm batch operation"):
        super().__init__()
        self.message = message
        self.title = title

    def compose(self) -> ComposeResult:
        with Vertical(id="batch-confirm-dialog"):
            yield Label(f"⚠  {self.title}", id="batch-confirm-title")
            yield Static(self.message, id="batch-confirm-message")
            yield Static(
                "[bold bright_green]Y: CONFIRM[/]   •   "
                "[bold bright_red]N: CANCEL[/]   •   "
                "[bold bright_cyan]ESC: CANCEL[/]",
                id="batch-confirm-keys",
            )

    def action_confirm(self) -> None:
        self.dismiss(True)

    def action_cancel(self) -> None:
        self.dismiss(False)


class BatchJobSelectorScreen(ModalScreen[dict[str, Any] | None]):
    """Compose an efficient Anki-side query plus optional local media filters."""

    DEFAULT_CSS = """
    BatchJobSelectorScreen { align: center middle; }
    BatchJobSelectorScreen #modal-dialog {
        width: 92%; height: auto; max-height: 100%; padding: 0 2;
        background: $background; border: double $accent; overflow: hidden;
    }
    BatchJobSelectorScreen #batch-selector-form {
        height: auto; background: $background; padding: 0 1;
    }
    BatchJobSelectorScreen #batch-selector-intro { margin-bottom: 1; }
    BatchJobSelectorScreen .batch-form-row { width: 100%; height: auto; margin-bottom: 1; }
    BatchJobSelectorScreen .batch-form-group { height: auto; margin-right: 2; }
    BatchJobSelectorScreen .batch-grow { width: 1fr; }
    BatchJobSelectorScreen .batch-half { width: 1fr; }
    BatchJobSelectorScreen .batch-small { width: 24; }
    BatchJobSelectorScreen .batch-date { width: 18; }
    BatchJobSelectorScreen Input, BatchJobSelectorScreen Select { margin: 0; }
    BatchJobSelectorScreen Input:disabled { color: $text-disabled; background: $panel-darken-1; }
    BatchJobSelectorScreen #batch-selector-context { color: $text-muted; margin-bottom: 1; }
    BatchJobSelectorScreen #batch-suggestions {
        display: none; height: 5; margin: 0 0 1 0;
        border: solid $accent; background: $background;
    }
    BatchJobSelectorScreen #batch-selector-status {
        width: 100%; height: 1; margin-top: 1; text-align: center; background: $surface;
    }
    """

    BINDINGS = [
        Binding("escape", "cancel", "Cancel", priority=True),
        Binding("c", "estimate", "Count matches", priority=True),
        Binding("s", "schedule", "Schedule", priority=True),
        Binding("tab", "focus_next_control", "", show=False, priority=True),
        Binding("shift+tab", "focus_previous_control", "", show=False, priority=True),
        Binding("1", "noop", "", show=False, priority=True),
        Binding("2", "noop", "", show=False, priority=True),
        Binding("3", "noop", "", show=False, priority=True),
        Binding("4", "noop", "", show=False, priority=True),
        Binding("5", "noop", "", show=False, priority=True),
        Binding("space", "noop", "", show=False, priority=True),
        Binding("b", "noop", "", show=False, priority=True),
        Binding("q", "noop", "", show=False, priority=True),
    ]

    def __init__(
        self, deck_choices: list[tuple[str, str]], model_names: list[str],
        template_names: list[str], tags: list[str], active_deck: str,
    ):
        super().__init__()
        self.deck_choices = deck_choices
        self.model_names = model_names
        self.template_names = template_names
        self.tags = tags
        self.active_deck = active_deck
        self._busy = False
        self._completion_input_id = ""
        self._completion_values: list[str] = []

    def compose(self) -> ComposeResult:
        with Vertical(id="modal-dialog"):
            yield Label("[bold accent]● NEW MODERNIZATION JOB[/]", classes="pane-title")
            yield Static(
                "Build the selection in Anki first, then apply optional media filters locally. "
                "This keeps large-deck scheduling fast and deterministic.",
                id="batch-selector-intro",
            )
            with Vertical(id="batch-selector-form"):
                with Horizontal(classes="batch-form-row"):
                    with Vertical(classes="batch-form-group batch-grow"):
                        yield Label("Mapped destination deck")
                        yield Select(self.deck_choices, value=self.active_deck, allow_blank=False, id="batch-select-deck")
                    with Vertical(classes="batch-form-group batch-small"):
                        yield Label("Image scope")
                        yield Select(
                            [("All cards", "all"), ("Has image", "with"), ("No image", "without")],
                            value="all", allow_blank=False, id="batch-select-media",
                        )
                    with Vertical(classes="batch-form-group batch-small"):
                        yield Label("Maximum notes")
                        yield Input(value="0", type="integer", id="batch-input-limit", tooltip="0 means all matches")
                with Horizontal(classes="batch-form-row"):
                    with Vertical(classes="batch-form-group batch-grow"):
                        yield Label("Added range preset")
                        yield Select(
                            [("Today", "today"), ("Yesterday", "yesterday"),
                             ("This week", "this_week"), ("This month", "this_month"),
                             ("Last 3 months", "last_3_months"), ("Last 6 months", "last_6_months"),
                             ("This year", "this_year"), ("Custom range", "custom")],
                            value="this_month", allow_blank=False, id="batch-select-date-preset",
                        )
                    with Vertical(classes="batch-form-group batch-date"):
                        yield Label("From (YYYY-MM-DD)")
                        yield Input(id="batch-input-date-from")
                    with Vertical(classes="batch-form-group batch-date"):
                        yield Label("To (YYYY-MM-DD)")
                        yield Input(id="batch-input-date-to")
                with Horizontal(classes="batch-form-row"):
                    with Vertical(classes="batch-form-group batch-half"):
                        yield Label("Note type")
                        yield Select(
                            [("Any note type", ""), *((name, name) for name in self.model_names)],
                            value="", allow_blank=False, id="batch-select-model",
                        )
                    with Vertical(classes="batch-form-group batch-half"):
                        yield Label("Card template")
                        yield Select(
                            [("Any card template", ""), *((name, name) for name in self.template_names)],
                            value="", allow_blank=False, id="batch-select-card",
                        )
                yield Static("", id="batch-selector-context")
                yield Label("Text / Anki query (Down selects suggestions)")
                yield BatchCompletionInput(
                    placeholder="e.g. is:due -is:suspended or a word", id="batch-input-text"
                )
                with Horizontal(classes="batch-form-row"):
                    with Vertical(classes="batch-form-group batch-half"):
                        yield Label("Required tags (Down selects suggestions)")
                        yield BatchCompletionInput(placeholder="jlpt_n3 reviewed", id="batch-input-tags")
                    with Vertical(classes="batch-form-group batch-half"):
                        yield Label("Excluded tags (Down selects suggestions)")
                        yield BatchCompletionInput(placeholder="do_not_modernize", id="batch-input-exclude-tags")
                yield OptionList(id="batch-suggestions")
            yield Static(
                "[bold bright_cyan]Tab / Shift+Tab: fields[/]   •   "
                "[bold bright_yellow]C: count matches[/]   •   "
                "[bold bright_green]S: schedule[/]   •   "
                "[bold bright_red]Esc: cancel[/]",
                id="batch-selector-status",
            )

    @staticmethod
    def _select_value(widget: Select) -> Any:
        return "" if widget.value is Select.BLANK else widget.value

    @staticmethod
    def _shift_months(value: dt.date, months: int) -> dt.date:
        absolute = value.year * 12 + value.month - 1 + months
        year, month_zero = divmod(absolute, 12)
        month = month_zero + 1
        return dt.date(year, month, min(value.day, calendar.monthrange(year, month)[1]))

    def on_mount(self) -> None:
        self._apply_date_preset("this_month")
        self._update_deck_context()
        self.query_one("#batch-select-deck", Select).focus()

    def on_select_changed(self, event: Select.Changed) -> None:
        if event.select.id == "batch-select-date-preset":
            self._apply_date_preset(str(self._select_value(event.select)))
        elif event.select.id == "batch-select-deck":
            self._update_deck_context()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id in {"batch-input-text", "batch-input-tags", "batch-input-exclude-tags"}:
            self.activate_completions(event.input)

    @staticmethod
    def _last_token(value: str) -> tuple[str, str]:
        match = re.match(r"^(.*?)([^\s]*)$", value, re.DOTALL)
        return (match.group(1), match.group(2)) if match else ("", value)

    def _query_completions(self, token: str) -> list[str]:
        roots = ["is:", "-is:", "prop:", "tag:", "-tag:", "deck:", "note:",
                 "card:", "flag:", "rated:", "added:", "introduced:"]
        stages = {
            "is:": ["is:due", "is:new", "is:review", "is:learn", "is:suspended", "is:buried"],
            "-is:": ["-is:suspended", "-is:buried", "-is:new", "-is:due"],
            "prop:": ["prop:ivl>=", "prop:ivl<=", "prop:due>=", "prop:due<=", "prop:ease>=", "prop:lapses>="],
            "flag:": ["flag:1", "flag:2", "flag:3", "flag:4", "flag:5", "flag:6", "flag:7"],
            "rated:": ["rated:1", "rated:7", "rated:30"],
            "added:": ["added:1", "added:7", "added:30", "added:90"],
            "introduced:": ["introduced:1", "introduced:7", "introduced:30"],
            "tag:": [f"tag:{tag}" for tag in self.tags],
            "-tag:": [f"-tag:{tag}" for tag in self.tags],
            "deck:": [f'deck:"{label.split("  [", 1)[0]}"' for label, _ in self.deck_choices],
            "note:": [f'note:"{name}"' for name in self.model_names],
            "card:": [f'card:"{name}"' for name in self.template_names],
        }
        if not token:
            return roots
        if token in stages:
            return stages[token]
        namespace = next((root for root in stages if token.startswith(root)), "")
        choices = stages.get(namespace, roots)
        return [choice for choice in choices if choice.casefold().startswith(token.casefold())]

    def activate_completions(self, input_widget: Input) -> None:
        if input_widget.id not in {"batch-input-text", "batch-input-tags", "batch-input-exclude-tags"}:
            return
        _, token = self._last_token(input_widget.value)
        values = (
            self._query_completions(token)
            if input_widget.id == "batch-input-text"
            else [tag for tag in self.tags if tag.casefold().startswith(token.casefold())]
        )[:30]
        suggestions = self.query_one("#batch-suggestions", OptionList)
        suggestions.clear_options()
        self._completion_input_id = input_widget.id or ""
        self._completion_values = values
        if not values:
            suggestions.display = False
            return
        suggestions.add_options([Option(value, id=f"completion-{index}") for index, value in enumerate(values)])
        suggestions.highlighted = 0
        suggestions.display = True

    def hide_completions(self) -> None:
        self.query_one("#batch-suggestions", OptionList).display = False

    def handle_completion_key(self, input_widget: Input, event) -> bool:
        suggestions = self.query_one("#batch-suggestions", OptionList)
        if not suggestions.display:
            return False
        if event.key in {"down", "up"}:
            suggestions.focus()
            suggestions.highlighted = max(0, suggestions.highlighted or 0)
            if event.key == "up" and suggestions.highlighted is not None:
                suggestions.highlighted = max(0, suggestions.highlighted - 1)
            return True
        if event.key == "enter" and suggestions.highlighted is not None:
            self._accept_completion(suggestions.highlighted)
            return True
        if event.key == "escape":
            self.hide_completions()
            return True
        return False

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        if event.option_list.id == "batch-suggestions":
            self._accept_completion(event.option_index)

    def _accept_completion(self, index: int) -> None:
        if not (0 <= index < len(self._completion_values)) or not self._completion_input_id:
            return
        input_widget = self.query_one(f"#{self._completion_input_id}", Input)
        prefix, _ = self._last_token(input_widget.value)
        completion = self._completion_values[index]
        continue_stage = completion.endswith(":") or completion.endswith((">=", "<=", ">", "<", "="))
        input_widget.value = prefix + completion + ("" if continue_stage else " ")
        input_widget.cursor_position = len(input_widget.value)
        input_widget.focus()
        self.activate_completions(input_widget)

    def _apply_date_preset(self, preset: str) -> None:
        today = dt.date.today()
        ranges = {
            "today": (today, today),
            "yesterday": (today - dt.timedelta(days=1), today - dt.timedelta(days=1)),
            "this_week": (today - dt.timedelta(days=today.weekday()), today),
            "this_month": (today.replace(day=1), today),
            "last_3_months": (self._shift_months(today, -3), today),
            "last_6_months": (self._shift_months(today, -6), today),
            "this_year": (today.replace(month=1, day=1), today),
        }
        date_from = self.query_one("#batch-input-date-from", Input)
        date_to = self.query_one("#batch-input-date-to", Input)
        custom = preset == "custom"
        if not custom:
            start, end = ranges.get(preset, ranges["this_month"])
            date_from.value, date_to.value = start.isoformat(), end.isoformat()
        date_from.disabled = date_to.disabled = not custom

    def _update_deck_context(self) -> None:
        select = self.query_one("#batch-select-deck", Select)
        key = str(self._select_value(select))
        label = next((label for label, value in self.deck_choices if value == key), key)
        self.query_one("#batch-selector-context", Static).update(
            f"[dim]Query context is automatically scoped to: {label}. Do not repeat deck: in the query field.[/]"
        )

    def selection(self) -> dict[str, Any]:
        try:
            limit = max(0, int(self.query_one("#batch-input-limit", Input).value.strip() or "0"))
        except ValueError as exc:
            raise ValueError("Maximum notes must be a whole number.") from exc
        try:
            date_from = dt.date.fromisoformat(self.query_one("#batch-input-date-from", Input).value.strip())
            date_to = dt.date.fromisoformat(self.query_one("#batch-input-date-to", Input).value.strip())
        except ValueError as exc:
            raise ValueError("From and To must use YYYY-MM-DD.") from exc
        if date_from > date_to:
            raise ValueError("From date must not be later than To date.")
        if date_to > dt.date.today():
            raise ValueError("To date cannot be in the future.")
        return {
            "deck_key": str(self._select_value(self.query_one("#batch-select-deck", Select))),
            "date_preset": str(self._select_value(self.query_one("#batch-select-date-preset", Select))),
            "date_from": date_from.isoformat(),
            "date_to": date_to.isoformat(),
            "model_name": str(self._select_value(self.query_one("#batch-select-model", Select))),
            "card_template": str(self._select_value(self.query_one("#batch-select-card", Select))),
            "query": self.query_one("#batch-input-text", Input).value.strip(),
            "required_tags": self.query_one("#batch-input-tags", Input).value.split(),
            "excluded_tags": self.query_one("#batch-input-exclude-tags", Input).value.split(),
            "media_scope": str(self._select_value(self.query_one("#batch-select-media", Select))),
            "limit": limit,
        }

    def action_estimate(self) -> None:
        if not self._busy:
            self.run_worker(self._estimate(), group="batch-selector", exclusive=True)

    async def _estimate(self) -> None:
        self._busy = True
        status = self.query_one("#batch-selector-status", Static)
        status.update("Counting matching notes…")
        try:
            count = await self.app.count_modernization_selection(self.selection())
            status.update(f"[bold green]{count:,} matching notes[/] · [bold]s[/] schedule · Esc cancel")
        except Exception as exc:
            status.update(f"[bold red]Selection failed: {exc}[/]")
        finally:
            self._busy = False

    def action_schedule(self) -> None:
        if self._busy:
            return
        try:
            self.dismiss(self.selection())
        except ValueError as exc:
            self.query_one("#batch-selector-status", Static).update(f"[bold red]{exc}[/]")

    def action_cancel(self) -> None:
        suggestions = self.query_one("#batch-suggestions", OptionList)
        if suggestions.display:
            self.hide_completions()
            if self._completion_input_id:
                self.query_one(f"#{self._completion_input_id}", Input).focus()
            return
        self.dismiss(None)

    def action_focus_next_control(self) -> None:
        self.focus_next()
        if not isinstance(self.focused, (BatchCompletionInput, OptionList)):
            self.hide_completions()

    def action_focus_previous_control(self) -> None:
        self.focus_previous()
        if not isinstance(self.focused, (BatchCompletionInput, OptionList)):
            self.hide_completions()

    def action_noop(self) -> None:
        """Shadow main-screen commands while the selector owns input."""


class BatchManagementScreen(Screen):
    """Create, monitor, resume, retry, cancel, delete, and roll back jobs."""

    PAGE_SIZE = 200
    BINDINGS = [
        Binding("escape", "close", "Back", priority=True),
        Binding("n", "next_or_new", "New / Next"),
        Binding("a", "new_deck", "Quick: active deck"),
        Binding("r", "resume", "Run / Resume"),
        Binding("p", "previous_or_pause", "Pause / Previous"),
        Binding("f", "retry_failed", "Retry failed"),
        Binding("c", "cancel", "Cancel"),
        Binding("u", "rollback", "Rollback job"),
        Binding("d", "delete", "Delete job"),
        Binding("pageup", "previous_page", "Previous page"),
        Binding("pagedown", "next_page", "Next page"),
        Binding("left", "previous_page", "", show=False, priority=True),
        Binding("right", "next_page", "", show=False, priority=True),
        Binding("tab", "next_table", "Switch table"),
        # Main-screen pane/palette bindings are inherited from App unless this
        # screen explicitly shadows them. They are invalid here and must not be
        # advertised by Footer.
        Binding("shift+tab", "noop", "", show=False, priority=True),
        Binding("1", "noop", "", show=False, priority=True),
        Binding("2", "noop", "", show=False, priority=True),
        Binding("3", "noop", "", show=False, priority=True),
        Binding("4", "noop", "", show=False, priority=True),
        Binding("5", "noop", "", show=False, priority=True),
        Binding("space", "noop", "", show=False, priority=True),
        Binding("b", "noop", "", show=False, priority=True),
        Binding("q", "noop", "", show=False, priority=True),
    ]

    def __init__(self, store):
        super().__init__()
        self.store = store
        self.jobs: list[dict] = []
        self.items: list[dict] = []
        self.selected_job_id: str | None = None
        self.item_offset = 0
        self._job_signature: tuple = ()
        self._item_signature: tuple = ()
        self._rendering = False

    def compose(self) -> ComposeResult:
        yield Label("[bold accent]● BATCH MODERNIZATION JOBS[/]", classes="pane-title")
        with Horizontal():
            with Vertical(classes="pane"):
                yield Label("Jobs", classes="pane-title")
                yield DataTable(id="batch-jobs")
            with Vertical(classes="pane"):
                yield Label("Cards [0/0]", id="batch-cards-title", classes="pane-title")
                yield DataTable(id="batch-items")
        with ScrollableContainer(classes="pane"):
            yield Static(Text(""), id="batch-details")
        yield Footer()

    def on_mount(self) -> None:
        jobs = self.query_one("#batch-jobs", DataTable)
        jobs.cursor_type = "row"
        jobs.add_columns("Created", "Deck", "State", "Progress", "Errors")
        cards = self.query_one("#batch-items", DataTable)
        cards.cursor_type = "row"
        cards.add_columns("#", "Word", "Note", "State", "Attempts", "Error")
        jobs.focus()
        self.refresh_data(force=True)
        self._refresh_timer = self.set_interval(1.5, self.refresh_data)

    def refresh_data(self, force: bool = False) -> None:
        self.run_worker(self._refresh_async(force), group="batch-refresh", exclusive=True)

    async def _refresh_async(self, force: bool = False) -> None:
        loop = asyncio.get_running_loop()
        jobs_data = await loop.run_in_executor(None, self.store.list_jobs)
        signature = tuple(
            (j["id"], j.get("status"), j.get("updated_at"), j.get("total"),
             j.get("completed"), j.get("failed"), j.get("reverted")) for j in jobs_data
        )
        if force or signature != self._job_signature:
            previous = self.selected_job_id
            self.jobs = jobs_data
            self._job_signature = signature
            table = self.query_one("#batch-jobs", DataTable)
            self._rendering = True
            table.clear()
            for job in self.jobs:
                total = int(job.get("total") or 0)
                done = int(job.get("completed") or 0) + int(job.get("reverted") or 0)
                table.add_row(
                    str(job.get("created_at", ""))[5:19].replace("T", " "),
                    str(job.get("deck_name", "")), str(job.get("status", "")),
                    f"{done}/{total}", str(job.get("failed") or 0), key=str(job["id"]),
                )
            if previous and any(job["id"] == previous for job in self.jobs):
                self.selected_job_id = previous
            else:
                self.selected_job_id = self.jobs[0]["id"] if self.jobs else None
                self.item_offset = 0
            if self.selected_job_id:
                index = next((i for i, job in enumerate(self.jobs) if job["id"] == self.selected_job_id), 0)
                table.move_cursor(row=index)
            self._rendering = False
        await self._refresh_items_async(force=force)

    async def _refresh_items_async(self, force: bool = False) -> None:
        job_id = self.selected_job_id
        if not job_id:
            self.items = []
            self.query_one("#batch-items", DataTable).clear()
            self.query_one("#batch-cards-title", Label).update("Cards [0/0]")
            self._show_details(None, {})
            return
        loop = asyncio.get_running_loop()
        job, counts, items = await asyncio.gather(
            loop.run_in_executor(None, self.store.get_job, job_id),
            loop.run_in_executor(None, self.store.item_counts, job_id),
            loop.run_in_executor(
                None, lambda: self.store.list_items(job_id, limit=self.PAGE_SIZE, offset=self.item_offset)
            ),
        )
        signature = (job_id, self.item_offset, (job or {}).get("updated_at"), tuple(sorted(counts.items())))
        if force or signature != self._item_signature:
            self._item_signature = signature
            self.items = items
            table = self.query_one("#batch-items", DataTable)
            table.clear()
            for item in self.items:
                error = str(item.get("last_error") or "").replace("\n", " ")
                table.add_row(
                    str(int(item.get("ordinal", 0)) + 1), str(item.get("word", "")),
                    str(item.get("note_id", "")), str(item.get("status", "")),
                    str(item.get("attempts", 0)), error[:60], key=str(item["id"]),
                )
            total = sum(counts.values())
            page_count = max(1, (total + self.PAGE_SIZE - 1) // self.PAGE_SIZE)
            page = min(page_count, self.item_offset // self.PAGE_SIZE + 1)
            self.query_one("#batch-cards-title", Label).update(f"Cards [{page}/{page_count}]")
            self._show_details(job, counts)

    def _show_details(self, job: dict | None, counts: dict[str, int]) -> None:
        if not job:
            text = "No jobs yet. Press n to build a modernization selection."
        else:
            settings = job.get("settings") or {}
            rates = settings.get("service_intervals", {})
            text = "\n".join([
                f"Job: {job['id']}", f"Deck: {job['deck_name']} ({job['deck_key']})",
                f"State: {job['status']}", f"Dry run: {'Yes' if job['dry_run'] else 'No'}",
                f"Created: {job['created_at']}", f"Started: {job.get('started_at') or '—'}",
                f"Finished: {job.get('finished_at') or '—'}",
                f"Card states: {', '.join(f'{k}={v}' for k, v in sorted(counts.items())) or '—'}",
                f"Maximum attempts: {settings.get('max_attempts', 3)}",
                "Service start intervals: " + (", ".join(f"{k}={v}s" for k, v in rates.items()) or "defaults"),
                f"Last job error: {job.get('last_error') or '—'}", "",
                "Recovery: interrupted work resumes at a card boundary. Commits are idempotent and retain "
                "their original snapshots. Deleting a job never reverts its commits.",
            ])
        self.query_one("#batch-details", Static).update(Text(text))

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if self._rendering:
            return
        if event.data_table.id == "batch-jobs" and event.row_key is not None:
            job_id = str(event.row_key.value)
            if job_id != self.selected_job_id:
                self.selected_job_id = job_id
                self.item_offset = 0
                self._item_signature = ()
                self.refresh_data(force=True)

    def action_next_table(self) -> None:
        jobs = self.query_one("#batch-jobs", DataTable)
        (self.query_one("#batch-items", DataTable) if self.focused is jobs else jobs).focus()

    def action_close(self) -> None:
        timer = getattr(self, "_refresh_timer", None)
        if timer is not None:
            timer.stop()
        self.workers.cancel_group(self, "batch-refresh")
        self.app.pop_screen()

    def on_unmount(self) -> None:
        timer = getattr(self, "_refresh_timer", None)
        if timer is not None:
            timer.stop()

    def action_noop(self) -> None:
        pass

    def action_new_job(self) -> None:
        self.app.run_worker(self.app.open_batch_job_selector(), exclusive=False)

    def action_next_or_new(self) -> None:
        if self.focused is self.query_one("#batch-items", DataTable):
            self.action_next_page()
        else:
            self.action_new_job()

    def action_previous_or_pause(self) -> None:
        if self.focused is self.query_one("#batch-items", DataTable):
            self.action_previous_page()
        else:
            self.action_pause()

    def action_new_deck(self) -> None:
        self.app.run_worker(self._new_deck(), exclusive=False)

    async def _new_deck(self) -> None:
        try:
            job_id = await self.app.create_modernization_job(entire_deck=True)
            self.selected_job_id = job_id
            self.refresh_data(force=True)
            self.notify(f"Scheduled {job_id}.")
        except Exception as exc:
            self.notify(f"Could not create batch: {exc}", severity="error")

    def action_resume(self) -> None:
        if self.selected_job_id:
            self.app.start_modernization_job(self.selected_job_id)

    def action_pause(self) -> None:
        if self.selected_job_id:
            self.app.pause_modernization_job(self.selected_job_id)
            self.refresh_data(force=True)

    def action_retry_failed(self) -> None:
        if self.selected_job_id:
            count = self.store.retry_failed(self.selected_job_id)
            self.notify(f"Reset {count} failed card(s).")
            self.refresh_data(force=True)

    def action_cancel(self) -> None:
        if not self.selected_job_id:
            return
        job_id = self.selected_job_id
        self.app.push_screen(
            BatchConfirmation(
                "[bold yellow]Cancel this job?[/]\n\n"
                "The current card finishes at a safe boundary. Pending cards will be marked skipped."
            ),
            lambda confirmed: self._cancel_confirmed(job_id, confirmed),
        )

    def _cancel_confirmed(self, job_id: str, confirmed: bool) -> None:
        if confirmed:
            self.app.cancel_modernization_job(job_id)
            self.refresh_data(force=True)

    def action_rollback(self) -> None:
        if not self.selected_job_id:
            return
        job_id = self.selected_job_id
        self.app.push_screen(
            BatchConfirmation(
                "[bold yellow]Restore every card changed by this job in reverse order?[/]\n\n"
                "Rollback is blocked when a newer batch changed the same note."
            ),
            lambda confirmed: self._rollback_confirmed(job_id, confirmed),
        )

    def _rollback_confirmed(self, job_id: str, confirmed: bool) -> None:
        if confirmed:
            self.app.start_batch_rollback(job_id)

    def action_delete(self) -> None:
        if not self.selected_job_id:
            return
        job_id = self.selected_job_id
        self.app.push_screen(
            BatchConfirmation(
                "[bold white on dark_red] WARNING: DELETION DOES NOT REVERT COMMITTED CARDS. [/]\n\n"
                "This removes the job record and cached processing artifacts. If you may need to undo it, "
                "cancel now and run [bold cyan]u[/] (rollback) first.",
                title="Delete batch job",
            ),
            lambda confirmed: self._delete_confirmed(job_id, confirmed),
        )

    def _delete_confirmed(self, job_id: str, confirmed: bool) -> None:
        if not confirmed:
            return
        try:
            result = self.store.delete_job(job_id)
            self.selected_job_id = None
            self.item_offset = 0
            self._job_signature = ()
            self._item_signature = ()
            self.refresh_data(force=True)
            self.notify(
                f"Deleted job metadata for {result['items']} card(s). Anki commits were not reverted.",
                severity="warning" if result["committed"] else "information",
            )
        except Exception as exc:
            self.notify(f"Could not delete job: {exc}", severity="error")

    def action_previous_page(self) -> None:
        if self.item_offset:
            self.item_offset = max(0, self.item_offset - self.PAGE_SIZE)
            self._item_signature = ()
            self.refresh_data(force=True)

    def action_next_page(self) -> None:
        total = next((int(job.get("total") or 0) for job in self.jobs if job["id"] == self.selected_job_id), 0)
        if self.item_offset + self.PAGE_SIZE < total:
            self.item_offset += self.PAGE_SIZE
            self._item_signature = ()
            self.refresh_data(force=True)
