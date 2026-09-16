# Native desktop accessibility contract

The Rust/Qt preview treats pointer input as primary while retaining complete
keyboard access.

- `Tab` follows visual order through controls. `Shift+Tab` reverses it.
- `Ctrl+K` focuses search; `Alt+Up` and `Alt+Down` move through review cards.
- `Ctrl+Enter` previews changes. Applying still requires the enabled Apply
  button, preserving the explicit preview gate.
- `Ctrl+Shift+I`, `Ctrl+Shift+B`, and `Ctrl+,` open import, batch, and settings.
- Lists expose list/list-item roles, selection, names, state, and counts to
  assistive technology. Actions and editors have accessible names.
- Custom transparent editors draw a two-pixel theme-accent focus ring. Native
  Qt controls retain their platform focus indicators.
- Japanese text editors enable input-method composition and do not bind plain
  Enter, so IME candidate confirmation cannot trigger an application action.
- Typography uses points and layout uses Qt logical pixels, honoring desktop
  font and display scaling. Dialogs are bounded to the available window.
- The interface has no automatic or decorative animations. `--reduce-motion`
  is accepted as an explicit policy flag for future motion; any later animation
  must remain disabled while it is present.
- Vim-style modal editing is not enabled. It may only be added as an opt-in
  setting, never as the default interaction model.
