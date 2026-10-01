-- Run from the repository root: nvim --headless -u NONE -i NONE -l clients/ide/neovim/smoke.lua
local repo = vim.fn.getcwd()
local temporary = vim.fn.tempname()
local function wait(description, check)
  assert(vim.wait(15000, check, 50), 'Timed out: ' .. description)
end
local function run()
  vim.fn.mkdir(temporary .. '/.xmd', 'p')
  for _, name in ipairs({ 'main.x.md', 'values.x.md' }) do
    vim.fn.writefile(vim.fn.readfile(repo .. '/examples/editor-smoke/' .. name), temporary .. '/' .. name)
  end
  vim.g.xmd_server_path = vim.env.XMD_SERVER_PATH
    or repo .. '/target/debug/xmd' .. (vim.fn.has('win32') == 1 and '.exe' or '')
  dofile(repo .. '/clients/ide/neovim/xmd.lua')
  vim.cmd('filetype on')
  vim.cmd.edit(vim.fn.fnameescape(temporary .. '/main.x.md'))
  local bufnr = vim.api.nvim_get_current_buf()
  local uri = vim.uri_from_bufnr(bufnr)
  local client
  wait('XMD attaches', function()
    client = vim.lsp.get_clients({ bufnr = bufnr, name = 'xmd' })[1]
    return client and client.initialized
  end)
  local function request(method, params)
    local result, err = client:request_sync(method, params, 10000, bufnr)
    assert(result and not result.err, vim.inspect(result or err))
    return result.result
  end
  local function at(text)
    for i, line in ipairs(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)) do
      local column = line:find(text, 1, true)
      if column then return { line = i - 1, character = column - 1 } end
    end
    error('Missing text: ' .. text)
  end
  local function visible_hints()
    return vim.inspect(vim.lsp.inlay_hint.get({ bufnr = bufnr }))
  end
  wait('inlay hints are displayed', function() return visible_hints():find('$75', 1, true) end)
  assert(vim.bo.filetype == 'xmd')
  local position = at('smoke_spent')
  local definitions = request('textDocument/definition', { textDocument = { uri = uri }, position = position })
  assert(vim.inspect(definitions):find('values.x.md', 1, true), 'Cross-file definition')
  assert(#request('textDocument/semanticTokens/full', { textDocument = { uri = uri } }).data > 0)

  local budget = at('[$125]')
  vim.api.nvim_buf_set_lines(bufnr, budget.line, budget.line + 1, false, { '[$150]:smoke_budget' })
  wait('unsaved calculation refresh', function() return visible_hints():find('$100', 1, true) end)

  local function action(text, title)
    local pos = at(text)
    local actions = request('textDocument/codeAction', {
      textDocument = { uri = uri }, range = { start = pos, ['end'] = pos }, context = { diagnostics = {} },
    })
    for _, a in ipairs(actions) do
      if a.title:sub(1, #title) == title then
        if a.edit then vim.lsp.util.apply_workspace_edit(a.edit, client.offset_encoding) end
        local c = type(a.command) == 'string' and a or a.command
        if c then request('workspace/executeCommand', { command = c.command, arguments = c.arguments }) end
        return
      end
    end
    error('Missing action: ' .. title)
  end
  local function contains(text)
    return table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), '\n'):find(text, 1, true)
  end
  action('- [ ] Try task', '✓ done')
  wait('task completed', function() return contains('- [x] Try task') end)
  vim.cmd.undo()
  wait('task undo', function() return contains('- [ ] Try task') end)
  action('[smoke_clock] :=', '▸ start')
  wait('running timer displayed', function() return visible_hints():find('running', 1, true) end)
  local before = visible_hints()
  wait('timer refresh without typing', function() return visible_hints() ~= before end)
  action('[smoke_clock] :=', '‖ pause')
  action('[smoke_clock] :=', '↺ reset')

  local edits = request('textDocument/rename', {
    textDocument = { uri = uri }, position = at('smoke_spent'), newName = 'smoke_expenses',
  })
  vim.lsp.util.apply_workspace_edit(edits, client.offset_encoding)
  assert(contains('smoke_budget - smoke_expenses'))
  local other = vim.fn.bufnr(temporary .. '/values.x.md')
  assert(table.concat(vim.api.nvim_buf_get_lines(other, 0, -1, false), '\n'):find(':smoke_expenses', 1, true))
  assert(vim.bo[other].modified, 'Rename leaves other buffer unsaved')
  client:stop()
  print('XMD Neovim smoke test passed: attach, hints, tokens, cross-file navigation, edits/undo, live timers, rename.')
end
local ok, err = xpcall(run, debug.traceback)
vim.fn.delete(temporary, 'rf')
if not ok then
  io.stderr:write(err .. '\n')
  vim.cmd('cquit 1')
end
vim.cmd('qa!')
