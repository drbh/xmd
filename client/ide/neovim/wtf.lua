-- Neovim 0.11+. Set vim.g.wtf_server_path to an absolute executable path,
-- then dofile('/absolute/path/to/wtf/client/ide/neovim/wtf.lua') from init.lua.
assert(vim.fn.has('nvim-0.11') == 1, 'WTF configuration requires Neovim 0.11+')
local binary = vim.g.wtf_server_path
assert(type(binary) == 'string' and vim.fn.executable(binary) == 1,
  'Set vim.g.wtf_server_path to the WTF executable built with cargo build')

vim.filetype.add({ extension = { wtf = 'wtf' } })

local function highlights()
  -- Link semantic tokens to the user's theme; all tokenization stays in Rust.
  local groups = {
    heading = 'Title', wtfMoney = 'Number', wtfDate = 'Constant',
    wtfTime = 'Constant', wtfDuration = 'Number', wtfRatio = 'Number',
    wtfBoolean = 'Boolean', wtfPunctuation = 'Delimiter', wtfCode = 'String',
    wtfLink = 'Underlined', wtfCheckbox = 'Todo', wtfCheckboxChecked = 'String',
    wtfTaskDone = 'Comment',
  }
  for token, group in pairs(groups) do
    vim.api.nvim_set_hl(0, '@lsp.type.' .. token .. '.wtf', { link = group, default = true })
  end
  vim.api.nvim_set_hl(0, '@lsp.mod.declaration.wtf', { bold = true, default = true })
end
highlights()
vim.api.nvim_create_autocmd('ColorScheme', {
  group = vim.api.nvim_create_augroup('WtfHighlights', { clear = true }),
  callback = highlights,
})

vim.lsp.config('wtf', {
  cmd = { binary, 'lsp' },
  filetypes = { 'wtf' },
  root_dir = function(bufnr, on_dir)
    on_dir(vim.fs.root(bufnr, { '.wtf', '.git' })
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
    map('<leader>ja', vim.lsp.buf.code_action, 'WTF actions')
    map('<leader>jl', vim.lsp.codelens.run, 'WTF task/timer controls')
    map('<leader>jf', function() vim.lsp.buf.format({ bufnr = bufnr }) end, 'Format WTF table')
    if vim.lsp.codelens.enable then
      vim.lsp.codelens.enable(true, { bufnr = bufnr })
    else
      -- Neovim 0.11 needs explicit CodeLens refreshes.
      local function refresh() vim.lsp.codelens.refresh({ bufnr = bufnr }) end
      vim.api.nvim_create_autocmd({ 'BufEnter', 'CursorHold', 'InsertLeave' }, {
        buffer = bufnr,
        group = vim.api.nvim_create_augroup('WtfCodeLens' .. bufnr, { clear = true }),
        callback = refresh,
      })
      refresh()
    end
  end,
})
vim.lsp.enable('wtf')
