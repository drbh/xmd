-- Neovim 0.11+. With `xmd` on PATH, just
-- dofile('/path/to/xmd/client/ide/neovim/xmd.lua') from init.lua.
-- vim.g.xmd_server_path overrides the executable.
assert(vim.fn.has('nvim-0.11') == 1, 'XMD configuration requires Neovim 0.11+')
local binary = vim.g.xmd_server_path or 'xmd'
assert(vim.fn.executable(binary) == 1,
  'xmd is not on PATH: install it with'
  .. ' `mkdir -p ~/.local/bin && curl -fsSL https://github.com/drbh/xmd/releases/latest/download/xmd-$(uname -s)-$(uname -m).tar.gz | tar -xzC ~/.local/bin xmd`,'
  .. ' or set vim.g.xmd_server_path')

-- `.x.md` (notes) is a two-part suffix, so match it as a pattern rather than
-- an extension; `.xmd` (libraries) is a plain one.
vim.filetype.add({ extension = { xmd = 'xmd' }, pattern = { ['.*%.x%.md'] = 'xmd' } })

local function highlights()
  -- Link semantic tokens to the user's theme; all tokenization stays in Rust.
  local groups = {
    heading = 'Title', xmdMoney = 'Number', xmdDate = 'Constant',
    xmdTime = 'Constant', xmdDuration = 'Number', xmdRatio = 'Number',
    xmdBoolean = 'Boolean', xmdPunctuation = 'Delimiter', xmdCode = 'String',
    xmdLink = 'Underlined', xmdToggle = 'Todo', xmdToggleOn = 'String',
    xmdToggleMixed = 'Number', xmdFinished = 'Comment', xmdKey = 'Identifier',
    -- The categorical palette: ten groups most themes color apart.
    xmdCategory1 = 'Special', xmdCategory2 = 'String', xmdCategory3 = 'Function',
    xmdCategory4 = 'Type', xmdCategory5 = 'Statement', xmdCategory6 = 'Constant',
    xmdCategory7 = 'PreProc', xmdCategory8 = 'Number', xmdCategory9 = 'Character',
    xmdCategory10 = 'Label',
  }
  for token, group in pairs(groups) do
    vim.api.nvim_set_hl(0, '@lsp.type.' .. token .. '.xmd', { link = group, default = true })
  end
  vim.api.nvim_set_hl(0, '@lsp.mod.declaration.xmd', { bold = true, default = true })
end
highlights()
vim.api.nvim_create_autocmd('ColorScheme', {
  group = vim.api.nvim_create_augroup('XmdHighlights', { clear = true }),
  callback = highlights,
})

vim.lsp.config('xmd', {
  cmd = { binary, 'lsp' },
  filetypes = { 'xmd' },
  root_dir = function(bufnr, on_dir)
    on_dir(vim.fs.root(bufnr, { '.xmd', '.git' })
      or vim.fs.dirname(vim.api.nvim_buf_get_name(bufnr)))
  end,
  on_attach = function(client, bufnr)
    vim.bo[bufnr].tabstop = 2
    vim.bo[bufnr].shiftwidth = 2
    vim.bo[bufnr].expandtab = true
    vim.bo[bufnr].commentstring = '<!-- %s -->'
    vim.lsp.inlay_hint.enable(true, { bufnr = bufnr })
    vim.lsp.completion.enable(true, client.id, bufnr, { autotrigger = true })
    if vim.lsp.on_type_formatting then
      vim.lsp.on_type_formatting.enable(true, { client_id = client.id })
    end
    local function map(key, action, description)
      vim.keymap.set('n', key, action, { buffer = bufnr, desc = description })
    end
    map('<leader>ja', vim.lsp.buf.code_action, 'XMD actions')
    map('<leader>jl', vim.lsp.codelens.run, 'XMD task/timer controls')
    map('<leader>jf', function() vim.lsp.buf.format({ bufnr = bufnr }) end, 'Format XMD table')
    if vim.lsp.codelens.enable then
      vim.lsp.codelens.enable(true, { bufnr = bufnr })
    else
      -- Neovim 0.11 needs explicit CodeLens refreshes.
      local function refresh() vim.lsp.codelens.refresh({ bufnr = bufnr }) end
      vim.api.nvim_create_autocmd({ 'BufEnter', 'CursorHold', 'InsertLeave' }, {
        buffer = bufnr,
        group = vim.api.nvim_create_augroup('XmdCodeLens' .. bufnr, { clear = true }),
        callback = refresh,
      })
      refresh()
    end
  end,
})
vim.lsp.enable('xmd')
