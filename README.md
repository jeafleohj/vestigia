# Vestigia.nvim

A native Neovim plugin for exploring Git file history, powered by a reusable
Rust core.

Vestigia focuses on a fast Neovim workflow: open the history for the current
file, move through revisions, inspect commit metadata, and review older
contents without mutating the original buffer. The Rust core owns the Git
history model and content loading; the Neovim adapter owns commands, buffers,
keymaps, and UI.

## lazy.nvim

Install from GitHub:

```lua
{
  "jeafleohj/vestigia",
  cmd = {
    "Vestigia",
    "VestigiaPrev",
    "VestigiaNext",
    "VestigiaMeta",
    "VestigiaToggleHighlights",
  },
  build = function()
    require("vestigia.install").install()
  end,
  config = function()
    require("vestigia").setup()
  end,
}
```

The Neovim adapter is a native plugin. The `build` step downloads a prebuilt
native library into `lib/`, and the Lua loader in `lua/vestigia.lua` loads that
library into Neovim.

## Usage

Open the history for the current file:

```vim
:Vestigia
```

Inside the Vestigia buffer:

- `[h`: move to the previous revision
- `]h`: move to the next revision
- `gm`: open revision metadata
- `gh`: toggle changed-line highlights
- `q`: close the buffer

Changed-line highlights use the `VestigiaChangedLine` highlight group, linked
to `DiffChange` by default. Override it in your Neovim config if you want a
different style:

```vim
highlight VestigiaChangedLine guibg=#3a3a24
```
