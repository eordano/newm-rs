-- The demo's Neovim: the author's editor with its look and editing behaviour, without language
-- servers, formatters, linters or anything that reaches the network. demo.nix loads the plugins
-- and appends helpers.lua (zen mode, completion toggle, tag-back), which is the author's own.

vim.g.mapleader = " "
vim.g.maplocalleader = ","

vim.opt.mouse = "a"
vim.opt.termguicolors = true
vim.opt.background = "dark"
vim.opt.number = true
vim.opt.signcolumn = "yes"
vim.opt.updatetime = 300
vim.opt.splitbelow = true
vim.opt.splitright = true
vim.opt.tabstop = 4
vim.opt.shiftwidth = 4
vim.opt.expandtab = true
vim.opt.swapfile = false

require("tokyonight").setup({ style = "night" })
vim.cmd.colorscheme("tokyonight-night")

require("notify").setup({})
vim.notify = require("notify")

require("lualine").setup({ options = { theme = "tokyonight" } })
require("bufferline").setup({ options = { indicator = { style = "icon" } } })

require("nvim-tree").setup({
  update_focused_file = { enable = true },
  renderer = { highlight_opened_files = "icon" },
})
vim.keymap.set("n", "<leader>t", "<cmd>NvimTreeToggle<CR>", { desc = "Toggle the file tree" })

local telescope = require("telescope.builtin")
vim.keymap.set("n", "<leader>ff", telescope.find_files, { desc = "Find files" })
vim.keymap.set("n", "<leader>fg", telescope.live_grep, { desc = "Live grep" })
vim.keymap.set("n", "<leader>fb", telescope.buffers, { desc = "Buffers" })
vim.keymap.set("n", "<leader>fh", telescope.help_tags, { desc = "Help" })

require("which-key").setup({})

require("hop").setup({})
vim.keymap.set("n", "<leader>h", "<cmd>HopPattern<CR>", { desc = "Hop to a pattern" })

-- Highlighting by tree-sitter for the languages whose grammars the demo carries.
vim.api.nvim_create_autocmd("FileType", {
  callback = function(args)
    pcall(vim.treesitter.start, args.buf)
  end,
})

local cmp = require("cmp")
cmp.setup({
  sources = { { name = "buffer" }, { name = "path" } },
  mapping = cmp.mapping.preset.insert({
    ["<C-Space>"] = cmp.mapping.complete(),
    ["<CR>"] = cmp.mapping.confirm({ select = false }),
    ["<Tab>"] = cmp.mapping.select_next_item(),
    ["<S-Tab>"] = cmp.mapping.select_prev_item(),
  }),
})
