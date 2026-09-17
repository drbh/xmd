-- Neovim 0.11+. Set vim.g.jot_server_path to an absolute executable path,
-- then dofile('/absolute/path/to/jot/neovim/jot.lua') from init.lua.
assert(vim.fn.has('nvim-0.11') == 1, 'Jot configuration requires Neovim 0.11+')
local binary = vim.g.jot_server_path
assert(type(binary) == 'string' and vim.fn.executable(binary) == 1,
  'Set vim.g.jot_server_path to the Jot executable built with cargo build')

vim.filetype.add({ extension = { jot = 'jot' } })

local function highlights()
  -- Link semantic tokens to the user's theme; all tokenization stays in Rust.
  local groups = {
    heading = 'Title', jotMoney = 'Number', jotDate = 'Constant',
    jotTime = 'Constant', jotDuration = 'Number', jotRatio = 'Number',
    jotBoolean = 'Boolean', jotPunctuation = 'Delimiter', jotCode = 'String',
    jotLink = 'Underlined', jotCheckbox = 'Todo', jotCheckboxChecked = 'String',
    jotTaskDone = 'Comment',
  }
  for token, group in pairs(groups) do
    vim.api.nvim_set_hl(0, '@lsp.type.' .. token .. '.jot', { link = group, default = true })
  end
  vim.api.nvim_set_hl(0, '@lsp.mod.declaration.jot', { bold = true, default = true })
end
highlights()
vim.api.nvim_create_autocmd('ColorScheme', {
  group = vim.api.nvim_create_augroup('JotHighlights', { clear = true }),
  callback = highlights,
})

vim.lsp.config('jot', {
  cmd = { binary, 'lsp' },
  filetypes = { 'jot' },
  root_dir = function(bufnr, on_dir)
    on_dir(vim.fs.root(bufnr, { '.jot', '.git' })
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
    map('<leader>ja', vim.lsp.buf.code_action, 'Jot actions')
    map('<leader>jl', vim.lsp.codelens.run, 'Jot task/timer controls')
    map('<leader>jf', function() vim.lsp.buf.format({ bufnr = bufnr }) end, 'Format Jot table')
    if vim.lsp.codelens.enable then
      vim.lsp.codelens.enable(true, { bufnr = bufnr })
    else
      -- Neovim 0.11 needs explicit CodeLens refreshes.
      local function refresh() vim.lsp.codelens.refresh({ bufnr = bufnr }) end
      vim.api.nvim_create_autocmd({ 'BufEnter', 'CursorHold', 'InsertLeave' }, {
        buffer = bufnr,
        group = vim.api.nvim_create_augroup('JotCodeLens' .. bufnr, { clear = true }),
        callback = refresh,
      })
      refresh()
    end
  end,
})
vim.lsp.enable('jot')
