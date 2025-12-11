# Using tree-sitter-twinkle in Neovim

This guide shows how to use the Twinkle tree-sitter grammar locally in Neovim.

## Prerequisites

- Neovim 0.9+ with treesitter support
- [nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter) plugin installed

## Setup Steps

### 1. Add Parser Configuration

Add this to your Neovim config (e.g., `~/.config/nvim/lua/config/treesitter.lua` or `init.lua`):

```lua
-- Register the Twinkle parser
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()

parser_config.twinkle = {
  install_info = {
    url = "~/playground/rust/twinkle/tree-sitter-twinkle", -- Local path
    files = {"src/parser.c"},
    branch = "main",
    generate_requires_npm = false,
    requires_generate_from_grammar = false,
  },
  filetype = "twinkle",
}
```

### 2. Set Up Filetype Detection

Create `~/.config/nvim/ftdetect/twinkle.vim` or add to your config:

```vim
" Detect .tw files as twinkle filetype
au BufRead,BufNewFile *.tw set filetype=twinkle
```

Or in Lua (in your `init.lua`):

```lua
vim.filetype.add({
  extension = {
    tw = "twinkle",
  },
})
```

### 3. Install the Parser

Run this command in Neovim:

```vim
:TSInstall twinkle
```

Or manually compile and install:

```bash
cd ~/playground/rust/twinkle/tree-sitter-twinkle

# Ensure parser is compiled
make

# Create parser directory if it doesn't exist
mkdir -p ~/.config/nvim/pack/parsers/start/tree-sitter-twinkle

# Copy parser
cp -r parser ~/.config/nvim/pack/parsers/start/tree-sitter-twinkle/
```

### 4. Configure Treesitter Highlighting

Add to your nvim-treesitter config:

```lua
require('nvim-treesitter.configs').setup({
  ensure_installed = {}, -- Don't auto-install, we're using local

  highlight = {
    enable = true,
    additional_vim_regex_highlighting = false,
  },

  -- Enable for twinkle files
  incremental_selection = {
    enable = true,
  },

  indent = {
    enable = true,
  },
})
```

### 5. Install Queries

Link or copy the queries directory:

```bash
# Create queries directory
mkdir -p ~/.config/nvim/pack/parsers/start/tree-sitter-twinkle/queries

# Link the highlights
ln -s ~/playground/rust/twinkle/tree-sitter-twinkle/queries/highlights.scm \
      ~/.config/nvim/pack/parsers/start/tree-sitter-twinkle/queries/highlights.scm
```

Or copy:

```bash
cp ~/playground/rust/twinkle/tree-sitter-twinkle/queries/highlights.scm \
   ~/.config/nvim/pack/parsers/start/tree-sitter-twinkle/queries/
```

## Alternative: Quick Setup Script

Create a setup script `install-neovim.sh`:

```bash
#!/bin/bash

PARSER_DIR="$HOME/.config/nvim/pack/parsers/start/tree-sitter-twinkle"
TWINKLE_DIR="$HOME/playground/rust/twinkle/tree-sitter-twinkle"

# Create directories
mkdir -p "$PARSER_DIR/queries"

# Compile parser
cd "$TWINKLE_DIR"
make

# Copy parser files
cp -r parser "$PARSER_DIR/"

# Link or copy queries
ln -sf "$TWINKLE_DIR/queries/highlights.scm" "$PARSER_DIR/queries/highlights.scm"

echo "✓ Parser installed to $PARSER_DIR"
echo ""
echo "Add this to your Neovim config:"
echo ""
echo "-- Filetype detection"
echo "vim.filetype.add({ extension = { tw = 'twinkle' } })"
echo ""
echo "-- Parser config"
echo "local parser_config = require('nvim-treesitter.parsers').get_parser_configs()"
echo "parser_config.twinkle = {"
echo "  install_info = {"
echo "    url = '$TWINKLE_DIR',"
echo "    files = {'src/parser.c'},"
echo "    generate_requires_npm = false,"
echo "  },"
echo "  filetype = 'twinkle',"
echo "}"
```

Make it executable and run:

```bash
chmod +x install-neovim.sh
./install-neovim.sh
```

## Verify Installation

1. Open a `.tw` file in Neovim
2. Run `:echo &filetype` - should show `twinkle`
3. Run `:TSBufEnable highlight` - should enable highlighting
4. Run `:InspectTree` - should show the parse tree

## Troubleshooting

### Parser not loading

```vim
" Check if parser is available
:echo nvim_treesitter#parsers#has_parser('twinkle')

" Force reload parsers
:TSUpdate
```

### Highlighting not working

```vim
" Check if highlights query is found
:echo nvim_get_runtime_file('parser/twinkle.so', 0)
:echo nvim_get_runtime_file('queries/twinkle/highlights.scm', 0)

" Enable treesitter highlight debug
:set verbose=9
:TSBufEnable highlight
```

### Queries not found

Ensure the queries are in the right location:
```
~/.config/nvim/pack/parsers/start/tree-sitter-twinkle/
├── parser/
│   └── twinkle.so
└── queries/
    └── highlights.scm
```

## Development Workflow

When making changes to the grammar:

```bash
# Regenerate parser
cd ~/playground/rust/twinkle/tree-sitter-twinkle
tree-sitter generate

# Reinstall
./install-neovim.sh

# Restart Neovim or reload
:e
```

## Using with lazy.nvim

If using lazy.nvim, add this to your plugins:

```lua
{
  "nvim-treesitter/nvim-treesitter",
  build = ":TSUpdate",
  config = function()
    -- Add parser config before setup
    local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
    parser_config.twinkle = {
      install_info = {
        url = "~/playground/rust/twinkle/tree-sitter-twinkle",
        files = {"src/parser.c"},
      },
      filetype = "twinkle",
    }

    -- Setup treesitter
    require("nvim-treesitter.configs").setup({
      highlight = { enable = true },
    })
  end,
}
```

And add filetype detection in your main config:

```lua
vim.filetype.add({ extension = { tw = "twinkle" } })
```
