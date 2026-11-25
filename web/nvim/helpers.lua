require("zen-mode").setup({})
vim.api.nvim_set_keymap('n', '<leader>tz', '<cmd>ZenMode<CR>', {
    noremap = true,
    silent = true,
    desc = "Toggle Zen Mode"
})
-- Kitty keyboard protocol distinguishes Ctrl+[ from Escape. Neovim enables it
-- automatically for known terminals; we append ghostty's terminfo names in
-- case the running nvim build doesn't list them by default. Older nvim builds
-- (e.g. the one nvf pulls on Darwin) lack this option; without it, <C-[> is
-- indistinguishable from <Esc>, so skip the mapping entirely in that case.
local ok = pcall(function()
  vim.opt.keyprotocol:append({
    'ghostty:kitty',
    'xterm-ghostty:kitty',
  })
end)

if ok then
  vim.keymap.set('n', '<C-[>', '<C-t>', {
    noremap = true,
    silent = true,
    desc = 'Jump back (tag stack)',
  })
end
-- Define a function to toggle nvim-cmp
function toggle_cmp()
  local cmp = require('cmp')
  if cmp.config.enabled then
    cmp.setup { enabled = false }
    print("nvim-cmp disabled")
  else
    cmp.setup { enabled = true }
    print("nvim-cmp enabled")
  end
end

-- Map the leader key combination to the toggle function
vim.api.nvim_set_keymap('n', '<leader>tc', ':lua toggle_cmp()<CR>', {
    noremap = true,
    silent = true
})

