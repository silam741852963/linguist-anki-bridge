import asyncio

from textual.app import App
from textual.widgets import Button, Footer, Input, OptionList, Select

from linguist_anki_bridge.tui.batch_screen import (
    BatchConfirmation, BatchJobSelectorScreen,
)


def test_batch_selector_mounts_visible_controls_without_nested_footer():
    async def run():
        app = App()
        async with app.run_test(size=(100, 36)) as pilot:
            app.push_screen(BatchJobSelectorScreen(
                [("Japanese", "japanese_vocab")], ["Legacy"],
                ["Recognition"], ["jlpt_n3"], "japanese_vocab",
            ))
            await pilot.pause()
            screen = app.screen
            assert len(screen.query(Select)) == 5
            assert len(screen.query(Input)) == 6
            # A Footer nested inside the modal docks over all form controls.
            assert len(screen.query(Footer)) == 0
            assert screen.active_bindings["c"].binding.action == "estimate"
            assert screen.active_bindings["s"].binding.action == "schedule"
            assert screen.query_one("#batch-input-date-from", Input).disabled
            assert screen.query_one("#batch-input-date-to", Input).disabled
            dialog = screen.query_one("#modal-dialog")
            status = screen.query_one("#batch-selector-status")
            assert status.region.y + status.region.height <= dialog.region.y + dialog.region.height
            screen.query_one("#batch-select-date-preset", Select).value = "custom"
            await pilot.pause()
            assert not screen.query_one("#batch-input-date-from", Input).disabled
            assert not screen.query_one("#batch-input-date-to", Input).disabled

    asyncio.run(run())


def test_batch_confirmation_is_keyboard_only():
    async def run():
        app = App()
        async with app.run_test(size=(90, 24)) as pilot:
            app.push_screen(BatchConfirmation("[bold red]Warning[/]"))
            await pilot.pause()
            assert len(app.screen.query(Button)) == 0
            assert app.screen.active_bindings["y"].binding.action == "confirm"
            assert app.screen.active_bindings["n"].binding.action == "cancel"

    asyncio.run(run())


def test_query_completion_opens_on_focus_and_advances_namespace():
    async def run():
        app = App()
        async with app.run_test(size=(120, 44)) as pilot:
            app.push_screen(BatchJobSelectorScreen(
                [("Japanese", "japanese_vocab")], ["Legacy"],
                ["Recognition"], ["jlpt_n3"], "japanese_vocab",
            ))
            await pilot.pause()
            screen = app.screen
            query = screen.query_one("#batch-input-text", Input)
            query.focus()
            await pilot.pause()
            suggestions = screen.query_one("#batch-suggestions", OptionList)
            assert suggestions.display
            assert screen._completion_values[0] == "is:"
            screen._accept_completion(0)
            await pilot.pause()
            assert query.value == "is:"
            assert screen._completion_values[:3] == ["is:due", "is:new", "is:review"]
            tags = screen.query_one("#batch-input-tags", Input)
            tags.focus()
            await pilot.pause()
            assert screen._completion_values == ["jlpt_n3"]

    asyncio.run(run())
