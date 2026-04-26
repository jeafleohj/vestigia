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
  cmd = { "Vestigia", "VestigiaPrev", "VestigiaNext", "VestigiaMeta" },
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
