local M = {}

local loaded = false

local LIBRARY_ENTRYPOINT = "luaopen_vestigia_nvim"

local function plugin_root()
  local source = debug.getinfo(1, "S").source
  local script_path = source:sub(1, 1) == "@" and source:sub(2) or source

  return vim.fn.fnamemodify(script_path, ":p:h:h")
end

local function join_path(...)
  return table.concat({ ... }, "/")
end

local function library_name()
  if vim.fn.has("macunix") == 1 then
    return "libvestigia_nvim.dylib"
  end

  if vim.fn.has("unix") == 1 then
    return "libvestigia_nvim.so"
  end

  error("Vestigia does not support this platform yet")
end

local function library_candidates()
  local root = plugin_root()
  local name = library_name()

  return {
    join_path(root, "lib", name),
    join_path(root, "target", "release", name),
  }
end

local function find_library()
  for _, path in ipairs(library_candidates()) do
    if vim.fn.filereadable(path) == 1 then
      return path
    end
  end

  return nil
end

local function format_missing_library_error(candidates)
  return table.concat({
    "Vestigia native library not found.",
    "Searched:",
    table.concat(candidates, "\n"),
    "Install it with: require('vestigia.install').install()",
    "Or build it with: cargo build --release -p vestigia-nvim",
  }, "\n")
end

local function load_library(path)
  local entrypoint, err = package.loadlib(path, LIBRARY_ENTRYPOINT)

  if not entrypoint then
    error(
      table.concat({
        "Failed to load Vestigia native library.",
        "Library: " .. path,
        err,
      }, "\n")
    )
  end

  entrypoint()
end

function M.setup()
  if loaded then
    return
  end

  local candidates = library_candidates()
  local path = find_library()

  if not path then
    error(format_missing_library_error(candidates))
  end

  load_library(path)
  loaded = true
end

return M
